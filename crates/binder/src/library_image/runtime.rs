//! Completed binds constructed directly at the loader's ordered leases.
use super::*;
use std::{cell::Cell, sync::Arc};
use tsc_syntax::library_image::FreshLibrarySource;

struct BindTimer(tsc_types::trace::Timer);

impl Drop for BindTimer {
    fn drop(&mut self) {
        self.0.mark("library image: binding");
    }
}

fn pairs(input: &mut Decoder<'_>, mut put: impl FnMut(u32, u32) -> Result<()>) -> Result<()> {
    for _ in 0..input.count(8)? {
        put(input.word()?, input.word()?)?;
    }
    Ok(())
}

fn table(
    input: &mut Decoder<'_>,
    name: &impl Fn(u32) -> Result<EscapedName>,
    symbol: &impl Fn(u32) -> Result<SymbolId>,
) -> Result<SymbolTable> {
    let count = input.count(8)?;
    let mut table = SymbolTable::new();
    table.reserve(count);
    for _ in 0..count {
        if table
            .insert(name(input.word()?)?, symbol(input.word()?)?)
            .is_some()
        {
            return Err(ImageError("duplicate library table name"));
        }
    }
    Ok(table)
}

fn shared_table(
    table: SymbolTable,
    alias: u32,
    previous: &mut Vec<Arc<SymbolTable>>,
) -> Result<Arc<SymbolTable>> {
    if alias == 0 {
        return Err(ImageError("present library table has no alias class"));
    }
    let index = alias as usize - 1;
    if index == previous.len() {
        let table = Arc::new(table);
        previous.push(Arc::clone(&table));
        return Ok(table);
    }
    let original = previous
        .get(index)
        .ok_or(ImageError("library table alias precedes its owner"))?;
    if **original != table {
        return Err(ImageError("library table alias has different entries"));
    }
    Ok(Arc::clone(original))
}

/// The source capability is issued only by image construction and is consumed
/// before any mutable source borrow is exposed to a caller.
pub fn restore_reserved(
    fresh: FreshLibrarySource,
    reservation: &crate::BindReservations,
    domain: &IdentityDomain,
) -> Result<(SourceFile, BindData)> {
    // Drop last so decoder scratch and temporary shared handles are counted.
    let _timer = BindTimer(tsc_types::trace::Timer::new());
    let (mut source, image) = fresh.into_parts();
    if !source.identity_owned_by(domain)
        || !reservation.symbols.belongs_to(domain)
        || !reservation.serials.belongs_to(domain)
        || reservation.symbols.space() != IdentitySpace::Symbol
        || reservation.serials.space() != IdentitySpace::PrivateNameSerial
    {
        return Err(ImageError(
            "library binding leases belong to another domain",
        ));
    }
    let mut input = Decoder::open_static(image.binding, MAGIC, provenance())?;
    let common_js = input.word()?;
    let unreachable = input.word()?;
    let emit_flags = input.word()? as i32;
    let serial_count = input.word()?;
    if serial_count > reservation.serials.range().len() {
        return Err(ImageError("library serials exceed their reservation"));
    }
    let serial_base = reservation.serials.range().start();
    let next_serial = serial_base
        .checked_add(serial_count)
        .ok_or(ImageError("library private serial exceeds u32"))?;
    let serials = PrivateNameSerialRelocation {
        old: IdentityRange::new(0, serial_count),
        new: IdentityRange::new(serial_base, next_serial),
    };
    let count = input.count(4)?;
    let mut names = Vec::with_capacity(count);
    for _ in 0..count {
        let text = tsc_types::JsStr::try_from_canonical_bytes(input.bytes()?)
            .ok_or(ImageError("invalid binding name text"))?;
        let mut name = EscapedName::from_escaped_text(text);
        serials
            .name(&mut name)
            .map_err(|_| ImageError("invalid binding private name"))?;
        names.push(name);
    }
    let name = |word: u32| {
        names
            .get(word as usize)
            .copied()
            .ok_or(ImageError("binding name outside dictionary"))
    };
    let symbol_count = input.count(48)?;
    if symbol_count > reservation.symbols.range().len() as usize {
        return Err(ImageError("library symbols exceed their reservation"));
    }
    let node = |word: u32| -> Result<NodeId> {
        if word == 0 || word as usize > source.arena.len() {
            return Err(ImageError("binding node outside source"));
        }
        Ok(NodeId::new(source.arena.node_base() + word - 1))
    };
    let symbol = |word: u32| -> Result<SymbolId> {
        if word == 0 || word as usize > symbol_count {
            return Err(ImageError("binding symbol outside source"));
        }
        Ok(SymbolId::new(
            reservation.symbols.range().start() + word - 1,
        ))
    };
    let max_flow = Cell::new(0u32);
    let flow_id = |word: u32| -> Result<FlowId> {
        let id = FlowId::from_position(word).ok_or(ImageError("absent binding flow"))?;
        max_flow.set(max_flow.get().max(word));
        Ok(id)
    };
    let optional_node = |word| {
        if word == 0 {
            Ok(None)
        } else {
            node(word).map(Some)
        }
    };
    let optional_symbol = |word| {
        if word == 0 {
            Ok(None)
        } else {
            symbol(word).map(Some)
        }
    };
    let tree_claim = TreeClaim {
        _claim: Some(
            source
                .arena
                .identity_leases()
                .ok_or(ImageError("unpublished library source"))?
                .nodes
                .claim()
                .ok_or(ImageError("library tree is already bound"))?,
        ),
    };
    let mut symbols = SymbolArena::at_reserved(
        reservation.symbols.range().start(),
        reservation.symbols.range().len(),
    );
    let mut declaration_scratch = Vec::new();
    let mut tables = Vec::new();
    for _ in 0..symbol_count {
        let flags = SymbolFlags::from_bits(input.word()? as i32);
        let escaped_name = name(input.word()?)?;
        let count = input.count(4)?;
        let mut inline = [source.root; 8];
        let declarations = if count <= inline.len() {
            for entry in &mut inline[..count] {
                *entry = node(input.word()?)?;
            }
            &inline[..count]
        } else {
            declaration_scratch.clear();
            declaration_scratch.reserve(count);
            for _ in 0..count {
                declaration_scratch.push(node(input.word()?)?);
            }
            &declaration_scratch
        };
        let list = symbols.declarations_from(declarations);
        let value = optional_node(input.word()?)?;
        let parent = optional_symbol(input.word()?)?;
        let members = table(&mut input, &name, &symbol)?;
        let exports = table(&mut input, &name, &symbol)?;
        let global_exports = table(&mut input, &name, &symbol)?;
        let export_symbol = optional_symbol(input.word()?)?;
        let const_enum = input.word()?;
        let latches = input.word()?;
        if const_enum > 2 || latches > 3 {
            return Err(ImageError("invalid library symbol latches"));
        }
        let aliases = if latches & 2 != 0 {
            [input.word()?, input.word()?, input.word()?]
        } else {
            [0; 3]
        };
        let mut assignments = indexmap::IndexMap::new();
        pairs(&mut input, |a, b| {
            assignments.insert(node(a)?, node(b)?);
            Ok(())
        })?;
        let id = symbols.alloc(flags, escaped_name);
        let record = symbols.symbol_mut(id);
        record.set_declarations(list);
        record.value_declaration = value;
        record.parent = parent;
        if latches & 2 != 0 {
            let extras = record.extras_mut();
            extras.members = shared_table(members, aliases[0], &mut tables)?;
            extras.exports = shared_table(exports, aliases[1], &mut tables)?;
            extras.global_exports = shared_table(global_exports, aliases[2], &mut tables)?;
            extras.export_symbol = export_symbol;
            extras.const_enum_only_module = (const_enum != 0).then_some(const_enum == 2);
            extras.is_replaceable_by_method = latches & 1 != 0;
            extras.assignment_declaration_members = assignments;
        } else if !members.is_empty()
            || !exports.is_empty()
            || !global_exports.is_empty()
            || export_symbol.is_some()
            || const_enum != 0
            || latches != 0
            || !assignments.is_empty()
        {
            return Err(ImageError("absent library symbol extras have fields"));
        }
    }
    let local_count = input.count(8)?;
    let mut locals = FxHashMap::with_capacity_and_hasher(local_count, Default::default());
    for _ in 0..local_count {
        let id = node(input.word()?)?;
        if locals
            .insert(id, table(&mut input, &name, &symbol)?)
            .is_some()
        {
            return Err(ImageError("duplicate library locals table"));
        }
    }
    if input.count(4)? != 11 {
        return Err(ImageError("unknown binding side-map schema"));
    }
    let mut node_symbol = NodeSymbolMap::for_tree(&source.arena);
    pairs(&mut input, |a, b| {
        node_symbol.insert(node(a)?, symbol(b)?);
        Ok(())
    })?;
    let mut node_local_symbol = FxHashMap::default();
    pairs(&mut input, |a, b| {
        node_local_symbol.insert(node(a)?, symbol(b)?);
        Ok(())
    })?;
    let mut assigned_symbol_ids = FxHashMap::default();
    pairs(&mut input, |a, b| {
        if b >= serial_count {
            return Err(ImageError("assigned library serial outside reservation"));
        }
        assigned_symbol_ids.insert(symbol(a)?, serial_base + b);
        Ok(())
    })?;
    let mut next_container = FxHashMap::default();
    pairs(&mut input, |a, b| {
        next_container.insert(node(a)?, node(b)?);
        Ok(())
    })?;
    let mut node_flow = NodeFlowMap::for_tree(&source.arena);
    pairs(&mut input, |a, b| {
        node_flow.insert(node(a)?, flow_id(b)?);
        Ok(())
    })?;
    let mut node_end_flow = FxHashMap::default();
    pairs(&mut input, |a, b| {
        node_end_flow.insert(node(a)?, flow_id(b)?);
        Ok(())
    })?;
    let mut node_return_flow = FxHashMap::default();
    pairs(&mut input, |a, b| {
        node_return_flow.insert(node(a)?, flow_id(b)?);
        Ok(())
    })?;
    let mut node_flow_when_true = FxHashMap::default();
    pairs(&mut input, |a, b| {
        node_flow_when_true.insert(node(a)?, flow_id(b)?);
        Ok(())
    })?;
    let mut node_flow_when_false = FxHashMap::default();
    pairs(&mut input, |a, b| {
        node_flow_when_false.insert(node(a)?, flow_id(b)?);
        Ok(())
    })?;
    let mut possibly_exhaustive = FxHashMap::default();
    pairs(&mut input, |a, b| {
        if b > 1 {
            return Err(ImageError("invalid library exhaustive switch latch"));
        }
        possibly_exhaustive.insert(node(a)?, b != 0);
        Ok(())
    })?;
    let mut node_fallthrough_flow = FxHashMap::default();
    pairs(&mut input, |a, b| {
        node_fallthrough_flow.insert(node(a)?, flow_id(b)?);
        Ok(())
    })?;
    let count = input.count(4)?;
    let mut classifiable_names = EscapedNameSet::new();
    classifiable_names.reserve(count);
    for _ in 0..count {
        classifiable_names.insert(name(input.word()?)?);
    }
    let count = input.count(12)?;
    let mut pattern_ambient_modules = Vec::with_capacity(count);
    for _ in 0..count {
        let a = tsc_types::JsStr::try_from_canonical_bytes(input.bytes()?)
            .ok_or(ImageError("invalid library ambient pattern"))?
            .to_owned();
        let b = tsc_types::JsStr::try_from_canonical_bytes(input.bytes()?)
            .ok_or(ImageError("invalid library ambient pattern"))?
            .to_owned();
        pattern_ambient_modules.push((a, b, symbol(input.word()?)?));
    }
    let count = input.count(16)?;
    let mut flow = crate::flow::FlowArena::for_image(count);
    let mut pool = Vec::new();
    let mut flow_scratch = Vec::new();
    for _ in 0..count {
        let flags = tsc_types::FlowFlags::from_bits(input.word()? as i32);
        let antecedent = match input.word()? {
            0 => None,
            n => Some(flow_id(n)?),
        };
        let tag = input.word()?;
        let length = input.count(4)?;
        let data = match (tag, length) {
            (0, 0) => FlowData::None,
            (1, 1) => FlowData::Node(node(input.word()?)?),
            (2, 3) => FlowData::SwitchClause {
                switch_statement: node(input.word()?)?,
                clause_start: input.word()?,
                clause_end: input.word()?,
            },
            (3, 1..) | (4, _) => {
                let target = if tag == 3 {
                    Some(flow_id(input.word()?)?)
                } else {
                    None
                };
                flow_scratch.clear();
                for _ in usize::from(target.is_some())..length {
                    flow_scratch.push(flow_id(input.word()?)?);
                }
                if let Some(target) = target {
                    FlowData::ReduceLabel {
                        target,
                        antecedents: &flow_scratch,
                    }
                } else {
                    FlowData::Label {
                        antecedents: &flow_scratch,
                    }
                }
            }
            _ => return Err(ImageError("invalid library flow payload")),
        };
        flow.push_image(flags, antecedent, data, &mut pool)?;
    }
    input.end()?;
    let unreachable_flow = flow_id(unreachable)?;
    if max_flow.get() as usize > count {
        return Err(ImageError("library flow reference outside graph"));
    }
    let common_js_module_indicator = optional_node(common_js)?;
    let flow = flow.finish_image(pool);
    image.apply_binding_edits_to(&mut source)?;
    if !symbols
        .attach_reserved_identity_lease(reservation.symbols.clone())
        .map_err(|_| ImageError("invalid library symbol lease"))?
    {
        return Err(ImageError("library symbols overflowed reservation"));
    }
    let data = BindData {
        common_js_module_indicator,
        symbols,
        node_symbol,
        node_local_symbol,
        locals,
        bind_diagnostics: Vec::new(),
        classifiable_names,
        assigned_symbol_ids,
        private_name_serial_base: serial_base,
        next_symbol_id: next_serial,
        private_name_serial_lease: Some(reservation.serials.clone()),
        next_container,
        pattern_ambient_modules,
        flow,
        unreachable_flow,
        node_flow,
        node_end_flow,
        node_return_flow,
        node_flow_when_true,
        node_flow_when_false,
        possibly_exhaustive,
        node_fallthrough_flow,
        emit_flags,
        _tree_claim: tree_claim,
    };
    Ok((source, data))
}

//! Canonical completed binding records and direct runtime construction.
//! Side maps are ordered by file-local
//! key; language-level tables retain their insertion and declaration order.

use super::*;
use crate::flow::{FlowData, FlowId};
use tsc_syntax::library_image::{Decoder, Encoder, ImageError, SyntaxImage};

#[path = "library_image/runtime.rs"]
mod runtime;
pub use runtime::restore_reserved;

type Result<T> = std::result::Result<T, ImageError>;
const MAGIC: &[u8; 8] = b"TSCLIBB3";

fn provenance() -> u64 {
    include!(concat!(env!("OUT_DIR"), "/library_image_schema.rs"))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ImageSymbol {
    flags: u32,
    name: u32,
    declarations: Vec<u32>,
    value: u32,
    parent: u32,
    members: Vec<[u32; 2]>,
    exports: Vec<[u32; 2]>,
    global_exports: Vec<[u32; 2]>,
    export_symbol: u32,
    const_enum: u32,
    replaceable: bool,
    extras_present: bool,
    table_aliases: [u32; 3],
    assignments: Vec<[u32; 2]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ImageFlow {
    flags: u32,
    antecedent: u32,
    tag: u32,
    words: Vec<u32>,
}

/// Logical bind objects. Capabilities and backing capacities are deliberately
/// absent; no restored BindData is published by this proof interface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BindingImage {
    names: Vec<Vec<u8>>,
    symbols: Vec<ImageSymbol>,
    locals: Vec<(u32, Vec<[u32; 2]>)>,
    maps: Vec<Vec<[u32; 2]>>,
    classifiable: Vec<u32>,
    patterns: Vec<(Vec<u8>, Vec<u8>, u32)>,
    flows: Vec<ImageFlow>,
    common_js: u32,
    unreachable: u32,
    emit_flags: u32,
    serial_count: u32,
}

struct Names {
    values: Vec<Vec<u8>>,
    indices: FxHashMap<EscapedName, u32>,
    serials: PrivateNameSerialRelocation,
}

impl Names {
    fn get(&mut self, mut name: EscapedName) -> Result<u32> {
        self.serials
            .name(&mut name)
            .map_err(|_| ImageError("invalid image private name"))?;
        Ok(*self.indices.entry(name).or_insert_with(|| {
            let index = self.values.len() as u32;
            self.values.push(name.as_js().as_bytes().to_vec());
            index
        }))
    }
    fn table(
        &mut self,
        table: &SymbolTable,
        symbol: impl Fn(SymbolId) -> Result<u32>,
    ) -> Result<Vec<[u32; 2]>> {
        table
            .iter()
            .map(|(name, &id)| Ok([self.get(*name)?, symbol(id)?]))
            .collect()
    }
}

fn pairs(values: impl IntoIterator<Item = Result<[u32; 2]>>) -> Result<Vec<[u32; 2]>> {
    let mut values = values.into_iter().collect::<Result<Vec<_>>>()?;
    values.sort_unstable_by_key(|pair| pair[0]);
    Ok(values)
}

fn relative(value: u32, base: u32, count: usize) -> Result<u32> {
    let index = value
        .checked_sub(base)
        .ok_or(ImageError("bind image reference below file base"))?;
    if index as usize >= count {
        return Err(ImageError("bind image reference outside file"));
    }
    Ok(index + 1)
}

impl BindingImage {
    pub fn capture(source: &SourceFile, data: &BindData) -> Result<Self> {
        // A new completed field must be explicitly accounted for here.
        let BindData {
            common_js_module_indicator,
            symbols,
            node_symbol,
            node_local_symbol,
            locals,
            bind_diagnostics,
            classifiable_names,
            assigned_symbol_ids,
            private_name_serial_base,
            next_symbol_id,
            private_name_serial_lease: _,
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
            _tree_claim: _,
        } = data;
        if !bind_diagnostics.is_empty() {
            return Err(ImageError("unsupported library bind diagnostics"));
        }
        let serial_count = next_symbol_id
            .checked_sub(*private_name_serial_base)
            .ok_or(ImageError("invalid image serial range"))?;
        let serials = PrivateNameSerialRelocation {
            old: IdentityRange::new(*private_name_serial_base, *next_symbol_id),
            new: IdentityRange::new(0, serial_count),
        };
        let node = |id: NodeId| relative(id.index(), source.arena.node_base(), source.arena.len());
        let symbol = |id: SymbolId| relative(id.index(), symbols.base(), symbols.len());
        let flow_id = |id: FlowId| relative(id.index(), 0, flow.len());
        let mut names = Names {
            values: Vec::new(),
            indices: FxHashMap::default(),
            serials,
        };
        let mut image_symbols = Vec::with_capacity(symbols.len());
        // Alias classes follow first use, including empty tables; completed
        // data can be captured on a different thread from its binder.
        let mut table_aliases = FxHashMap::default();
        for s in symbols.symbols() {
            let crate::symbols::SymbolExtras {
                members,
                exports,
                global_exports,
                export_symbol,
                const_enum_only_module,
                is_replaceable_by_method,
                assignment_declaration_members,
            } = s.extras();
            image_symbols.push(ImageSymbol {
                flags: s.flags.bits() as u32,
                name: names.get(s.escaped_name)?,
                declarations: s
                    .declarations()
                    .iter()
                    .copied()
                    .map(node)
                    .collect::<Result<_>>()?,
                value: s.value_declaration.map(node).transpose()?.unwrap_or(0),
                parent: s.parent.map(symbol).transpose()?.unwrap_or(0),
                members: names.table(members, symbol)?,
                exports: names.table(exports, symbol)?,
                global_exports: names.table(global_exports, symbol)?,
                export_symbol: export_symbol.map(symbol).transpose()?.unwrap_or(0),
                const_enum: const_enum_only_module.map_or(0, |value| u32::from(value) + 1),
                replaceable: *is_replaceable_by_method,
                extras_present: s.tables().next().is_some(),
                table_aliases: if s.tables().next().is_some() {
                    [members, exports, global_exports].map(|table| {
                        let next = table_aliases.len() as u32 + 1;
                        *table_aliases
                            .entry(std::sync::Arc::as_ptr(table))
                            .or_insert(next)
                    })
                } else {
                    [0; 3]
                },
                assignments: assignment_declaration_members
                    .iter()
                    .map(|(&a, &b)| Ok([node(a)?, node(b)?]))
                    .collect::<Result<_>>()?,
            });
        }
        let mut image_locals: Vec<_> = locals
            .iter()
            .map(|(&id, table)| Ok((node(id)?, table)))
            .collect::<Result<_>>()?;
        image_locals.sort_unstable_by_key(|&(id, _)| id);
        let image_locals = image_locals
            .into_iter()
            .map(|(id, table)| Ok((id, names.table(table, symbol)?)))
            .collect::<Result<_>>()?;
        let maps = vec![
            pairs(node_symbol.iter().map(|(a, b)| Ok([node(a)?, symbol(b)?])))?,
            pairs(
                node_local_symbol
                    .iter()
                    .map(|(&a, &b)| Ok([node(a)?, symbol(b)?])),
            )?,
            pairs(assigned_symbol_ids.iter().map(|(&a, &b)| {
                let mut serial = b;
                serials
                    .serial(&mut serial)
                    .map_err(|_| ImageError("invalid assigned image serial"))?;
                Ok([symbol(a)?, serial])
            }))?,
            pairs(
                next_container
                    .iter()
                    .map(|(&a, &b)| Ok([node(a)?, node(b)?])),
            )?,
            pairs(node_flow.iter().map(|(a, b)| Ok([node(a)?, flow_id(b)?])))?,
            pairs(
                node_end_flow
                    .iter()
                    .map(|(&a, &b)| Ok([node(a)?, flow_id(b)?])),
            )?,
            pairs(
                node_return_flow
                    .iter()
                    .map(|(&a, &b)| Ok([node(a)?, flow_id(b)?])),
            )?,
            pairs(
                node_flow_when_true
                    .iter()
                    .map(|(&a, &b)| Ok([node(a)?, flow_id(b)?])),
            )?,
            pairs(
                node_flow_when_false
                    .iter()
                    .map(|(&a, &b)| Ok([node(a)?, flow_id(b)?])),
            )?,
            pairs(
                possibly_exhaustive
                    .iter()
                    .map(|(&a, &b)| Ok([node(a)?, u32::from(b)])),
            )?,
            pairs(
                node_fallthrough_flow
                    .iter()
                    .map(|(&a, &b)| Ok([node(a)?, flow_id(b)?])),
            )?,
        ];
        let classifiable = classifiable_names
            .iter()
            .map(|&name| names.get(name))
            .collect::<Result<_>>()?;
        let patterns = pattern_ambient_modules
            .iter()
            .map(|(a, b, id)| {
                Ok((
                    a.as_js().as_bytes().to_vec(),
                    b.as_js().as_bytes().to_vec(),
                    symbol(*id)?,
                ))
            })
            .collect::<Result<_>>()?;
        let mut flows = Vec::with_capacity(flow.len());
        for index in 0..flow.len() {
            let id = FlowId::new(index as u32);
            let record = flow.flow(id);
            let (tag, words) = match flow.payload(id) {
                FlowData::None => (0, vec![]),
                FlowData::Node(id) => (1, vec![node(id)?]),
                FlowData::SwitchClause {
                    switch_statement,
                    clause_start,
                    clause_end,
                } => (2, vec![node(switch_statement)?, clause_start, clause_end]),
                FlowData::ReduceLabel {
                    target,
                    antecedents,
                } => {
                    let mut words = vec![flow_id(target)?];
                    words.extend(
                        antecedents
                            .iter()
                            .copied()
                            .map(flow_id)
                            .collect::<Result<Vec<_>>>()?,
                    );
                    (3, words)
                }
                FlowData::Label { antecedents } => (
                    4,
                    antecedents
                        .iter()
                        .copied()
                        .map(flow_id)
                        .collect::<Result<_>>()?,
                ),
            };
            flows.push(ImageFlow {
                flags: record.flags.bits() as u32,
                antecedent: record.antecedent().map(flow_id).transpose()?.unwrap_or(0),
                tag,
                words,
            });
        }
        Ok(Self {
            names: names.values,
            symbols: image_symbols,
            locals: image_locals,
            maps,
            classifiable,
            patterns,
            flows,
            common_js: common_js_module_indicator
                .map(node)
                .transpose()?
                .unwrap_or(0),
            unreachable: flow_id(*unreachable_flow)?,
            emit_flags: *emit_flags as u32,
            serial_count,
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut out = Encoder::default();
        out.word(self.common_js);
        out.word(self.unreachable);
        out.word(self.emit_flags);
        out.word(self.serial_count);
        out.count(self.names.len())?;
        for name in &self.names {
            out.bytes(name)?;
        }
        out.count(self.symbols.len())?;
        for s in &self.symbols {
            out.word(s.flags);
            out.word(s.name);
            out.words(&s.declarations)?;
            out.word(s.value);
            out.word(s.parent);
            for table in [&s.members, &s.exports, &s.global_exports] {
                write_pairs(&mut out, table)?;
            }
            out.word(s.export_symbol);
            out.word(s.const_enum);
            out.word(u32::from(s.replaceable) | (u32::from(s.extras_present) << 1));
            if s.extras_present {
                for alias in s.table_aliases {
                    out.word(alias);
                }
            }
            write_pairs(&mut out, &s.assignments)?;
        }
        out.count(self.locals.len())?;
        for (node, table) in &self.locals {
            out.word(*node);
            write_pairs(&mut out, table)?;
        }
        out.count(self.maps.len())?;
        for map in &self.maps {
            write_pairs(&mut out, map)?;
        }
        out.words(&self.classifiable)?;
        out.count(self.patterns.len())?;
        for (a, b, id) in &self.patterns {
            out.bytes(a)?;
            out.bytes(b)?;
            out.word(*id);
        }
        out.count(self.flows.len())?;
        for flow in &self.flows {
            out.word(flow.flags);
            out.word(flow.antecedent);
            out.word(flow.tag);
            out.words(&flow.words)?;
        }
        out.finish(MAGIC, provenance())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut input = Decoder::open(bytes, MAGIC, provenance())?;
        let common_js = input.word()?;
        let unreachable = input.word()?;
        let emit_flags = input.word()?;
        let serial_count = input.word()?;
        let mut names = Vec::new();
        for _ in 0..input.count(4)? {
            let bytes = input.bytes()?;
            if tsc_types::JsStr::try_from_canonical_bytes(bytes).is_none() {
                return Err(ImageError("invalid bind name text"));
            }
            names.push(bytes.to_vec());
        }
        let mut symbols = Vec::new();
        for _ in 0..input.count(48)? {
            let flags = input.word()?;
            let name = input.word()?;
            let declarations = input.words()?;
            let value = input.word()?;
            let parent = input.word()?;
            let members = read_pairs(&mut input)?;
            let exports = read_pairs(&mut input)?;
            let global_exports = read_pairs(&mut input)?;
            let export_symbol = input.word()?;
            let const_enum = input.word()?;
            let replaceable = input.word()?;
            if name as usize >= names.len() || const_enum > 2 || replaceable > 3 {
                return Err(ImageError("invalid bind symbol header"));
            }
            let table_aliases = if replaceable & 2 != 0 {
                [input.word()?, input.word()?, input.word()?]
            } else {
                [0; 3]
            };
            let assignments = read_pairs(&mut input)?;
            if replaceable & 2 == 0
                && (!members.is_empty()
                    || !exports.is_empty()
                    || !global_exports.is_empty()
                    || export_symbol != 0
                    || const_enum != 0
                    || replaceable & 1 != 0
                    || !assignments.is_empty())
            {
                return Err(ImageError("absent symbol extras have nondefault fields"));
            }
            symbols.push(ImageSymbol {
                flags,
                name,
                declarations,
                value,
                parent,
                members,
                exports,
                global_exports,
                export_symbol,
                const_enum,
                replaceable: replaceable & 1 != 0,
                extras_present: replaceable & 2 != 0,
                table_aliases,
                assignments,
            });
        }
        let mut locals = Vec::new();
        for _ in 0..input.count(8)? {
            locals.push((input.word()?, read_pairs(&mut input)?));
        }
        let mut maps = Vec::new();
        for _ in 0..input.count(4)? {
            maps.push(read_pairs(&mut input)?);
        }
        if maps.len() != 11 {
            return Err(ImageError("unknown bind side-map schema"));
        }
        let classifiable = input.words()?;
        let mut patterns = Vec::new();
        for _ in 0..input.count(12)? {
            patterns.push((
                input.bytes()?.to_vec(),
                input.bytes()?.to_vec(),
                input.word()?,
            ));
        }
        let mut flows = Vec::new();
        for _ in 0..input.count(16)? {
            let flags = input.word()?;
            let antecedent = input.word()?;
            let tag = input.word()?;
            let words = input.words()?;
            if tag > 4
                || (tag == 0 && !words.is_empty())
                || (tag == 1 && words.len() != 1)
                || (tag == 2 && words.len() != 3)
                || (tag == 3 && words.is_empty())
            {
                return Err(ImageError("invalid bind flow payload"));
            }
            flows.push(ImageFlow {
                flags,
                antecedent,
                tag,
                words,
            });
        }
        input.end()?;
        let mut tables: Vec<&[[u32; 2]]> = Vec::new();
        for symbol in &symbols {
            if !symbol.extras_present {
                continue;
            }
            for (alias, entries) in symbol.table_aliases.into_iter().zip([
                &symbol.members,
                &symbol.exports,
                &symbol.global_exports,
            ]) {
                if alias == 0 {
                    return Err(ImageError("present bind table has no alias class"));
                }
                let index = alias as usize - 1;
                if index == tables.len() {
                    tables.push(entries);
                } else if tables.get(index).copied() != Some(entries.as_slice()) {
                    return Err(ImageError("invalid bind table alias"));
                }
            }
        }
        Ok(Self {
            names,
            symbols,
            locals,
            maps,
            classifiable,
            patterns,
            flows,
            common_js,
            unreachable,
            emit_flags,
            serial_count,
        })
    }
}

fn write_pairs(out: &mut Encoder, pairs: &[[u32; 2]]) -> Result<()> {
    out.count(pairs.len())?;
    for &[a, b] in pairs {
        out.word(a);
        out.word(b);
    }
    Ok(())
}
fn read_pairs(input: &mut Decoder<'_>) -> Result<Vec<[u32; 2]>> {
    let count = input.count(8)?;
    (0..count)
        .map(|_| Ok([input.word()?, input.word()?]))
        .collect()
}

/// Build artifacts using the actual parser and binder. Nothing in the
/// command loader calls this; a generator owns the only image-writing path.
pub fn build_library_image(
    name: &str,
    text: &str,
    mode: tsc_syntax::JSDocParsingMode,
) -> Result<(SyntaxImage, SyntaxImage, BindingImage)> {
    let mut source = tsc_syntax::parse_source_file(
        name,
        text,
        tsc_syntax::ParseOptions {
            js_doc_parsing_mode: mode,
            ..Default::default()
        },
        None,
    );
    let parsed = SyntaxImage::capture(&source)?;
    let bound = BinderWorker::bind_local(&mut source).into_bind_data();
    Ok((
        parsed,
        SyntaxImage::capture(&source)?,
        BindingImage::capture(&source, &bound)?,
    ))
}

/// The three sections the compiler's host build embeds for command parsing.
pub struct CommandLibraryImage {
    pub syntax: Vec<u8>,
    pub binding: Vec<u8>,
    pub edits: Vec<u8>,
}

pub fn build_command_library_image(name: &str, text: &str) -> Result<CommandLibraryImage> {
    let (parsed, bound, binding) =
        build_library_image(name, text, tsc_syntax::JSDocParsingMode::ParseForTypeErrors)?;
    Ok(CommandLibraryImage {
        syntax: parsed.encode()?,
        binding: binding.encode()?,
        edits: parsed.encode_binding_edits(&bound)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsc_syntax::{JSDocParsingMode, ParseOptions};

    #[test]
    fn completed_bind_alias_classes_do_not_depend_on_the_capturing_thread() {
        let text = "interface CrossThreadAlias { field: string }";
        let (source, data) = std::thread::spawn(move || {
            let mut source = tsc_syntax::parse_source_file(
                "alias-thread.d.ts",
                text,
                ParseOptions::default(),
                None,
            );
            let data = BinderWorker::bind_local(&mut source).into_bind_data();
            (source, data)
        })
        .join()
        .unwrap();
        let captured = BindingImage::capture(&source, &data).unwrap();
        let mut ordinary =
            tsc_syntax::parse_source_file("alias-thread.d.ts", text, ParseOptions::default(), None);
        let expected = BinderWorker::bind_local(&mut ordinary).into_bind_data();
        assert_eq!(
            captured,
            BindingImage::capture(&ordinary, &expected).unwrap()
        );
    }

    #[test]
    fn generated_binding_provenance_equals_all_source_inputs() {
        fn collect(directory: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(directory).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    collect(&path, files);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    files.push(path);
                }
            }
        }
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = vec![directory.join("build.rs")];
        collect(&directory.join("src"), &mut files);
        files.sort();
        let mut hash = 0xcbf29ce484222325u64;
        for path in files {
            for byte in std::fs::read(path).unwrap() {
                hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            }
        }
        assert_eq!(provenance(), hash);
    }

    #[test]
    fn every_bundled_library_image_equals_ordinary_syntax_and_binding() {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../vendor/typescript-native/7.1.0-dev-aa814927/upstream/tsc/internal/bundled/libs",
        );
        let mut files: Vec<_> = std::fs::read_dir(directory)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "ts"))
            .collect();
        files.sort();
        assert_eq!(files.len(), 116);
        for path in files {
            let name = format!(
                "bundled:///libs/{}",
                path.file_name().unwrap().to_str().unwrap()
            );
            let text = std::fs::read_to_string(&path).unwrap();
            for mode in [
                JSDocParsingMode::ParseAll,
                JSDocParsingMode::ParseNone,
                JSDocParsingMode::ParseForTypeErrors,
                JSDocParsingMode::ParseForTypeInfo,
            ] {
                let (parsed, bound_tree, binding) = build_library_image(&name, &text, mode)
                    .unwrap_or_else(|e| panic!("{name}, {mode:?}: {e}"));
                let encoded = parsed.encode().unwrap();
                let decoded = SyntaxImage::decode(&encoded, &text).unwrap();
                let mut ordinary = tsc_syntax::parse_source_file(
                    name.as_str(),
                    text.as_str(),
                    ParseOptions {
                        js_doc_parsing_mode: mode,
                        ..Default::default()
                    },
                    None,
                );
                assert_eq!(
                    ordinary,
                    decoded.restore().unwrap(),
                    "parsed {name}, {mode:?}"
                );
                let data = BinderWorker::bind_local(&mut ordinary).into_bind_data();
                assert_eq!(
                    ordinary,
                    SyntaxImage::decode(&bound_tree.encode().unwrap(), &text)
                        .unwrap()
                        .restore()
                        .unwrap(),
                    "bound tree {name}, {mode:?}"
                );
                let edits = parsed.binding_flag_edits(&bound_tree).unwrap();
                let mut edited = parsed.clone();
                edited.apply_binding_flags(&edits).unwrap();
                assert_eq!(bound_tree, edited, "binding edits {name}, {mode:?}");
                let mut edited = parsed.clone();
                edited
                    .apply_binding_edits(&parsed.encode_binding_edits(&bound_tree).unwrap())
                    .unwrap();
                assert_eq!(bound_tree, edited, "encoded binding edits {name}, {mode:?}");
                let encoded = binding.encode().unwrap();
                let decoded = BindingImage::decode(&encoded).unwrap();
                assert_eq!(
                    BindingImage::capture(&ordinary, &data).unwrap(),
                    decoded,
                    "binding {name}, {mode:?}"
                );
                assert_eq!(
                    encoded,
                    decoded.encode().unwrap(),
                    "determinism {name}, {mode:?}"
                );
                // The portable form must not depend on node/array bases.
                let mut moved = tsc_syntax::parse_source_file(
                    name.as_str(),
                    text.as_str(),
                    ParseOptions {
                        node_id_base: 701,
                        node_array_id_base: 911,
                        js_doc_parsing_mode: mode,
                        ..Default::default()
                    },
                    None,
                );
                assert_eq!(
                    parsed,
                    SyntaxImage::capture(&moved).unwrap(),
                    "bases {name}, {mode:?}"
                );
                let mut worker = BinderWorker::with_bases(&mut moved, 4321, 1201);
                worker.bind_source_file();
                let moved_bind = worker.into_bind_data();
                assert_eq!(
                    binding,
                    BindingImage::capture(&moved, &moved_bind).unwrap(),
                    "symbol bases {name}, {mode:?}"
                );
            }
        }
    }

    #[test]
    fn bind_image_checks_provenance_lengths_and_payload_integrity() {
        let (_, _, image) = build_library_image(
            "lib.test.d.ts",
            "interface A { x: string; f(a: number): void }",
            JSDocParsingMode::ParseForTypeErrors,
        )
        .unwrap();
        let bytes = image.encode().unwrap();
        assert_eq!(image, BindingImage::decode(&bytes).unwrap());
        for position in [0, 8, 12, 16, 24, 32, bytes.len() - 1] {
            let mut corrupt = bytes.clone();
            corrupt[position] ^= 1;
            assert!(BindingImage::decode(&corrupt).is_err(), "byte {position}");
        }
        for length in [0, 7, 31, bytes.len() - 1] {
            assert!(BindingImage::decode(&bytes[..length]).is_err());
        }
        let mut extra = bytes;
        extra.push(0);
        assert!(BindingImage::decode(&extra).is_err());
        for bad_alias in [0, u32::MAX] {
            let mut corrupt = image.clone();
            let symbol = corrupt
                .symbols
                .iter_mut()
                .find(|symbol| !symbol.members.is_empty())
                .unwrap();
            symbol.table_aliases[0] = bad_alias;
            assert!(BindingImage::decode(&corrupt.encode().unwrap()).is_err());
        }
    }
}

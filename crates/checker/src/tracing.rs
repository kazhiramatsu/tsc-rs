//! The checker's part of `--generateTrace` (tsgo `checker/tracer.go`): the
//! `checkSourceFiles` phase and, when a checker ends, its types file: one
//! descriptor per type in the checker's type arena, in creation order. The
//! descriptors are tsgo's (`tracing.TypeDescriptor`); the ids are this
//! checker's own (`TypeId` index + 1, as tsgo's ids start at 1), so they
//! follow this port's type creation order rather than tsgo's.

use std::panic::{catch_unwind, AssertUnwindSafe};

use tsc_syntax::NodeId;
use tsc_types::tracing::{
    format_type_flags, Args, LineAndChar, Location, Phase, Sample, Span, TypeDescriptor,
    TypesFileWriter,
};
use tsc_types::{InternalSymbolName, ObjectFlags, SymbolId, TypeData, TypeFlags, TypeId};

use crate::engine::RecursionIdentity;
use crate::state::CheckerState;

/// tsgo `TypeFlagsIntrinsic`: the types that carry an intrinsic name (the
/// boolean literals are literal types in tsgo).
const INTRINSIC_FLAGS: i32 = TypeFlags::ANY.bits()
    | TypeFlags::UNKNOWN.bits()
    | TypeFlags::STRING.bits()
    | TypeFlags::NUMBER.bits()
    | TypeFlags::BIG_INT.bits()
    | TypeFlags::ES_SYMBOL.bits()
    | TypeFlags::VOID.bits()
    | TypeFlags::UNDEFINED.bits()
    | TypeFlags::NULL.bits()
    | TypeFlags::NEVER.bits()
    | TypeFlags::NON_PRIMITIVE.bits();

/// The types tsgo writes a display text for: anonymous objects and the
/// literal, template literal, union and intersection types.
const DISPLAYED_FLAGS: i32 = TypeFlags::STRING_LITERAL.bits()
    | TypeFlags::NUMBER_LITERAL.bits()
    | TypeFlags::BIG_INT_LITERAL.bits()
    | TypeFlags::BOOLEAN_LITERAL.bits()
    | TypeFlags::TEMPLATE_LITERAL.bits()
    | TypeFlags::UNION.bits()
    | TypeFlags::INTERSECTION.bits();

fn trace_id(ty: TypeId) -> u32 {
    ty.index() + 1
}

fn trace_ids(types: &[TypeId]) -> Vec<u32> {
    types.iter().copied().map(trace_id).collect()
}

/// The trace id of a type in this checker (see the module docs).
pub(crate) fn trace_type_id(ty: TypeId) -> u32 {
    trace_id(ty)
}

impl CheckerState<'_> {
    /// An instant of the `checkTypes` phase where the checker hit a depth
    /// limit (tsgo `Tracer.Instant`).
    pub(crate) fn trace_instant(&self, name: &'static str, args: impl FnOnce() -> Args) {
        if let Some(tracer) = &self.tracer {
            tracer.instant(Phase::CheckTypes, name, args());
        }
    }

    /// A sampled event (tsgo `Tracer.Push` without `separateBeginAndEnd`):
    /// none in a deterministic session.
    pub(crate) fn trace_sample(
        &self,
        phase: Phase,
        name: &'static str,
        args: impl FnOnce() -> Args,
    ) -> Option<Sample> {
        let tracer = self.tracer.as_ref()?;
        Some(tracer.sample(phase, name, args))
    }

    /// A sampled event of a node's check with tsgo's arguments: the node's
    /// kind, its range and its file.
    pub(crate) fn trace_node_sample(&self, name: &'static str, node: NodeId) -> Option<Sample> {
        self.trace_sample(Phase::Check, name, || {
            let source = self.binder.source_of_node(node);
            let record = source.arena.node(node);
            Args::new()
                .with("kind", record.kind as u32)
                .with("pos", record.pos)
                .with("end", record.end)
                .with("path", source.file_name.to_string_lossy().into_owned())
        })
    }

    /// A `checkSourceFiles` span on the main thread around this checker's
    /// check of the program's files (tsgo's `emitFilesAndReportErrors`
    /// around the semantic getter).
    pub(crate) fn trace_check_phase(&self) -> Option<Span> {
        self.tracer.as_ref().map(|tracer| {
            tracer
                .tracing()
                .begin(Phase::Check, "checkSourceFiles", Args::new())
        })
    }

    /// Stores this checker's types file in its session (tsgo `DumpTypes`).
    /// The types the descriptors' display creates are not described.
    pub(crate) fn finish_trace(&mut self) {
        let Some(tracer) = self.tracer.take() else {
            return;
        };
        let count = self.tables.len();
        let mut writer = TypesFileWriter::new();
        let mut recursion_ids = rustc_hash::FxHashMap::<RecursionIdentity, usize>::default();
        for index in 0..count {
            let ty = TypeId::new(index as u32);
            let descriptor = self.trace_type_descriptor(ty, &mut recursion_ids);
            writer.push(&descriptor);
        }
        if let Some(text) = writer.finish() {
            tracer.set_types(text);
        }
    }

    /// tsgo `buildTypeDescriptor` over the adapter of checker/tracer.go.
    fn trace_type_descriptor(
        &mut self,
        ty: TypeId,
        recursion_ids: &mut rustc_hash::FxHashMap<RecursionIdentity, usize>,
    ) -> TypeDescriptor {
        let record = self.tables.type_of(ty).clone();
        let flags = record.flags;
        let object_flags = record.object_flags;
        let mut descriptor = TypeDescriptor {
            id: trace_id(ty),
            flags: format_type_flags(flags.bits()),
            ..TypeDescriptor::default()
        };
        // A token per recursion identity, in the order identities appear.
        if let Ok(identity) = self.get_recursion_identity(ty) {
            let next = recursion_ids.len();
            descriptor.recursion_id = Some(*recursion_ids.entry(identity).or_insert(next));
        }
        if flags.bits() & INTRINSIC_FLAGS != 0 {
            if let TypeData::Intrinsic { name, .. } = &record.data {
                descriptor.intrinsic_name = Some((*name).to_owned());
            }
        }
        let named = record.alias_symbol.or(record.symbol);
        if let Some(symbol) = named {
            descriptor.symbol_name = Some(self.trace_symbol_name(symbol));
        }
        descriptor.is_tuple = object_flags.intersects(ObjectFlags::TUPLE);
        match &record.data {
            TypeData::Union { types, .. } if flags.intersects(TypeFlags::UNION) => {
                descriptor.union_types = trace_ids(types);
            }
            TypeData::Intersection { types } if flags.intersects(TypeFlags::INTERSECTION) => {
                descriptor.intersection_types = trace_ids(types);
            }
            TypeData::Index { ty: target, .. } => {
                descriptor.keyof_type = Some(trace_id(*target));
            }
            TypeData::IndexedAccess {
                object_type,
                index_type,
                ..
            } => {
                descriptor.indexed_access_object_type = Some(trace_id(*object_type));
                descriptor.indexed_access_index_type = Some(trace_id(*index_type));
            }
            TypeData::Conditional(data) => {
                descriptor.conditional_check_type = Some(trace_id(data.check_type));
                descriptor.conditional_extends_type = Some(trace_id(data.extends_type));
                let cold = self.links.type_cold();
                let resolved = |slot: &crate::links::LinkSlot<TypeId>| {
                    slot.resolved()
                        .map_or(-1, |branch| i64::from(trace_id(branch)))
                };
                descriptor.conditional_true_type =
                    Some(resolved(cold.conditional_true_type.get(ty)));
                descriptor.conditional_false_type =
                    Some(resolved(cold.conditional_false_type.get(ty)));
            }
            TypeData::Substitution(data) => {
                descriptor.substitution_base_type = Some(trace_id(data.base_type));
                descriptor.constraint_type = Some(trace_id(data.constraint));
            }
            TypeData::ReverseMapped(data) => {
                descriptor.reverse_mapped_source_type = Some(trace_id(data.source));
                descriptor.reverse_mapped_mapped_type = Some(trace_id(data.mapped_type));
                descriptor.reverse_mapped_constraint_type = Some(trace_id(data.constraint_type));
            }
            TypeData::EvolvingArray { element_type } => {
                descriptor.evolving_array_element_type = Some(trace_id(*element_type));
                descriptor.evolving_array_final_type =
                    self.final_array_types.get(&ty).copied().map(trace_id);
            }
            _ => {}
        }
        if let Some(arguments) = &record.alias_type_arguments {
            descriptor.alias_type_arguments = trace_ids(arguments);
        }
        // A reference's target and its resolved arguments (a target is a
        // reference to itself over its type parameters).
        if flags.intersects(TypeFlags::OBJECT) && object_flags.intersects(ObjectFlags::REFERENCE) {
            descriptor.instantiated_type = Some(trace_id(self.tables.reference_target(ty)));
            // A deferred reference whose arguments were never resolved has
            // none (tsgo `resolvedTypeArguments`).
            descriptor.type_arguments = self
                .tables
                .try_type_arguments(ty)
                .map(trace_ids)
                .unwrap_or_default();
            if let Some(node) = self.links.ty(ty).deferred_node {
                descriptor.reference_location = self.trace_location(node);
            }
        }
        if let Some(pattern) = *self.links.type_cold().pattern.get(ty) {
            descriptor.destructuring_pattern = self.trace_location(pattern);
        }
        if let Some(symbol) = named {
            if let Some(&declaration) = self.binder.symbol(symbol).declarations.first() {
                descriptor.first_declaration = self.trace_location(declaration);
            }
        }
        if object_flags.intersects(ObjectFlags::ANONYMOUS) || flags.bits() & DISPLAYED_FLAGS != 0 {
            // tsgo recovers from a display that fails on an incomplete type.
            descriptor.display = catch_unwind(AssertUnwindSafe(|| self.type_to_string(ty)))
                .ok()
                .and_then(Result::ok)
                .map(|text| text.to_string_lossy().into_owned());
        }
        descriptor
    }

    /// tsgo `EscapeAllInternalSymbolNames(symbol.Name)`.
    fn trace_symbol_name(&self, symbol: SymbolId) -> String {
        let name = self
            .binder
            .symbol(symbol)
            .escaped_name
            .unescape()
            .to_string_lossy()
            .into_owned();
        name.replace(InternalSymbolName::LATE_BOUND_PREFIX, "__@")
    }

    /// tsgo `getLocation`: the node's file as a case-insensitive path, and
    /// its range from its first token, 1-based in lines and UTF-16
    /// characters.
    fn trace_location(&self, node: NodeId) -> Option<Location> {
        let file = self.binder.try_file_index_of_node(node)?;
        let source = self.binder.source(file);
        let raw = source.arena.node(node);
        let start = tsc_syntax::skip_trivia(source.text(), raw.pos as usize);
        let positions = source.positions();
        let start = positions.line_and_character_byte(u32::try_from(start).ok()?)?;
        let end = positions.line_and_character_byte(raw.end)?;
        Some(Location {
            path: tsc_program::to_file_name_lower_case_js(source.file_name.as_js())
                .to_string_lossy()
                .into_owned(),
            start: LineAndChar {
                line: start.line + 1,
                character: start.character + 1,
            },
            end: LineAndChar {
                line: end.line + 1,
                character: end.character + 1,
            },
        })
    }
}

impl Drop for CheckerState<'_> {
    fn drop(&mut self) {
        if self.tracer.is_some() && !std::thread::panicking() {
            self.finish_trace();
        }
    }
}

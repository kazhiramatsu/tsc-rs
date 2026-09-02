use std::collections::{HashMap, HashSet};

use tsc_binder::SymbolId;
use tsc_emitter::{
    EmitInternalNodeBuilderFlags, EmitNodeBuilderFlags, EmitResolverError, EmitSymbolExpansionOut,
    EmitSymbolMeaning, EmitSymbolTracker, TransformArena, TransformNode, TransformSourceId,
};
use tsc_syntax::{NodeId, SyntaxKind};
use tsc_types::{MapperId, ObjectFlags, TypeId};

use crate::program::ProgramFileId;
use crate::state::CheckerState;

use super::tracker::NodeBuilderTracker;

/// `_tsc.js`'s two truncation limits, kept as `u32` with the context's
/// `approximateLength` accumulator.
/// tsc-port: defaultMaximumTruncationLength/noTruncationMaximumTruncationLength @6.0.3
/// tsc-hash: 88f7e6242efb8ce661e6956b8e684acb4c4be9ce36b893363ef93a11b673cb6c
/// tsc-span: _tsc.js:12640-12641
pub(crate) const DEFAULT_MAXIMUM_TRUNCATION_LENGTH: u32 = 160;
pub(crate) const NO_TRUNCATION_MAXIMUM_TRUNCATION_LENGTH: u32 = 1_000_000;

pub(crate) type TrackedSymbol = (SymbolId, Option<NodeId>, EmitSymbolMeaning);
pub(crate) type RecoveryTrackedSymbol = (
    SymbolId,
    tsc_types::SymbolFlags,
    Option<NodeId>,
    bool,
    EmitSymbolMeaning,
    bool,
);

/// The explicit emit-channel NodeBuilder state. This is deliberately separate
/// from `CheckerState::slice_*`, whose fields belong to the display channel.
///
/// `Option<TypeId>` in `type_stack` is the typed Rust spelling of upstream's
/// `type.id | -1` stack: `None` is the `-1` declaration-serialization marker.
pub(crate) struct NodeBuilderContext<'tracker> {
    pub(crate) enclosing_declaration: Option<NodeId>,
    /// Upstream can replace `enclosingDeclaration` with a synthesized Block
    /// carrying the temporary parameter/type-parameter locals for a reused
    /// signature. Rust keeps those locals in context maps, so this bit retains
    /// the synthesized identity observed by tracker callbacks.
    pub(crate) enclosing_declaration_is_synthetic: bool,
    pub(crate) enclosing_file: Option<NodeId>,
    pub(crate) flags: EmitNodeBuilderFlags,
    pub(crate) internal_flags: EmitInternalNodeBuilderFlags,
    pub(crate) tracker: NodeBuilderTracker<'tracker>,
    pub(crate) max_truncation_length: u32,
    pub(crate) max_expansion_depth: i32,
    pub(crate) encountered_error: bool,
    pub(crate) suppress_report_inference_fallback: bool,
    pub(crate) reported_diagnostic: bool,
    pub(crate) visited_types: Option<HashSet<TypeId>>,
    pub(crate) symbol_depth: Option<HashMap<SymbolId, u32>>,
    pub(crate) infer_type_parameters: Option<Vec<TypeId>>,
    pub(crate) approximate_length: u32,
    pub(crate) tracked_symbols: Option<Vec<TrackedSymbol>>,
    pub(crate) bundled: bool,
    pub(crate) truncating: bool,
    pub(crate) used_symbol_names: Option<HashSet<String>>,
    pub(crate) remapped_symbol_names: Option<HashMap<SymbolId, String>>,
    pub(crate) remapped_symbol_references: Option<HashMap<SymbolId, SymbolId>>,
    pub(crate) reverse_mapped_stack: Option<Vec<SymbolId>>,
    pub(crate) must_create_type_parameter_symbol_list: bool,
    pub(crate) type_parameter_symbol_list: Option<HashSet<SymbolId>>,
    pub(crate) must_create_type_parameters_names_lookups: bool,
    pub(crate) type_parameter_names: Option<HashMap<TypeId, TransformNode>>,
    pub(crate) type_parameter_names_by_text: Option<HashSet<String>>,
    pub(crate) type_parameter_names_by_text_next_name_count: Option<HashMap<String, u32>>,
    /// Locals installed on upstream's synthesized `pushFakeScope` Block.
    /// Rust keeps the owning parse declaration separately, so preserve the
    /// lookup overlay explicitly.
    pub(crate) synthetic_scope_locals: Option<HashMap<String, SymbolId>>,
    /// Kind of the synthesized declaration represented by the overlay.
    /// Upstream observes this through `context.enclosingDeclaration.kind`.
    pub(crate) synthetic_scope_kind: Option<SyntaxKind>,
    pub(crate) enclosing_symbol_types: HashMap<SymbolId, TypeId>,
    pub(crate) mapper: Option<MapperId>,
    pub(crate) depth: i32,
    pub(crate) type_stack: Vec<Option<TypeId>>,
    pub(crate) out: EmitSymbolExpansionOut,

    // createSyntacticTypeNodeBuilder's dynamic state. These are emit-context
    // fields, not aliases of the display slice's recovery fields.
    pub(crate) no_inference_fallback: Option<bool>,
    pub(crate) recovery_boundary_had_error: bool,
    pub(crate) recovery_boundary_depth: u32,
    pub(crate) recovery_tracked_symbols: Option<Vec<RecoveryTrackedSymbol>>,
}

pub(crate) struct SyntheticModuleScope<'scope> {
    pub(crate) enclosing_declaration: Option<NodeId>,
    pub(crate) locals: &'scope tsc_binder::SymbolTable,
}

/// tsc-port: withContext @6.0.3
/// tsc-hash: 125cf164389b2220563c80c84b08f1269efdad8469590526d730de6bc857bbd3
/// tsc-span: _tsc.js:51205-51256
#[allow(clippy::too_many_arguments)]
pub(crate) fn with_context<'program, 'tracker, T: ReplayProduced>(
    checker: &mut CheckerState<'program>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    enclosing_declaration: Option<NodeId>,
    flags: Option<EmitNodeBuilderFlags>,
    internal_flags: Option<EmitInternalNodeBuilderFlags>,
    tracker: Option<&'tracker mut dyn EmitSymbolTracker>,
    maximum_length: Option<u32>,
    verbosity_level: Option<i32>,
    callback: impl FnOnce(
        &mut CheckerState<'program>,
        &mut TransformArena,
        TransformSourceId,
        &mut NodeBuilderContext<'tracker>,
    ) -> Result<T, EmitResolverError>,
    out: Option<&mut EmitSymbolExpansionOut>,
) -> Result<Option<T>, EmitResolverError> {
    let flags = flags.unwrap_or(EmitNodeBuilderFlags::NONE);
    // Upstream `maximumLength || (…)` (:51208): numeric ZERO is falsy and
    // falls through to the flag-selected default, so Some(0) maps to the
    // default here as well.
    let max_truncation_length = maximum_length.filter(|&length| length != 0).unwrap_or(
        if flags.contains(EmitNodeBuilderFlags::NO_TRUNCATION) {
            NO_TRUNCATION_MAXIMUM_TRUNCATION_LENGTH
        } else {
            DEFAULT_MAXIMUM_TRUNCATION_LENGTH
        },
    );
    let enclosing_file = enclosing_declaration.map(|node| checker.binder.source_of_node(node).root);
    let bundled = checker
        .options
        .out_file
        .as_deref()
        .is_some_and(|out_file| !out_file.is_empty())
        && enclosing_declaration
            .is_some_and(|node| checker.binder.is_external_or_common_js_module_of_node(node));
    let mut context = NodeBuilderContext {
        enclosing_declaration,
        enclosing_declaration_is_synthetic: false,
        enclosing_file,
        flags,
        internal_flags: internal_flags.unwrap_or(EmitInternalNodeBuilderFlags::NONE),
        tracker: NodeBuilderTracker::new(tracker),
        max_truncation_length,
        max_expansion_depth: verbosity_level.unwrap_or(-1),
        encountered_error: false,
        suppress_report_inference_fallback: false,
        reported_diagnostic: false,
        visited_types: None,
        symbol_depth: None,
        infer_type_parameters: None,
        approximate_length: 0,
        tracked_symbols: None,
        bundled,
        truncating: false,
        used_symbol_names: None,
        remapped_symbol_names: None,
        remapped_symbol_references: None,
        reverse_mapped_stack: None,
        must_create_type_parameter_symbol_list: true,
        type_parameter_symbol_list: None,
        must_create_type_parameters_names_lookups: true,
        type_parameter_names: None,
        type_parameter_names_by_text: None,
        type_parameter_names_by_text_next_name_count: None,
        synthetic_scope_locals: None,
        synthetic_scope_kind: None,
        enclosing_symbol_types: HashMap::new(),
        mapper: None,
        depth: 0,
        type_stack: Vec::new(),
        out: EmitSymbolExpansionOut::default(),
        no_inference_fallback: None,
        recovery_boundary_had_error: false,
        recovery_boundary_depth: 0,
        recovery_tracked_symbols: None,
    };

    let resulting_node = callback(checker, arena, target, &mut context)?;
    if context.truncating && context.flags.contains(EmitNodeBuilderFlags::NO_TRUNCATION) {
        context
            .tracker
            .report_truncation_error(&mut context.reported_diagnostic);
    }
    if let Some(out) = out {
        *out = context.out;
    }
    // The probe observes `resultingNode` immediately before upstream filters
    // an encountered error to `undefined`; keep the decision payload on that
    // raw value while preserving the filtered resolver return below.
    let resulting_is_absent = resulting_node.is_absent();
    let resulting_class = resulting_node.class(arena);
    let produced = (!context.encountered_error).then_some(resulting_node);
    if crate::node_builder::replay_sink::armed() {
        let status = if context.encountered_error {
            "error"
        } else if resulting_is_absent {
            "fallback-undefined"
        } else {
            "node"
        };
        let record = crate::node_builder::replay_sink::DecisionEvent::WithContextResult {
            status,
            flags: context.flags.0,
            internal_flags: context.internal_flags.0,
            approximate_length: context.approximate_length,
            type_stack_len: context.type_stack.len(),
            truncating: context.truncating,
            out_truncated: context.out.truncated,
            encountered_error: context.encountered_error,
            produced: resulting_class,
        };
        crate::node_builder::replay_sink::record(move || record);
    }
    Ok(produced)
}

/// tsrs-native: explicit one-call adapter for the existing synthetic module-scope overlay.
#[allow(clippy::too_many_arguments)]
pub(crate) fn with_context_in_synthetic_module_scope<'program, 'tracker, T: ReplayProduced>(
    checker: &mut CheckerState<'program>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    enclosing_declaration: Option<NodeId>,
    flags: Option<EmitNodeBuilderFlags>,
    internal_flags: Option<EmitInternalNodeBuilderFlags>,
    tracker: Option<&'tracker mut dyn EmitSymbolTracker>,
    maximum_length: Option<u32>,
    verbosity_level: Option<i32>,
    scope: SyntheticModuleScope<'_>,
    callback: impl FnOnce(
        &mut CheckerState<'program>,
        &mut TransformArena,
        TransformSourceId,
        &mut NodeBuilderContext<'tracker>,
    ) -> Result<T, EmitResolverError>,
    out: Option<&mut EmitSymbolExpansionOut>,
) -> Result<Option<T>, EmitResolverError> {
    with_context(
        checker,
        arena,
        target,
        enclosing_declaration,
        flags,
        internal_flags,
        tracker,
        maximum_length,
        verbosity_level,
        move |checker, arena, target, context| {
            let restore = super::with_synthetic_module_scope(
                context,
                scope.enclosing_declaration,
                scope.locals,
            );
            let result = callback(checker, arena, target, context);
            super::restore_synthetic_module_scope(context, restore);
            result
        },
        out,
    )
}

/// Projection of a withContext result into the harness sink's classes. The
/// generic result is examined through [`ReplayProduced`]; non-node payloads
/// project as containers/absent exactly like the probe's non-node sentinel.
pub(crate) trait ReplayProduced {
    fn is_absent(&self) -> bool;
    fn class(&self, arena: &TransformArena) -> crate::node_builder::replay_sink::ProducedClass;
}

/// tsrs-native: harness §6.3 produced-class projection.
pub(crate) fn transform_node_class(
    arena: &TransformArena,
    node: tsc_emitter::TransformNode,
) -> crate::node_builder::replay_sink::ProducedClass {
    use crate::node_builder::replay_sink::ProducedClass;
    match arena.parse_tree_resolver_node(node) {
        Ok(Some(parse)) => {
            let projected = ProducedClass::ParseOwn {
                source: parse.source().raw(),
                node: parse.node().0,
            };
            match arena.is_parsed_node(node) {
                Ok(true) => projected,
                _ => {
                    let ProducedClass::ParseOwn { source, node } = projected else {
                        unreachable!()
                    };
                    ProducedClass::OriginalProjected { source, node }
                }
            }
        }
        _ => ProducedClass::SyntheticWithoutOriginal,
    }
}

impl ReplayProduced for tsc_emitter::TransformNode {
    fn is_absent(&self) -> bool {
        false
    }
    fn class(&self, arena: &TransformArena) -> crate::node_builder::replay_sink::ProducedClass {
        transform_node_class(arena, *self)
    }
}

impl ReplayProduced for Option<tsc_emitter::TransformNode> {
    fn is_absent(&self) -> bool {
        self.is_none()
    }
    fn class(&self, arena: &TransformArena) -> crate::node_builder::replay_sink::ProducedClass {
        match self {
            None => crate::node_builder::replay_sink::ProducedClass::Absent,
            Some(node) => transform_node_class(arena, *node),
        }
    }
}

impl ReplayProduced for Vec<tsc_emitter::TransformNode> {
    fn is_absent(&self) -> bool {
        false
    }
    fn class(&self, _arena: &TransformArena) -> crate::node_builder::replay_sink::ProducedClass {
        crate::node_builder::replay_sink::ProducedClass::Container { length: self.len() }
    }
}

impl ReplayProduced for Option<Vec<tsc_emitter::TransformNode>> {
    fn is_absent(&self) -> bool {
        self.is_none()
    }
    fn class(&self, _arena: &TransformArena) -> crate::node_builder::replay_sink::ProducedClass {
        match self {
            None => crate::node_builder::replay_sink::ProducedClass::Absent,
            Some(nodes) => crate::node_builder::replay_sink::ProducedClass::Container {
                length: nodes.len(),
            },
        }
    }
}

impl ReplayProduced for () {
    fn is_absent(&self) -> bool {
        false
    }
    fn class(&self, _arena: &TransformArena) -> crate::node_builder::replay_sink::ProducedClass {
        crate::node_builder::replay_sink::ProducedClass::SyntheticWithoutOriginal
    }
}

/// Opaque payloads (focused-test plumbing and multi-value internal
/// callbacks) project as synthetic: the sink is never armed on those paths
/// and the class is inert bookkeeping.
macro_rules! replay_produced_opaque {
    ($($ty:ty),+ $(,)?) => {$(
        impl ReplayProduced for $ty {
            fn is_absent(&self) -> bool {
                false
            }
            fn class(
                &self,
                _arena: &TransformArena,
            ) -> crate::node_builder::replay_sink::ProducedClass {
                crate::node_builder::replay_sink::ProducedClass::SyntheticWithoutOriginal
            }
        }
    )+};
}
replay_produced_opaque!(
    u8,
    u32,
    (
        tsc_emitter::TransformNode,
        tsc_emitter::TransformNode,
        tsc_emitter::TransformNode
    ),
    (Option<tsc_emitter::TransformNode>, bool),
);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SymbolTypeRestore {
    pub(crate) symbol: SymbolId,
    pub(crate) old_type: Option<TypeId>,
}

/// tsc-port: addSymbolTypeToContext @6.0.3
/// tsc-hash: 28e4cca53f3194ad72572e7e61c4041a35ebfbd82ea86a752414c8e2d00e82ca
/// tsc-span: _tsc.js:51257-51269
pub(crate) fn add_symbol_type_to_context(
    context: &mut NodeBuilderContext<'_>,
    symbol: SymbolId,
    r#type: TypeId,
) -> SymbolTypeRestore {
    let old_type = context.enclosing_symbol_types.insert(symbol, r#type);
    SymbolTypeRestore { symbol, old_type }
}

/// tsc-port: addSymbolTypeToContext.restore @6.0.3
/// tsc-hash: 115d331e51a02ad5476b222e7f77eb44b831532019cf48a0a797aa5c1ff745dd
/// tsc-span: _tsc.js:51262-51268
pub(crate) fn restore_symbol_type_to_context(
    context: &mut NodeBuilderContext<'_>,
    restore: SymbolTypeRestore,
) {
    if let Some(old_type) = restore.old_type {
        context
            .enclosing_symbol_types
            .insert(restore.symbol, old_type);
    } else {
        context.enclosing_symbol_types.remove(&restore.symbol);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FlagsRestore {
    pub(crate) flags: EmitNodeBuilderFlags,
    pub(crate) internal_flags: EmitInternalNodeBuilderFlags,
    pub(crate) depth: i32,
}

/// tsc-port: saveRestoreFlags @6.0.3
/// tsc-hash: 26fc3c3d3a3814ebc41349564a6f215f6c2b67dce776e38e48482186169aa073
/// tsc-span: _tsc.js:51270-51280
pub(crate) fn save_restore_flags(context: &NodeBuilderContext<'_>) -> FlagsRestore {
    FlagsRestore {
        flags: context.flags,
        internal_flags: context.internal_flags,
        depth: context.depth,
    }
}

/// tsc-port: saveRestoreFlags.restore @6.0.3
/// tsc-hash: b53d40cb52e88abc4457336215e9e75cc1a762be3e6e3d16315e60837d48fe1d
/// tsc-span: _tsc.js:51275-51279
pub(crate) fn restore_flags(context: &mut NodeBuilderContext<'_>, restore: FlagsRestore) {
    context.flags = restore.flags;
    context.internal_flags = restore.internal_flags;
    context.depth = restore.depth;
}

/// tsc-port: checkTruncationLengthIfExpanding @6.0.3
/// tsc-hash: ced966cab3bb64499f41644ddcddfde8d7afa004462557a5b0f3b7eb5f6d6be0
/// tsc-span: _tsc.js:51281-51283
pub(crate) fn check_truncation_length_if_expanding(context: &mut NodeBuilderContext<'_>) -> bool {
    context.max_expansion_depth >= 0 && check_truncation_length(context)
}

/// tsc-port: checkTruncationLength @6.0.3
/// tsc-hash: 487c4a58aa166fe4725c57bdefaa15d36737ad40f6c64fa1476f33bd83d24e06
/// tsc-span: _tsc.js:51284-51287
pub(crate) fn check_truncation_length(context: &mut NodeBuilderContext<'_>) -> bool {
    if context.truncating {
        return true;
    }
    context.truncating = context.approximate_length > context.max_truncation_length;
    context.truncating
}

/// tsc-port: canPossiblyExpandType @6.0.3
/// tsc-hash: 842325baaf5e05099c30dfb1cce6638972f0db8265ee964e2b1b529344e81c7e
/// tsc-span: _tsc.js:51288-51295
pub(crate) fn can_possibly_expand_type(r#type: TypeId, context: &NodeBuilderContext<'_>) -> bool {
    if context
        .type_stack
        .iter()
        .take(context.type_stack.len().saturating_sub(1))
        .any(|entry| *entry == Some(r#type))
    {
        return false;
    }
    context.depth < context.max_expansion_depth
        || context.depth == context.max_expansion_depth && !context.out.can_increase_expansion_depth
}

/// tsc-port: shouldExpandType @6.0.3
/// tsc-hash: f3e4f58cc1c115dea69c6fb212003c5519624306ffd92d3eac18d788f62f5d97
/// tsc-span: _tsc.js:51296-51310
pub(crate) fn should_expand_type(
    checker: &CheckerState<'_>,
    r#type: TypeId,
    context: &mut NodeBuilderContext<'_>,
    is_alias: bool,
) -> bool {
    if !is_alias && is_lib_type(checker, r#type) {
        return false;
    }
    if context
        .type_stack
        .iter()
        .take(context.type_stack.len().saturating_sub(1))
        .any(|entry| *entry == Some(r#type))
    {
        return false;
    }
    let result = context.depth < context.max_expansion_depth;
    if !result {
        context.out.can_increase_expansion_depth = true;
    }
    result
}

/// tsc-port: isLibType @6.0.3
/// tsc-hash: 0481528ca734b60d6ec6ee852c78f57500c6aba5802fb587c27d7c31eeeb6a9f
/// tsc-span: _tsc.js:55452-55456
fn is_lib_type(checker: &CheckerState<'_>, r#type: TypeId) -> bool {
    if checker.tables.is_tuple_type(r#type) {
        return true;
    }
    let symbol_type = if checker
        .tables
        .object_flags_of(r#type)
        .intersects(ObjectFlags::REFERENCE)
    {
        checker.tables.reference_target(r#type)
    } else {
        r#type
    };
    checker
        .tables
        .type_of(symbol_type)
        .symbol
        .is_some_and(|symbol| {
            checker
                .binder
                .symbol(symbol)
                .declarations
                .iter()
                .any(|&decl| {
                    let file = checker.binder.file_index_of_node(decl);
                    let file = ProgramFileId::from_raw(
                        u32::try_from(file).expect("Program file index exceeds u32"),
                    );
                    checker.binder.file_facts(file).is_default_library()
                })
        })
}

/// tsc-port: inferTypeOfDeclaration @6.0.3 (noInferenceFallback gate)
/// tsc-hash: 5f3dbaf8de892a7b132823367ffa11bb05b745c8fd45205b7d928d883ba2764b
/// tsc-span: _tsc.js:133947-133949
pub(crate) fn no_inference_fallback_is_set(context: &NodeBuilderContext<'_>) -> bool {
    context.no_inference_fallback == Some(true)
}

/// tsc-port: typeFromArrayLiteral @6.0.3 (save/set noInferenceFallback)
/// tsc-hash: 74c7b5f888b75bb5850ba85980e34ba500823c151f573ec9168b245198a18c4d
/// tsc-span: _tsc.js:134126-134127
pub(crate) fn save_no_inference_fallback(context: &mut NodeBuilderContext<'_>) -> Option<bool> {
    let old_no_inference_fallback = context.no_inference_fallback;
    context.no_inference_fallback = Some(true);
    old_no_inference_fallback
}

/// tsc-port: typeFromArrayLiteral @6.0.3 (restore noInferenceFallback)
/// tsc-hash: 1e92fe09799fc66ddc68b706b0ef37ff4a4ac71f8d195217a8593043c9c6b4f0
/// tsc-span: _tsc.js:134143-134143
pub(crate) fn restore_no_inference_fallback(
    context: &mut NodeBuilderContext<'_>,
    old_no_inference_fallback: Option<bool>,
) {
    context.no_inference_fallback = old_no_inference_fallback;
}

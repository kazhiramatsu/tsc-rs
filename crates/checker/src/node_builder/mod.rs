#![allow(dead_code, unused_imports)]

mod chains;
mod context;
mod serialize;
mod signatures;
pub(crate) mod specifier;
mod statements;
mod tracker;
mod type_nodes;

pub(crate) use crate::syntactic_type_node_builder::SyntacticTypeNodeBuilder;
pub(crate) use chains::specifier_for_module_symbol;
pub(crate) use chains::{
    chains_get_property_name_node_for_symbol, chains_lookup_symbol_chain,
    chains_symbol_to_entity_name_node, chains_symbol_to_expression, chains_symbol_to_type_node,
    clone_node_builder_context,
    existing_type_node_is_not_reference_or_is_reference_with_compatible_type_argument_count,
    get_declaration_with_type_annotation, get_enclosing_declaration_ignoring_fake_scope,
    get_module_specifier_override, get_type_from_type_node2, restore_cloned_node_builder_context,
    serialize_inferred_type_for_declaration, set_text_range2, symbol_to_node,
    type_parameter_to_name, ClonedNodeBuilderContextRestore,
};
pub(crate) use context::transform_node_class;
pub(crate) use context::{
    add_symbol_type_to_context, can_possibly_expand_type, check_truncation_length,
    check_truncation_length_if_expanding, no_inference_fallback_is_set, restore_flags,
    restore_no_inference_fallback, restore_symbol_type_to_context, save_no_inference_fallback,
    save_restore_flags, should_expand_type, with_context, FlagsRestore, NodeBuilderContext,
    RecoveryTrackedSymbol, SymbolTypeRestore, TrackedSymbol, DEFAULT_MAXIMUM_TRUNCATION_LENGTH,
    NO_TRUNCATION_MAXIMUM_TRUNCATION_LENGTH,
};
pub(crate) use serialize::{
    index_info_to_index_signature_declaration, serialize_return_type_for_signature,
    serialize_return_type_for_signature_seam, serialize_type_for_declaration,
    serialize_type_for_declaration_seam, serialize_type_for_expression,
    syntactic_serialize_name_of_parameter_seam, syntactic_try_reuse_existing_type_node,
    type_predicate_to_type_predicate_node, type_to_type_node,
};
pub(crate) use signatures::{
    create_recovery_boundary, enter_new_scope, index_info_to_index_signature_declaration_helper,
    prime_type_parameter_names_for_scope, signature_to_signature_declaration_helper,
    symbol_to_parameter_declaration, type_parameter_to_declaration,
    type_predicate_to_type_predicate_node_helper, SignatureDeclarationOptions,
};
pub(crate) use statements::{symbol_table_to_declaration_statements, symbol_to_declarations};
pub(crate) use tracker::NodeBuilderTracker;
use type_nodes::{
    add_approximate_length, checker_abort_error, clone_parse_node, create_identifier, create_node,
    create_node_array, create_token, factory_error, project_parse_node, BuildResult,
};
pub(crate) use type_nodes::{map_to_type_nodes, type_to_type_node_helper};

/// The declaration facts read directly from upstream's optional `symbol`
/// argument by the syntactic variable-declaration arm. Keeping them beside
/// the opaque checker identity avoids widening the dormant seam with symbol-
/// table queries that are not members of `syntacticBuilderResolver`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SyntacticSymbol {
    pub(crate) id: tsc_binder::SymbolId,
    pub(crate) declaration_count: usize,
    pub(crate) variable_declaration_count: usize,
}

/// Rust spelling of `getAllAccessorDeclarationsForDeclaration`'s record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SyntacticAccessorDeclarations {
    pub(crate) first_accessor: tsc_emitter::TransformNode,
    pub(crate) second_accessor: Option<tsc_emitter::TransformNode>,
    pub(crate) get_accessor: Option<tsc_emitter::TransformNode>,
    pub(crate) set_accessor: Option<tsc_emitter::TransformNode>,
}

/// Rust spelling of `trackExistingEntityName`'s two-field result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SyntacticTrackedEntityName {
    pub(crate) node: tsc_emitter::TransformNode,
    pub(crate) introduces_error: bool,
}

/// TypeScript's local/export projections share one symbol-id space for the
/// statement serializer. Rust retains both binder symbols, so test remapping
/// through their export projection before invoking the scoped tracker.
/// tsrs-native: statement-serializer remap probe (Rust borrow shape).
pub(crate) fn is_statement_symbol_remapped(
    checker: &crate::state::CheckerState<'_>,
    context: &NodeBuilderContext<'_>,
    symbol: tsc_binder::SymbolId,
) -> bool {
    let normalized = checker.get_export_symbol_of_value_symbol_if_exported(symbol);
    context.remapped_symbol_names.as_ref().is_some_and(|names| {
        names.keys().copied().any(|candidate| {
            candidate == symbol
                || checker.get_export_symbol_of_value_symbol_if_exported(candidate) == normalized
        })
    })
}

/// Owned cleanup returned by the checker-side `enterNewScope` callback.
/// It captures exactly the context slots restored by
/// `cloneNodeBuilderContext`, plus enclosing-declaration and mapper.
pub(crate) struct SyntacticScopeCleanup {
    enclosing_declaration: Option<tsc_syntax::NodeId>,
    enclosing_declaration_is_synthetic: bool,
    mapper: Option<tsc_types::MapperId>,
    must_create_type_parameter_symbol_list: bool,
    type_parameter_symbol_list: Option<std::collections::HashSet<tsc_binder::SymbolId>>,
    must_create_type_parameters_names_lookups: bool,
    type_parameter_names:
        Option<std::collections::HashMap<tsc_types::TypeId, tsc_emitter::TransformNode>>,
    type_parameter_names_by_text: Option<std::collections::HashSet<String>>,
    type_parameter_names_by_text_next_name_count: Option<std::collections::HashMap<String, u32>>,
    synthetic_scope_locals: Option<std::collections::HashMap<String, tsc_binder::SymbolId>>,
}

impl SyntacticScopeCleanup {
    /// tsrs-native: harness decision-sink frame capture.
    pub(crate) fn capture(context: &NodeBuilderContext<'_>) -> Self {
        Self {
            enclosing_declaration: context.enclosing_declaration,
            enclosing_declaration_is_synthetic: context.enclosing_declaration_is_synthetic,
            mapper: context.mapper,
            must_create_type_parameter_symbol_list: context.must_create_type_parameter_symbol_list,
            type_parameter_symbol_list: context.type_parameter_symbol_list.clone(),
            must_create_type_parameters_names_lookups: context
                .must_create_type_parameters_names_lookups,
            type_parameter_names: context.type_parameter_names.clone(),
            type_parameter_names_by_text: context.type_parameter_names_by_text.clone(),
            type_parameter_names_by_text_next_name_count: context
                .type_parameter_names_by_text_next_name_count
                .clone(),
            synthetic_scope_locals: context.synthetic_scope_locals.clone(),
        }
    }

    /// tsrs-native: scoped save/restore completion (upstream closure capture).
    pub(crate) fn restore(self, context: &mut NodeBuilderContext<'_>) {
        context.enclosing_declaration = self.enclosing_declaration;
        context.enclosing_declaration_is_synthetic = self.enclosing_declaration_is_synthetic;
        context.mapper = self.mapper;
        context.must_create_type_parameter_symbol_list =
            self.must_create_type_parameter_symbol_list;
        context.type_parameter_symbol_list = self.type_parameter_symbol_list;
        context.must_create_type_parameters_names_lookups =
            self.must_create_type_parameters_names_lookups;
        context.type_parameter_names = self.type_parameter_names;
        context.type_parameter_names_by_text = self.type_parameter_names_by_text;
        context.type_parameter_names_by_text_next_name_count =
            self.type_parameter_names_by_text_next_name_count;
        context.synthetic_scope_locals = self.synthetic_scope_locals;
    }
}

/// One `startRecoveryScope` snapshot inside a syntactic reuse boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SyntacticRecoveryScope {
    had_error: bool,
    tracked_symbols_top: usize,
}

/// Object-safe Rust spelling of the four closures returned by upstream's
/// `createRecoveryBoundary`. Checker callbacks mark the context slot while
/// this owned token supplies start/recover/finalize lifetime discipline.
pub(crate) struct SyntacticRecoveryBoundary {
    previous_had_error: bool,
    previous_depth: u32,
    previous_recovery_tracked_symbols: Option<Vec<RecoveryTrackedSymbol>>,
    previous_tracked_symbols: Option<Vec<TrackedSymbol>>,
}

impl SyntacticRecoveryBoundary {
    /// tsrs-native: Rust constructor for the ported machinery.
    pub(crate) fn new(context: &mut NodeBuilderContext<'_>) -> Self {
        let previous_had_error = context.recovery_boundary_had_error;
        let previous_depth = context.recovery_boundary_depth;
        let previous_recovery_tracked_symbols = context.recovery_tracked_symbols.take();
        let previous_tracked_symbols = context.tracked_symbols.take();
        context.recovery_boundary_had_error = false;
        context.recovery_boundary_depth = previous_depth.saturating_add(1);
        context.recovery_tracked_symbols = Some(Vec::new());
        Self {
            previous_had_error,
            previous_depth,
            previous_recovery_tracked_symbols,
            previous_tracked_symbols,
        }
    }

    /// tsrs-native: recovery-boundary error probe (upstream closure capture).
    pub(crate) fn had_error(&self, context: &NodeBuilderContext<'_>) -> bool {
        context.recovery_boundary_had_error
    }

    /// tsrs-native: recovery-boundary error latch (upstream closure capture).
    pub(crate) fn mark_error(&mut self, context: &mut NodeBuilderContext<'_>) {
        context.recovery_boundary_had_error = true;
    }

    /// tsrs-native: recovery-scope entry (upstream closure capture).
    pub(crate) fn start_recovery_scope(
        &self,
        context: &NodeBuilderContext<'_>,
    ) -> SyntacticRecoveryScope {
        SyntacticRecoveryScope {
            had_error: context.recovery_boundary_had_error,
            tracked_symbols_top: context
                .recovery_tracked_symbols
                .as_ref()
                .map_or(0, Vec::len),
        }
    }

    /// tsrs-native: recovery-scope rollback token (upstream closure capture).
    pub(crate) fn recover(
        &mut self,
        context: &mut NodeBuilderContext<'_>,
        scope: SyntacticRecoveryScope,
    ) {
        context.recovery_boundary_had_error = scope.had_error;
        if let Some(tracked) = context.recovery_tracked_symbols.as_mut() {
            tracked.truncate(scope.tracked_symbols_top);
        }
    }

    /// tsrs-native: recovery-boundary completion (upstream closure return).
    pub(crate) fn finalize(
        self,
        context: &mut NodeBuilderContext<'_>,
        access: &mut dyn tsc_emitter::EmitTrackerAccess,
    ) -> Result<bool, tsc_emitter::EmitResolverError> {
        let succeeded = !context.recovery_boundary_had_error;
        let buffered = context.recovery_tracked_symbols.take().unwrap_or_default();
        context.recovery_tracked_symbols = self.previous_recovery_tracked_symbols;
        context.tracked_symbols = self.previous_tracked_symbols;
        context.recovery_boundary_had_error = self.previous_had_error;
        context.recovery_boundary_depth = self.previous_depth;
        if succeeded {
            for (symbol, symbol_flags, enclosing, synthetic, meaning, symbol_is_remapped) in
                buffered
            {
                context.tracker.track_symbol(
                    &mut context.reported_diagnostic,
                    &mut context.tracked_symbols,
                    &mut context.recovery_tracked_symbols,
                    access,
                    symbol,
                    symbol_flags,
                    enclosing,
                    synthetic,
                    meaning,
                    symbol_is_remapped,
                )?;
            }
        }
        Ok(succeeded)
    }
}

/// Checker-supplied callback object consumed by
/// `createSyntacticTypeNodeBuilder`.
///
/// tsc-port: syntacticBuilderResolver @6.0.3
/// tsc-hash: 4435e40ac4ba06bf9e97dd48b84835ddcec09e878d5b6163f041aa5ea0398894
/// tsc-span: _tsc.js:50778-50956
pub(crate) trait SyntacticBuilderResolver: tsc_emitter::EmitTrackerAccess {
    fn evaluate_entity_name_expression(
        &mut self,
        expression: tsc_emitter::TransformNode,
    ) -> Result<crate::evaluate::EvaluatorResult, tsc_emitter::EmitResolverError>;

    fn is_expando_function_declaration(
        &mut self,
        node: tsc_emitter::TransformNode,
    ) -> Result<bool, tsc_emitter::EmitResolverError>;

    fn has_late_bindable_name(
        &mut self,
        node: tsc_emitter::TransformNode,
    ) -> Result<bool, tsc_emitter::EmitResolverError>;

    fn should_remove_declaration(
        &mut self,
        context: &mut NodeBuilderContext<'_>,
        node: tsc_emitter::TransformNode,
    ) -> Result<bool, tsc_emitter::EmitResolverError>;

    fn create_recovery_boundary(
        &mut self,
        context: &mut NodeBuilderContext<'_>,
    ) -> Result<SyntacticRecoveryBoundary, tsc_emitter::EmitResolverError>;

    fn is_definitely_reference_to_global_symbol_object(
        &mut self,
        node: tsc_emitter::TransformNode,
    ) -> Result<bool, tsc_emitter::EmitResolverError>;

    fn get_all_accessor_declarations(
        &mut self,
        node: tsc_emitter::TransformNode,
    ) -> Result<SyntacticAccessorDeclarations, tsc_emitter::EmitResolverError>;

    fn requires_adding_implicit_undefined(
        &mut self,
        declaration: tsc_emitter::TransformNode,
        symbol: Option<SyntacticSymbol>,
        enclosing_declaration: Option<tsc_syntax::NodeId>,
    ) -> Result<bool, tsc_emitter::EmitResolverError>;

    fn is_optional_parameter(
        &mut self,
        parameter: tsc_emitter::TransformNode,
    ) -> Result<bool, tsc_emitter::EmitResolverError>;

    fn is_undefined_identifier_expression(
        &mut self,
        node: tsc_emitter::TransformNode,
    ) -> Result<bool, tsc_emitter::EmitResolverError>;

    fn is_entity_name_visible(
        &mut self,
        context: &mut NodeBuilderContext<'_>,
        entity_name: tsc_emitter::TransformNode,
        should_compute_aliases_to_make_visible: bool,
    ) -> Result<tsc_emitter::EmitSymbolAccessibilityResult, tsc_emitter::EmitResolverError>;

    fn serialize_existing_type_node(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        target: tsc_emitter::TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        type_node: tsc_emitter::TransformNode,
        add_undefined: bool,
    ) -> Result<Option<tsc_emitter::TransformNode>, tsc_emitter::EmitResolverError>;

    fn serialize_return_type_for_signature(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        target: tsc_emitter::TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        signature_declaration: tsc_emitter::TransformNode,
        symbol: Option<SyntacticSymbol>,
    ) -> Result<Option<tsc_emitter::TransformNode>, tsc_emitter::EmitResolverError>;

    fn serialize_type_of_expression(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        target: tsc_emitter::TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        expression: tsc_emitter::TransformNode,
    ) -> Result<Option<tsc_emitter::TransformNode>, tsc_emitter::EmitResolverError>;

    fn serialize_type_of_declaration(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        target: tsc_emitter::TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        declaration: tsc_emitter::TransformNode,
        symbol: Option<SyntacticSymbol>,
    ) -> Result<Option<tsc_emitter::TransformNode>, tsc_emitter::EmitResolverError>;

    fn serialize_name_of_parameter(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        target: tsc_emitter::TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        parameter: tsc_emitter::TransformNode,
    ) -> Result<tsc_emitter::TransformNode, tsc_emitter::EmitResolverError>;

    fn serialize_entity_name(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        target: tsc_emitter::TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        node: tsc_emitter::TransformNode,
    ) -> Result<Option<tsc_emitter::TransformNode>, tsc_emitter::EmitResolverError>;

    fn serialize_type_name(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        target: tsc_emitter::TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        node: tsc_emitter::TransformNode,
        is_type_of: bool,
        type_arguments: Option<tsc_emitter::TransformNodeArray>,
    ) -> Result<Option<tsc_emitter::TransformNode>, tsc_emitter::EmitResolverError>;

    fn get_js_doc_property_override(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        target: tsc_emitter::TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        js_doc_type_literal: tsc_emitter::TransformNode,
        js_doc_property: tsc_emitter::TransformNode,
    ) -> Result<Option<tsc_emitter::TransformNode>, tsc_emitter::EmitResolverError>;

    fn enter_new_scope(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        target: tsc_emitter::TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        node: tsc_emitter::TransformNode,
    ) -> Result<SyntacticScopeCleanup, tsc_emitter::EmitResolverError>;

    fn mark_node_reuse(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        context: &mut NodeBuilderContext<'_>,
        range: tsc_emitter::TransformNode,
        location: tsc_emitter::TransformNode,
    ) -> Result<tsc_emitter::TransformNode, tsc_emitter::EmitResolverError>;

    fn track_existing_entity_name(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        target: tsc_emitter::TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        node: tsc_emitter::TransformNode,
    ) -> Result<SyntacticTrackedEntityName, tsc_emitter::EmitResolverError>;

    fn track_computed_name(
        &mut self,
        context: &mut NodeBuilderContext<'_>,
        access_expression: tsc_emitter::TransformNode,
    ) -> Result<(), tsc_emitter::EmitResolverError>;

    fn get_module_specifier_override(
        &mut self,
        context: &mut NodeBuilderContext<'_>,
        parent: tsc_emitter::TransformNode,
        literal: tsc_emitter::TransformNode,
    ) -> Result<Option<String>, tsc_emitter::EmitResolverError>;

    fn can_reuse_type_node(
        &mut self,
        context: &mut NodeBuilderContext<'_>,
        type_node: tsc_emitter::TransformNode,
    ) -> Result<bool, tsc_emitter::EmitResolverError>;

    fn can_reuse_type_node_annotation(
        &mut self,
        context: &mut NodeBuilderContext<'_>,
        node: tsc_emitter::TransformNode,
        existing: tsc_emitter::TransformNode,
        symbol: Option<SyntacticSymbol>,
        requires_adding_undefined: Option<bool>,
    ) -> Result<bool, tsc_emitter::EmitResolverError>;
}

/// Standalone tracker access for resolver members that call the RAW
/// caller tracker outside any NodeBuilder context (upstream
/// createLateBoundIndexSignatures' trackComputedName calls
/// `tracker.trackSymbol` directly, :88676-88690). Token protocol mirrors
/// the production syntactic resolver: raw checker ids, validated on the
/// way back in.
pub(crate) struct StandaloneTrackerAccess<'c, 'p> {
    pub(crate) checker: &'c mut crate::state::CheckerState<'p>,
    pub(crate) method: tsc_emitter::EmitResolverMethod,
}

impl StandaloneTrackerAccess<'_, '_> {
    fn node(&self, node: tsc_emitter::EmitTrackerNode) -> Option<tsc_syntax::NodeId> {
        u32::try_from(node.0)
            .ok()
            .map(tsc_syntax::NodeId)
            .filter(|&node| self.checker.binder.try_file_index_of_node(node).is_some())
    }

    fn invalid_token(&self) -> tsc_emitter::EmitResolverError {
        let node = self.checker.binder.source(0).root;
        let source = u32::try_from(self.checker.binder.file_index_of_node(node)).unwrap_or(0);
        tsc_emitter::EmitResolverError::CheckerAborted {
            method: self.method,
            node: tsc_emitter::EmitResolverNode::from_raw_source(source, node),
            reason: "standalone tracker access received an invalid checker token",
        }
    }

    fn abort(
        &self,
        node: tsc_syntax::NodeId,
        abort: crate::state::CheckAbort,
    ) -> tsc_emitter::EmitResolverError {
        let source = u32::try_from(self.checker.binder.file_index_of_node(node)).unwrap_or(0);
        tsc_emitter::EmitResolverError::CheckerAborted {
            method: self.method,
            node: tsc_emitter::EmitResolverNode::from_raw_source(source, node),
            reason: abort.description(),
        }
    }
}

impl tsc_emitter::EmitTrackerAccess for StandaloneTrackerAccess<'_, '_> {
    fn is_symbol_accessible(
        &mut self,
        symbol: tsc_emitter::EmitTrackerSymbol,
        enclosing_declaration: Option<tsc_emitter::EmitTrackerNode>,
        meaning: tsc_emitter::EmitSymbolMeaning,
        should_compute_aliases: bool,
    ) -> Result<tsc_emitter::EmitSymbolAccessibilityResult, tsc_emitter::EmitResolverError> {
        let symbol = u32::try_from(symbol.0)
            .ok()
            .map(tsc_binder::SymbolId)
            .filter(|&symbol| self.checker.binder.try_symbol(symbol).is_some())
            .ok_or_else(|| self.invalid_token())?;
        let enclosing = enclosing_declaration
            .and_then(|node| self.node(node))
            .ok_or_else(|| self.invalid_token())?;
        self.checker
            .emit_is_symbol_accessible(symbol, enclosing, meaning, should_compute_aliases)
            .map_err(|abort| self.abort(enclosing, abort))
    }

    fn is_expando_function_declaration(
        &mut self,
        node: tsc_emitter::EmitTrackerNode,
    ) -> Result<bool, tsc_emitter::EmitResolverError> {
        let node = self.node(node).ok_or_else(|| self.invalid_token())?;
        self.checker
            .emit_is_expando_function_declaration(node)
            .map_err(|abort| self.abort(node, abort))
    }

    fn get_properties_of_container_function(
        &mut self,
        node: tsc_emitter::EmitTrackerNode,
    ) -> Result<Vec<tsc_emitter::EmitFunctionProperty>, tsc_emitter::EmitResolverError> {
        let node = self.node(node).ok_or_else(|| self.invalid_token())?;
        self.checker
            .emit_get_properties_of_container_function(node, 0)
            .map_err(|abort| self.abort(node, abort))
    }

    fn requires_adding_implicit_undefined(
        &mut self,
        parameter: tsc_emitter::EmitTrackerNode,
        enclosing_declaration: Option<tsc_emitter::EmitTrackerNode>,
    ) -> Result<bool, tsc_emitter::EmitResolverError> {
        let parameter = self.node(parameter).ok_or_else(|| self.invalid_token())?;
        let enclosing = enclosing_declaration.and_then(|node| self.node(node));
        self.checker
            .emit_requires_adding_implicit_undefined(parameter, enclosing)
            .map_err(|abort| self.abort(parameter, abort))
    }

    fn describe_symbol(
        &mut self,
        symbol: tsc_emitter::EmitTrackerSymbol,
    ) -> tsc_emitter::EmitTrackerSymbolDescription {
        let Some(symbol) = u32::try_from(symbol.0)
            .ok()
            .map(tsc_binder::SymbolId)
            .filter(|&symbol| self.checker.binder.try_symbol(symbol).is_some())
        else {
            return tsc_emitter::EmitTrackerSymbolDescription::default();
        };
        let data = self.checker.binder.symbol(symbol);
        let declarations: Vec<_> = data
            .declarations
            .iter()
            .take(8)
            .map(|&declaration| {
                let source =
                    u32::try_from(self.checker.binder.file_index_of_node(declaration)).unwrap_or(0);
                tsc_emitter::EmitTrackerNodeDescription {
                    parse: Some(tsc_emitter::EmitResolverNode::from_raw_source(
                        source,
                        declaration,
                    )),
                    original: None,
                }
            })
            .collect();
        tsc_emitter::EmitTrackerSymbolDescription {
            escaped_name: data.escaped_name.clone(),
            declaration_count: u32::try_from(data.declarations.len()).unwrap_or(u32::MAX),
            declarations,
        }
    }

    fn describe_node(
        &mut self,
        node: tsc_emitter::EmitTrackerNode,
    ) -> tsc_emitter::EmitTrackerNodeDescription {
        let Some(node) = self.node(node) else {
            return tsc_emitter::EmitTrackerNodeDescription::default();
        };
        let source = u32::try_from(self.checker.binder.file_index_of_node(node)).unwrap_or(0);
        tsc_emitter::EmitTrackerNodeDescription {
            parse: Some(tsc_emitter::EmitResolverNode::from_raw_source(source, node)),
            original: None,
        }
    }
}

/// tsc-port: createLateBoundIndexSignatures @6.0.3 (member body)
/// tsc-hash: 57a5aa62b412607a3d4c1fc9811e8e9ec66f85ef4aa82dab2cc6afe36885e6c9
/// tsc-span: _tsc.js:88624-88691
#[allow(clippy::too_many_arguments)]
pub(crate) fn late_bound_index_signatures(
    checker: &mut crate::state::CheckerState<'_>,
    arena: &mut tsc_emitter::TransformArena,
    target: tsc_emitter::TransformSourceId,
    container: tsc_syntax::NodeId,
    enclosing_declaration: tsc_syntax::NodeId,
    flags: tsc_emitter::EmitNodeBuilderFlags,
    internal_flags: tsc_emitter::EmitInternalNodeBuilderFlags,
    tracker: &mut dyn tsc_emitter::EmitSymbolTracker,
) -> Result<Option<Vec<tsc_emitter::TransformNode>>, tsc_emitter::EmitResolverError> {
    use tsc_binder::InternalSymbolName;
    use tsc_syntax::{NodeData, NodeId, SyntaxKind};
    let method = tsc_emitter::EmitResolverMethod::CreateLateBoundIndexSignatures;
    let abort_at = |checker: &crate::state::CheckerState<'_>,
                    node: NodeId,
                    abort: crate::state::CheckAbort| {
        let source = u32::try_from(checker.binder.file_index_of_node(node)).unwrap_or(0);
        tsc_emitter::EmitResolverError::CheckerAborted {
            method,
            node: tsc_emitter::EmitResolverNode::from_raw_source(source, node),
            reason: abort.description(),
        }
    };
    let factory_error = |error| tsc_emitter::EmitResolverError::Factory {
        method,
        error: Box::new(error),
    };

    let symbol = checker
        .get_symbol_of_declaration(container)
        .map_err(|abort| abort_at(checker, container, abort))?;
    let container_type = checker
        .get_type_of_symbol(symbol)
        .map_err(|abort| abort_at(checker, container, abort))?;
    let static_infos = checker
        .get_index_infos_of_type(container_type)
        .map_err(|abort| abort_at(checker, container, abort))?;
    let members = checker
        .get_members_of_symbol(symbol)
        .map_err(|abort| abort_at(checker, container, abort))?;
    let instance_infos = match members.get(InternalSymbolName::INDEX) {
        Some(&index_symbol) => {
            let siblings: Vec<tsc_binder::SymbolId> =
                members.iter().map(|(_, &member)| member).collect();
            Some(
                checker
                    .get_index_infos_of_index_symbol(index_symbol, Some(siblings))
                    .map_err(|abort| abort_at(checker, container, abort))?,
            )
        }
        None => None,
    };

    let mut result: Option<Vec<tsc_emitter::TransformNode>> = None;
    for (info_list, is_static) in [(Some(static_infos), true), (instance_infos, false)] {
        let Some(info_list) = info_list else { continue };
        if info_list.is_empty() {
            continue;
        }
        let result = result.get_or_insert_with(Vec::new);
        for info in &info_list {
            if info.declaration.is_some() {
                continue;
            }
            // Upstream also skips the anyBaseTypeIndexInfo singleton
            // (:88634); the Rust checker never synthesizes that identity
            // (no constructor exists), so the arm is a documented no-op.
            if let Some(components) = &info.components {
                let mut all_serializable = true;
                for &component in components {
                    let source = checker.binder.source_of_node(component);
                    let name = tsc_binder::node_util::get_name_of_declaration(source, component);
                    let expression = name.and_then(|name| {
                        match &checker.binder.source_of_node(name).arena.node(name).data {
                            NodeData::ComputedPropertyName(data) => data.expression,
                            _ => None,
                        }
                    });
                    let serializable = match expression {
                        Some(expression) if checker.is_entity_name_expression(expression) => {
                            let verdict = checker
                                .emit_is_entity_name_visible(
                                    expression,
                                    enclosing_declaration,
                                    false,
                                )
                                .map_err(|abort| abort_at(checker, expression, abort))?;
                            verdict.accessibility
                                == tsc_emitter::EmitSymbolAccessibility::Accessible
                        }
                        _ => false,
                    };
                    if !serializable {
                        all_serializable = false;
                        break;
                    }
                }
                if all_serializable {
                    for &component in components {
                        if checker
                            .has_late_bindable_name(component)
                            .map_err(|abort| abort_at(checker, component, abort))?
                        {
                            continue;
                        }
                        let source = checker.binder.source_of_node(component);
                        let name =
                            tsc_binder::node_util::get_name_of_declaration(source, component)
                                .expect("serializable component carries a computed name");
                        let name_expression =
                            match &checker.binder.source_of_node(name).arena.node(name).data {
                                NodeData::ComputedPropertyName(data) => data
                                    .expression
                                    .expect("computed name carries an expression"),
                                _ => unreachable!("serializability proved the computed name"),
                            };
                        // trackComputedName (:88676-88690): the RAW caller
                        // tracker, outside any NodeBuilder context.
                        if tracker.can_track_symbol() {
                            let first_identifier = checker.get_first_identifier(name_expression);
                            let text = match &checker
                                .binder
                                .source_of_node(first_identifier)
                                .arena
                                .node(first_identifier)
                                .data
                            {
                                NodeData::Identifier(data) => data.escaped_text.clone(),
                                _ => String::new(),
                            };
                            let resolved = checker
                                .resolve_name(
                                    Some(first_identifier),
                                    &text,
                                    tsc_types::SymbolFlags::VALUE
                                        | tsc_types::SymbolFlags::EXPORT_VALUE,
                                    None,
                                    true,
                                    false,
                                )
                                .map_err(|abort| abort_at(checker, first_identifier, abort))?;
                            if let Some(resolved) = resolved {
                                let mut access = StandaloneTrackerAccess { checker, method };
                                let symbol_token =
                                    tsc_emitter::EmitTrackerSymbol(u64::from(resolved.0));
                                let enclosing_token = tsc_emitter::EmitTrackerNode(u64::from(
                                    enclosing_declaration.0,
                                ));
                                tracker.track_symbol(
                                    &mut access,
                                    symbol_token,
                                    Some(enclosing_token),
                                    tsc_emitter::EmitSymbolMeaning(111_551),
                                )?;
                            }
                        }
                        let component_symbol = checker
                            .get_symbol_of_declaration(component)
                            .map_err(|abort| abort_at(checker, component, abort))?;
                        let component_type = checker
                            .get_type_of_symbol(component_symbol)
                            .map_err(|abort| abort_at(checker, component, abort))?;
                        let type_node = crate::node_builder::type_to_type_node(
                            checker,
                            arena,
                            target,
                            component_type,
                            Some(enclosing_declaration),
                            Some(flags),
                            Some(internal_flags),
                            Some(tracker),
                            None,
                            None,
                            None,
                        )?;
                        let mut modifiers: Vec<tsc_emitter::TransformNode> = Vec::new();
                        if is_static {
                            modifiers.push(
                                arena
                                    .factory()
                                    .create_token(
                                        target,
                                        SyntaxKind::StaticKeyword,
                                        tsc_emitter::TransformFlags::NONE,
                                    )
                                    .map_err(factory_error)?,
                            );
                        }
                        if info.is_readonly {
                            modifiers.push(
                                arena
                                    .factory()
                                    .create_token(
                                        target,
                                        SyntaxKind::ReadonlyKeyword,
                                        tsc_emitter::TransformFlags::NONE,
                                    )
                                    .map_err(factory_error)?,
                            );
                        }
                        let has_question = matches!(
                            &checker.binder.source_of_node(component).arena.node(component).data,
                            NodeData::PropertySignature(data) if data.question_token.is_some()
                        ) || matches!(
                            &checker.binder.source_of_node(component).arena.node(component).data,
                            NodeData::PropertyDeclaration(data) if data.question_token.is_some()
                        ) || matches!(
                            &checker.binder.source_of_node(component).arena.node(component).data,
                            NodeData::MethodSignature(data) if data.question_token.is_some()
                        ) || matches!(
                            &checker.binder.source_of_node(component).arena.node(component).data,
                            NodeData::MethodDeclaration(data) if data.question_token.is_some()
                        );
                        let question_token = if has_question {
                            Some(
                                arena
                                    .factory()
                                    .create_token(
                                        target,
                                        SyntaxKind::QuestionToken,
                                        tsc_emitter::TransformFlags::NONE,
                                    )
                                    .map_err(factory_error)?
                                    .node(),
                            )
                        } else {
                            None
                        };
                        let name_in_arena = arena
                            .parse_tree_transform_node(resolver_node_at(checker, name))
                            .map_err(factory_error)?
                            .expect("component name is a mounted parse node");
                        let modifiers_array = if modifiers.is_empty() {
                            None
                        } else {
                            Some(type_nodes::create_node_array(arena, target, modifiers)?)
                        };
                        let property = arena
                            .factory()
                            .create_node(
                                target,
                                NodeData::PropertyDeclaration(
                                    tsc_syntax::nodes::PropertyDeclarationData {
                                        name: Some(name_in_arena.node()),
                                        modifiers: modifiers_array,
                                        question_token,
                                        exclamation_token: None,
                                        r#type: type_node.map(|node| node.node()),
                                        initializer: None,
                                    },
                                ),
                                tsc_emitter::TransformFlags::NONE,
                            )
                            .map_err(factory_error)?;
                        result.push(property);
                    }
                    continue;
                }
            }
            let node = crate::node_builder::index_info_to_index_signature_declaration(
                checker,
                arena,
                target,
                info,
                Some(enclosing_declaration),
                Some(flags),
                Some(internal_flags),
                Some(tracker),
            )?;
            if let Some(node) = node {
                let node = if is_static {
                    prepend_static_modifier(arena, target, node, info.is_readonly)
                        .map_err(factory_error)?
                } else {
                    node
                };
                result.push(node);
            }
        }
    }
    Ok(result)
}

fn resolver_node_at(
    checker: &crate::state::CheckerState<'_>,
    node: tsc_syntax::NodeId,
) -> tsc_emitter::EmitResolverNode {
    let source = u32::try_from(checker.binder.file_index_of_node(node)).unwrap_or(0);
    tsc_emitter::EmitResolverNode::from_raw_source(source, node)
}

/// The upstream static-modifier unshift (:88680-88683) recomposed without
/// node mutation. indexInfoToIndexSignatureDeclarationHelper synthesizes at
/// most the readonly modifier, so rebuilding [static, readonly?] from the
/// info is member-for-member identical to upstream's in-place unshift.
fn prepend_static_modifier(
    arena: &mut tsc_emitter::TransformArena,
    target: tsc_emitter::TransformSourceId,
    node: tsc_emitter::TransformNode,
    is_readonly: bool,
) -> Result<tsc_emitter::TransformNode, tsc_emitter::TransformError> {
    use tsc_syntax::{NodeData, SyntaxKind};
    let data = arena
        .source(node.source())?
        .syntax()
        .arena
        .node(node.node())
        .data
        .clone();
    let NodeData::IndexSignature(mut data) = data else {
        return Ok(node);
    };
    let mut factory = arena.factory();
    let mut modifiers = vec![factory.create_token(
        target,
        SyntaxKind::StaticKeyword,
        tsc_emitter::TransformFlags::NONE,
    )?];
    if is_readonly {
        modifiers.push(factory.create_token(
            target,
            SyntaxKind::ReadonlyKeyword,
            tsc_emitter::TransformFlags::NONE,
        )?);
    }
    let array = factory.create_node_array(target, modifiers)?;
    data.modifiers = Some(array.array());
    factory.update_node(
        node,
        NodeData::IndexSignature(data),
        tsc_emitter::TransformFlags::NONE,
    )
}

#[cfg(test)]
#[path = "../../tests/unit/node_builder_core/tests.rs"]
mod tests;

/// h2-7a-m-3 P5: the harness-only decision-lane sink. When armed by the
/// declaration replay observer, the NodeBuilder records the probe-comparable
/// decision events — withContext exits, syntactic front-door frames, and
/// tracker callbacks — for the union-domain replay. Production paths never
/// arm it; an unarmed sink records nothing.
pub(crate) mod replay_sink {
    use std::cell::RefCell;

    #[derive(Clone, Debug, PartialEq)]
    pub(crate) enum DecisionEvent {
        /// nodebuilder.withContext.result: (status, flags, internal_flags,
        /// approximate_length, type_stack_len, truncating, out_truncated,
        /// encountered_error, produced-node class).
        WithContextResult {
            status: &'static str,
            flags: u32,
            internal_flags: u32,
            approximate_length: u32,
            type_stack_len: usize,
            truncating: bool,
            out_truncated: bool,
            encountered_error: bool,
            produced: ProducedClass,
        },
        /// syntactic.serialize*.entry/.result frames.
        SyntacticFrame {
            site: &'static str,
            fallback: bool,
            produced: ProducedClass,
        },
        /// syntactic.*.checkerFallback markers.
        SyntacticFallback {
            site: &'static str,
            report_fallback: bool,
        },
        /// tracker.* callback records (payload projected by the recording
        /// tracker in the harness).
        Tracker {
            site: &'static str,
            payload: serde_json::Value,
        },
    }

    /// The §6.3 node-reference classes projected from a produced value.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum ProducedClass {
        Absent,
        ParseOwn { source: u32, node: u32 },
        OriginalProjected { source: u32, node: u32 },
        SyntheticWithoutOriginal,
        Container { length: usize },
    }

    thread_local! {
        static SINK: RefCell<Option<Vec<DecisionEvent>>> = const { RefCell::new(None) };
    }

    /// tsrs-native: harness decision-sink arming (h2-7a-m-3 §6).
    pub(crate) fn arm() {
        SINK.with(|sink| *sink.borrow_mut() = Some(Vec::new()));
    }

    /// tsrs-native: harness decision-sink drain.
    pub(crate) fn disarm() -> Vec<DecisionEvent> {
        SINK.with(|sink| sink.borrow_mut().take().unwrap_or_default())
    }

    /// tsrs-native: harness decision-sink append.
    pub(crate) fn record(event: impl FnOnce() -> DecisionEvent) {
        SINK.with(|sink| {
            if let Some(events) = sink.borrow_mut().as_mut() {
                events.push(event());
            }
        });
    }

    /// tsrs-native: harness decision-sink state probe.
    pub(crate) fn armed() -> bool {
        SINK.with(|sink| sink.borrow().is_some())
    }

    thread_local! {
        static SYNTACTIC_FRAMES: RefCell<Vec<bool>> = const { RefCell::new(Vec::new()) };
    }

    /// Enter a probed syntactic front-door frame (upstream
    /// __h27aProbeSyntacticCall pushes {fallback:false}).
    /// tsrs-native: harness syntactic-frame entry (h2-7a-m-3 §6.2).
    pub(crate) fn enter_syntactic_frame() {
        if armed() {
            SYNTACTIC_FRAMES.with(|frames| frames.borrow_mut().push(false));
        }
    }

    /// The __h27aMarkSyntacticFallback discipline: every OPEN frame flips to
    /// fallback, and the marker event records the reportFallback flag.
    /// tsrs-native: harness checkerFallback marker (probe protocol).
    pub(crate) fn mark_syntactic_fallback(site: &'static str, report_fallback: bool) {
        if !armed() {
            return;
        }
        SYNTACTIC_FRAMES.with(|frames| {
            for frame in frames.borrow_mut().iter_mut() {
                *frame = true;
            }
        });
        record(|| DecisionEvent::SyntacticFallback {
            site,
            report_fallback,
        });
    }

    /// Exit the probed frame, recording its fallback verdict + result class.
    /// tsrs-native: harness syntactic-frame exit record.
    pub(crate) fn exit_syntactic_frame(site: &'static str, produced: ProducedClass) {
        if !armed() {
            return;
        }
        let fallback = SYNTACTIC_FRAMES.with(|frames| frames.borrow_mut().pop().unwrap_or(false));
        record(|| DecisionEvent::SyntacticFrame {
            site,
            fallback,
            produced,
        });
    }
}

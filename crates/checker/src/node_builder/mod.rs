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
    SymbolTypeRestore, TrackedSymbol, DEFAULT_MAXIMUM_TRUNCATION_LENGTH,
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
    signature_to_signature_declaration_helper, symbol_to_parameter_declaration,
    type_parameter_to_declaration, type_predicate_to_type_predicate_node_helper,
    SignatureDeclarationOptions,
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

/// Owned cleanup returned by the checker-side `enterNewScope` callback.
/// It captures exactly the context slots restored by
/// `cloneNodeBuilderContext`, plus enclosing-declaration and mapper.
pub(crate) struct SyntacticScopeCleanup {
    enclosing_declaration: Option<tsc_syntax::NodeId>,
    mapper: Option<tsc_types::MapperId>,
    must_create_type_parameter_symbol_list: bool,
    type_parameter_symbol_list: Option<std::collections::HashSet<tsc_binder::SymbolId>>,
    must_create_type_parameters_names_lookups: bool,
    type_parameter_names:
        Option<std::collections::HashMap<tsc_types::TypeId, tsc_emitter::TransformNode>>,
    type_parameter_names_by_text: Option<std::collections::HashSet<String>>,
    type_parameter_names_by_text_next_name_count: Option<std::collections::HashMap<String, u32>>,
}

impl SyntacticScopeCleanup {
    pub(crate) fn capture(context: &NodeBuilderContext<'_>) -> Self {
        Self {
            enclosing_declaration: context.enclosing_declaration,
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
        }
    }

    pub(crate) fn restore(self, context: &mut NodeBuilderContext<'_>) {
        context.enclosing_declaration = self.enclosing_declaration;
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
    }
}

/// One `startRecoveryScope` snapshot inside a syntactic reuse boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SyntacticRecoveryScope {
    had_error: bool,
}

/// Object-safe Rust spelling of the four closures returned by upstream's
/// `createRecoveryBoundary`. Checker callbacks mark the context slot while
/// this owned token supplies start/recover/finalize lifetime discipline.
pub(crate) struct SyntacticRecoveryBoundary {
    previous_had_error: bool,
    previous_depth: u32,
}

impl SyntacticRecoveryBoundary {
    pub(crate) fn new(context: &mut NodeBuilderContext<'_>) -> Self {
        let previous_had_error = context.recovery_boundary_had_error;
        let previous_depth = context.recovery_boundary_depth;
        context.recovery_boundary_had_error = false;
        context.recovery_boundary_depth = previous_depth.saturating_add(1);
        Self {
            previous_had_error,
            previous_depth,
        }
    }

    pub(crate) fn had_error(&self, context: &NodeBuilderContext<'_>) -> bool {
        context.recovery_boundary_had_error
    }

    pub(crate) fn mark_error(&mut self, context: &mut NodeBuilderContext<'_>) {
        context.recovery_boundary_had_error = true;
    }

    pub(crate) fn start_recovery_scope(
        &self,
        context: &NodeBuilderContext<'_>,
    ) -> SyntacticRecoveryScope {
        SyntacticRecoveryScope {
            had_error: context.recovery_boundary_had_error,
        }
    }

    pub(crate) fn recover(
        &mut self,
        context: &mut NodeBuilderContext<'_>,
        scope: SyntacticRecoveryScope,
    ) {
        context.recovery_boundary_had_error = scope.had_error;
    }

    pub(crate) fn finalize(self, context: &mut NodeBuilderContext<'_>) -> bool {
        let succeeded = !context.recovery_boundary_had_error;
        context.recovery_boundary_had_error = self.previous_had_error;
        context.recovery_boundary_depth = self.previous_depth;
        succeeded
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
/// tsc-hash: 6a54cbb417327cf9ea18bd39a041f61e6598afe7c7bfa16ce46f0a54abf6bc27
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
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use tsc_binder::SymbolId;
    use tsc_emitter::{
        EmitFunctionProperty, EmitInternalNodeBuilderFlags, EmitNodeBuilderFlags,
        EmitResolverError, EmitResolverMethod, EmitResolverNode, EmitSymbolAccessibility,
        EmitSymbolAccessibilityResult, EmitSymbolExpansionOut, EmitSymbolMeaning,
        EmitSymbolTracker, EmitTrackerAccess, EmitTrackerNode, EmitTrackerNodeDescription,
        EmitTrackerSymbol, EmitTrackerSymbolDescription, SourceFileId, TransformArena,
    };
    use tsc_syntax::NodeId;
    use tsc_types::{CompilerOptions, SymbolFlags, TupleTargetFlags};

    use crate::state::test_support::with_program_state;

    use super::*;

    #[derive(Default)]
    struct TrackerLog {
        track_calls: Vec<(u64, Option<u64>, EmitSymbolMeaning)>,
        inference_fallbacks: Vec<u64>,
        truncation_errors: u32,
        fallback_stack_events: Vec<Option<u64>>,
        report_events: Vec<String>,
    }

    struct RecordingTracker {
        log: Rc<RefCell<TrackerLog>>,
        track_mode: Rc<Cell<u8>>,
        fail_inference_fallback: Rc<Cell<bool>>,
    }

    impl EmitSymbolTracker for RecordingTracker {
        fn can_track_symbol(&self) -> bool {
            true
        }

        fn track_symbol(
            &mut self,
            access: &mut dyn EmitTrackerAccess,
            symbol: EmitTrackerSymbol,
            enclosing_declaration: Option<EmitTrackerNode>,
            meaning: EmitSymbolMeaning,
        ) -> Result<bool, EmitResolverError> {
            self.log.borrow_mut().track_calls.push((
                symbol.0,
                enclosing_declaration.map(|node| node.0),
                meaning,
            ));
            match self.track_mode.get() {
                0 => Ok(false),
                1 => Ok(true),
                2 => access.is_expando_function_declaration(
                    enclosing_declaration.unwrap_or(EmitTrackerNode(0)),
                ),
                mode => panic!("unexpected tracker mode {mode}"),
            }
        }

        fn report_inference_fallback(
            &mut self,
            access: &mut dyn EmitTrackerAccess,
            node: EmitTrackerNode,
        ) -> Result<(), EmitResolverError> {
            self.log.borrow_mut().inference_fallbacks.push(node.0);
            if self.fail_inference_fallback.get() {
                access.is_expando_function_declaration(node)?;
            }
            Ok(())
        }

        fn report_truncation_error(&mut self) {
            self.log.borrow_mut().truncation_errors += 1;
        }

        fn report_inaccessible_this_error(&mut self) {
            self.log
                .borrow_mut()
                .report_events
                .push("inaccessible-this".to_owned());
        }

        fn report_private_in_base_of_class_expression(&mut self, property_name: &str) {
            self.log
                .borrow_mut()
                .report_events
                .push(format!("private-base:{property_name}"));
        }

        fn report_inaccessible_unique_symbol_error(&mut self) {
            self.log
                .borrow_mut()
                .report_events
                .push("inaccessible-unique".to_owned());
        }

        fn report_cyclic_structure_error(&mut self) {
            self.log
                .borrow_mut()
                .report_events
                .push("cyclic".to_owned());
        }

        fn report_likely_unsafe_import_required_error(
            &mut self,
            specifier: &str,
            symbol_name: Option<&str>,
        ) {
            self.log.borrow_mut().report_events.push(format!(
                "unsafe-import:{specifier}:{}",
                symbol_name.unwrap_or_default()
            ));
        }

        fn report_nonlocal_augmentation(
            &mut self,
            containing_file: EmitTrackerNode,
            parent_symbol: EmitTrackerSymbol,
            augmenting_symbol: EmitTrackerSymbol,
        ) {
            self.log.borrow_mut().report_events.push(format!(
                "nonlocal:{}:{}:{}",
                containing_file.0, parent_symbol.0, augmenting_symbol.0
            ));
        }

        fn report_non_serializable_property(&mut self, property_name: &str) {
            self.log
                .borrow_mut()
                .report_events
                .push(format!("nonserial:{property_name}"));
        }

        fn push_error_fallback_node(&mut self, node: Option<EmitTrackerNode>) {
            self.log
                .borrow_mut()
                .fallback_stack_events
                .push(node.map(|node| node.0));
        }

        fn pop_error_fallback_node(&mut self) {
            self.log.borrow_mut().fallback_stack_events.push(None);
        }
    }

    struct MockTrackerAccess {
        fail: bool,
    }

    impl MockTrackerAccess {
        fn maybe_fail<T>(
            &self,
            method: EmitResolverMethod,
            node: EmitTrackerNode,
            value: T,
        ) -> Result<T, EmitResolverError> {
            if self.fail {
                Err(EmitResolverError::Unavailable {
                    method,
                    node: EmitResolverNode::from_raw_source(0, NodeId(node.0 as u32)),
                })
            } else {
                Ok(value)
            }
        }
    }

    impl EmitTrackerAccess for MockTrackerAccess {
        fn is_symbol_accessible(
            &mut self,
            _symbol: EmitTrackerSymbol,
            enclosing_declaration: Option<EmitTrackerNode>,
            _meaning: EmitSymbolMeaning,
            _should_compute_aliases: bool,
        ) -> Result<EmitSymbolAccessibilityResult, EmitResolverError> {
            self.maybe_fail(
                EmitResolverMethod::IsSymbolAccessible,
                enclosing_declaration.unwrap_or(EmitTrackerNode(0)),
                EmitSymbolAccessibilityResult {
                    accessibility: EmitSymbolAccessibility::Accessible,
                    aliases_to_make_visible: None,
                    error_symbol_name: None,
                    error_module_name: None,
                    error_node: None,
                },
            )
        }

        fn is_expando_function_declaration(
            &mut self,
            node: EmitTrackerNode,
        ) -> Result<bool, EmitResolverError> {
            self.maybe_fail(
                EmitResolverMethod::IsExpandoFunctionDeclaration,
                node,
                false,
            )
        }

        fn get_properties_of_container_function(
            &mut self,
            node: EmitTrackerNode,
        ) -> Result<Vec<EmitFunctionProperty>, EmitResolverError> {
            self.maybe_fail(
                EmitResolverMethod::GetPropertiesOfContainerFunction,
                node,
                Vec::new(),
            )
        }

        fn requires_adding_implicit_undefined(
            &mut self,
            parameter: EmitTrackerNode,
            _enclosing_declaration: Option<EmitTrackerNode>,
        ) -> Result<bool, EmitResolverError> {
            self.maybe_fail(
                EmitResolverMethod::RequiresAddingImplicitUndefined,
                parameter,
                false,
            )
        }

        fn describe_symbol(&mut self, _symbol: EmitTrackerSymbol) -> EmitTrackerSymbolDescription {
            EmitTrackerSymbolDescription::default()
        }

        fn describe_node(&mut self, _node: EmitTrackerNode) -> EmitTrackerNodeDescription {
            EmitTrackerNodeDescription::default()
        }
    }

    struct RecordingFixture {
        tracker: RecordingTracker,
        log: Rc<RefCell<TrackerLog>>,
        track_mode: Rc<Cell<u8>>,
        fail_inference_fallback: Rc<Cell<bool>>,
    }

    fn recording_tracker() -> RecordingFixture {
        let log = Rc::new(RefCell::new(TrackerLog::default()));
        let track_mode = Rc::new(Cell::new(0));
        let fail_inference_fallback = Rc::new(Cell::new(false));
        RecordingFixture {
            tracker: RecordingTracker {
                log: Rc::clone(&log),
                track_mode: Rc::clone(&track_mode),
                fail_inference_fallback: Rc::clone(&fail_inference_fallback),
            },
            log,
            track_mode,
            fail_inference_fallback,
        }
    }

    fn with_test_context<R>(
        options: &CompilerOptions,
        tracker: Option<&mut dyn EmitSymbolTracker>,
        run: impl FnOnce(&mut crate::state::CheckerState<'_>, &mut NodeBuilderContext<'_>) -> R,
    ) -> R {
        with_program_state(&[("/main.ts", "export {};\n")], options, |checker| {
            let root = checker.binder.source(0).root;
            let mut arena = TransformArena::new();
            let target =
                arena.add_source(checker.binder.source(0), Some(SourceFileId::from_raw(0)));
            let mut result = None;
            with_context(
                checker,
                &mut arena,
                target,
                Some(root),
                None,
                None,
                tracker,
                None,
                None,
                |checker, _arena, _target, context| {
                    result = Some(run(checker, context));
                    Ok(())
                },
                None,
            )
            .expect("withContext succeeds");
            result.expect("callback ran")
        })
    }

    #[test]
    fn node_builder_context_construction_uses_upstream_defaults_and_bundled_gate() {
        let options = CompilerOptions {
            out_file: Some("/dist/bundle.js".to_owned()),
            ..CompilerOptions::default()
        };
        with_program_state(&[("/main.ts", "export {};\n")], &options, |checker| {
            let root = checker.binder.source(0).root;
            let mut arena = TransformArena::new();
            let target =
                arena.add_source(checker.binder.source(0), Some(SourceFileId::from_raw(0)));
            let value = with_context(
                checker,
                &mut arena,
                target,
                Some(root),
                None,
                None,
                None,
                None,
                None,
                |_checker, _arena, actual_target, context| {
                    assert_eq!(actual_target, target);
                    assert_eq!(context.enclosing_declaration, Some(root));
                    assert_eq!(context.enclosing_file, Some(root));
                    assert_eq!(context.flags, EmitNodeBuilderFlags::NONE);
                    assert_eq!(context.internal_flags, EmitInternalNodeBuilderFlags::NONE);
                    assert_eq!(
                        context.max_truncation_length,
                        DEFAULT_MAXIMUM_TRUNCATION_LENGTH
                    );
                    assert_eq!(context.max_expansion_depth, -1);
                    assert!(!context.encountered_error);
                    assert!(!context.suppress_report_inference_fallback);
                    assert!(!context.reported_diagnostic);
                    assert!(context.visited_types.is_none());
                    assert!(context.symbol_depth.is_none());
                    assert!(context.infer_type_parameters.is_none());
                    assert_eq!(context.approximate_length, 0);
                    assert!(context.tracked_symbols.is_none());
                    assert!(context.bundled);
                    assert!(!context.truncating);
                    assert!(context.used_symbol_names.is_none());
                    assert!(context.remapped_symbol_names.is_none());
                    assert!(context.remapped_symbol_references.is_none());
                    assert!(context.reverse_mapped_stack.is_none());
                    assert!(context.must_create_type_parameter_symbol_list);
                    assert!(context.type_parameter_symbol_list.is_none());
                    assert!(context.must_create_type_parameters_names_lookups);
                    assert!(context.type_parameter_names.is_none());
                    assert!(context.type_parameter_names_by_text.is_none());
                    assert!(context
                        .type_parameter_names_by_text_next_name_count
                        .is_none());
                    assert!(context.enclosing_symbol_types.is_empty());
                    assert!(context.mapper.is_none());
                    assert_eq!(context.depth, 0);
                    assert!(context.type_stack.is_empty());
                    assert_eq!(context.out, EmitSymbolExpansionOut::default());
                    assert!(context.no_inference_fallback.is_none());
                    assert!(!context.recovery_boundary_had_error);
                    assert_eq!(context.recovery_boundary_depth, 0);
                    assert!(context.tracker.inner.is_none());
                    assert!(!context.tracker.can_track_symbol);
                    assert!(context.tracker.uses_basic_module_resolver_host);
                    assert!(context.tracker.caller_module_resolver_host().is_none());
                    Ok(17_u32)
                },
                None,
            )
            .expect("withContext succeeds");
            assert_eq!(value, Some(17));
        });
    }

    #[test]
    fn node_builder_with_context_truncation_arm_out_copy_and_error_gate_match_upstream() {
        let RecordingFixture {
            mut tracker, log, ..
        } = recording_tracker();
        let options = CompilerOptions::default();
        with_program_state(&[("/main.ts", "export {};\n")], &options, |checker| {
            let root = checker.binder.source(0).root;
            let mut arena = TransformArena::new();
            let target =
                arena.add_source(checker.binder.source(0), Some(SourceFileId::from_raw(0)));

            let ordinary = with_context(
                checker,
                &mut arena,
                target,
                Some(root),
                None,
                None,
                Some(&mut tracker),
                None,
                None,
                |_checker, _arena, _target, context| {
                    assert_eq!(
                        context.max_truncation_length,
                        DEFAULT_MAXIMUM_TRUNCATION_LENGTH
                    );
                    context.truncating = true;
                    Ok(1_u8)
                },
                None,
            )
            .expect("ordinary context succeeds");
            assert_eq!(ordinary, Some(1));
            assert_eq!(log.borrow().truncation_errors, 0);

            let mut out = EmitSymbolExpansionOut::default();
            let no_truncation = with_context(
                checker,
                &mut arena,
                target,
                Some(root),
                Some(EmitNodeBuilderFlags::NO_TRUNCATION),
                None,
                Some(&mut tracker),
                None,
                Some(4),
                |_checker, _arena, _target, context| {
                    assert_eq!(
                        context.max_truncation_length,
                        NO_TRUNCATION_MAXIMUM_TRUNCATION_LENGTH
                    );
                    assert_eq!(context.max_expansion_depth, 4);
                    context.truncating = true;
                    context.encountered_error = true;
                    context.out.can_increase_expansion_depth = true;
                    context.out.truncated = true;
                    Ok(2_u8)
                },
                Some(&mut out),
            )
            .expect("NoTruncation context succeeds");
            assert_eq!(no_truncation, None);
            assert_eq!(log.borrow().truncation_errors, 1);
            assert_eq!(
                out,
                EmitSymbolExpansionOut {
                    can_increase_expansion_depth: true,
                    truncated: true,
                }
            );

            with_context(
                checker,
                &mut arena,
                target,
                Some(root),
                Some(EmitNodeBuilderFlags::NO_TRUNCATION),
                None,
                Some(&mut tracker),
                Some(0),
                None,
                |_checker, _arena, _target, context| {
                    // Upstream `maximumLength || …` treats zero as absent
                    // (:51208): the flag-selected default wins.
                    assert_eq!(
                        context.max_truncation_length,
                        NO_TRUNCATION_MAXIMUM_TRUNCATION_LENGTH
                    );
                    Ok(())
                },
                None,
            )
            .expect("explicit maximum succeeds");
        });
    }

    #[test]
    fn node_builder_tracker_forwards_gates_and_records_only_non_type_parameters() {
        let RecordingFixture {
            mut tracker,
            log,
            track_mode,
            ..
        } = recording_tracker();
        let options = CompilerOptions::default();
        with_test_context(&options, Some(&mut tracker), |_checker, context| {
            let mut access = MockTrackerAccess { fail: false };
            let property = SymbolId(11);
            let enclosing = Some(NodeId(22));
            assert!(!context
                .tracker
                .track_symbol(
                    &mut context.reported_diagnostic,
                    &mut context.tracked_symbols,
                    &mut access,
                    property,
                    SymbolFlags::PROPERTY,
                    enclosing,
                    EmitSymbolMeaning::TYPE,
                )
                .expect("false tracker result"));
            assert_eq!(
                context.tracked_symbols.as_deref(),
                Some(&[(property, enclosing, EmitSymbolMeaning::TYPE)][..])
            );
            assert!(!context.reported_diagnostic);

            context.tracker.disable_track_symbol = true;
            assert!(!context
                .tracker
                .track_symbol(
                    &mut context.reported_diagnostic,
                    &mut context.tracked_symbols,
                    &mut access,
                    SymbolId(12),
                    SymbolFlags::PROPERTY,
                    enclosing,
                    EmitSymbolMeaning::VALUE_EXPORT_VALUE,
                )
                .expect("disabled tracker result"));
            assert_eq!(log.borrow().track_calls.len(), 1);
            context.tracker.disable_track_symbol = false;

            context.tracked_symbols = None;
            assert!(!context
                .tracker
                .track_symbol(
                    &mut context.reported_diagnostic,
                    &mut context.tracked_symbols,
                    &mut access,
                    SymbolId(13),
                    SymbolFlags::TYPE_PARAMETER,
                    enclosing,
                    EmitSymbolMeaning::TYPE,
                )
                .expect("type-parameter tracker result"));
            assert!(context.tracked_symbols.is_none());

            track_mode.set(1);
            assert!(context
                .tracker
                .track_symbol(
                    &mut context.reported_diagnostic,
                    &mut context.tracked_symbols,
                    &mut access,
                    SymbolId(14),
                    SymbolFlags::PROPERTY,
                    enclosing,
                    EmitSymbolMeaning::TYPE,
                )
                .expect("diagnostic tracker result"));
            assert!(context.reported_diagnostic);

            context.reported_diagnostic = false;
            context.suppress_report_inference_fallback = true;
            context
                .tracker
                .report_inference_fallback(
                    &mut context.reported_diagnostic,
                    context.suppress_report_inference_fallback,
                    &mut access,
                    NodeId(30),
                )
                .expect("suppressed inference fallback");
            assert!(!context.reported_diagnostic);
            assert!(log.borrow().inference_fallbacks.is_empty());

            context.suppress_report_inference_fallback = false;
            context
                .tracker
                .report_inference_fallback(
                    &mut context.reported_diagnostic,
                    context.suppress_report_inference_fallback,
                    &mut access,
                    NodeId(31),
                )
                .expect("forwarded inference fallback");
            assert!(context.reported_diagnostic);
            assert_eq!(log.borrow().inference_fallbacks, vec![31]);

            context.tracker.push_error_fallback_node(Some(NodeId(40)));
            context.tracker.pop_error_fallback_node();
            assert_eq!(log.borrow().fallback_stack_events, vec![Some(40), None]);

            context.reported_diagnostic = false;
            context
                .tracker
                .report_inaccessible_this_error(&mut context.reported_diagnostic);
            assert!(context.reported_diagnostic);
            context.reported_diagnostic = false;
            context.tracker.report_private_in_base_of_class_expression(
                &mut context.reported_diagnostic,
                "privateName",
            );
            assert!(context.reported_diagnostic);
            context.reported_diagnostic = false;
            context
                .tracker
                .report_inaccessible_unique_symbol_error(&mut context.reported_diagnostic);
            assert!(context.reported_diagnostic);
            context.reported_diagnostic = false;
            context
                .tracker
                .report_cyclic_structure_error(&mut context.reported_diagnostic);
            assert!(context.reported_diagnostic);
            context.reported_diagnostic = false;
            context.tracker.report_likely_unsafe_import_required_error(
                &mut context.reported_diagnostic,
                "pkg",
                Some("Thing"),
            );
            assert!(context.reported_diagnostic);
            context.reported_diagnostic = false;
            context.tracker.report_nonlocal_augmentation(
                &mut context.reported_diagnostic,
                NodeId(41),
                SymbolId(42),
                SymbolId(43),
            );
            assert!(context.reported_diagnostic);
            context.reported_diagnostic = false;
            context
                .tracker
                .report_non_serializable_property(&mut context.reported_diagnostic, "property");
            assert!(context.reported_diagnostic);
            assert_eq!(
                log.borrow().report_events,
                vec![
                    "inaccessible-this",
                    "private-base:privateName",
                    "inaccessible-unique",
                    "cyclic",
                    "unsafe-import:pkg:Thing",
                    "nonlocal:41:42:43",
                    "nonserial:property",
                ]
            );

            let mut absent = NodeBuilderTracker::new(None);
            let mut absent_reported = false;
            absent.report_cyclic_structure_error(&mut absent_reported);
            assert!(!absent_reported);
        });
    }

    #[test]
    fn node_builder_tracker_propagates_fallible_access_errors_fail_closed() {
        let RecordingFixture {
            mut tracker,
            track_mode,
            fail_inference_fallback,
            ..
        } = recording_tracker();
        track_mode.set(2);
        fail_inference_fallback.set(true);
        let options = CompilerOptions::default();
        with_test_context(&options, Some(&mut tracker), |_checker, context| {
            let mut access = MockTrackerAccess { fail: true };
            let track_error = context.tracker.track_symbol(
                &mut context.reported_diagnostic,
                &mut context.tracked_symbols,
                &mut access,
                SymbolId(1),
                SymbolFlags::PROPERTY,
                Some(NodeId(2)),
                EmitSymbolMeaning::TYPE,
            );
            assert!(matches!(
                track_error,
                Err(EmitResolverError::Unavailable {
                    method: EmitResolverMethod::IsExpandoFunctionDeclaration,
                    ..
                })
            ));
            assert!(!context.reported_diagnostic);
            assert!(context.tracked_symbols.is_none());

            let inference_error = context.tracker.report_inference_fallback(
                &mut context.reported_diagnostic,
                false,
                &mut access,
                NodeId(3),
            );
            assert!(matches!(
                inference_error,
                Err(EmitResolverError::Unavailable {
                    method: EmitResolverMethod::IsExpandoFunctionDeclaration,
                    ..
                })
            ));
            assert!(context.reported_diagnostic);
        });
    }

    #[test]
    fn node_builder_save_restore_and_expansion_helpers_restore_all_owned_state() {
        let options = CompilerOptions::default();
        with_test_context(&options, None, |checker, context| {
            let symbol = SymbolId(70);
            let absent_symbol = SymbolId(71);
            let old_type = checker.tables.intrinsics.any;
            let new_type = checker.tables.intrinsics.string;
            context.enclosing_symbol_types.insert(symbol, old_type);

            let restore = add_symbol_type_to_context(context, symbol, new_type);
            assert_eq!(context.enclosing_symbol_types.get(&symbol), Some(&new_type));
            restore_symbol_type_to_context(context, restore);
            assert_eq!(context.enclosing_symbol_types.get(&symbol), Some(&old_type));

            let restore = add_symbol_type_to_context(context, absent_symbol, new_type);
            assert_eq!(
                context.enclosing_symbol_types.get(&absent_symbol),
                Some(&new_type)
            );
            restore_symbol_type_to_context(context, restore);
            assert!(!context.enclosing_symbol_types.contains_key(&absent_symbol));

            context.flags = EmitNodeBuilderFlags::NO_TRUNCATION;
            context.internal_flags = EmitInternalNodeBuilderFlags::WRITE_COMPUTED_PROPS;
            context.depth = 7;
            let restore = save_restore_flags(context);
            context.flags = EmitNodeBuilderFlags::NONE;
            context.internal_flags = EmitInternalNodeBuilderFlags::NONE;
            context.depth = 99;
            restore_flags(context, restore);
            assert_eq!(context.flags, EmitNodeBuilderFlags::NO_TRUNCATION);
            assert_eq!(
                context.internal_flags,
                EmitInternalNodeBuilderFlags::WRITE_COMPUTED_PROPS
            );
            assert_eq!(context.depth, 7);

            context.no_inference_fallback = Some(false);
            context.recovery_boundary_had_error = true;
            context.recovery_boundary_depth = 3;
            let old_no_inference = save_no_inference_fallback(context);
            assert!(no_inference_fallback_is_set(context));
            restore_no_inference_fallback(context, old_no_inference);
            assert_eq!(context.no_inference_fallback, Some(false));
            assert!(context.recovery_boundary_had_error);
            assert_eq!(context.recovery_boundary_depth, 3);

            context.max_truncation_length = 5;
            context.approximate_length = 5;
            context.truncating = false;
            assert!(!check_truncation_length(context));
            context.approximate_length = 6;
            assert!(check_truncation_length(context));
            context.approximate_length = 0;
            assert!(check_truncation_length(context));

            context.truncating = false;
            context.max_expansion_depth = -1;
            context.approximate_length = 6;
            assert!(!check_truncation_length_if_expanding(context));
            assert!(!context.truncating);
            context.max_expansion_depth = 1;
            assert!(check_truncation_length_if_expanding(context));

            context.depth = 1;
            context.out.can_increase_expansion_depth = false;
            context.type_stack.clear();
            assert!(can_possibly_expand_type(old_type, context));
            context.out.can_increase_expansion_depth = true;
            assert!(!can_possibly_expand_type(old_type, context));
            context.out.can_increase_expansion_depth = false;
            context.type_stack = vec![Some(old_type), None];
            assert!(!can_possibly_expand_type(old_type, context));

            context.type_stack.clear();
            context.depth = 0;
            context.max_expansion_depth = 1;
            context.out.can_increase_expansion_depth = false;
            assert!(should_expand_type(checker, old_type, context, false));
            context.depth = 1;
            assert!(!should_expand_type(checker, old_type, context, false));
            assert!(context.out.can_increase_expansion_depth);

            context.out.can_increase_expansion_depth = false;
            context.type_stack = vec![Some(old_type), None];
            assert!(!should_expand_type(checker, old_type, context, false));
            assert!(!context.out.can_increase_expansion_depth);

            let empty_flags = [];
            let tuple = checker.tables.get_tuple_target_type(
                TupleTargetFlags::new(&empty_flags).expect("empty tuple flags"),
                false,
                None,
            );
            context.type_stack.clear();
            context.depth = 0;
            context.max_expansion_depth = 1;
            assert!(!should_expand_type(checker, tuple, context, false));
            assert!(should_expand_type(checker, tuple, context, true));
        });
    }
}

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

    pub(crate) fn arm() {
        SINK.with(|sink| *sink.borrow_mut() = Some(Vec::new()));
    }

    pub(crate) fn disarm() -> Vec<DecisionEvent> {
        SINK.with(|sink| sink.borrow_mut().take().unwrap_or_default())
    }

    pub(crate) fn record(event: impl FnOnce() -> DecisionEvent) {
        SINK.with(|sink| {
            if let Some(events) = sink.borrow_mut().as_mut() {
                events.push(event());
            }
        });
    }

    pub(crate) fn armed() -> bool {
        SINK.with(|sink| sink.borrow().is_some())
    }

    thread_local! {
        static SYNTACTIC_FRAMES: RefCell<Vec<bool>> = const { RefCell::new(Vec::new()) };
    }

    /// Enter a probed syntactic front-door frame (upstream
    /// __h27aProbeSyntacticCall pushes {fallback:false}).
    pub(crate) fn enter_syntactic_frame() {
        if armed() {
            SYNTACTIC_FRAMES.with(|frames| frames.borrow_mut().push(false));
        }
    }

    /// The __h27aMarkSyntacticFallback discipline: every OPEN frame flips to
    /// fallback, and the marker event records the reportFallback flag.
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

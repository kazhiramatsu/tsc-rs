pub(crate) mod chains;
mod context;
mod pseudo;
mod serialize;
mod signatures;
pub(crate) mod specifier;
mod tracker;
mod type_nodes;

pub(crate) use crate::syntactic_type_node_builder::SyntacticTypeNodeBuilder;
pub(crate) use chains::{
    chains_get_property_name_node_for_symbol, chains_symbol_to_entity_name_node,
    chains_symbol_to_expression, chains_symbol_to_type_node,
    existing_type_node_is_not_reference_or_is_reference_with_compatible_type_argument_count,
    get_enclosing_declaration_ignoring_fake_scope, get_module_specifier_override,
    get_type_from_type_node2, set_text_range2, symbol_to_node, type_parameter_to_name,
};
pub(crate) use context::{
    add_symbol_type_to_context, can_possibly_expand_type, check_truncation_length, restore_flags,
    restore_symbol_type_to_context, save_restore_flags, should_expand_type, with_context,
    with_context_in_synthetic_module_scope, NodeBuilderContext, RecoveryTrackedSymbol,
    SyntheticModuleScope, TrackedSymbol,
};
pub(crate) use serialize::{
    build_symbol_display_node, index_info_to_index_signature_declaration,
    serialize_return_type_for_signature, serialize_return_type_for_signature_seam,
    serialize_type_for_declaration, serialize_type_for_declaration_seam,
    serialize_type_for_expression, serialize_type_for_symbolless_declaration,
    serialize_type_parameters_for_signature, syntactic_serialize_name_of_parameter_seam,
    syntactic_try_reuse_existing_type_node, try_js_type_node_to_type_node, type_to_type_node,
};
pub(crate) use signatures::{
    enter_new_scope, index_info_to_index_signature_declaration_helper,
    type_predicate_to_type_predicate_node_helper,
};
pub(crate) use tracker::tracker_symbol;
pub(crate) use type_nodes::{create_factory_node, update_factory_node};
pub(crate) use type_nodes::{map_to_type_nodes, type_to_type_node_helper};

pub(crate) struct SyntheticModuleScopeRestore {
    pub(crate) enclosing_declaration: Option<tsc_syntax::NodeId>,
    pub(crate) enclosing_declaration_is_synthetic: bool,
    pub(crate) synthetic_scope_locals:
        Option<rustc_hash::FxHashMap<tsc_types::EscapedName, tsc_binder::SymbolId>>,
    pub(crate) synthetic_type_param_names: Vec<tsc_types::EscapedName>,
    pub(crate) synthetic_type_params_scope_active: bool,
    pub(crate) synthetic_scope_kind: Option<tsc_syntax::SyntaxKind>,
    pub(crate) fake_scopes: context::FakeScopeChain,
}

/// tsrs-native: shared install frame for the existing synthetic module-scope overlay.
pub(crate) fn with_synthetic_module_scope(
    context: &mut NodeBuilderContext<'_>,
    enclosing_declaration: Option<tsc_syntax::NodeId>,
    locals: &tsc_binder::SymbolTable,
) -> SyntheticModuleScopeRestore {
    let restore = SyntheticModuleScopeRestore {
        enclosing_declaration: context.enclosing_declaration,
        enclosing_declaration_is_synthetic: context.enclosing_declaration_is_synthetic,
        synthetic_scope_locals: context.synthetic_scope_locals.clone(),
        synthetic_type_param_names: std::mem::take(&mut context.synthetic_type_param_names),
        synthetic_type_params_scope_active: std::mem::replace(
            &mut context.synthetic_type_params_scope_active,
            false,
        ),
        synthetic_scope_kind: context.synthetic_scope_kind,
        fake_scopes: context.fake_scopes,
    };
    context.enclosing_declaration = enclosing_declaration;
    context.enclosing_declaration_is_synthetic = true;
    context.replace_fake_scopes_with_new_enclosing();
    context.synthetic_scope_kind = Some(tsc_syntax::SyntaxKind::ModuleDeclaration);
    context.synthetic_scope_locals = Some(
        locals
            .iter()
            .map(|(name, &symbol)| (*name, symbol))
            .collect(),
    );
    restore
}

/// tsrs-native: shared restore half of the synthetic module-scope overlay frame.
pub(crate) fn restore_synthetic_module_scope(
    context: &mut NodeBuilderContext<'_>,
    restore: SyntheticModuleScopeRestore,
) {
    context.enclosing_declaration = restore.enclosing_declaration;
    context.enclosing_declaration_is_synthetic = restore.enclosing_declaration_is_synthetic;
    context.synthetic_scope_locals = restore.synthetic_scope_locals;
    context.synthetic_type_param_names = restore.synthetic_type_param_names;
    context.synthetic_type_params_scope_active = restore.synthetic_type_params_scope_active;
    context.synthetic_scope_kind = restore.synthetic_scope_kind;
    context.fake_scopes = restore.fake_scopes;
}

/// tsrs-native: stable checker-node projection prepared before a tracker callback.
pub(crate) fn tracker_node_description(
    checker: &crate::state::CheckerState<'_>,
    node: tsc_syntax::NodeId,
) -> tsc_emitter::EmitTrackerNodeDescription {
    let file_index = checker.binder.file_index_of_node(node);
    let source = checker
        .authoritative_source_tokens
        .get(file_index)
        .map_or_else(|| u32::try_from(file_index).unwrap_or(0), |token| token.0);
    tsc_emitter::EmitTrackerNodeDescription {
        parse: Some(tsc_emitter::EmitResolverNode::from_raw_source(source, node)),
        original: None,
    }
}

/// The entity-in-type arms of tsgo's getIsolatedDeclarationError
/// (declarations/diagnostics.go:707-712): a node in a type or a type query,
/// and any entity name.
fn tracker_is_entity_in_type_node(
    checker: &crate::state::CheckerState<'_>,
    node: tsc_syntax::NodeId,
) -> bool {
    use tsc_syntax::SyntaxKind;
    checker.is_part_of_type_node(node)
        || matches!(
            checker.kind_of(node),
            SyntaxKind::TypeQuery | SyntaxKind::Identifier | SyntaxKind::QualifiedName
        )
        || checker.is_entity_name_expression(node)
}

/// tsgo-port: SymbolTrackerImpl.isChildOfBoundExpando @7.1 (declarations/tracker.go:59-83):
/// an ancestor below the enclosing block is an assignment `f.x = …` whose
/// leftmost name refers to an expando function.
fn tracker_is_child_of_bound_expando(
    checker: &mut crate::state::CheckerState<'_>,
    node: tsc_syntax::NodeId,
) -> crate::state::CheckResult<bool> {
    use tsc_syntax::{NodeData, SyntaxKind};
    let mut current = Some(node);
    while let Some(candidate) = current {
        if matches!(
            checker.kind_of(candidate),
            SyntaxKind::SourceFile | SyntaxKind::Block
        ) {
            return Ok(false);
        }
        // isBoundExpando: only an assignment rooted at an identifier
        // (`f.x = …`) binds an expando property.
        if let NodeData::BinaryExpression(data) = checker.data_of(candidate) {
            if let Some(left) = data
                .left
                .filter(|&left| checker.kind_of(left) == SyntaxKind::PropertyAccessExpression)
            {
                // GetLeftmostAccessExpression.
                let mut leftmost = left;
                loop {
                    let expression = match checker.data_of(leftmost) {
                        NodeData::PropertyAccessExpression(access) => access.expression,
                        NodeData::ElementAccessExpression(access) => access.expression,
                        _ => break,
                    };
                    match expression {
                        Some(expression) => leftmost = expression,
                        None => break,
                    }
                }
                if checker.kind_of(leftmost) == SyntaxKind::Identifier {
                    if let Some(reference) =
                        checker.emit_get_referenced_value_declaration(leftmost)?
                    {
                        if checker.emit_is_expando_function_declaration(reference)? {
                            return Ok(true);
                        }
                    }
                }
            }
        }
        current = checker.parent_of(candidate);
    }
    Ok(false)
}

/// tsc-port: getAllAccessorDeclarations @6.0.3
/// tsc-hash: 8e23b58d85c286c6344992bac81b90a2c92285508dcf40a9c80d316dca13286a
/// tsc-span: _tsc.js:16719-16760
/// Project the accessor pair consumed by the diagnostic. The syntactic
/// builder's getAllAccessorDeclarationsForDeclaration is a different owner.
fn tracker_accessor_declarations(
    checker: &crate::state::CheckerState<'_>,
    node: tsc_syntax::NodeId,
) -> Option<tsc_emitter::EmitAccessorDeclarations> {
    use tsc_syntax::SyntaxKind;
    let kind = checker.kind_of(node);
    if !matches!(kind, SyntaxKind::GetAccessor | SyntaxKind::SetAccessor) {
        return None;
    }
    let source = checker.binder.source_of_node(node);
    let mut result = tsc_emitter::EmitAccessorDeclarations::default();
    if tsc_binder::node_util::has_dynamic_name(source, node) {
        let description = Some(tracker_node_description(checker, node));
        if kind == SyntaxKind::GetAccessor {
            result.get_accessor = description;
        } else {
            result.set_accessor = description;
        }
        return Some(result);
    }
    let symbol = checker.binder.node_symbol(node)?;
    let name = |node| {
        tsc_binder::node_util::get_name_of_declaration(checker.binder.source_of_node(node), node)
            .and_then(|name| checker.property_name_for_property_name_node(name))
    };
    let accessor_name = name(node);
    let accessor_static = checker.has_static_modifier(node);
    for &member in &checker.binder.symbol(symbol).declarations {
        let member_kind = checker.kind_of(member);
        if matches!(
            member_kind,
            SyntaxKind::GetAccessor | SyntaxKind::SetAccessor
        ) && checker.has_static_modifier(member) == accessor_static
            && name(member) == accessor_name
        {
            let slot = if member_kind == SyntaxKind::GetAccessor {
                &mut result.get_accessor
            } else {
                &mut result.set_accessor
            };
            if slot.is_none() {
                *slot = Some(tracker_node_description(checker, member));
            }
        }
    }
    Some(result)
}

/// Rust spelling of `trackExistingEntityName`'s two-field result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SyntacticTrackedEntityName {
    pub(crate) node: tsc_emitter::TransformNode,
    pub(crate) introduces_error: bool,
}

/// Owned cleanup returned by the checker-side `enterNewScope` callback.
/// It captures exactly the context slots restored by
/// `cloneNodeBuilderContext`, plus enclosing-declaration and mapper, and the
/// fake scopes' locals.
///
/// tsgo's cleanup (checker/nodebuilderscopes.go:142-152, 243-252) removes
/// every name the scope added to a fake scope it reused and puts back every
/// name it replaced; a fake scope the scope created is dropped with the
/// restored enclosing declaration. Either way the locals are as they were
/// before the scope. (tsc 6.0 passed `Map.delete`/`Map.set` to its
/// short-circuiting `forEach`, which undid only the first of each; a sibling
/// conditional type's `infer _E` then became `_E_1`.)
pub(crate) struct SyntacticScopeCleanup {
    enclosing_declaration: Option<tsc_syntax::NodeId>,
    enclosing_declaration_is_synthetic: bool,
    mapper: Option<tsc_types::MapperId>,
    must_create_type_parameter_symbol_list: bool,
    type_parameter_symbol_list: Option<rustc_hash::FxHashSet<tsc_binder::SymbolId>>,
    must_create_type_parameters_names_lookups: bool,
    type_parameter_names:
        Option<rustc_hash::FxHashMap<tsc_types::TypeId, tsc_emitter::TransformNode>>,
    type_parameter_names_by_text: Option<rustc_hash::FxHashSet<String>>,
    type_parameter_names_by_text_next_name_count: Option<rustc_hash::FxHashMap<String, u32>>,
    synthetic_scope_locals:
        Option<rustc_hash::FxHashMap<tsc_types::EscapedName, tsc_binder::SymbolId>>,
    synthetic_type_param_names: Vec<tsc_types::EscapedName>,
    synthetic_type_params_scope_active: bool,
    synthetic_scope_kind: Option<tsc_syntax::SyntaxKind>,
    fake_scopes: context::FakeScopeChain,
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
            synthetic_type_param_names: context.synthetic_type_param_names.clone(),
            synthetic_type_params_scope_active: context.synthetic_type_params_scope_active,
            synthetic_scope_kind: context.synthetic_scope_kind,
            fake_scopes: context.fake_scopes,
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
        context.synthetic_type_param_names = self.synthetic_type_param_names;
        context.synthetic_type_params_scope_active = self.synthetic_type_params_scope_active;
        context.synthetic_scope_kind = self.synthetic_scope_kind;
        context.fake_scopes = self.fake_scopes;
    }
}

/// One `startRecoveryScope` snapshot inside a syntactic reuse boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SyntacticRecoveryScope {
    had_error: bool,
    tracked_symbols_top: usize,
    tracker_had_error: bool,
    deferred_reports_top: usize,
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
        context.tracker.push_recovery_frame();
        Self {
            previous_had_error,
            previous_depth,
            previous_recovery_tracked_symbols,
            previous_tracked_symbols,
        }
    }

    /// tsrs-native: recovery-boundary error probe (upstream closure capture).
    pub(crate) fn had_error(&self, context: &NodeBuilderContext<'_>) -> bool {
        context.recovery_boundary_had_error || context.tracker.recovery_had_error()
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
        let (tracker_had_error, deferred_reports_top) = context.tracker.recovery_scope_top();
        SyntacticRecoveryScope {
            had_error: context.recovery_boundary_had_error,
            tracked_symbols_top: context
                .recovery_tracked_symbols
                .as_ref()
                .map_or(0, Vec::len),
            tracker_had_error,
            deferred_reports_top,
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
        context
            .tracker
            .recover_recovery_scope(scope.tracker_had_error, scope.deferred_reports_top);
    }

    /// tsrs-native: recovery-boundary completion (upstream closure return).
    pub(crate) fn finalize(
        self,
        context: &mut NodeBuilderContext<'_>,
        access: &mut dyn tsc_emitter::EmitTrackerAccess,
    ) -> Result<bool, tsc_emitter::EmitResolverError> {
        let tracker_had_error = context
            .tracker
            .pop_recovery_frame(&mut context.reported_diagnostic);
        let succeeded = !context.recovery_boundary_had_error && !tracker_had_error;
        let buffered = context.recovery_tracked_symbols.take().unwrap_or_default();
        context.recovery_tracked_symbols = self.previous_recovery_tracked_symbols;
        context.tracked_symbols = self.previous_tracked_symbols;
        context.recovery_boundary_had_error = self.previous_had_error;
        context.recovery_boundary_depth = self.previous_depth;
        if succeeded {
            for (symbol, symbol_flags, enclosing, synthetic, meaning) in buffered {
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
    fn has_late_bindable_name(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        node: tsc_emitter::TransformNode,
    ) -> Result<bool, tsc_emitter::EmitResolverError>;

    fn should_remove_declaration(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        context: &mut NodeBuilderContext<'_>,
        node: tsc_emitter::TransformNode,
    ) -> Result<bool, tsc_emitter::EmitResolverError>;

    fn create_recovery_boundary(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        context: &mut NodeBuilderContext<'_>,
    ) -> Result<SyntacticRecoveryBoundary, tsc_emitter::EmitResolverError>;

    fn serialize_existing_type_node(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        target: tsc_emitter::TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        type_node: tsc_emitter::TransformNode,
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

    fn get_module_specifier_override(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        context: &mut NodeBuilderContext<'_>,
        parent: tsc_emitter::TransformNode,
        literal: tsc_emitter::TransformNode,
    ) -> Result<Option<tsc_types::JsString>, tsc_emitter::EmitResolverError>;

    fn can_reuse_type_node(
        &mut self,
        arena: &mut tsc_emitter::TransformArena,
        context: &mut NodeBuilderContext<'_>,
        type_node: tsc_emitter::TransformNode,
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
            .map(tsc_syntax::NodeId::new)
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
    fn is_entity_in_type_node(
        &mut self,
        node: tsc_emitter::EmitTrackerNode,
    ) -> Result<bool, tsc_emitter::EmitResolverError> {
        let node = self.node(node).ok_or_else(|| self.invalid_token())?;
        Ok(tracker_is_entity_in_type_node(self.checker, node))
    }

    fn is_child_of_bound_expando(
        &mut self,
        node: tsc_emitter::EmitTrackerNode,
    ) -> Result<bool, tsc_emitter::EmitResolverError> {
        let node = self.node(node).ok_or_else(|| self.invalid_token())?;
        tracker_is_child_of_bound_expando(self.checker, node)
            .map_err(|abort| self.abort(node, abort))
    }

    fn accessor_declarations(
        &mut self,
        node: tsc_emitter::EmitTrackerNode,
    ) -> Result<tsc_emitter::EmitAccessorDeclarations, tsc_emitter::EmitResolverError> {
        let node = self.node(node).ok_or_else(|| self.invalid_token())?;
        tracker_accessor_declarations(self.checker, node).ok_or_else(|| self.invalid_token())
    }

    fn parent_node(
        &mut self,
        node: tsc_emitter::EmitTrackerNode,
    ) -> Result<Option<tsc_emitter::EmitTrackerNode>, tsc_emitter::EmitResolverError> {
        let node = self.node(node).ok_or_else(|| self.invalid_token())?;
        Ok(self
            .checker
            .parent_of(node)
            .map(|parent| tsc_emitter::EmitTrackerNode(u64::from(parent.index()))))
    }

    fn is_symbol_accessible(
        &mut self,
        symbol: tsc_emitter::EmitTrackerSymbol,
        enclosing_declaration: Option<tsc_emitter::EmitTrackerNode>,
        meaning: tsc_emitter::EmitSymbolMeaning,
        should_compute_aliases: bool,
    ) -> Result<tsc_emitter::EmitSymbolAccessibilityResult, tsc_emitter::EmitResolverError> {
        let symbol = u32::try_from(symbol.0)
            .ok()
            .map(tsc_binder::SymbolId::new)
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
            .map(tsc_binder::SymbolId::new)
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
            escaped_name: data.escaped_name.as_js().to_owned(),
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
            if info.declaration.is_some() || info.is_any_base_type_index_info {
                continue;
            }
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
                                NodeData::Identifier(data) => data.escaped_text,
                                _ => tsc_types::EscapedName::default(),
                            };
                            let resolved = checker
                                .resolve_name(
                                    Some(first_identifier),
                                    text,
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
                                    tsc_emitter::EmitTrackerSymbol(u64::from(resolved.index()));
                                let symbol_flags = access.checker.symbol_flags(resolved);
                                let enclosing_token = tsc_emitter::EmitTrackerNode(u64::from(
                                    enclosing_declaration.index(),
                                ));
                                tracker.track_symbol(
                                    &mut access,
                                    symbol_token,
                                    symbol_flags,
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
                                    .create_modifier(target, SyntaxKind::StaticKeyword)
                                    .map_err(factory_error)?,
                            );
                        }
                        if info.is_readonly {
                            modifiers.push(
                                arena
                                    .factory()
                                    .create_modifier(target, SyntaxKind::ReadonlyKeyword)
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
                            .create_property_declaration(
                                target,
                                modifiers_array.map(|array| {
                                    tsc_emitter::TransformNodeArray::new(target, array)
                                }),
                                name_in_arena,
                                question_token
                                    .map(|token| tsc_emitter::TransformNode::new(target, token)),
                                type_node,
                                None,
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
    use tsc_syntax::SyntaxKind;
    if arena.node(node)?.kind != SyntaxKind::IndexSignature {
        return Ok(node);
    }
    let mut factory = arena.factory();
    let mut modifiers = vec![factory.create_modifier(target, SyntaxKind::StaticKeyword)?];
    if is_readonly {
        modifiers.push(factory.create_modifier(target, SyntaxKind::ReadonlyKeyword)?);
    }
    let array = factory.create_node_array(target, modifiers)?;
    factory.replace_modifiers(node, Some(array))
}

#[cfg(test)]
#[path = "../../tests/unit/node_builder_core/tests.rs"]
mod tests;

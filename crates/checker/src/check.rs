//! The check driver (M4 5.4): checkSourceFileWorker's two-phase pass —
//! eager statements IN SOURCE ORDER, then the deferred-node drain —
//! plus the first live statement-position checks (type parameter
//! lists and the 2636 variance-annotation probe).
//!
//! Dispatch discipline: checkSourceElementWorker's switch is ported
//! with the FULL kind list. A CheckAbort unwind abandons the CURRENT
//! element's remaining checks only — the driver
//! continues with the next element, so one out-of-slice construct
//! never silences a whole file.
//!
//! Grammar checks: checkGrammarStatementInAmbientContext is LIVE from
//! 5.5a (checkExpressionStatement's head; the EmptyStatement/Debugger
//! and checkBlock routes share it); checkGrammarModifiers is LIVE from
//! M7 8.1a; declaration-file source grammar is LIVE from M7 8.1c.2.
//!
//! The unreachable-code slice (checkSourceElementUnreachable 86763 +
//! the withinUnreachableCode save/restore) is live. M5 supplied the
//! flow mechanism and explicit-error face; M7 8.4 supplies the
//! default-options suggestion projection.

use tsc_binder::{node_util, SymbolId};
use tsc_diagnostics::{gen as diagnostics, Diagnostic, DiagnosticCategory, DiagnosticMessage};
use tsc_syntax::nodes::{ImportTypeData, JSDocFunctionTypeData, JSDocTypeLiteralData};
use tsc_syntax::{for_each_child, NodeArrayId, NodeData, NodeId, SyntaxKind};
use tsc_types::{
    CheckFlags, ElementFlags, ModifierFlags, NodeCheckFlags, ObjectFlags, SymbolFlags, TypeData,
    TypeFacts, TypeFlags, TypeId, UnionReduction,
};

use crate::evaluate::EvalValue;
use crate::program::ProgramFileId;
use crate::state::{
    CheckAbort, CheckResult, CheckerState, OracleCrashKind, SignatureId, SignatureKind,
};

/// Debug-only unwind census (the abort-unwind invariant):
/// every transient stack an element check may push must be back at
/// its ENTRY depth when the element completes — Ok or Err alike —
/// and no `Resolving` sentinel may stay open across elements. A
/// deeper stack or a leaked sentinel after a CheckAbort unwind is
/// the state-leak bug class the Err-revert twins exist for (the
/// 5.7b lateBind revert was one instance); this makes the whole
/// class fail loud in dev builds instead of surfacing as downstream
/// nondeterminism.
#[cfg(debug_assertions)]
#[derive(Debug, Eq, PartialEq)]
struct UnwindSnapshot {
    resolution_targets: usize,
    resolution_results: usize,
    resolution_property_names: usize,
    resolution_start: usize,
    contextual_type_nodes: usize,
    contextual_types: usize,
    contextual_is_cache: usize,
    contextual_binding_patterns: usize,
    inference_context_nodes: usize,
    inference_contexts: usize,
    awaited_type_stack: usize,
    active_type_mappers: usize,
    active_type_mappers_caches: usize,
    slice_display_mappers: usize,
    slice_infer_type_parameters: usize,
    slice_reuse_had_error: bool,
    slice_reuse_visit_depth: usize,
    slice_display_clone_indent: usize,
    slice_display_clone_at_line_start: bool,
    variance_handler_stack: usize,
    class_interface_declared_in_progress: usize,
    type_parameter_defaults_in_progress: usize,
    mapped_types_in_progress: usize,
    // widening_contexts is deliberately ABSENT: it is an arena
    // (WideningContextId-indexed, tsc's GC'd context objects), not a
    // transient stack — growth across an element is allocation, not
    // leaked in-progress state.
    speculation_depth: u32,
    instantiation_depth: u32,
    in_variance_computation: bool,
    variance_type_parameter: Option<TypeId>,
    flow_loop_start: u32,
    flow_loop_stack: usize,
    // The m4-review B34 blind spots — transient state the census
    // missed until 7.0t widened it: the shared-flow window, the
    // ReduceLabel override map, the exhaustive-switch cycle set, and
    // the inlineLevel budget counter.
    shared_flow: usize,
    reduce_label_overrides: usize,
    exhaustive_switch_computing: usize,
    inline_level: u32,
    resolving_open: i64,
}

impl<'a> CheckerState<'a> {
    #[cfg(debug_assertions)]
    fn unwind_snapshot(&self) -> UnwindSnapshot {
        UnwindSnapshot {
            resolution_targets: self.resolution_targets.len(),
            resolution_results: self.resolution_results.len(),
            resolution_property_names: self.resolution_property_names.len(),
            resolution_start: self.resolution_start,
            contextual_type_nodes: self.contextual_type_nodes.len(),
            contextual_types: self.contextual_types.len(),
            contextual_is_cache: self.contextual_is_cache.len(),
            contextual_binding_patterns: self.contextual_binding_patterns.len(),
            inference_context_nodes: self.inference_context_nodes.len(),
            inference_contexts: self.inference_contexts.len(),
            awaited_type_stack: self.awaited_type_stack.len(),
            active_type_mappers: self.active_type_mappers.len(),
            active_type_mappers_caches: self.active_type_mappers_caches.len(),
            slice_display_mappers: self.slice_display_mappers.len(),
            slice_infer_type_parameters: self.slice_infer_type_parameters.len(),
            slice_reuse_had_error: self.slice_reuse_had_error,
            slice_reuse_visit_depth: self.slice_reuse_visit_depth,
            slice_display_clone_indent: self.slice_display_clone_indent,
            slice_display_clone_at_line_start: self.slice_display_clone_at_line_start,
            variance_handler_stack: self.variance_handler_stack.len(),
            class_interface_declared_in_progress: self.class_interface_declared_in_progress.len(),
            type_parameter_defaults_in_progress: self.type_parameter_defaults_in_progress.len(),
            mapped_types_in_progress: self.mapped_types_in_progress.len(),
            speculation_depth: self.speculation_depth,
            instantiation_depth: self.instantiation_depth,
            in_variance_computation: self.in_variance_computation,
            variance_type_parameter: self.variance_type_parameter,
            flow_loop_start: self.flow_loop_start,
            flow_loop_stack: self.flow_loop_stack.len(),
            shared_flow: self.shared_flow.len(),
            reduce_label_overrides: self.reduce_label_overrides.len(),
            exhaustive_switch_computing: self.exhaustive_switch_computing.len(),
            inline_level: self.inline_level,
            resolving_open: crate::links::debug_resolving_open(),
        }
    }

    #[cfg(debug_assertions)]
    fn assert_unwound(&self, entry: &UnwindSnapshot, node: NodeId, boundary: &str) {
        let exit = self.unwind_snapshot();
        assert_eq!(
            &exit, entry,
            "abort-unwind invariant violated after {boundary} of {node:?} \
             (an Err path left checker state behind — add/fix its revert twin)"
        );
    }

    /// Per-file driver entry — checkSourceFile (86969) minus the
    /// tracing/perf marks and the nodesToCheck partial-check path
    /// (unported; conformance always full-checks).
    /// tsc-port: checkSourceFile @6.0.3
    /// tsc-hash: 4fac0ca4e39f116ddab5a55ab69ba01689c7194437677ab3cfd3d18556e55992
    /// tsc-span: _tsc.js:86969-86986
    pub fn check_source_file(&mut self, file_index: usize) {
        let root = self.binder.source(file_index).root;
        self.check_source_file_worker(root);
        // 86985: reportedUnreachableNodes resets per checked file.
        self.reported_unreachable_nodes.clear();
    }

    /// tsc-port: checkSourceFileWorker @6.0.3
    /// tsc-hash: 13eed3b3fd0489121dea467d08e6b5ef9bdcf489da5af16bdc0c460a414fbe8f
    /// tsc-span: _tsc.js:87003-87061
    ///
    /// Elisions, each with its owner:
    /// - the PartiallyTypeChecked restore block: nodesToCheck path.
    /// - the unused-identifiers lazy block is live incrementally from
    ///   M7 8.3; its class-member producer lands first. The eager Rust
    ///   drain keeps tsc's position after deferred nodes.
    ///   checkPotentialUncheckedRenamedBindingElementsInTypes shares
    ///   that addLazyDiagnostic block but is NOT option-gated — live
    ///   from 5.8a and runs immediately after the unused drain.
    /// - checkExternalModuleExports (86505): module export checking is
    ///   5.8d (needs alias declaration resolution).
    fn check_source_file_worker(&mut self, root: NodeId) {
        if self
            .links
            .node(root)
            .check_flags
            .intersects(NodeCheckFlags::TYPE_CHECKED)
        {
            return;
        }
        if self.skip_type_checking(root) {
            return;
        }
        self.check_grammar_source_file(root);
        // 87010-87014: the five per-file accumulators clear at worker
        // entry (the PartiallyTypeChecked restore stays elided).
        self.potential_this_collisions.clear();
        self.potential_new_target_collisions.clear();
        self.potential_weak_map_set_collisions.clear();
        self.potential_reflect_collisions.clear();
        self.potential_unused_renamed_binding_elements_in_types
            .clear();
        let NodeData::SourceFile(data) = self.data_of(root) else {
            unreachable!("check_source_file_worker takes source-file roots");
        };
        let end_of_file_token = data.end_of_file_token;
        let is_declaration_file = self.binder.source_of_node(root).is_declaration_file;
        for statement in self.nodes_of(data.statements) {
            self.check_source_element(Some(statement));
        }
        self.check_source_element(end_of_file_token);
        self.check_deferred_nodes(root);
        let is_unused_source_file_owner = self.is_effective_external_module(root)
            || self.is_in_js_file(root)
                && self.binder.is_external_or_common_js_module_of_node(root);
        if is_unused_source_file_owner {
            self.register_for_unused_identifiers_check(root);
        }
        // checkSourceFileWithEagerDiagnostics 87104-87109 replaces
        // addLazyDiagnostic with an inline callback. Consequently the
        // 87028 unused/renamed-binding block runs here, before
        // checkExternalModuleExports and the four collision drains.
        // The Rust checker projects its suggestion rows from the same
        // drain; no later collision worker records identifier uses.
        self.check_registered_unused_identifiers(root);
        if !is_declaration_file {
            self.check_potential_unchecked_renamed_binding_elements_in_types();
        }
        // 87041: external/CJS module → checkExternalModuleExports
        // (§8; the checkExportAssignment-driven run dedupes through
        // the exportsChecked once-guard). CheckAbort containment
        // matches check_source_element's element boundary.
        if self.binder.is_external_or_common_js_module_of_node(root) {
            if let Err(err) = self.check_external_module_exports(root) {
                // Preserve only directive accounting. Oracle-crash
                // aborts mirror tsc behavior and are not published as
                // partial-model audit records.
                self.mark_oracle_crash_range(root, err);
                if std::env::var_os("TSRS_TRACE_CONTAIN").is_some() {
                    eprintln!("contained @{root:?}: {err}");
                }
            }
        }
        // 87042-87058: the four collision drains IN ORDER; each clears
        // its vector like tsc's clear() tail.
        let this_collisions = std::mem::take(&mut self.potential_this_collisions);
        for node in this_collisions {
            self.check_if_this_is_captured_in_enclosing_scope(node);
        }
        let new_target_collisions = std::mem::take(&mut self.potential_new_target_collisions);
        for node in new_target_collisions {
            self.check_if_new_target_is_captured_in_enclosing_scope(node);
        }
        let weak_map_set_collisions = std::mem::take(&mut self.potential_weak_map_set_collisions);
        for node in weak_map_set_collisions {
            self.check_weak_map_set_collision(node);
        }
        let reflect_collisions = std::mem::take(&mut self.potential_reflect_collisions);
        for node in reflect_collisions {
            self.check_reflect_collision(node);
        }
        // File-boundary unwind invariant: between files every
        // transient stack is EMPTY (not merely restored) and no
        // Resolving sentinel is open — the per-element guards bound
        // leaks to an element; this pins the absolute baseline.
        #[cfg(debug_assertions)]
        {
            let end = self.unwind_snapshot();
            let baseline = UnwindSnapshot {
                resolution_targets: 0,
                resolution_results: 0,
                resolution_property_names: 0,
                resolution_start: 0,
                contextual_type_nodes: 0,
                contextual_types: 0,
                contextual_is_cache: 0,
                contextual_binding_patterns: 0,
                inference_context_nodes: 0,
                inference_contexts: 0,
                awaited_type_stack: 0,
                active_type_mappers: 0,
                active_type_mappers_caches: 0,
                slice_display_mappers: 0,
                slice_infer_type_parameters: 0,
                slice_reuse_had_error: false,
                slice_reuse_visit_depth: 0,
                slice_display_clone_indent: 0,
                slice_display_clone_at_line_start: false,
                variance_handler_stack: 0,
                class_interface_declared_in_progress: 0,
                type_parameter_defaults_in_progress: 0,
                mapped_types_in_progress: 0,
                speculation_depth: 0,
                instantiation_depth: 0,
                in_variance_computation: false,
                variance_type_parameter: None,
                flow_loop_start: 0,
                flow_loop_stack: 0,
                shared_flow: 0,
                reduce_label_overrides: 0,
                exhaustive_switch_computing: 0,
                inline_level: 0,
                resolving_open: 0,
            };
            assert_eq!(
                end, baseline,
                "abort-unwind invariant violated at the end of file {root:?}"
            );
        }
        self.links
            .or_node_check_flags(self.speculation_depth, root, NodeCheckFlags::TYPE_CHECKED);
    }

    /// tsc-port: skipTypeCheckingWorker @6.0.3
    /// tsc-hash: 8dcc4a08f5b94c3c9ada5b6c1e86885714d7db12c71cbf857ca88531632bd0c3
    /// tsc-span: _tsc.js:18877-18903
    ///
    /// Program membership is part of the decision: the same reusable bound
    /// document can be a default library in one Program and an ordinary
    /// declaration source in another.
    fn skip_type_checking(&self, root: NodeId) -> bool {
        let file = ProgramFileId::from_raw(
            u32::try_from(self.binder.file_index_of_node(root))
                .expect("Program file index overflow"),
        );
        self.skip_type_checking_file(file)
    }

    /// tsrs-native: project a ProgramFileId through the binder-owned source
    /// and file facts into the shared skipTypeCheckingWorker policy.
    pub(crate) fn skip_type_checking_file(&self, file: ProgramFileId) -> bool {
        crate::should_skip_type_checking_file(
            self.binder.source(file.index()),
            self.binder.file_facts(file),
            self.options,
        )
    }

    /// tsc-port: checkGrammarTopLevelElementForRequiredDeclareModifier @6.0.3
    /// tsc-hash: 029880ee66b0dc833ebf2b1ce77fd314851abc4ed46c1840ffb2c5c0c343fa37
    /// tsc-span: _tsc.js:90307-90312
    /// d2: d2:058d3174aacd6253cc00e5f75311c8bc20cf7ac706d18600c03c20a932cf9dbe
    fn check_grammar_top_level_element_for_required_declare_modifier(
        &mut self,
        node: NodeId,
    ) -> bool {
        if matches!(
            self.kind_of(node),
            SyntaxKind::InterfaceDeclaration
                | SyntaxKind::TypeAliasDeclaration
                | SyntaxKind::ImportDeclaration
                | SyntaxKind::ImportEqualsDeclaration
                | SyntaxKind::ExportDeclaration
                | SyntaxKind::ExportAssignment
                | SyntaxKind::NamespaceExportDeclaration
        ) {
            return false;
        }
        let source = self.binder.source_of_node(node);
        let allowed_modifiers = ModifierFlags::from_bits(
            ModifierFlags::AMBIENT.bits()
                | ModifierFlags::EXPORT.bits()
                | ModifierFlags::DEFAULT.bits(),
        );
        if node_util::has_syntactic_modifier(source, node, allowed_modifiers) {
            return false;
        }
        self.grammar_error_on_first_token(
            node,
            &diagnostics::Top_level_declarations_in_d_ts_files_must_start_with_either_a_declare_or_export_modifier,
            &[],
        )
    }

    /// tsc-port: checkGrammarTopLevelElementsForRequiredDeclareModifier @6.0.3
    /// tsc-hash: 8948bed73a676d0742e0587e8a8807d89c5a72cfecf478343c25b5d4df26aa15
    /// tsc-span: _tsc.js:90313-90322
    /// d2: d2:922f3734d14bf87fb62c3d7ef6ffcf89e41b19c3e53569122f8d3dcc9737eff7
    fn check_grammar_top_level_elements_for_required_declare_modifier(
        &mut self,
        root: NodeId,
    ) -> bool {
        let NodeData::SourceFile(data) = self.data_of(root) else {
            return false;
        };
        for declaration in self.nodes_of(data.statements) {
            if matches!(
                self.kind_of(declaration),
                SyntaxKind::FunctionDeclaration
                    | SyntaxKind::ClassDeclaration
                    | SyntaxKind::InterfaceDeclaration
                    | SyntaxKind::TypeAliasDeclaration
                    | SyntaxKind::EnumDeclaration
                    | SyntaxKind::ModuleDeclaration
                    | SyntaxKind::ImportEqualsDeclaration
                    | SyntaxKind::NamespaceExportDeclaration
                    | SyntaxKind::VariableStatement
            ) && self.check_grammar_top_level_element_for_required_declare_modifier(declaration)
            {
                return true;
            }
        }
        false
    }

    /// tsc-port: checkGrammarSourceFile @6.0.3
    /// tsc-hash: 4927fc26371ca77477c792d6e9a2d5faa19f9d8baa9947030e85cb98c610bf7d
    /// tsc-span: _tsc.js:90323-90325
    /// d2: d2:d0812accb6a2508527ea4d9a6a3c0363228d252a5b201bb14d6a2d73680e95b6
    fn check_grammar_source_file(&mut self, root: NodeId) -> bool {
        self.node_flags(root) & tsc_types::NodeFlags::AMBIENT.bits() != 0
            && self.check_grammar_top_level_elements_for_required_declare_modifier(root)
    }

    /// tsc-port: checkGrammarModifiers @6.0.3
    /// tsc-hash: 4ae83b985bfc4d9c367541290d29b207ce34af46a4b465b0e36cae2056847f03
    /// tsc-span: _tsc.js:89010-89325
    /// d2: d2:984775a91d6ec0d2e27b820a9d34a31328ef5845e0fd5dd8a5e751f3040d2ca8
    pub(crate) fn check_grammar_modifiers(&mut self, node: NodeId) -> bool {
        let source = self.binder.source_of_node(node);
        let node_kind = self.kind_of(node);
        let parent = self.parent_of(node);
        let parent_kind = parent.map(|parent| self.kind_of(parent));
        if self.report_obvious_decorator_errors(node) == Some(true) {
            return true;
        }
        if let Some(result) = self.report_obvious_modifier_errors(node) {
            return result;
        }
        let modifiers = self.nodes_of(node_util::modifiers_of(source, node));
        if node_kind == SyntaxKind::Parameter {
            let is_this = match self.data_of(node) {
                NodeData::Parameter(data) => data
                    .name
                    .is_some_and(|name| self.identifier_text_of(name) == Some("this")),
                _ => false,
            };
            if is_this {
                return self.grammar_error_on_first_token(
                    node,
                    &diagnostics::Neither_decorators_nor_modifiers_may_be_applied_to_this_parameters,
                    &[],
                );
            }
        }
        let block_scope_kind = if node_kind == SyntaxKind::VariableStatement {
            match self.data_of(node) {
                NodeData::VariableStatement(data) => data
                    .declaration_list
                    .map(|list| self.node_flags(list) & tsc_types::NodeFlags::BLOCK_SCOPED.bits())
                    .unwrap_or(0),
                _ => 0,
            }
        } else {
            0
        };
        let using_kinds = (
            tsc_types::NodeFlags::USING.bits(),
            tsc_types::NodeFlags::AWAIT_USING.bits(),
        );
        let parent_is_class_like = matches!(
            parent_kind,
            Some(SyntaxKind::ClassDeclaration) | Some(SyntaxKind::ClassExpression)
        );
        let is_private_identifier_class_element = matches!(
            node_kind,
            SyntaxKind::PropertyDeclaration
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::GetAccessor
                | SyntaxKind::SetAccessor
        ) && self
            .name_of_node(node)
            .is_some_and(|name| self.kind_of(name) == SyntaxKind::PrivateIdentifier);
        let parent_is_ambient = parent.is_some_and(|parent| {
            self.node_flags(parent) & tsc_types::NodeFlags::AMBIENT.bits() != 0
        });
        let mut last_static = None;
        let mut last_declare = None;
        let mut last_async = None;
        let mut last_override = None;
        let mut first_decorator = None;
        let mut flags = ModifierFlags::from_bits(0);
        let mut has_leading_decorators = false;
        let mut saw_export_before_decorators = false;
        for &modifier in &modifiers {
            let modifier_kind = self.kind_of(modifier);
            if modifier_kind == SyntaxKind::Decorator {
                let grandparent = parent.and_then(|parent| self.parent_of(parent));
                if !self.node_can_be_decorated(
                    self.options.experimental_decorators,
                    node,
                    parent,
                    grandparent,
                ) {
                    let is_method_overload = node_kind == SyntaxKind::MethodDeclaration
                        && node_util::node_is_missing(source, node_util::body_of(source, node));
                    return self.grammar_error_on_first_token(
                        node,
                        if is_method_overload {
                            &diagnostics::A_decorator_can_only_decorate_a_method_implementation_not_an_overload
                        } else {
                            &diagnostics::Decorators_are_not_valid_here
                        },
                        &[],
                    );
                }
                if self.options.experimental_decorators
                    && matches!(node_kind, SyntaxKind::GetAccessor | SyntaxKind::SetAccessor)
                {
                    // getAllAccessorDeclarationsForDeclaration off the
                    // symbol: decorators on the SECOND accessor of a
                    // decorated pair report.
                    if let Some(symbol) = self.node_symbol(node) {
                        let accessors: Vec<NodeId> = self
                            .binder
                            .symbol(symbol)
                            .declarations
                            .iter()
                            .copied()
                            .filter(|&declaration| {
                                matches!(
                                    self.kind_of(declaration),
                                    SyntaxKind::GetAccessor | SyntaxKind::SetAccessor
                                )
                            })
                            .collect();
                        if accessors.len() >= 2 && node == accessors[1] {
                            let first_source = self.binder.source_of_node(accessors[0]);
                            let first_has_decorators = self
                                .nodes_of(node_util::modifiers_of(first_source, accessors[0]))
                                .iter()
                                .any(|&m| self.kind_of(m) == SyntaxKind::Decorator);
                            if first_has_decorators {
                                return self.grammar_error_on_first_token(
                                    node,
                                    &diagnostics::Decorators_cannot_be_applied_to_multiple_get_set_accessors_of_the_same_name,
                                    &[],
                                );
                            }
                        }
                    }
                }
                if flags.bits()
                    & !(ModifierFlags::EXPORT_DEFAULT.bits() | ModifierFlags::DECORATOR.bits())
                    != 0
                {
                    return self.grammar_error_on_node(
                        modifier,
                        &diagnostics::Decorators_are_not_valid_here,
                        &[],
                    );
                }
                if has_leading_decorators && flags.intersects(ModifierFlags::MODIFIER) {
                    if self.has_parse_diagnostics(modifier) {
                        return false;
                    }
                    let first = first_decorator.expect("leading decorator was recorded");
                    let related = self.related_info_for_node(
                        first,
                        &diagnostics::Decorator_used_before_export_here,
                        &[],
                    );
                    self.error_at_with_related(
                        Some(modifier),
                        &diagnostics::Decorators_may_not_appear_after_export_or_export_default_if_they_also_appear_before_export,
                        &[],
                        vec![related],
                    );
                    return true;
                }
                flags |= ModifierFlags::DECORATOR;
                if !flags.intersects(ModifierFlags::MODIFIER) {
                    has_leading_decorators = true;
                } else if flags.intersects(ModifierFlags::EXPORT) {
                    saw_export_before_decorators = true;
                }
                first_decorator.get_or_insert(modifier);
                continue;
            }
            let modifier_text = tsc_syntax::tokens::token_to_string(modifier_kind).unwrap_or("?");
            if modifier_kind != SyntaxKind::ReadonlyKeyword {
                if matches!(
                    node_kind,
                    SyntaxKind::PropertySignature | SyntaxKind::MethodSignature
                ) {
                    return self.grammar_error_on_node(
                        modifier,
                        &diagnostics::_0_modifier_cannot_appear_on_a_type_member,
                        &[modifier_text],
                    );
                }
                if node_kind == SyntaxKind::IndexSignature
                    && (modifier_kind != SyntaxKind::StaticKeyword || !parent_is_class_like)
                {
                    return self.grammar_error_on_node(
                        modifier,
                        &diagnostics::_0_modifier_cannot_appear_on_an_index_signature,
                        &[modifier_text],
                    );
                }
            }
            if !matches!(
                modifier_kind,
                SyntaxKind::InKeyword | SyntaxKind::OutKeyword | SyntaxKind::ConstKeyword
            ) && node_kind == SyntaxKind::TypeParameter
            {
                return self.grammar_error_on_node(
                    modifier,
                    &diagnostics::_0_modifier_cannot_appear_on_a_type_parameter,
                    &[modifier_text],
                );
            }
            match modifier_kind {
                SyntaxKind::ConstKeyword => {
                    if !matches!(
                        node_kind,
                        SyntaxKind::EnumDeclaration | SyntaxKind::TypeParameter
                    ) {
                        return self.grammar_error_on_node(
                            node,
                            &diagnostics::A_class_member_cannot_have_the_0_keyword,
                            &["const"],
                        );
                    }
                    let effective_parent = if parent_kind == Some(SyntaxKind::JSDocTemplateTag) {
                        parent
                            .and_then(|parent| self.get_effective_jsdoc_host(parent))
                            .or(parent)
                    } else {
                        parent
                    };
                    let effective_parent_kind = effective_parent.map(|parent| self.kind_of(parent));
                    if node_kind == SyntaxKind::TypeParameter
                        && !matches!(
                            effective_parent_kind,
                            Some(SyntaxKind::FunctionDeclaration)
                                | Some(SyntaxKind::FunctionExpression)
                                | Some(SyntaxKind::ArrowFunction)
                                | Some(SyntaxKind::MethodDeclaration)
                                | Some(SyntaxKind::Constructor)
                                | Some(SyntaxKind::GetAccessor)
                                | Some(SyntaxKind::SetAccessor)
                                | Some(SyntaxKind::ClassDeclaration)
                                | Some(SyntaxKind::ClassExpression)
                                | Some(SyntaxKind::FunctionType)
                                | Some(SyntaxKind::ConstructorType)
                                | Some(SyntaxKind::CallSignature)
                                | Some(SyntaxKind::ConstructSignature)
                                | Some(SyntaxKind::MethodSignature)
                        )
                    {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_can_only_appear_on_a_type_parameter_of_a_function_method_or_class,
                            &["const"],
                        );
                    }
                }
                SyntaxKind::OverrideKeyword => {
                    if flags.intersects(ModifierFlags::OVERRIDE) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_already_seen,
                            &["override"],
                        );
                    } else if flags.intersects(ModifierFlags::AMBIENT) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_cannot_be_used_with_1_modifier,
                            &["override", "declare"],
                        );
                    } else if flags.intersects(ModifierFlags::READONLY) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_must_precede_1_modifier,
                            &["override", "readonly"],
                        );
                    } else if flags.intersects(ModifierFlags::ACCESSOR) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_must_precede_1_modifier,
                            &["override", "accessor"],
                        );
                    } else if flags.intersects(ModifierFlags::ASYNC) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_must_precede_1_modifier,
                            &["override", "async"],
                        );
                    }
                    flags |= ModifierFlags::OVERRIDE;
                    last_override = Some(modifier);
                }
                SyntaxKind::PublicKeyword
                | SyntaxKind::ProtectedKeyword
                | SyntaxKind::PrivateKeyword => {
                    if flags.intersects(ModifierFlags::ACCESSIBILITY_MODIFIER) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::Accessibility_modifier_already_seen,
                            &[],
                        );
                    } else if flags.intersects(ModifierFlags::OVERRIDE) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_must_precede_1_modifier,
                            &[modifier_text, "override"],
                        );
                    } else if flags.intersects(ModifierFlags::STATIC) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_must_precede_1_modifier,
                            &[modifier_text, "static"],
                        );
                    } else if flags.intersects(ModifierFlags::ACCESSOR) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_must_precede_1_modifier,
                            &[modifier_text, "accessor"],
                        );
                    } else if flags.intersects(ModifierFlags::READONLY) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_must_precede_1_modifier,
                            &[modifier_text, "readonly"],
                        );
                    } else if flags.intersects(ModifierFlags::ASYNC) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_must_precede_1_modifier,
                            &[modifier_text, "async"],
                        );
                    }
                    if matches!(
                        parent_kind,
                        Some(SyntaxKind::ModuleBlock) | Some(SyntaxKind::SourceFile)
                    ) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_cannot_appear_on_a_module_or_namespace_element,
                            &[modifier_text],
                        );
                    }
                    if flags.intersects(ModifierFlags::ABSTRACT) {
                        return self.grammar_error_on_node(
                            modifier,
                            if modifier_kind == SyntaxKind::PrivateKeyword {
                                &diagnostics::_0_modifier_cannot_be_used_with_1_modifier
                            } else {
                                &diagnostics::_0_modifier_must_precede_1_modifier
                            },
                            &[modifier_text, "abstract"],
                        );
                    }
                    if is_private_identifier_class_element {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::An_accessibility_modifier_cannot_be_used_with_a_private_identifier,
                            &[],
                        );
                    }
                    flags |= node_util::modifier_to_flag(modifier_kind);
                }
                SyntaxKind::StaticKeyword => {
                    if flags.intersects(ModifierFlags::STATIC) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_already_seen,
                            &["static"],
                        );
                    } else if flags.intersects(ModifierFlags::READONLY) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_must_precede_1_modifier,
                            &["static", "readonly"],
                        );
                    } else if flags.intersects(ModifierFlags::ASYNC) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_must_precede_1_modifier,
                            &["static", "async"],
                        );
                    } else if flags.intersects(ModifierFlags::ACCESSOR) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_must_precede_1_modifier,
                            &["static", "accessor"],
                        );
                    }
                    if matches!(
                        parent_kind,
                        Some(SyntaxKind::ModuleBlock) | Some(SyntaxKind::SourceFile)
                    ) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_cannot_appear_on_a_module_or_namespace_element,
                            &["static"],
                        );
                    } else if node_kind == SyntaxKind::Parameter {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_cannot_appear_on_a_parameter,
                            &["static"],
                        );
                    } else if flags.intersects(ModifierFlags::ABSTRACT) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_cannot_be_used_with_1_modifier,
                            &["static", "abstract"],
                        );
                    } else if flags.intersects(ModifierFlags::OVERRIDE) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_must_precede_1_modifier,
                            &["static", "override"],
                        );
                    }
                    flags |= ModifierFlags::STATIC;
                    last_static = Some(modifier);
                }
                SyntaxKind::AccessorKeyword => {
                    if flags.intersects(ModifierFlags::ACCESSOR) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_already_seen,
                            &["accessor"],
                        );
                    } else if flags.intersects(ModifierFlags::READONLY) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_cannot_be_used_with_1_modifier,
                            &["accessor", "readonly"],
                        );
                    } else if flags.intersects(ModifierFlags::AMBIENT) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_cannot_be_used_with_1_modifier,
                            &["accessor", "declare"],
                        );
                    } else if node_kind != SyntaxKind::PropertyDeclaration {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::accessor_modifier_can_only_appear_on_a_property_declaration,
                            &[],
                        );
                    }
                    flags |= ModifierFlags::ACCESSOR;
                }
                SyntaxKind::ReadonlyKeyword => {
                    if flags.intersects(ModifierFlags::READONLY) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_already_seen,
                            &["readonly"],
                        );
                    } else if !matches!(
                        node_kind,
                        SyntaxKind::PropertyDeclaration
                            | SyntaxKind::PropertySignature
                            | SyntaxKind::IndexSignature
                            | SyntaxKind::Parameter
                    ) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::readonly_modifier_can_only_appear_on_a_property_declaration_or_index_signature,
                            &[],
                        );
                    } else if flags.intersects(ModifierFlags::ACCESSOR) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_cannot_be_used_with_1_modifier,
                            &["readonly", "accessor"],
                        );
                    }
                    flags |= ModifierFlags::READONLY;
                }
                SyntaxKind::ExportKeyword => {
                    if self.options.verbatim_module_syntax == Some(true)
                        && self.node_flags(node) & tsc_types::NodeFlags::AMBIENT.bits() == 0
                        && !matches!(
                            node_kind,
                            SyntaxKind::TypeAliasDeclaration
                                | SyntaxKind::InterfaceDeclaration
                                | SyntaxKind::ModuleDeclaration
                        )
                        && parent_kind == Some(SyntaxKind::SourceFile)
                        && self.emit_module_format_of_file(node) == 1
                    {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::A_top_level_export_modifier_cannot_be_used_on_value_declarations_in_a_CommonJS_module_when_verbatimModuleSyntax_is_enabled,
                            &[],
                        );
                    }
                    if flags.intersects(ModifierFlags::EXPORT) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_already_seen,
                            &["export"],
                        );
                    } else if flags.intersects(ModifierFlags::AMBIENT) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_must_precede_1_modifier,
                            &["export", "declare"],
                        );
                    } else if flags.intersects(ModifierFlags::ABSTRACT) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_must_precede_1_modifier,
                            &["export", "abstract"],
                        );
                    } else if flags.intersects(ModifierFlags::ASYNC) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_must_precede_1_modifier,
                            &["export", "async"],
                        );
                    }
                    if parent_is_class_like {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_cannot_appear_on_class_elements_of_this_kind,
                            &["export"],
                        );
                    } else if node_kind == SyntaxKind::Parameter {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_cannot_appear_on_a_parameter,
                            &["export"],
                        );
                    } else if block_scope_kind == using_kinds.0 {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_cannot_appear_on_a_using_declaration,
                            &["export"],
                        );
                    } else if block_scope_kind == using_kinds.1 {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_cannot_appear_on_an_await_using_declaration,
                            &["export"],
                        );
                    }
                    flags |= ModifierFlags::EXPORT;
                }
                SyntaxKind::DefaultKeyword => {
                    let container = match parent_kind {
                        Some(SyntaxKind::SourceFile) => parent,
                        _ => parent.and_then(|parent| self.parent_of(parent)),
                    };
                    if let Some(container) = container {
                        if self.kind_of(container) == SyntaxKind::ModuleDeclaration
                            && !node_util::is_ambient_module(
                                self.binder.source_of_node(container),
                                container,
                            )
                        {
                            return self.grammar_error_on_node(
                                modifier,
                                &diagnostics::A_default_export_can_only_be_used_in_an_ECMAScript_style_module,
                                &[],
                            );
                        }
                    }
                    if block_scope_kind == using_kinds.0 {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_cannot_appear_on_a_using_declaration,
                            &["default"],
                        );
                    } else if block_scope_kind == using_kinds.1 {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_cannot_appear_on_an_await_using_declaration,
                            &["default"],
                        );
                    } else if !flags.intersects(ModifierFlags::EXPORT) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_must_precede_1_modifier,
                            &["export", "default"],
                        );
                    } else if saw_export_before_decorators {
                        return self.grammar_error_on_node(
                            first_decorator.expect("decorator before default was recorded"),
                            &diagnostics::Decorators_are_not_valid_here,
                            &[],
                        );
                    }
                    flags |= ModifierFlags::DEFAULT;
                }
                SyntaxKind::DeclareKeyword => {
                    if flags.intersects(ModifierFlags::AMBIENT) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_already_seen,
                            &["declare"],
                        );
                    } else if flags.intersects(ModifierFlags::ASYNC) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_cannot_be_used_in_an_ambient_context,
                            &["async"],
                        );
                    } else if flags.intersects(ModifierFlags::OVERRIDE) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_cannot_be_used_in_an_ambient_context,
                            &["override"],
                        );
                    } else if parent_is_class_like && node_kind != SyntaxKind::PropertyDeclaration {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_cannot_appear_on_class_elements_of_this_kind,
                            &["declare"],
                        );
                    } else if node_kind == SyntaxKind::Parameter {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_cannot_appear_on_a_parameter,
                            &["declare"],
                        );
                    } else if block_scope_kind == using_kinds.0 {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_cannot_appear_on_a_using_declaration,
                            &["declare"],
                        );
                    } else if block_scope_kind == using_kinds.1 {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_cannot_appear_on_an_await_using_declaration,
                            &["declare"],
                        );
                    } else if parent_is_ambient && parent_kind == Some(SyntaxKind::ModuleBlock) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::A_declare_modifier_cannot_be_used_in_an_already_ambient_context,
                            &[],
                        );
                    } else if is_private_identifier_class_element {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_cannot_be_used_with_a_private_identifier,
                            &["declare"],
                        );
                    } else if flags.intersects(ModifierFlags::ACCESSOR) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_cannot_be_used_with_1_modifier,
                            &["declare", "accessor"],
                        );
                    }
                    flags |= ModifierFlags::AMBIENT;
                    last_declare = Some(modifier);
                }
                SyntaxKind::AbstractKeyword => {
                    if flags.intersects(ModifierFlags::ABSTRACT) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_already_seen,
                            &["abstract"],
                        );
                    }
                    if !matches!(
                        node_kind,
                        SyntaxKind::ClassDeclaration | SyntaxKind::ConstructorType
                    ) {
                        if !matches!(
                            node_kind,
                            SyntaxKind::MethodDeclaration
                                | SyntaxKind::PropertyDeclaration
                                | SyntaxKind::GetAccessor
                                | SyntaxKind::SetAccessor
                        ) {
                            return self.grammar_error_on_node(
                                modifier,
                                &diagnostics::abstract_modifier_can_only_appear_on_a_class_method_or_property_declaration,
                                &[],
                            );
                        }
                        let parent_is_abstract_class = parent.is_some_and(|parent| {
                            self.kind_of(parent) == SyntaxKind::ClassDeclaration
                                && node_util::has_syntactic_modifier(
                                    self.binder.source_of_node(parent),
                                    parent,
                                    ModifierFlags::ABSTRACT,
                                )
                        });
                        if !parent_is_abstract_class {
                            return self.grammar_error_on_node(
                                modifier,
                                if node_kind == SyntaxKind::PropertyDeclaration {
                                    &diagnostics::Abstract_properties_can_only_appear_within_an_abstract_class
                                } else {
                                    &diagnostics::Abstract_methods_can_only_appear_within_an_abstract_class
                                },
                                &[],
                            );
                        }
                        if flags.intersects(ModifierFlags::STATIC) {
                            return self.grammar_error_on_node(
                                modifier,
                                &diagnostics::_0_modifier_cannot_be_used_with_1_modifier,
                                &["static", "abstract"],
                            );
                        }
                        if flags.intersects(ModifierFlags::PRIVATE) {
                            return self.grammar_error_on_node(
                                modifier,
                                &diagnostics::_0_modifier_cannot_be_used_with_1_modifier,
                                &["private", "abstract"],
                            );
                        }
                        if flags.intersects(ModifierFlags::ASYNC) {
                            return self.grammar_error_on_node(
                                last_async.expect("async flag records its modifier"),
                                &diagnostics::_0_modifier_cannot_be_used_with_1_modifier,
                                &["async", "abstract"],
                            );
                        }
                        if flags.intersects(ModifierFlags::OVERRIDE) {
                            return self.grammar_error_on_node(
                                modifier,
                                &diagnostics::_0_modifier_must_precede_1_modifier,
                                &["abstract", "override"],
                            );
                        }
                        if flags.intersects(ModifierFlags::ACCESSOR) {
                            return self.grammar_error_on_node(
                                modifier,
                                &diagnostics::_0_modifier_must_precede_1_modifier,
                                &["abstract", "accessor"],
                            );
                        }
                    }
                    if self
                        .name_of_node(node)
                        .is_some_and(|name| self.kind_of(name) == SyntaxKind::PrivateIdentifier)
                    {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_cannot_be_used_with_a_private_identifier,
                            &["abstract"],
                        );
                    }
                    flags |= ModifierFlags::ABSTRACT;
                }
                SyntaxKind::AsyncKeyword => {
                    if flags.intersects(ModifierFlags::ASYNC) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_already_seen,
                            &["async"],
                        );
                    } else if flags.intersects(ModifierFlags::AMBIENT) || parent_is_ambient {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_cannot_be_used_in_an_ambient_context,
                            &["async"],
                        );
                    } else if node_kind == SyntaxKind::Parameter {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_cannot_appear_on_a_parameter,
                            &["async"],
                        );
                    }
                    if flags.intersects(ModifierFlags::ABSTRACT) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_cannot_be_used_with_1_modifier,
                            &["async", "abstract"],
                        );
                    }
                    flags |= ModifierFlags::ASYNC;
                    last_async = Some(modifier);
                }
                SyntaxKind::InKeyword | SyntaxKind::OutKeyword => {
                    let in_out_flag = if modifier_kind == SyntaxKind::InKeyword {
                        ModifierFlags::IN
                    } else {
                        ModifierFlags::OUT
                    };
                    // JSDoc template parameters are governed by their
                    // effective host. With no host, a sibling typedef
                    // tag is the type-alias container.
                    let effective_parent = if parent_kind == Some(SyntaxKind::JSDocTemplateTag) {
                        parent
                            .and_then(|template| {
                                self.get_effective_jsdoc_host(template).or_else(|| {
                                    self.parent_of(template).and_then(|document| {
                                        let NodeData::JSDoc(data) = self.data_of(document) else {
                                            return None;
                                        };
                                        self.nodes_of(data.tags).into_iter().find(|&tag| {
                                            self.kind_of(tag) == SyntaxKind::JSDocTypedefTag
                                        })
                                    })
                                })
                            })
                            .or(parent)
                    } else {
                        parent
                    };
                    let effective_parent_kind = effective_parent.map(|parent| self.kind_of(parent));
                    // `node.kind !== TypeParameter || parent && !(...)`:
                    // a parentless type parameter does NOT report.
                    if node_kind != SyntaxKind::TypeParameter
                        || effective_parent_kind.is_some_and(|kind| {
                            !matches!(
                                kind,
                                SyntaxKind::InterfaceDeclaration
                                    | SyntaxKind::ClassDeclaration
                                    | SyntaxKind::ClassExpression
                                    | SyntaxKind::TypeAliasDeclaration
                                    | SyntaxKind::JSDocTypedefTag
                            )
                        })
                    {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_can_only_appear_on_a_type_parameter_of_a_class_interface_or_type_alias,
                            &[if modifier_kind == SyntaxKind::InKeyword {
                                "in"
                            } else {
                                "out"
                            }],
                        );
                    }
                    if flags.intersects(in_out_flag) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_already_seen,
                            &[if modifier_kind == SyntaxKind::InKeyword {
                                "in"
                            } else {
                                "out"
                            }],
                        );
                    }
                    if in_out_flag == ModifierFlags::IN && flags.intersects(ModifierFlags::OUT) {
                        return self.grammar_error_on_node(
                            modifier,
                            &diagnostics::_0_modifier_must_precede_1_modifier,
                            &["in", "out"],
                        );
                    }
                    flags |= in_out_flag;
                }
                _ => {}
            }
        }
        if node_kind == SyntaxKind::Constructor {
            if flags.intersects(ModifierFlags::STATIC) {
                return self.grammar_error_on_node(
                    last_static.expect("static flag records its modifier"),
                    &diagnostics::_0_modifier_cannot_appear_on_a_constructor_declaration,
                    &["static"],
                );
            }
            if flags.intersects(ModifierFlags::OVERRIDE) {
                return self.grammar_error_on_node(
                    last_override.expect("override flag records its modifier"),
                    &diagnostics::_0_modifier_cannot_appear_on_a_constructor_declaration,
                    &["override"],
                );
            }
            if flags.intersects(ModifierFlags::ASYNC) {
                return self.grammar_error_on_node(
                    last_async.expect("async flag records its modifier"),
                    &diagnostics::_0_modifier_cannot_appear_on_a_constructor_declaration,
                    &["async"],
                );
            }
            return false;
        }
        if matches!(
            node_kind,
            SyntaxKind::ImportDeclaration | SyntaxKind::ImportEqualsDeclaration
        ) && flags.intersects(ModifierFlags::AMBIENT)
        {
            return self.grammar_error_on_node(
                last_declare.expect("declare flag records its modifier"),
                &diagnostics::A_0_modifier_cannot_be_used_with_an_import_declaration,
                &["declare"],
            );
        }
        if node_kind == SyntaxKind::Parameter
            && flags.intersects(ModifierFlags::PARAMETER_PROPERTY_MODIFIER)
        {
            let (name_is_pattern, has_dot_dot_dot) = match self.data_of(node) {
                NodeData::Parameter(data) => (
                    data.name.is_some_and(|name| {
                        matches!(
                            self.kind_of(name),
                            SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern
                        )
                    }),
                    data.dot_dot_dot_token.is_some(),
                ),
                _ => (false, false),
            };
            if name_is_pattern {
                return self.grammar_error_on_node(
                    node,
                    &diagnostics::A_parameter_property_may_not_be_declared_using_a_binding_pattern,
                    &[],
                );
            }
            if has_dot_dot_dot {
                return self.grammar_error_on_node(
                    node,
                    &diagnostics::A_parameter_property_cannot_be_declared_using_a_rest_parameter,
                    &[],
                );
            }
        }
        if flags.intersects(ModifierFlags::ASYNC) {
            return self.check_grammar_async_modifier(
                node,
                last_async.expect("async flag records its modifier"),
            );
        }
        false
    }

    /// tsc-port: reportObviousModifierErrors @6.0.3
    /// tsc-hash: 1253f02e67c0d9dc5a5d8bc962446417abdabf9f6836e69e20ca13383781592e
    /// tsc-span: _tsc.js:89326-89330
    /// d2: d2:89d6695aecb00ac6fe50a0fd96b200447b88716016cdbe55f33ead8ee910e089
    fn report_obvious_modifier_errors(&mut self, node: NodeId) -> Option<bool> {
        let source = self.binder.source_of_node(node);
        let Some(modifiers) = node_util::modifiers_of(source, node) else {
            return Some(false);
        };
        let modifiers = self.binder.node_array(modifiers).nodes.clone();
        let first_modifier_except = |allowed: Option<SyntaxKind>| {
            modifiers
                .iter()
                .copied()
                .find(|&modifier| self.kind_of(modifier) != SyntaxKind::Decorator)
                .filter(|&modifier| Some(self.kind_of(modifier)) != allowed)
        };
        let parent_kind = self.parent_of(node).map(|parent| self.kind_of(parent));
        let illegal = match self.kind_of(node) {
            SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::Constructor
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::PropertySignature
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::MethodSignature
            | SyntaxKind::IndexSignature
            | SyntaxKind::ModuleDeclaration
            | SyntaxKind::ImportDeclaration
            | SyntaxKind::ImportEqualsDeclaration
            | SyntaxKind::ExportDeclaration
            | SyntaxKind::ExportAssignment
            | SyntaxKind::FunctionExpression
            | SyntaxKind::ArrowFunction
            | SyntaxKind::Parameter
            | SyntaxKind::TypeParameter => None,
            SyntaxKind::ClassStaticBlockDeclaration
            | SyntaxKind::PropertyAssignment
            | SyntaxKind::ShorthandPropertyAssignment
            | SyntaxKind::NamespaceExportDeclaration
            | SyntaxKind::MissingDeclaration => first_modifier_except(None),
            _ if matches!(
                parent_kind,
                Some(SyntaxKind::ModuleBlock) | Some(SyntaxKind::SourceFile)
            ) =>
            {
                None
            }
            SyntaxKind::FunctionDeclaration => {
                first_modifier_except(Some(SyntaxKind::AsyncKeyword))
            }
            SyntaxKind::ClassDeclaration | SyntaxKind::ConstructorType => {
                first_modifier_except(Some(SyntaxKind::AbstractKeyword))
            }
            SyntaxKind::ClassExpression
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::TypeAliasDeclaration => first_modifier_except(None),
            SyntaxKind::VariableStatement => {
                let using = match self.data_of(node) {
                    NodeData::VariableStatement(data) => {
                        data.declaration_list.is_some_and(|list| {
                            self.node_flags(list) & tsc_types::NodeFlags::USING.bits() != 0
                        })
                    }
                    _ => false,
                };
                first_modifier_except(using.then_some(SyntaxKind::AwaitKeyword))
            }
            SyntaxKind::EnumDeclaration => first_modifier_except(Some(SyntaxKind::ConstKeyword)),
            _ => None,
        };
        illegal.map(|modifier| {
            self.grammar_error_on_first_token(
                modifier,
                &diagnostics::Modifiers_cannot_appear_here,
                &[],
            )
        })
    }

    /// tsc-port: reportObviousDecoratorErrors @6.0.3
    /// tsc-hash: 6d7dc7cfc009f9a1a3a358fcc89a00cb93c909d2d7862e95ac462d258192fc38
    /// tsc-span: _tsc.js:89384-89387
    /// d2: d2:55c15520a95ac1344d3d7821855541d5399e383a7f086017cba15c88381e54f9
    fn report_obvious_decorator_errors(&mut self, node: NodeId) -> Option<bool> {
        let can_have_illegal_decorators = matches!(
            self.kind_of(node),
            SyntaxKind::PropertyAssignment
                | SyntaxKind::ShorthandPropertyAssignment
                | SyntaxKind::FunctionDeclaration
                | SyntaxKind::Constructor
                | SyntaxKind::IndexSignature
                | SyntaxKind::ClassStaticBlockDeclaration
                | SyntaxKind::MissingDeclaration
                | SyntaxKind::VariableStatement
                | SyntaxKind::InterfaceDeclaration
                | SyntaxKind::TypeAliasDeclaration
                | SyntaxKind::EnumDeclaration
                | SyntaxKind::ModuleDeclaration
                | SyntaxKind::ImportEqualsDeclaration
                | SyntaxKind::ImportDeclaration
                | SyntaxKind::NamespaceExportDeclaration
                | SyntaxKind::ExportDeclaration
                | SyntaxKind::ExportAssignment
        );
        if !can_have_illegal_decorators {
            return None;
        }
        let source = self.binder.source_of_node(node);
        self.nodes_of(node_util::modifiers_of(source, node))
            .into_iter()
            .find(|&modifier| self.kind_of(modifier) == SyntaxKind::Decorator)
            .map(|decorator| {
                self.grammar_error_on_first_token(
                    decorator,
                    &diagnostics::Decorators_are_not_valid_here,
                    &[],
                )
            })
    }

    /// tsc-port: checkGrammarAsyncModifier @6.0.3
    /// tsc-hash: 24cae4dbfb53b55566767e7afe44557b79096ed1d2c346f2176d00afbc384716
    /// tsc-span: _tsc.js:89391-89400
    /// d2: d2:af3c695b7f591048514cd42492ae7f9dc689c40d3ecfe82de03d20f97eb0d22c
    fn check_grammar_async_modifier(&mut self, node: NodeId, async_modifier: NodeId) -> bool {
        if matches!(
            self.kind_of(node),
            SyntaxKind::MethodDeclaration
                | SyntaxKind::FunctionDeclaration
                | SyntaxKind::FunctionExpression
                | SyntaxKind::ArrowFunction
        ) {
            return false;
        }
        self.grammar_error_on_node(
            async_modifier,
            &diagnostics::_0_modifier_cannot_be_used_here,
            &["async"],
        )
    }

    /// tsc-port: checkGrammarStatementInAmbientContext @6.0.3
    /// tsc-hash: c3ff8c8e4b3e50b58e8e6424b52b33c91680dae809a10c8901d04c1d586a447e
    /// tsc-span: _tsc.js:90326-90341
    ///
    /// Live from 5.5a (checkExpressionStatement's head); the
    /// EmptyStatement/DebuggerStatement arm and checkBlock's Block arm
    /// were already routed here as 5.4 stub hooks.
    pub(crate) fn check_grammar_statement_in_ambient_context(&mut self, node: NodeId) {
        if self.node_flags(node) & tsc_types::NodeFlags::AMBIENT.bits() == 0 {
            return;
        }
        let parent = self.parent_of(node);
        let parent_kind = parent.map(|parent| self.kind_of(parent));
        let parent_is_function_like_or_accessor = parent_kind.is_some_and(|kind| {
            tsc_binder::node_util::is_function_like_kind(kind)
                || matches!(kind, SyntaxKind::GetAccessor | SyntaxKind::SetAccessor)
        });
        if !self
            .links
            .node(node)
            .has_reported_statement_in_ambient_context
            && parent_is_function_like_or_accessor
        {
            if self.grammar_error_on_first_token(
                node,
                &diagnostics::An_implementation_cannot_be_declared_in_ambient_contexts,
                &[],
            ) {
                self.links
                    .set_node_has_reported_statement_in_ambient_context(
                        self.speculation_depth,
                        node,
                    );
            }
            return;
        }
        if matches!(
            parent_kind,
            Some(SyntaxKind::Block) | Some(SyntaxKind::ModuleBlock) | Some(SyntaxKind::SourceFile)
        ) {
            let parent = parent.expect("kind implies presence");
            if !self
                .links
                .node(parent)
                .has_reported_statement_in_ambient_context
                && self.grammar_error_on_first_token(
                    node,
                    &diagnostics::Statements_are_not_allowed_in_ambient_contexts,
                    &[],
                )
            {
                self.links
                    .set_node_has_reported_statement_in_ambient_context(
                        self.speculation_depth,
                        parent,
                    );
            }
        }
    }

    /// tsc-port: checkSourceElement @6.0.3
    /// tsc-hash: c12862a5ae92efd7462578857c33c1ac3e25d6866d53c33c1166571161ecf821
    /// tsc-span: _tsc.js:86546-86556
    pub(crate) fn check_source_element(&mut self, node: Option<NodeId>) {
        let Some(node) = node else { return };
        let save_current_node = self.current_node;
        let save_within_unreachable_code = self.within_unreachable_code;
        self.current_node = Some(node);
        self.instantiation_count = 0;
        #[cfg(debug_assertions)]
        let unwind_entry = self.unwind_snapshot();
        // CheckAbort containment boundary: tsc has no failure channel
        // here; an Err abandons this element's remaining checks
        // and the caller's loop continues. TSRS_TRACE_CONTAIN=1 prints
        // the typed abort (debug aid).
        if let Err(err) = self.check_source_element_worker(node) {
            self.mark_oracle_crash_range(node, err);
            if std::env::var_os("TSRS_TRACE_CONTAIN").is_some() {
                eprintln!("contained @{node:?}: {err}");
            }
        }
        #[cfg(debug_assertions)]
        self.assert_unwound(&unwind_entry, node, "check_source_element");
        self.current_node = save_current_node;
        self.within_unreachable_code = save_within_unreachable_code;
    }

    /// tsc-port: checkSourceElementUnreachable @6.0.3
    /// tsc-hash: 1f190f12f81e1a59e42e5348233a3c30cbc2b2562d19e0a1c3c35d5fd19811e4
    /// tsc-span: _tsc.js:86763-86807
    /// d2: d2:ba92c132f50831c49a7b357db1af2a54283175b6b29818ac75227ee378bb6871
    ///
    /// The aggregation walk widens the report range over ADJACENT
    /// unreachable statements of the same canHaveStatements parent
    /// (marking each reported) so ONE 7027 covers the run.
    /// addErrorOrSuggestion's tri-state option projection is preserved:
    /// absent = suggestion, explicit false = error, true = suppressed.
    fn check_source_element_unreachable(&mut self, node: NodeId) -> CheckResult<bool> {
        if !tsc_binder::node_util::is_potentially_executable_node(
            self.binder.source_of_node(node),
            node,
        ) {
            return Ok(false);
        }
        if self.reported_unreachable_nodes.contains(&node) {
            return Ok(true);
        }
        if !self.is_source_element_unreachable(node)? {
            return Ok(false);
        }
        self.reported_unreachable_nodes.insert(node);
        let mut start_node = node;
        let mut end_node = node;
        if let Some(parent) = self.parent_of(node) {
            // canHaveStatements (20193): Block | ModuleBlock |
            // SourceFile | CaseClause | DefaultClause.
            let statements = match self.data_of(parent) {
                NodeData::Block(data) => data.statements,
                NodeData::ModuleBlock(data) => data.statements,
                NodeData::SourceFile(data) => data.statements,
                NodeData::CaseClause(data) => data.statements,
                NodeData::DefaultClause(data) => data.statements,
                _ => None,
            };
            let statements: Vec<NodeId> = statements
                .map(|statements| self.binder.node_array(statements).nodes.clone())
                .unwrap_or_default();
            if let Some(offset) = statements.iter().position(|&statement| statement == node) {
                let mut first = offset;
                for index in (0..offset).rev() {
                    let prev_node = statements[index];
                    if !tsc_binder::node_util::is_potentially_executable_node(
                        self.binder.source_of_node(prev_node),
                        prev_node,
                    ) || self.reported_unreachable_nodes.contains(&prev_node)
                        || !self.is_source_element_unreachable(prev_node)?
                    {
                        break;
                    }
                    first = index;
                    self.reported_unreachable_nodes.insert(prev_node);
                }
                let mut last = offset;
                for (index, &next_node) in statements.iter().enumerate().skip(offset + 1) {
                    if !tsc_binder::node_util::is_potentially_executable_node(
                        self.binder.source_of_node(next_node),
                        next_node,
                    ) || !self.is_source_element_unreachable(next_node)?
                    {
                        break;
                    }
                    last = index;
                    self.reported_unreachable_nodes.insert(next_node);
                }
                start_node = statements[first];
                end_node = statements[last];
            }
        }
        // getTokenPosOfNode = skipTrivia from the node's pos.
        let start = tsc_syntax::skip_trivia(
            self.binder.source_of_node(start_node).text(),
            self.pos_of(start_node) as usize,
        );
        let end = self.end_of(end_node) as usize;
        let index = self.error_at_byte_range(
            start_node,
            start,
            end,
            &diagnostics::Unreachable_code_detected,
        );
        if self.options.allow_unreachable_code != Some(false) {
            self.diagnostics[index].message.category = DiagnosticCategory::Suggestion;
        }
        Ok(true)
    }

    /// tsc-port: isSourceElementUnreachable @6.0.3
    /// tsc-hash: 5f7c848932df1b81ac6c8d321b23d171a50d8818c9f3999e224f9814ee2f440e
    /// tsc-span: _tsc.js:86808-86822
    ///
    /// `canHaveFlowNode(node) && node.flowNode` collapses to the
    /// node_flow side-table probe — the binder records flow only for
    /// canHaveFlowNode kinds.
    fn is_source_element_unreachable(&mut self, node: NodeId) -> CheckResult<bool> {
        if self.node_flags(node) & tsc_types::NodeFlags::UNREACHABLE.bits() != 0 {
            return Ok(match self.kind_of(node) {
                SyntaxKind::EnumDeclaration => {
                    !self.is_enum_const(node) || self.options.should_preserve_const_enums()
                }
                SyntaxKind::ModuleDeclaration => self.is_instantiated_module(node),
                _ => true,
            });
        }
        if let Some(flow) = self.flow_node_of(node) {
            let file = self.binder.file_index_of_node(node);
            return Ok(!self.is_reachable_flow_node(file, flow)?);
        }
        Ok(false)
    }

    /// tsc-port: checkSourceElementWorker @6.0.3
    /// tsc-hash: d6ea535a4da409c325e4d3f6e1f725363167efcae08f3c5a8e6258bfdabbbe36
    /// tsc-span: _tsc.js:86557-86762
    ///
    /// The parser-owned `js_doc` arrays are walked before the host,
    /// exactly like tsc. This is the sole JSDoc dispatch path: checker
    /// consumers do not reconstruct tags from source text.
    fn check_source_element_worker(&mut self, node: NodeId) -> CheckResult<()> {
        self.check_attached_jsdoc(node);
        if self.options.allow_unreachable_code != Some(true)
            && !self.within_unreachable_code
            && self.check_source_element_unreachable(node)?
        {
            self.within_unreachable_code = true;
        }
        match self.kind_of(node) {
            SyntaxKind::TypeParameter => self.check_type_parameter(node),
            SyntaxKind::Parameter => self.check_parameter(node),
            SyntaxKind::PropertyDeclaration => self.check_property_declaration(node),
            SyntaxKind::PropertySignature => self.check_property_signature(node),
            SyntaxKind::ConstructorType
            | SyntaxKind::FunctionType
            | SyntaxKind::CallSignature
            | SyntaxKind::ConstructSignature
            | SyntaxKind::IndexSignature => self.check_signature_declaration(node),
            SyntaxKind::MethodDeclaration | SyntaxKind::MethodSignature => {
                self.check_method_declaration(node)
            }
            SyntaxKind::ClassStaticBlockDeclaration => {
                self.check_class_static_block_declaration(node)
            }
            SyntaxKind::Constructor => self.check_constructor_declaration(node),
            SyntaxKind::GetAccessor | SyntaxKind::SetAccessor => {
                self.check_accessor_declaration(node)
            }
            SyntaxKind::TypeReference => self.check_type_reference_node(node),
            SyntaxKind::TypePredicate => self.check_type_predicate(node),
            SyntaxKind::TypeQuery => self.check_type_query(node),
            SyntaxKind::TypeLiteral => self.check_type_literal(node),
            SyntaxKind::ArrayType => self.check_array_type(node),
            SyntaxKind::TupleType => self.check_tuple_type(node),
            SyntaxKind::UnionType | SyntaxKind::IntersectionType => {
                self.check_union_or_intersection_type(node)
            }
            SyntaxKind::ParenthesizedType => {
                let NodeData::ParenthesizedType(data) = self.data_of(node) else {
                    unreachable!("kind/data agree");
                };
                self.check_source_element(data.r#type);
                Ok(())
            }
            SyntaxKind::OptionalType => {
                let NodeData::OptionalType(data) = self.data_of(node) else {
                    unreachable!("kind/data agree");
                };
                self.check_source_element(data.r#type);
                Ok(())
            }
            SyntaxKind::RestType => {
                let NodeData::RestType(data) = self.data_of(node) else {
                    unreachable!("kind/data agree");
                };
                self.check_source_element(data.r#type);
                Ok(())
            }
            SyntaxKind::ThisType => self.check_this_type(node),
            SyntaxKind::TypeOperator => self.check_type_operator(node),
            SyntaxKind::ConditionalType => self.check_conditional_type(node),
            SyntaxKind::InferType => self.check_infer_type(node),
            SyntaxKind::TemplateLiteralType => self.check_template_literal_type(node),
            SyntaxKind::ImportType => self.check_import_type(node),
            SyntaxKind::NamedTupleMember => self.check_named_tuple_member(node),
            SyntaxKind::JSDocAugmentsTag => self.check_jsdoc_augments_tag(node),
            SyntaxKind::JSDocImplementsTag => self.check_jsdoc_implements_tag(node),
            SyntaxKind::JSDocTypedefTag
            | SyntaxKind::JSDocCallbackTag
            | SyntaxKind::JSDocEnumTag => self.check_jsdoc_type_alias_tag(node),
            SyntaxKind::JSDocTemplateTag => self.check_jsdoc_template_tag(node),
            SyntaxKind::JSDocTypeTag => self.check_jsdoc_type_tag(node),
            SyntaxKind::JSDocLink | SyntaxKind::JSDocLinkCode | SyntaxKind::JSDocLinkPlain => {
                self.check_jsdoc_link_like_tag(node)
            }
            SyntaxKind::JSDocParameterTag | SyntaxKind::JSDocPropertyTag => {
                self.check_jsdoc_property_like_tag(node)
            }
            SyntaxKind::JSDocFunctionType => {
                self.check_jsdoc_function_type(node)?;
                self.check_jsdoc_type_is_in_js_file(node)?;
                self.check_source_element_children(node);
                Ok(())
            }
            SyntaxKind::JSDocNonNullableType | SyntaxKind::JSDocNullableType => {
                self.check_jsdoc_type_is_in_js_file(node)?;
                let inner = match self.data_of(node) {
                    NodeData::JSDocNonNullableType(data) => data.r#type,
                    NodeData::JSDocNullableType(data) => data.r#type,
                    _ => unreachable!("kind/data agree"),
                };
                self.check_source_element(inner);
                Ok(())
            }
            SyntaxKind::JSDocAllType
            | SyntaxKind::JSDocUnknownType
            | SyntaxKind::JSDocTypeLiteral => {
                self.check_jsdoc_type_is_in_js_file(node)?;
                self.check_source_element_children(node);
                Ok(())
            }
            SyntaxKind::JSDocVariadicType => self.check_jsdoc_variadic_type(node),
            SyntaxKind::JSDocTypeExpression => {
                let NodeData::JSDocTypeExpression(data) = self.data_of(node) else {
                    unreachable!("kind/data agree");
                };
                self.check_source_element(data.r#type);
                Ok(())
            }
            SyntaxKind::JSDocPublicTag
            | SyntaxKind::JSDocProtectedTag
            | SyntaxKind::JSDocPrivateTag => self.check_jsdoc_accessibility_modifier(node),
            SyntaxKind::JSDocSatisfiesTag => self.check_jsdoc_satisfies_tag(node),
            SyntaxKind::JSDocThisTag => self.check_jsdoc_this_tag(node),
            SyntaxKind::JSDocImportTag => self.check_jsdoc_import_tag(node),
            SyntaxKind::IndexedAccessType => self.check_indexed_access_type(node),
            SyntaxKind::MappedType => self.check_mapped_type(node),
            SyntaxKind::FunctionDeclaration => self.check_function_declaration(node),
            SyntaxKind::Block | SyntaxKind::ModuleBlock => self.check_block(node),
            SyntaxKind::VariableStatement => self.check_variable_statement(node),
            SyntaxKind::ExpressionStatement => self.check_expression_statement(node),
            SyntaxKind::IfStatement => self.check_if_statement(node),
            SyntaxKind::DoStatement => self.check_do_statement(node),
            SyntaxKind::WhileStatement => self.check_while_statement(node),
            SyntaxKind::ForStatement => self.check_for_statement(node),
            SyntaxKind::ForInStatement => self.check_for_in_statement(node),
            SyntaxKind::ForOfStatement => self.check_for_of_statement(node),
            SyntaxKind::ContinueStatement | SyntaxKind::BreakStatement => {
                self.check_break_or_continue_statement(node)
            }
            SyntaxKind::ReturnStatement => self.check_return_statement(node),
            SyntaxKind::WithStatement => self.check_with_statement(node),
            SyntaxKind::SwitchStatement => self.check_switch_statement(node),
            SyntaxKind::LabeledStatement => self.check_labeled_statement(node),
            SyntaxKind::ThrowStatement => self.check_throw_statement(node),
            SyntaxKind::TryStatement => self.check_try_statement(node),
            SyntaxKind::VariableDeclaration => self.check_variable_declaration(node),
            SyntaxKind::BindingElement => self.check_binding_element(node),
            SyntaxKind::ClassDeclaration => self.check_class_declaration(node),
            SyntaxKind::InterfaceDeclaration => self.check_interface_declaration(node),
            SyntaxKind::TypeAliasDeclaration => self.check_type_alias_declaration(node),
            SyntaxKind::EnumDeclaration => self.check_enum_declaration(node),
            SyntaxKind::EnumMember => self.check_enum_member(node),
            SyntaxKind::ModuleDeclaration => self.check_module_declaration(node),
            SyntaxKind::ImportDeclaration => self.check_import_declaration(node),
            SyntaxKind::ImportEqualsDeclaration => self.check_import_equals_declaration(node),
            SyntaxKind::ExportDeclaration => self.check_export_declaration(node),
            SyntaxKind::ExportAssignment => self.check_export_assignment(node),
            SyntaxKind::EmptyStatement | SyntaxKind::DebuggerStatement => {
                self.check_grammar_statement_in_ambient_context(node);
                Ok(())
            }
            SyntaxKind::MissingDeclaration => self.check_missing_declaration(node),
            // Tokens (incl. the EndOfFileToken pass) and every kind
            // outside tsc's switch: fall through with no work.
            _ => Ok(()),
        }
    }

    /// tsc-port: checkSourceElementWorker/canHaveJSDoc @6.0.3
    /// tsc-hash: d195ad94e42417345e69bbaabc9f941485fa317f70e27cb3556dfb7a02bc3d4e
    /// tsc-span: _tsc.js:86557-86570
    ///
    /// Comments and tags are parser-owned arena nodes. Links in comment
    /// fragments are checked in every source kind; semantic tag nodes
    /// are dispatched only for JS files, matching tsc.
    fn check_attached_jsdoc(&mut self, host: NodeId) {
        let documents = self.direct_jsdoc_documents(host);
        let in_js_file = self.is_in_js_file(host);
        for document in documents {
            self.check_jsdoc_comment_links(document);
            let tags = match self.data_of(document) {
                NodeData::JSDoc(data) => self.nodes_of(data.tags),
                _ => Vec::new(),
            };
            for tag in tags {
                self.check_jsdoc_comment_links(tag);
                if in_js_file {
                    self.check_source_element(Some(tag));
                }
            }
        }
    }

    /// tsc-port: checkJSDocCommentWorker @6.0.3.
    /// tsc-hash: 07dcd076e06630aafcaced1128dd4a68bd58b80a8e53ce88a22d1155c44d73ff
    /// tsc-span: _tsc.js:86823-86831
    fn check_jsdoc_comment_links(&mut self, owner: NodeId) {
        let source = self.binder.source_of_node(owner);
        let mut links = Vec::new();
        for_each_child(&source.arena, source.arena.node(owner), |child| {
            if matches!(
                source.arena.node(child).kind,
                SyntaxKind::JSDocLink | SyntaxKind::JSDocLinkCode | SyntaxKind::JSDocLinkPlain
            ) {
                links.push(child);
            }
            false
        });
        for link in links {
            self.check_source_element(Some(link));
        }
    }

    fn check_source_element_children(&mut self, node: NodeId) {
        let source = self.binder.source_of_node(node);
        let mut children = Vec::new();
        for_each_child(&source.arena, source.arena.node(node), |child| {
            children.push(child);
            false
        });
        for child in children {
            self.check_source_element(Some(child));
        }
    }

    /// tsc-port: checkJSDocTypeAliasTag @6.0.3.
    /// tsc-hash: e8584035fea4768c02dfe9033860570a747273628f548991de4124fed619139d
    /// tsc-span: _tsc.js:82792-82801
    fn check_jsdoc_type_alias_tag(&mut self, node: NodeId) -> CheckResult<()> {
        let (name, type_expression) = match self.data_of(node) {
            NodeData::JSDocTypedefTag(data) => (data.name, data.type_expression),
            NodeData::JSDocCallbackTag(data) => (data.name, data.type_expression),
            NodeData::JSDocEnumTag(data) => (None, data.type_expression),
            _ => unreachable!("kind/data agree"),
        };
        let name = name.or_else(|| {
            node_util::name_for_nameless_jsdoc_typedef(self.binder.source_of_node(node), node)
        });
        if type_expression.is_none() {
            self.error_at(
                name,
                &diagnostics::JSDoc_typedef_tag_should_either_have_a_type_annotation_or_be_followed_by_property_or_member_tags,
                &[],
            );
        }
        if let Some(name) = name {
            self.check_type_name_is_reserved(name, &diagnostics::Type_alias_name_cannot_be_0);
        }
        self.check_source_element(type_expression);
        let type_parameters = self.type_parameter_declarations_of(node);
        self.check_type_parameters(&type_parameters)
    }

    /// tsc-port: checkJSDocTemplateTag @6.0.3.
    /// tsc-hash: ded4243dfc2699c2c1344c2f1c8bc4df304f588dac1493cd4ebec82e76b568ef
    /// tsc-span: _tsc.js:82802-82807
    fn check_jsdoc_template_tag(&mut self, node: NodeId) -> CheckResult<()> {
        let NodeData::JSDocTemplateTag(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let constraint = data.constraint;
        let type_parameters = self.nodes_of(data.type_parameters);
        self.check_source_element(constraint);
        for type_parameter in type_parameters {
            self.check_source_element(Some(type_parameter));
        }
        Ok(())
    }

    /// tsc-port: checkJSDocTypeTag @6.0.3.
    /// tsc-hash: 2e202c0bf55e29a7ac3d3d5a8e96aef15064b356c66ee9bd8065012026e4ceae
    /// tsc-span: _tsc.js:82808-82810
    fn check_jsdoc_type_tag(&mut self, node: NodeId) -> CheckResult<()> {
        let NodeData::JSDocTypeTag(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        self.check_source_element(data.type_expression);
        Ok(())
    }

    /// tsc-port: checkJSDocSatisfiesTag @6.0.3.
    /// tsc-hash: 06ba243cd86ac0b5ccf0af74f1537067992744abd80f186d85d8ae427648070a
    /// tsc-span: _tsc.js:82811-82823
    fn check_jsdoc_satisfies_tag(&mut self, node: NodeId) -> CheckResult<()> {
        let NodeData::JSDocSatisfiesTag(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        self.check_source_element(data.type_expression);
        if let Some(host) = self.get_effective_jsdoc_host(node) {
            let tags = self.all_jsdoc_tags(host, SyntaxKind::JSDocSatisfiesTag);
            for duplicate in tags.into_iter().skip(1) {
                let tag_name = match self.data_of(duplicate) {
                    NodeData::JSDocSatisfiesTag(data) => data.tag_name,
                    _ => None,
                };
                let text = tag_name
                    .and_then(|name| self.identifier_text_of(name))
                    .unwrap_or("satisfies")
                    .to_owned();
                self.error_at(
                    tag_name.or(Some(duplicate)),
                    &diagnostics::_0_tag_already_specified,
                    &[&text],
                );
            }
        }
        Ok(())
    }

    /// tsc-port: checkJSDocLinkLikeTag @6.0.3.
    /// tsc-hash: 670de5faef306240a1f40aedcbe389e3bdcb2495dfe29b06ca8086c83118a0af
    /// tsc-span: _tsc.js:82824-82832
    fn check_jsdoc_link_like_tag(&mut self, node: NodeId) -> CheckResult<()> {
        let name = match self.data_of(node) {
            NodeData::JSDocLink(data) => data.name,
            NodeData::JSDocLinkCode(data) => data.name,
            NodeData::JSDocLinkPlain(data) => data.name,
            _ => unreachable!("kind/data agree"),
        };
        if let Some(name) = name {
            self.resolve_jsdoc_member_name(name, true, None)?;
        }
        Ok(())
    }

    /// tsc-port: resolveJSDocMemberName @6.0.3
    /// tsc-hash: e7e2debbbb67bd344dca35bbc7e3d03f8c89d8fcde7c332878bc880f05c73a51
    /// tsc-span: _tsc.js:87505-87530
    fn resolve_jsdoc_member_name(
        &mut self,
        name: NodeId,
        ignore_errors: bool,
        container: Option<SymbolId>,
    ) -> CheckResult<Option<SymbolId>> {
        let meaning = SymbolFlags::TYPE | SymbolFlags::NAMESPACE | SymbolFlags::VALUE;
        if matches!(
            self.kind_of(name),
            SyntaxKind::Identifier | SyntaxKind::QualifiedName
        ) {
            let mut symbol = self.resolve_entity_name_ex(
                name,
                meaning,
                ignore_errors,
                self.get_host_signature_from_jsdoc(name),
                true,
            )?;
            if symbol.is_none() && self.kind_of(name) == SyntaxKind::Identifier {
                if let Some(container) = container {
                    let exports = self.get_exports_of_symbol(container)?;
                    if let Some(text) = self.identifier_text_of(name).map(str::to_owned) {
                        symbol = self.get_symbol_in_table(&exports, &text, meaning)?;
                    }
                }
            }
            if symbol.is_some() {
                return Ok(symbol);
            }
        }

        let (left, right) = match self.data_of(name) {
            NodeData::Identifier(data) => (container, data.escaped_text.clone()),
            NodeData::JSDocMemberName(data) => {
                let left = match data.left {
                    Some(left) => self.resolve_jsdoc_member_name(left, ignore_errors, container)?,
                    None => None,
                };
                let right = data
                    .right
                    .and_then(|right| self.identifier_text_of(right))
                    .unwrap_or_default()
                    .to_owned();
                (left, right)
            }
            _ => (None, String::new()),
        };
        let Some(left) = left else {
            return Ok(None);
        };
        let prototype = if self
            .binder
            .symbol(left)
            .flags
            .intersects(SymbolFlags::VALUE)
        {
            let left_type = self.get_type_of_symbol(left)?;
            self.get_property_of_type_full(left_type, "prototype")?
        } else {
            None
        };
        let container_type = if let Some(prototype) = prototype {
            self.get_type_of_symbol(prototype)?
        } else {
            self.get_declared_type_of_symbol_slice(left)?
        };
        self.get_property_of_type_full(container_type, &right)
    }

    /// tsc-port: checkJSDocParameterTag @6.0.3.
    /// tsc-hash: e70c6b994ada1bce6583a1e9ccd7bd9fbaedce9605c4a26349506a80985d0d78
    /// tsc-span: _tsc.js:82833-82835
    /// tsc-port: checkJSDocPropertyTag @6.0.3.
    /// tsc-hash: bbcb0b6e77882141e513682989e45af8057e40acd9ee399e0cba630be39b34bd
    /// tsc-span: _tsc.js:82836-82838
    fn check_jsdoc_property_like_tag(&mut self, node: NodeId) -> CheckResult<()> {
        let type_expression = match self.data_of(node) {
            NodeData::JSDocParameterTag(data) => data.type_expression,
            NodeData::JSDocPropertyTag(data) => data.type_expression,
            _ => unreachable!("kind/data agree"),
        };
        self.check_source_element(type_expression);
        Ok(())
    }

    /// tsc-port: checkJSDocFunctionType @6.0.3.
    /// tsc-hash: dcd5df8de17d4ba1c2cd700b063e6627ec2d6916778de6a0160053eaa97e9c6f
    /// tsc-span: _tsc.js:82839-82847
    fn check_jsdoc_function_type(&mut self, node: NodeId) -> CheckResult<()> {
        self.check_signature_declaration(node)?;
        let NodeData::JSDocFunctionType(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        if data.r#type.is_none()
            && !node_util::is_jsdoc_construct_signature(self.binder.source_of_node(node), node)
        {
            self.report_implicit_any(node, self.tables.intrinsics.any, None)?;
        }
        Ok(())
    }

    /// tsc-port: checkJSDocThisTag @6.0.3.
    /// tsc-hash: 18946feb0425c704efe0833f43e6929eb2f252e20d79493b13c4671207604afa
    /// tsc-span: _tsc.js:82848-82853
    fn check_jsdoc_this_tag(&mut self, node: NodeId) -> CheckResult<()> {
        if self
            .get_effective_jsdoc_host(node)
            .is_some_and(|host| self.kind_of(host) == SyntaxKind::ArrowFunction)
        {
            let tag_name = match self.data_of(node) {
                NodeData::JSDocThisTag(data) => data.tag_name,
                _ => None,
            };
            self.error_at(
                tag_name.or(Some(node)),
                &diagnostics::An_arrow_function_cannot_have_a_this_parameter,
                &[],
            );
        }
        Ok(())
    }

    /// tsc-port: checkJSDocImportTag @6.0.3.
    /// tsc-hash: d5bedc1e5d403ebe956b54ea38ea0d9ab0726319180f4e24c61b1422882e258c
    /// tsc-span: _tsc.js:82854-82856
    fn check_jsdoc_import_tag(&mut self, node: NodeId) -> CheckResult<()> {
        self.check_import_attributes_of(node)
    }

    /// tsc's `error(classLike, ...)` is a global diagnostic when the
    /// effective JSDoc host is absent. The raw checker sink retains
    /// every locationless probe; publish only the producer-owned row
    /// through the aggregate global-diagnostic stream.
    fn error_jsdoc_not_attached_to_class(&mut self, class_like: Option<NodeId>, tag_text: &str) {
        let diagnostics_before = self.diagnostics.len();
        self.error_at(
            class_like,
            &diagnostics::JSDoc_0_is_not_attached_to_a_class,
            &[tag_text],
        );
        if class_like.is_none() {
            self.publish_visible_global_diagnostics_since(diagnostics_before);
        }
    }

    /// tsc-port: checkJSDocImplementsTag @6.0.3.
    /// tsc-hash: 44e500ddec853eafef3e591be086bd6d8b14f5ba03c88791117f60656274bcbb
    /// tsc-span: _tsc.js:82857-82862
    fn check_jsdoc_implements_tag(&mut self, node: NodeId) -> CheckResult<()> {
        let tag_name = match self.data_of(node) {
            NodeData::JSDocImplementsTag(data) => data.tag_name,
            _ => None,
        };
        let host = self.get_effective_jsdoc_host(node);
        if !host.is_some_and(|host| {
            matches!(
                self.kind_of(host),
                SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
            )
        }) {
            let text = tag_name
                .and_then(|name| self.identifier_text_of(name))
                .unwrap_or("implements")
                .to_owned();
            self.error_jsdoc_not_attached_to_class(host, &text);
        }
        Ok(())
    }

    /// tsc-port: checkJSDocAugmentsTag @6.0.3.
    /// tsc-hash: 66091283e252c6a78b8ea20c9fd3df37d75fd8db4ab430a1c02ff07652c3ce6e
    /// tsc-span: _tsc.js:82863-82882
    fn check_jsdoc_augments_tag(&mut self, node: NodeId) -> CheckResult<()> {
        let (tag_name, class) = match self.data_of(node) {
            NodeData::JSDocAugmentsTag(data) => (data.tag_name, data.class),
            _ => unreachable!("kind/data agree"),
        };
        let tag_text = tag_name
            .and_then(|name| self.identifier_text_of(name))
            .unwrap_or("augments")
            .to_owned();
        let class_like = self.get_effective_jsdoc_host(node);
        if !class_like.is_some_and(|host| {
            matches!(
                self.kind_of(host),
                SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
            )
        }) {
            self.error_jsdoc_not_attached_to_class(class_like, &tag_text);
            return Ok(());
        }
        let class_like = class_like.expect("class-like tested above");
        let tags = self.all_jsdoc_tags(class_like, SyntaxKind::JSDocAugmentsTag);
        if tags.len() > 1 {
            self.error_at(
                Some(tags[1]),
                &diagnostics::Class_declarations_cannot_have_more_than_one_augments_or_extends_tag,
                &[],
            );
        }

        let target_name = class
            .and_then(|class| match self.data_of(class) {
                NodeData::ExpressionWithTypeArguments(data) => data.expression,
                _ => None,
            })
            .and_then(|expression| self.identifier_from_entity_name_expression(expression));
        let extends_name = self
            .get_class_extends_heritage_element(class_like)
            .and_then(|extends| match self.data_of(extends) {
                NodeData::ExpressionWithTypeArguments(data) => data.expression,
                _ => None,
            })
            .and_then(|expression| self.identifier_from_entity_name_expression(expression));
        if let (Some(target_name), Some(extends_name)) = (target_name, extends_name) {
            let target_text = self
                .identifier_text_of(target_name)
                .unwrap_or_default()
                .to_owned();
            let extends_text = self
                .identifier_text_of(extends_name)
                .unwrap_or_default()
                .to_owned();
            if target_text != extends_text {
                self.error_at(
                    Some(target_name),
                    &diagnostics::JSDoc_0_1_does_not_match_the_extends_2_clause,
                    &[&tag_text, &target_text, &extends_text],
                );
            }
        }
        Ok(())
    }

    fn identifier_from_entity_name_expression(&self, node: NodeId) -> Option<NodeId> {
        match self.data_of(node) {
            NodeData::Identifier(_) => Some(node),
            NodeData::PropertyAccessExpression(data) => data.name,
            _ => None,
        }
    }

    /// tsc-port: checkJSDocAccessibilityModifiers @6.0.3.
    /// tsc-hash: 8f6c5add520fd318d80853065eedd5d538e71dc176a67afd17360c3686578e9d
    /// tsc-span: _tsc.js:82883-82888
    fn check_jsdoc_accessibility_modifier(&mut self, node: NodeId) -> CheckResult<()> {
        if let Some(host) = self.get_jsdoc_host(node) {
            let private_identifier_class_element = matches!(
                self.kind_of(host),
                SyntaxKind::PropertyDeclaration
                    | SyntaxKind::MethodDeclaration
                    | SyntaxKind::GetAccessor
                    | SyntaxKind::SetAccessor
            ) && self
                .name_of_node(host)
                .is_some_and(|name| self.kind_of(name) == SyntaxKind::PrivateIdentifier);
            if private_identifier_class_element {
                self.error_at(
                    Some(node),
                    &diagnostics::An_accessibility_modifier_cannot_be_used_with_a_private_identifier,
                    &[],
                );
            }
        }
        Ok(())
    }

    /// tsc-port: checkJSDocVariadicType @6.0.3.
    /// tsc-hash: 80b3b4021eb1511dec9f975d4cf804983d70c8578727f1c38bf3b5b29948ed53
    /// tsc-span: _tsc.js:86852-86878
    fn check_jsdoc_variadic_type(&mut self, node: NodeId) -> CheckResult<()> {
        self.check_jsdoc_type_is_in_js_file(node)?;
        let NodeData::JSDocVariadicType(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        self.check_source_element(data.r#type);
        let Some(parent) = self.parent_of(node) else {
            return Ok(());
        };
        if self.kind_of(parent) == SyntaxKind::Parameter {
            if let Some(function) = self.parent_of(parent) {
                if self.kind_of(function) == SyntaxKind::JSDocFunctionType {
                    if self.parameters_of_function(function).last().copied() != Some(parent) {
                        self.error_at(
                            Some(node),
                            &diagnostics::A_rest_parameter_must_be_last_in_a_parameter_list,
                            &[],
                        );
                    }
                    return Ok(());
                }
            }
        }
        if self.kind_of(parent) != SyntaxKind::JSDocTypeExpression {
            self.error_at(
                Some(node),
                &diagnostics::JSDoc_may_only_appear_in_the_last_parameter_of_a_signature,
                &[],
            );
        }
        let parameter_tag = self.parent_of(parent);
        if !parameter_tag.is_some_and(|parameter_tag| {
            self.kind_of(parameter_tag) == SyntaxKind::JSDocParameterTag
        }) {
            self.error_at(
                Some(node),
                &diagnostics::JSDoc_may_only_appear_in_the_last_parameter_of_a_signature,
                &[],
            );
            return Ok(());
        }
        let parameter_tag = parameter_tag.expect("tested Some above");
        let Some(parameter_symbol) = self.parameter_symbol_from_jsdoc(parameter_tag) else {
            return Ok(());
        };
        let Some(host) = self.get_host_signature_from_jsdoc(parameter_tag) else {
            return Ok(());
        };
        let last_symbol = self
            .parameters_of_function(host)
            .last()
            .and_then(|&parameter| self.node_symbol(parameter));
        if last_symbol != Some(parameter_symbol) {
            self.error_at(
                Some(node),
                &diagnostics::A_rest_parameter_must_be_last_in_a_parameter_list,
                &[],
            );
        }
        Ok(())
    }

    /// tsc-port: checkJSDocTypeIsInJsFile @6.0.3
    /// tsc-hash: 7444e9c93db2af328f6a313bfe8c6d8316b03b06017c82d42c38603ad1b52440
    /// tsc-span: _tsc.js:86832-86851
    ///
    /// M8-P12 closes the TS8020 arm for JSDoc-only source type nodes.
    /// The parser preserves tsc's nullable/non-nullable `postfix`
    /// observable as equal wrapper and operand starts, so both arms
    /// share the upstream boundary without a parallel syntax model.
    ///
    /// d2: d2:abafc814b78c24d9620dd74abb47d59c8a8ec014f90fb0e16a1948578320740d
    fn check_jsdoc_type_is_in_js_file(&mut self, node: NodeId) -> CheckResult<()> {
        if self.is_in_js_file(node) {
            return Ok(());
        }
        let kind = self.kind_of(node);
        if !matches!(
            kind,
            SyntaxKind::JSDocNonNullableType | SyntaxKind::JSDocNullableType
        ) {
            self.grammar_error_on_node(
                node,
                &diagnostics::JSDoc_types_can_only_be_used_inside_documentation_comments,
                &[],
            );
            return Ok(());
        }
        let inner = match self.data_of(node) {
            NodeData::JSDocNonNullableType(data) => data.r#type,
            NodeData::JSDocNullableType(data) => data.r#type,
            _ => unreachable!("kind/data agree"),
        }
        .expect("parser invariant: JSDoc nullable operand always parsed");
        let postfix = self.pos_of(node) == self.pos_of(inner);
        let diagnostic = if postfix {
            &diagnostics::_0_at_the_end_of_a_type_is_not_valid_TypeScript_syntax_Did_you_mean_to_write_1
        } else {
            &diagnostics::_0_at_the_start_of_a_type_is_not_valid_TypeScript_syntax_Did_you_mean_to_write_1
        };
        let token = if kind == SyntaxKind::JSDocNonNullableType {
            "!"
        } else {
            "?"
        };
        let ty = self.get_type_from_type_node(inner)?;
        let suggestion_type = if kind == SyntaxKind::JSDocNullableType
            && ty != self.tables.intrinsics.never
            && ty != self.tables.intrinsics.void
        {
            let mut types = vec![ty, self.tables.intrinsics.undefined];
            if !postfix {
                types.push(self.tables.intrinsics.null);
            }
            self.get_union_type_ex(&types, UnionReduction::Literal)?
        } else {
            ty
        };
        let suggestion = self.type_to_string_slice(suggestion_type)?;
        self.grammar_error_on_node(node, diagnostic, &[token, &suggestion]);
        Ok(())
    }

    /// tsc-port: checkUnmatchedJSDocParameters @6.0.3
    /// tsc-hash: 24e986c3a0401df91b37f56fe493d3f88ba77f96fda7b6f3d48bc970c597b49c
    /// tsc-span: _tsc.js:84792-84829
    pub(crate) fn check_unmatched_jsdoc_parameters(&mut self, node: NodeId) -> CheckResult<()> {
        let jsdoc_parameters: Vec<NodeId> = self
            .get_jsdoc_tags(node)
            .into_iter()
            .filter(|&tag| self.kind_of(tag) == SyntaxKind::JSDocParameterTag)
            .collect();
        if jsdoc_parameters.is_empty() {
            return Ok(());
        }

        let mut parameter_names = std::collections::BTreeSet::new();
        let mut excluded_parameters = std::collections::BTreeSet::new();
        for (index, parameter) in self.parameters_of_function(node).into_iter().enumerate() {
            let NodeData::Parameter(data) = self.data_of(parameter) else {
                continue;
            };
            let Some(name) = data.name else { continue };
            if let Some(name) = self.identifier_text_of(name) {
                parameter_names.insert(name.to_owned());
            } else if matches!(
                self.kind_of(name),
                SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern
            ) {
                excluded_parameters.insert(index);
            }
        }

        let is_js = self.is_in_js_file(node);
        if self.contains_arguments_reference(node)? {
            let index = jsdoc_parameters.len() - 1;
            let tag = jsdoc_parameters[index];
            let (name, type_expression) = match self.data_of(tag) {
                NodeData::JSDocParameterTag(data) => (data.name, data.type_expression),
                _ => unreachable!("filtered above"),
            };
            if is_js
                && name.is_some_and(|name| self.kind_of(name) == SyntaxKind::Identifier)
                && !excluded_parameters.contains(&index)
                && name
                    .and_then(|name| self.identifier_text_of(name))
                    .is_some_and(|name| !parameter_names.contains(name))
            {
                if let Some(type_node) = self.jsdoc_type_expression_type(type_expression) {
                    let ty = self.get_type_from_type_node(type_node)?;
                    if !self.is_array_type(ty)? {
                        let name = name.expect("identifier tested above");
                        let text = self.identifier_text_of(name).unwrap_or_default().to_owned();
                        self.error_at(
                            Some(name),
                            &diagnostics::JSDoc_param_tag_has_name_0_but_there_is_no_parameter_with_that_name_It_would_match_arguments_if_it_had_an_array_type,
                            &[&text],
                        );
                    }
                }
            }
            return Ok(());
        }

        for (index, tag) in jsdoc_parameters.into_iter().enumerate() {
            if excluded_parameters.contains(&index) {
                continue;
            }
            let (name, is_name_first) = match self.data_of(tag) {
                NodeData::JSDocParameterTag(data) => (data.name, data.is_name_first),
                _ => unreachable!("filtered above"),
            };
            let Some(name) = name else { continue };
            if self.kind_of(name) == SyntaxKind::Identifier
                && self
                    .identifier_text_of(name)
                    .is_some_and(|name| parameter_names.contains(name))
            {
                continue;
            }
            if self.kind_of(name) == SyntaxKind::QualifiedName {
                if is_js {
                    let whole = self.entity_name_to_string(name)?;
                    let left = match self.data_of(name) {
                        NodeData::QualifiedName(data) => data.left,
                        _ => None,
                    }
                    .map(|left| self.entity_name_to_string(left))
                    .transpose()?
                    .unwrap_or_default();
                    self.error_at(
                        Some(name),
                        &diagnostics::Qualified_name_0_is_not_allowed_without_a_leading_param_object_1,
                        &[&whole, &left],
                    );
                }
            } else if !is_name_first && is_js {
                let text = self.identifier_text_of(name).unwrap_or_default().to_owned();
                self.error_at(
                    Some(name),
                    &diagnostics::JSDoc_param_tag_has_name_0_but_there_is_no_parameter_with_that_name,
                    &[&text],
                );
            }
        }
        Ok(())
    }

    /// tsc-port: containsArgumentsReference @6.0.3.
    /// tsc-hash: 82aa2d904f94382ddb60f203c344d356e010d8d6173d04d90361324fcec964a0
    /// tsc-span: _tsc.js:59689-59718
    ///
    /// The exact body traversal is cached on NodeLinks and therefore
    /// runs at most once per declaration.
    pub(crate) fn contains_arguments_reference(
        &mut self,
        declaration: NodeId,
    ) -> CheckResult<bool> {
        if let Some(cached) = self.links.node(declaration).contains_arguments_reference {
            return Ok(cached);
        }
        if self
            .links
            .node(declaration)
            .check_flags
            .intersects(NodeCheckFlags::CAPTURE_ARGUMENTS)
        {
            self.links.set_node_contains_arguments_reference(
                self.speculation_depth,
                declaration,
                true,
            );
            return Ok(true);
        }
        let Some(body) = node_util::body_of(self.binder.source_of_node(declaration), declaration)
        else {
            self.links.set_node_contains_arguments_reference(
                self.speculation_depth,
                declaration,
                false,
            );
            return Ok(false);
        };
        let mut stack = vec![body];
        while let Some(current) = stack.pop() {
            match self.kind_of(current) {
                SyntaxKind::Identifier if self.identifier_text_of(current) == Some("arguments") => {
                    let resolved = self.resolve_name(
                        Some(current),
                        "arguments",
                        SymbolFlags::VALUE | SymbolFlags::EXPORT_VALUE,
                        None,
                        true,
                        false,
                    )?;
                    if resolved == Some(self.arguments_symbol) {
                        self.links.set_node_contains_arguments_reference(
                            self.speculation_depth,
                            declaration,
                            true,
                        );
                        return Ok(true);
                    }
                    continue;
                }
                SyntaxKind::PropertyDeclaration
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::GetAccessor
                | SyntaxKind::SetAccessor => {
                    if let Some(name) = self.name_of_node(current) {
                        if self.kind_of(name) == SyntaxKind::ComputedPropertyName {
                            stack.push(name);
                        }
                    }
                    continue;
                }
                SyntaxKind::PropertyAccessExpression => {
                    if let NodeData::PropertyAccessExpression(data) = self.data_of(current) {
                        if let Some(expression) = data.expression {
                            stack.push(expression);
                        }
                    }
                    continue;
                }
                SyntaxKind::ElementAccessExpression => {
                    if let NodeData::ElementAccessExpression(data) = self.data_of(current) {
                        if let Some(expression) = data.expression {
                            stack.push(expression);
                        }
                    }
                    continue;
                }
                SyntaxKind::PropertyAssignment => {
                    if let NodeData::PropertyAssignment(data) = self.data_of(current) {
                        if let Some(initializer) = data.initializer {
                            stack.push(initializer);
                        }
                    }
                    continue;
                }
                SyntaxKind::Constructor
                | SyntaxKind::FunctionExpression
                | SyntaxKind::FunctionDeclaration
                | SyntaxKind::ArrowFunction
                | SyntaxKind::ModuleDeclaration
                | SyntaxKind::SourceFile
                    if current != body =>
                {
                    continue;
                }
                _ => {}
            }
            if self.is_part_of_type_node(current) {
                continue;
            }
            let source = self.binder.source_of_node(current);
            let mut children = Vec::new();
            for_each_child(&source.arena, source.arena.node(current), |child| {
                children.push(child);
                false
            });
            stack.extend(children);
        }
        self.links.set_node_contains_arguments_reference(
            self.speculation_depth,
            declaration,
            false,
        );
        Ok(false)
    }

    /// tsc-port: checkBlock @6.0.3
    /// tsc-hash: ea6aec550a59633f1e11e780af1c7be7f4c89f5b46519add41fcaa41c4c823ad
    /// tsc-span: _tsc.js:83214-83228
    ///
    /// The isFunctionOrModuleBlock flowAnalysisDisabled save/restore is
    /// still represented by the common statement walk. Block-local
    /// unused identifiers are registered after that walk, matching
    /// tsc's deferred producer order.
    pub(crate) fn check_block(&mut self, node: NodeId) -> CheckResult<()> {
        if self.kind_of(node) == SyntaxKind::Block {
            self.check_grammar_statement_in_ambient_context(node);
        }
        let statements = match self.data_of(node) {
            NodeData::Block(data) => data.statements,
            NodeData::ModuleBlock(data) => data.statements,
            _ => unreachable!("kind/data agree"),
        };
        for statement in self.nodes_of(statements) {
            self.check_source_element(Some(statement));
        }
        if self.binder.locals_of(node).is_some() {
            self.register_for_unused_identifiers_check(node);
        }
        Ok(())
    }

    /// tsc-port: checkExpressionStatement @6.0.3
    /// tsc-hash: b4829bc7abe698be517a74f5f9fd6c9bf9c80b681ce0429dceee7a0221903beb
    /// tsc-span: _tsc.js:83622-83625
    ///
    /// The 5.5 forcing seam: the ONLY new eager driver arm at 5.5a —
    /// expression subtrees route through checkExpression from here.
    fn check_expression_statement(&mut self, node: NodeId) -> CheckResult<()> {
        self.check_grammar_statement_in_ambient_context(node);
        let NodeData::ExpressionStatement(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let Some(expression) = data.expression else {
            return Ok(());
        };
        self.check_expression(expression, tsc_types::CheckMode::NORMAL)?;
        Ok(())
    }

    // ---- type parameter checking (the live 5.4 slice) ----

    /// tsc-port: checkTypeParameter @6.0.3
    /// tsc-hash: 201134b5969a61f67c7464f938e17d5169558444d3b624c1da7e2b49c879e53c
    /// tsc-span: _tsc.js:81128-81147
    ///
    /// The `node.expression` grammarErrorOnFirstToken (Type_expected,
    /// parse-recovery trees) is an M7-stub grammar site. The
    /// addLazyDiagnostic wrapper runs its callback inline: the only
    /// diagnostics mode this program has is the eager one
    /// (checkSourceFileWithEagerDiagnostics 87104-87110 sets
    /// `addLazyDiagnostic = cb => cb()`), so eager execution IS the
    /// tsc order.
    fn check_type_parameter(&mut self, node: NodeId) -> CheckResult<()> {
        self.check_grammar_modifiers(node);
        let NodeData::TypeParameter(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let (name, constraint, default) = (data.name, data.constraint, data.r#default);
        self.check_source_element(constraint);
        self.check_source_element(default);
        let symbol = self.get_symbol_of_declaration(node)?;
        let type_parameter = self.get_declared_type_of_type_parameter(symbol);
        self.get_base_constraint_of_type(type_parameter)?;
        if !self.has_non_circular_type_parameter_default(type_parameter)? {
            let display = self.type_to_string_slice(type_parameter)?;
            self.error_at(
                default,
                &diagnostics::Type_parameter_0_has_a_circular_default,
                &[&display],
            );
        }
        let constraint_type = self.get_constraint_of_type_parameter(type_parameter)?;
        let default_type = self.get_default_from_type_parameter(type_parameter)?;
        if let (Some(constraint_type), Some(default_type)) = (constraint_type, default_type) {
            let mapper = self.make_unary_type_mapper(type_parameter, default_type);
            let instantiated = self.instantiate_type(constraint_type, Some(mapper))?;
            let target =
                self.get_type_with_this_argument(instantiated, Some(default_type), false)?;
            self.check_type_assignable_to(
                default_type,
                target,
                default,
                &diagnostics::Type_0_does_not_satisfy_the_constraint_1,
            )?;
        }
        self.check_node_deferred(node);
        if let Some(name) = name {
            self.check_type_name_is_reserved(name, &diagnostics::Type_parameter_name_cannot_be_0);
        }
        Ok(())
    }

    /// tsc-port: checkTypeParameters @6.0.3
    /// tsc-hash: 5e124ded52cde3c152843525db20639fb6ab9d1d0f840393dfff3751a44fedba
    /// tsc-span: _tsc.js:84830-84854
    ///
    /// createCheckTypeParameterDiagnostic closures run inline (eager
    /// addLazyDiagnostic identity — see check_type_parameter), which
    /// preserves the seenDefault fold order exactly.
    pub(crate) fn check_type_parameters(&mut self, declarations: &[NodeId]) -> CheckResult<()> {
        let mut seen_default = false;
        for (index, &node) in declarations.iter().enumerate() {
            // Direct checkTypeParameter call (no checkSourceElement
            // wrapper — tsc resets neither currentNode nor
            // instantiationCount here); Err containment is
            // per-parameter so one out-of-slice parameter does not
            // silence its siblings.
            let _ = self.check_type_parameter(node);
            let NodeData::TypeParameter(data) = self.data_of(node) else {
                unreachable!("type parameter lists hold type parameters");
            };
            let (name, default) = (data.name, data.r#default);
            if let Some(default) = default {
                seen_default = true;
                let _ = self.check_type_parameters_not_referenced(default, declarations, index);
            } else if seen_default {
                self.error_at(
                    Some(node),
                    &diagnostics::Required_type_parameters_may_not_follow_optional_type_parameters,
                    &[],
                );
            }
            let node_symbol = self.get_symbol_of_declaration(node).ok();
            for &previous in &declarations[..index] {
                if self.get_symbol_of_declaration(previous).ok() == node_symbol
                    && node_symbol.is_some()
                {
                    let name = name.expect("bound type parameters have names");
                    let text = tsc_binder::node_util::declaration_name_to_string(
                        self.binder.source_of_node(name),
                        Some(name),
                    );
                    self.error_at(Some(name), &diagnostics::Duplicate_identifier_0, &[&text]);
                }
            }
        }
        Ok(())
    }

    /// tsc-port: checkTypeParametersNotReferenced @6.0.3
    /// tsc-hash: fef532fdc2a78f1e9c690bf2855def4d033f52b0a8854b8e55d2ef07fe1dc6ad
    /// tsc-span: _tsc.js:84855-84871
    ///
    /// Pre-order over the default's subtree with an explicit stack
    /// (M1/M2 deep-tree rule: no recursive walkers).
    fn check_type_parameters_not_referenced(
        &mut self,
        root: NodeId,
        type_parameters: &[NodeId],
        index: usize,
    ) -> CheckResult<()> {
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if self.kind_of(node) == SyntaxKind::TypeReference {
                let ty = self.get_type_from_type_reference(node)?;
                if self
                    .tables
                    .flags_of(ty)
                    .intersects(TypeFlags::TYPE_PARAMETER)
                {
                    let symbol = self.tables.type_of(ty).symbol;
                    for &later in &type_parameters[index..] {
                        if symbol.is_some() && self.get_symbol_of_declaration(later).ok() == symbol
                        {
                            self.error_at(
                                Some(node),
                                &diagnostics::Type_parameter_defaults_can_only_reference_previously_declared_type_parameters,
                                &[],
                            );
                        }
                    }
                }
            }
            let source = self.binder.source_of_node(node);
            let mut children = Vec::new();
            for_each_child(&source.arena, source.arena.node(node), |child| {
                children.push(child);
                false
            });
            for child in children.into_iter().rev() {
                stack.push(child);
            }
        }
        Ok(())
    }

    /// tsc-port: checkTypeNameIsReserved @6.0.3
    /// tsc-hash: 6753876527b4f036c118dffe0b6006384c63e44bbd140fb488a592a44f4ab577
    /// tsc-span: _tsc.js:84771-84786
    pub(crate) fn check_type_name_is_reserved(
        &mut self,
        name: NodeId,
        message: &'static DiagnosticMessage,
    ) {
        let Some(text) = self.identifier_text_of(name) else {
            return;
        };
        match text {
            "any" | "unknown" | "never" | "number" | "bigint" | "boolean" | "string" | "symbol"
            | "void" | "object" | "undefined" => {
                let text = text.to_owned();
                self.error_at(Some(name), message, &[&text]);
            }
            _ => {}
        }
    }

    // ---- the three declaration arms that own type parameter lists ----

    /// tsc-port: checkInterfaceDeclaration @6.0.3
    /// tsc-hash: 6fe6388be7f049b58542cc3671974c0e7a0e156d49320e8180940cd69187782d
    /// tsc-span: _tsc.js:85525-85560
    ///
    /// Whole since 5.8c. addLazyDiagnostic = eager identity: both lazy
    /// blocks run inline at their queue points. The interface-extends
    /// relation reports AT node.name with the 2430 head — no
    /// member-specific elaboration (unlike classes);
    /// registerForUnusedIdentifiersCheck is inert until M7. A missing
    /// name (parse recovery) skips the name-anchored lazy block.
    fn check_interface_declaration(&mut self, node: NodeId) -> CheckResult<()> {
        // A modifier grammar error suppresses the interface grammar
        // walk (duplicate-extends family).
        if !self.check_grammar_modifiers(node) {
            self.check_grammar_interface_declaration(node);
        }
        let NodeData::InterfaceDeclaration(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let (name, type_parameters, members) = (data.name, data.type_parameters, data.members);
        if self
            .parent_of(node)
            .is_some_and(|parent| !self.allow_block_declarations(parent))
        {
            self.grammar_error_on_node(
                node,
                &diagnostics::_0_declarations_can_only_be_declared_inside_a_block,
                &["interface"],
            );
        }
        let type_parameters = self.nodes_of(type_parameters);
        self.check_type_parameters(&type_parameters)?;
        if let Some(name) = name {
            self.check_type_name_is_reserved(name, &diagnostics::Interface_name_cannot_be_0);
        }
        self.check_exports_on_merged_declarations(node)?;
        let symbol = self.get_symbol_of_declaration(node)?;
        self.check_type_parameter_lists_identical(symbol)?;
        let first_interface_declaration =
            self.get_declaration_of_kind(symbol, SyntaxKind::InterfaceDeclaration);
        if first_interface_declaration == Some(node) {
            if let Some(name) = name {
                let ty = self.get_declared_type_of_symbol_slice(symbol)?;
                let type_with_this = self.get_type_with_this_argument(ty, None, false)?;
                if self.check_inherited_properties_are_identical(ty, name)? {
                    let this_type = self.this_type_of_class_or_interface(ty);
                    for base_type in self.get_base_types(ty)? {
                        let base_with_this =
                            self.get_type_with_this_argument(base_type, this_type, false)?;
                        self.check_type_assignable_to(
                            type_with_this,
                            base_with_this,
                            Some(name),
                            &diagnostics::Interface_0_incorrectly_extends_interface_1,
                        )?;
                    }
                    self.check_index_constraints(ty, symbol, /*is_static_index*/ false)?;
                }
            }
        }
        self.check_object_type_for_duplicate_declarations(node)?;
        for heritage_element in self.interface_base_type_nodes(node) {
            let expression = match self.data_of(heritage_element) {
                NodeData::ExpressionWithTypeArguments(data) => data.expression,
                _ => None,
            };
            let expression_is_entity = expression.is_some_and(|expression| {
                let source = self.binder.source_of_node(expression);
                tsc_binder::node_util::is_entity_name_expression(source, expression)
                    && !tsc_binder::node_util::is_optional_chain(source, expression)
            });
            if !expression_is_entity {
                self.error_at(
                    expression.or(Some(heritage_element)),
                    &diagnostics::An_interface_can_only_extend_an_identifier_qualified_name_with_optional_type_arguments,
                    &[],
                );
            }
            self.check_type_reference_node(heritage_element)?;
        }
        for member in self.nodes_of(members) {
            self.check_source_element(Some(member));
        }
        self.check_type_for_duplicate_index_signatures(node)?;
        self.register_for_unused_identifiers_check(node);
        Ok(())
    }

    /// tsc-port: checkTypeAliasDeclaration @6.0.3
    /// tsc-hash: 0913cf2c0e396d42118c7452712bafc208e014da0f657f04666dd295eaaf36ff
    /// tsc-span: _tsc.js:85561-85579
    ///
    /// Whole since 5.8c: the allowBlockDeclarations grammar row and
    /// checkExportsOnMergedDeclarations join in tsc order —
    /// name-reserved BEFORE the block row (m4-58 §7);
    /// registerForUnusedIdentifiersCheck is inert until M7. The
    /// intrinsic-keyword validity arm is live (intrinsicTypeKinds
    /// membership == instantiate.rs intrinsic_type_kind).
    fn check_type_alias_declaration(&mut self, node: NodeId) -> CheckResult<()> {
        self.check_grammar_modifiers(node);
        let NodeData::TypeAliasDeclaration(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let (name, type_parameters, alias_type) = (data.name, data.type_parameters, data.r#type);
        if let Some(name) = name {
            self.check_type_name_is_reserved(name, &diagnostics::Type_alias_name_cannot_be_0);
        }
        if self
            .parent_of(node)
            .is_some_and(|parent| !self.allow_block_declarations(parent))
        {
            self.grammar_error_on_node(
                node,
                &diagnostics::_0_declarations_can_only_be_declared_inside_a_block,
                &["type"],
            );
        }
        self.check_exports_on_merged_declarations(node)?;
        let type_parameters = self.nodes_of(type_parameters);
        self.check_type_parameters(&type_parameters)?;
        let Some(alias_type) = alias_type else {
            return Ok(());
        };
        if self.kind_of(alias_type) == SyntaxKind::IntrinsicKeyword {
            let name_text = name.and_then(|name| self.identifier_text_of(name));
            let valid = if type_parameters.is_empty() {
                name_text == Some("BuiltinIteratorReturn")
            } else {
                type_parameters.len() == 1
                    && name_text
                        .is_some_and(|text| crate::instantiate::intrinsic_type_kind(text).is_some())
            };
            if !valid {
                self.error_at(
                    Some(alias_type),
                    &diagnostics::The_intrinsic_keyword_can_only_be_used_to_declare_compiler_provided_intrinsic_types,
                    &[],
                );
            }
        } else {
            self.check_source_element(Some(alias_type));
            self.register_for_unused_identifiers_check(node);
        }
        Ok(())
    }

    // checkClassDeclaration moved to class.rs at 5.8c (§6 whole).

    // ---- type reference checking ----

    /// tsc-port: checkTypeReferenceNode @6.0.3
    /// tsc-hash: 8bc58cb944b1afd5fb2b8da5ff63a54692112c617bb0cc121e3f3526555ad472
    /// tsc-span: _tsc.js:81760-81770
    ///
    /// checkGrammarTypeArguments and the JSDoc-dot probe
    /// share this owner. This arm also makes
    /// checkSourceElement(default/constraint) FORCE
    /// references BEFORE hasNonCircularTypeParameterDefault reads the
    /// default slot — the 2716-lands-on-the-second-parameter ordering
    /// depends on it (oracle-pinned). Heritage
    /// ExpressionWithTypeArguments routes here since 5.8c (§6/§7).
    ///
    /// d2: d2:a0ca43e0404dffef54d7ea308b862df5cb15a8cc5708025ff33931c2a3a9a183
    pub(crate) fn check_type_reference_node(&mut self, node: NodeId) -> CheckResult<()> {
        let (type_name, type_arguments) = match self.data_of(node) {
            NodeData::TypeReference(data) => (data.type_name, data.type_arguments),
            NodeData::ExpressionWithTypeArguments(data) => (None, data.type_arguments),
            _ => unreachable!("kind/data agree"),
        };
        self.check_grammar_type_arguments(node, type_arguments);
        let jsdoc_dot_start =
            type_name
                .zip(type_arguments)
                .and_then(|(type_name, type_arguments)| {
                    if self.is_in_js_file(node) {
                        return None;
                    }
                    let source = self.binder.source_of_node(node);
                    let type_name_end = source.arena.node(type_name).end as usize;
                    if type_name_end == source.arena.node_array(type_arguments).pos as usize {
                        return None;
                    }
                    let start = tsc_syntax::skip_trivia(source.text(), type_name_end);
                    (source.text().as_bytes().get(start) == Some(&b'.')).then(|| {
                        source
                            .positions()
                            .byte_to_utf16((start) as u32)
                            .unwrap_or(start as u32)
                    })
                });
        if let Some(start) = jsdoc_dot_start {
            self.grammar_error_at_pos(
                node,
                start,
                1,
                &diagnostics::JSDoc_types_can_only_be_used_inside_documentation_comments,
                &[],
            );
        }
        for argument in self.nodes_of(type_arguments) {
            self.check_source_element(Some(argument));
        }
        self.check_type_reference_or_import(node, type_arguments.is_some())
    }

    /// tsc-port: checkTypeReferenceOrImport @6.0.3
    /// tsc-hash: 0530fc32ad383a5bd0d271dcce464434ccc750513aa2907e511fc65df2ee907c
    /// tsc-span: _tsc.js:81771-81793
    ///
    /// Unresolved names flow as alias-bearing error types; isErrorType
    /// keeps their type-argument/deprecation tails silent after the
    /// resolver has emitted the 2304/2503 family.
    fn check_type_reference_or_import(
        &mut self,
        node: NodeId,
        has_type_arguments: bool,
    ) -> CheckResult<()> {
        if has_type_arguments
            && (self.type_reference_arguments_may_resolve_alias(node)?
                || self.is_self_referential_type_alias_reference(node)?)
        {
            // Queue the force when an argument can re-enter an alias
            // still being declared. The deferred worker runs both the
            // constraint block and this owner's deprecation tail once
            // the reference can be materialized safely.
            self.check_node_deferred(node);
            return Ok(());
        }
        let ty = self.get_type_from_type_node(node)?;
        if !self.tables.is_error_type(ty) {
            if has_type_arguments {
                self.check_node_deferred(node);
            }
            self.check_deprecated_type_reference_or_import(node);
        }
        Ok(())
    }

    fn check_deprecated_type_reference_or_import(&mut self, node: NodeId) {
        let Some(symbol) = self.links.node(node).resolved_symbol.resolved() else {
            return;
        };
        let declarations = self.binder.symbol(symbol).declarations.clone();
        if declarations.iter().copied().any(|declaration| {
            self.is_type_declaration_for_deprecated_reference(declaration)
                && self.is_deprecated_declaration(declaration)
        }) {
            let location = self.get_deprecated_suggestion_node(node);
            let name =
                tsc_binder::unescape_leading_underscores(&self.binder.symbol(symbol).escaped_name)
                    .to_owned();
            self.add_deprecated_suggestion(location, &declarations, &name);
        }
    }

    /// tsc-port: isTypeDeclaration @6.0.3
    /// tsc-hash: b2274df074ed8639268970588736dbae37b3f9f0e10f20792b347297677273e1
    /// tsc-span: _tsc.js:19262-19284
    ///
    /// Local copy for checkTypeReferenceOrImport's declaration filter.
    /// Import/export arms read the containing clause's type-only bit.
    fn is_type_declaration_for_deprecated_reference(&self, declaration: NodeId) -> bool {
        match self.kind_of(declaration) {
            SyntaxKind::TypeParameter
            | SyntaxKind::ClassDeclaration
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::EnumDeclaration
            | SyntaxKind::JSDocTypedefTag
            | SyntaxKind::JSDocCallbackTag
            | SyntaxKind::JSDocEnumTag => true,
            SyntaxKind::ImportClause => {
                matches!(
                    self.data_of(declaration),
                    NodeData::ImportClause(data) if data.is_type_only
                )
            }
            SyntaxKind::ImportSpecifier => self
                .parent_of(declaration)
                .and_then(|parent| self.parent_of(parent))
                .is_some_and(|clause| {
                    matches!(
                        self.data_of(clause),
                        NodeData::ImportClause(data) if data.is_type_only
                    )
                }),
            SyntaxKind::ExportSpecifier => self
                .parent_of(declaration)
                .and_then(|parent| self.parent_of(parent))
                .is_some_and(|export| {
                    matches!(
                        self.data_of(export),
                        NodeData::ExportDeclaration(data) if data.is_type_only
                    )
                }),
            _ => false,
        }
    }

    fn is_self_referential_type_alias_reference(&mut self, node: NodeId) -> CheckResult<bool> {
        let NodeData::TypeReference(data) = self.data_of(node) else {
            return Ok(false);
        };
        let Some(type_name) = data.type_name else {
            return Ok(false);
        };
        let Some(referenced) = self.resolve_entity_name(
            type_name,
            SymbolFlags::TYPE,
            /*ignore_errors*/ true,
            None,
        )?
        else {
            return Ok(false);
        };
        let mut nested_in_type_reference = false;
        let mut current = self.parent_of(node);
        while let Some(candidate) = current {
            if self.kind_of(candidate) == SyntaxKind::TypeReference {
                nested_in_type_reference = true;
            }
            if self.kind_of(candidate) == SyntaxKind::TypeAliasDeclaration {
                return Ok(nested_in_type_reference
                    && self
                        .node_symbol(candidate)
                        .is_some_and(|symbol| self.get_merged_symbol(symbol) == referenced));
            }
            current = self.parent_of(candidate);
        }
        Ok(false)
    }

    /// tsc-port: getTypeParametersForTypeReferenceOrImport @6.0.3
    /// (covers getTypeParametersForTypeAndSymbol in the same span)
    /// tsc-hash: cb54b2481679e0a7eb4e9530f2d7710e9bf374f4323522fcf273ab2d8d9aab8f
    /// tsc-span: _tsc.js:81703-81718
    pub(crate) fn get_type_parameters_for_type_reference_or_import(
        &mut self,
        node: NodeId,
    ) -> CheckResult<Option<Vec<TypeId>>> {
        let ty = self.get_type_from_type_node(node)?;
        if self.tables.is_error_type(ty) {
            return Ok(None);
        }
        let Some(symbol) = self.links.node(node).resolved_symbol.resolved() else {
            return Ok(None);
        };
        if self
            .binder
            .symbol(symbol)
            .flags
            .intersects(tsc_types::SymbolFlags::TYPE_ALIAS)
        {
            if let Some(type_parameters) = self.links.symbol(symbol).type_parameters.clone() {
                return Ok(Some(type_parameters));
            }
        }
        if self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::REFERENCE)
        {
            let target = self.tables.reference_target(ty);
            if let TypeData::GenericType {
                type_parameters,
                outer_type_parameter_count,
                ..
            } = &self.tables.type_of(target).data
            {
                // type.target.localTypeParameters.
                let locals = type_parameters[*outer_type_parameter_count..].to_vec();
                return Ok((!locals.is_empty()).then_some(locals));
            }
        }
        Ok(None)
    }

    /// tsc-port: checkTypeArgumentConstraints @6.0.3
    /// tsc-hash: 632dc7d6d2fcd0bcd146be70cce07a9480ff60072e3a501a884303ca4976475e
    /// tsc-span: _tsc.js:81682-81702
    ///
    /// getEffectiveTypeArguments is the annotate.rs port (5.2g).
    /// TypeReference + ImportType route here; heritage
    /// ExpressionWithTypeArguments joined at 5.8c (§6 generalization).
    pub(crate) fn check_type_argument_constraints(
        &mut self,
        node: NodeId,
        type_parameters: &[TypeId],
    ) -> CheckResult<bool> {
        let type_argument_nodes = match self.data_of(node) {
            NodeData::TypeReference(data) => data.type_arguments,
            NodeData::ImportType(data) => data.type_arguments,
            NodeData::ExpressionWithTypeArguments(data) => data.type_arguments,
            _ => unreachable!("TypeReference/ImportType/heritage route here"),
        };
        let argument_nodes = self.nodes_of(type_argument_nodes);
        let mut type_arguments: Option<Vec<TypeId>> = None;
        let mut mapper = None;
        let mut result = true;
        for (index, &type_parameter) in type_parameters.iter().enumerate() {
            let Some(constraint) = self.get_constraint_of_type_parameter(type_parameter)? else {
                continue;
            };
            // tsc's `result = result && checkTypeAssignableTo(...)`
            // deliberately short-circuits the diagnostic-producing
            // check after the first failed constraint.  Constraint
            // lookup itself still runs for every parameter.
            if !result {
                continue;
            }
            if type_arguments.is_none() {
                let filled = self.get_effective_type_arguments(node, type_parameters)?;
                mapper =
                    Some(self.create_type_mapper(type_parameters.to_vec(), Some(filled.clone())));
                type_arguments = Some(filled);
            }
            let arguments = type_arguments.as_ref().expect("filled above");
            let instantiated = self.instantiate_type(constraint, mapper)?;
            let checked = self.check_type_assignable_to(
                arguments[index],
                instantiated,
                argument_nodes.get(index).copied(),
                &diagnostics::Type_0_does_not_satisfy_the_constraint_1,
            )?;
            result = result && checked;
        }
        Ok(result)
    }

    // ---- §11 type-node arms (m4-58, L81838-82023) ----

    /// tsc-port: checkTypeQuery @6.0.3
    /// tsc-hash: a286ebe08d784672b568547713b5de7467388c5c12c4164d9ebe414bf021fb16
    /// tsc-span: _tsc.js:81838-81840
    fn check_type_query(&mut self, node: NodeId) -> CheckResult<()> {
        self.get_type_from_type_query_node(node)?;
        Ok(())
    }

    /// tsc-port: checkTypeLiteral @6.0.3
    /// tsc-hash: af0e82a9973f07ca63af60ceec2148cc5efff3b06708128338038bda9f5c6cf2
    /// tsc-span: _tsc.js:81841-81850
    ///
    /// addLazyDiagnostic = eager identity: the lazy block's forcing +
    /// index-constraint + duplicate checks run inline (class.rs seed).
    fn check_type_literal(&mut self, node: NodeId) -> CheckResult<()> {
        let NodeData::TypeLiteral(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let members = data.members;
        for member in self.nodes_of(members) {
            self.check_source_element(Some(member));
        }
        let ty = self.get_type_from_type_literal_or_fn_ctor_node(node)?;
        if let Some(symbol) = self.tables.type_of(ty).symbol {
            self.check_index_constraints(ty, symbol, /*is_static_index*/ false)?;
        }
        self.check_type_for_duplicate_index_signatures(node)?;
        self.check_object_type_for_duplicate_declarations(node)?;
        Ok(())
    }

    /// tsc-port: checkArrayType @6.0.3
    /// tsc-hash: 7c9a9b2e9c511cfdb0d095a4e1a95b6c58a25c4d9e2365ef7caed76d5478912f
    /// tsc-span: _tsc.js:81851-81853
    ///
    /// Element recursion only — SELF-FORCING ABSENT (no re-entrancy
    /// trap exposure).
    fn check_array_type(&mut self, node: NodeId) -> CheckResult<()> {
        let NodeData::ArrayType(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        self.check_source_element(data.element_type);
        Ok(())
    }

    /// tsc-port: checkTupleType @6.0.3
    /// tsc-hash: 45cdb43dde757cc99bacb74d03e41f5b36753aa7bfa6a61793135a59af7f3df9
    /// tsc-span: _tsc.js:81854-81888
    ///
    /// The self-force rides getTypeFromTypeNode's memo (re-entrancy
    /// trap §0: reads-before-writes; the write-once panic is the
    /// tripwire for default-subtree exposure).
    fn check_tuple_type(&mut self, node: NodeId) -> CheckResult<()> {
        let NodeData::TupleType(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let elements = self.nodes_of(data.elements);
        let mut seen_optional_element = false;
        let mut seen_rest_element = false;
        for &element in &elements {
            let mut flags = self.get_tuple_element_flags(element);
            if flags.intersects(tsc_types::ElementFlags::VARIADIC) {
                let inner = match self.data_of(element) {
                    NodeData::RestType(data) => data.r#type,
                    NodeData::NamedTupleMember(data) => data.r#type,
                    _ => None,
                };
                if let Some(inner) = inner {
                    let ty = self.get_type_from_type_node(inner)?;
                    if !self.is_array_like_type(ty)? {
                        self.error_at(
                            Some(element),
                            &diagnostics::A_rest_element_type_must_be_an_array_type,
                            &[],
                        );
                        break;
                    }
                    if self.is_array_type(ty)?
                        || self.tables.is_tuple_type(ty)
                            && self
                                .tuple_combined_flags(ty)
                                .intersects(tsc_types::ElementFlags::REST)
                    {
                        flags |= tsc_types::ElementFlags::REST;
                    }
                }
            }
            if flags.intersects(tsc_types::ElementFlags::REST) {
                if seen_rest_element {
                    self.grammar_error_on_node(
                        element,
                        &diagnostics::A_rest_element_cannot_follow_another_rest_element,
                        &[],
                    );
                    break;
                }
                seen_rest_element = true;
            } else if flags.intersects(tsc_types::ElementFlags::OPTIONAL) {
                if seen_rest_element {
                    self.grammar_error_on_node(
                        element,
                        &diagnostics::An_optional_element_cannot_follow_a_rest_element,
                        &[],
                    );
                    break;
                }
                seen_optional_element = true;
            } else if flags.intersects(tsc_types::ElementFlags::REQUIRED) && seen_optional_element {
                self.grammar_error_on_node(
                    element,
                    &diagnostics::A_required_element_cannot_follow_an_optional_element,
                    &[],
                );
                break;
            }
        }
        for element in elements {
            self.check_source_element(Some(element));
        }
        self.get_type_from_type_node(node)?;
        Ok(())
    }

    /// type.target.combinedFlags for tuple references.
    fn tuple_combined_flags(&self, ty: TypeId) -> tsc_types::ElementFlags {
        let target = if self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::REFERENCE)
        {
            self.tables.reference_target(ty)
        } else {
            ty
        };
        match &self.tables.type_of(target).data {
            TypeData::TupleTarget(data) => data
                .element_flags
                .iter()
                .fold(tsc_types::ElementFlags::from_bits(0), |acc, &flags| {
                    acc | flags
                }),
            _ => tsc_types::ElementFlags::from_bits(0),
        }
    }

    /// tsc-port: checkUnionOrIntersectionType @6.0.3
    /// tsc-hash: fb99110bb4ec225868bfc2a8215247de48be9c3b4c2e50d4283b5bafc74da82b
    /// tsc-span: _tsc.js:81889-81892
    fn check_union_or_intersection_type(&mut self, node: NodeId) -> CheckResult<()> {
        let types = match self.data_of(node) {
            NodeData::UnionType(data) => data.types,
            NodeData::IntersectionType(data) => data.types,
            _ => unreachable!("kind/data agree"),
        };
        for member in self.nodes_of(types) {
            self.check_source_element(Some(member));
        }
        self.get_type_from_type_node(node)?;
        Ok(())
    }

    /// tsc-port: checkIndexedAccessType @6.0.3
    /// tsc-hash: b9f47c8db7e5d08720094c3f6903c6876193cec060eb761bb3c17332f4834241
    /// tsc-span: _tsc.js:81919-81923
    ///
    /// The CHECK-side of the 5.2g resolver rows: tsc's resolver
    /// reports through the same helper on access EXPRESSIONS, the
    /// type-node path reports HERE (pinned against double-reports).
    fn check_indexed_access_type(&mut self, node: NodeId) -> CheckResult<()> {
        let NodeData::IndexedAccessType(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let (object_type, index_type) = (data.object_type, data.index_type);
        self.check_source_element(object_type);
        self.check_source_element(index_type);
        let resolved = self.get_type_from_indexed_access_type_node(node)?;
        self.check_indexed_access_index_type(resolved, node)?;
        Ok(())
    }

    /// tsc-port: checkMappedType @6.0.3
    /// tsc-hash: 12a5060787f6d1849d7f77ba2d3beb1f786fb8263e2fcd929c49d5c9673375e4
    /// tsc-span: _tsc.js:81924-81940
    fn check_mapped_type(&mut self, node: NodeId) -> CheckResult<()> {
        self.check_grammar_mapped_type(node);
        let NodeData::MappedType(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let (type_parameter, name_type, mapped_type) =
            (data.type_parameter, data.name_type, data.r#type);
        self.check_source_element(type_parameter);
        self.check_source_element(name_type);
        self.check_source_element(mapped_type);
        if mapped_type.is_none() {
            let any = self.tables.intrinsics.any;
            self.report_implicit_any(node, any, None)?;
        }
        let ty = self.get_type_from_type_node(node)?;
        let string_number_symbol = self.tables.intrinsics.string_number_symbol;
        if let Some(resolved_name_type) = self.get_name_type_from_mapped_type(ty)? {
            self.check_type_assignable_to(
                resolved_name_type,
                string_number_symbol,
                name_type,
                &diagnostics::Type_0_is_not_assignable_to_type_1,
            )?;
        } else {
            let constraint = self.get_constraint_type_from_mapped_type(ty)?;
            let constraint_node = type_parameter.and_then(|parameter| {
                let NodeData::TypeParameter(data) = self.data_of(parameter) else {
                    return None;
                };
                data.constraint
            });
            self.check_type_assignable_to(
                constraint,
                string_number_symbol,
                constraint_node,
                &diagnostics::Type_0_is_not_assignable_to_type_1,
            )?;
        }
        Ok(())
    }

    /// tsc-port: checkGrammarMappedType @6.0.3
    /// tsc-hash: 802be8a8f24d762dd0798504e86d1e35b61dd97e4cf8c63aa19481b345d72d5c
    /// tsc-span: _tsc.js:81941-81946
    fn check_grammar_mapped_type(&mut self, node: NodeId) -> bool {
        let NodeData::MappedType(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let members = self.nodes_of(data.members);
        if let Some(&first) = members.first() {
            return self.grammar_error_on_node(
                first,
                &diagnostics::A_mapped_type_may_not_declare_properties_or_methods,
                &[],
            );
        }
        false
    }

    /// tsc-port: checkThisType @6.0.3
    /// tsc-hash: 020890db1cf60fb0cc561e6645d70cb91378192c0c86dab624ba13f87ab93ffc
    /// tsc-span: _tsc.js:81947-81949
    fn check_this_type(&mut self, node: NodeId) -> CheckResult<()> {
        self.get_type_from_this_type_node(node)?;
        Ok(())
    }

    /// tsc-port: checkTypeOperator @6.0.3
    /// tsc-hash: 887ed97e8defb9d4edfae94a11eec1b2fd95836cc3f6a620fc0ed3ff07edc620
    /// tsc-span: _tsc.js:81950-81953
    fn check_type_operator(&mut self, node: NodeId) -> CheckResult<()> {
        self.check_grammar_type_operator_node(node);
        let NodeData::TypeOperator(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        self.check_source_element(data.r#type);
        Ok(())
    }

    /// tsc-port: checkConditionalType @6.0.3
    /// tsc-hash: 8b19e799fa6c783fd212472aae2cac4d26d0969e145ab225d79ef608e80dd573
    /// tsc-span: _tsc.js:81954-81956
    ///
    /// forEachChild recursion ONLY — the conditional-model boundary
    /// stays on the annotate side, with no self-force here.
    fn check_conditional_type(&mut self, node: NodeId) -> CheckResult<()> {
        let source = self.binder.source_of_node(node);
        let mut children = Vec::new();
        for_each_child(&source.arena, source.arena.node(node), |child| {
            children.push(child);
            false
        });
        for child in children {
            self.check_source_element(Some(child));
        }
        Ok(())
    }

    /// tsc-port: checkInferType @6.0.3
    /// tsc-hash: ed384c17a08679e21b2aebb3031c7d2c4116124e7ab40de146483d42d9a4209e
    /// tsc-span: _tsc.js:81957-81978
    ///
    /// Whole since 5.8c: the multi-declaration constraint-identity
    /// walk consumes the §6 areTypeParametersIdentical kit
    /// (getTypeParameterDeclarations = decl => [decl], 81969);
    /// registerForUnusedIdentifiersCheck is inert until M7.
    fn check_infer_type(&mut self, node: NodeId) -> CheckResult<()> {
        let mut in_extends_clause = false;
        let mut current = Some(node);
        while let Some(candidate) = current {
            let parent = self.parent_of(candidate);
            if let Some(parent) = parent {
                if self.kind_of(parent) == SyntaxKind::ConditionalType {
                    let extends = match self.data_of(parent) {
                        NodeData::ConditionalType(data) => data.extends_type,
                        _ => None,
                    };
                    if extends == Some(candidate) {
                        in_extends_clause = true;
                        break;
                    }
                }
            }
            current = parent;
        }
        if !in_extends_clause {
            self.grammar_error_on_node(
                node,
                &diagnostics::infer_declarations_are_only_permitted_in_the_extends_clause_of_a_conditional_type,
                &[],
            );
        }
        let NodeData::InferType(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let type_parameter = data.type_parameter;
        self.check_source_element(type_parameter);
        if let Some(type_parameter) = type_parameter {
            let symbol = self.get_symbol_of_declaration(type_parameter)?;
            if self.binder.symbol(symbol).declarations.len() > 1
                && !self.links.symbol(symbol).type_parameters_checked
            {
                self.links
                    .set_symbol_type_parameters_checked(self.speculation_depth, symbol);
                let declared = self.get_declared_type_of_type_parameter(symbol);
                let declarations: Vec<NodeId> = self
                    .binder
                    .symbol(symbol)
                    .declarations
                    .iter()
                    .copied()
                    .filter(|&declaration| self.kind_of(declaration) == SyntaxKind::TypeParameter)
                    .collect();
                if !self.are_type_parameters_identical(&declarations, &[declared])? {
                    let name = self.symbol_display_name(symbol);
                    for declaration in declarations {
                        let declaration_name = self.name_of_node(declaration);
                        self.error_at(
                            declaration_name,
                            &diagnostics::All_declarations_of_0_must_have_identical_constraints,
                            &[&name],
                        );
                    }
                }
            }
        }
        self.register_for_unused_identifiers_check(node);
        Ok(())
    }

    /// tsc-port: checkTemplateLiteralType @6.0.3
    /// tsc-hash: 584dbe841ce2a956ded87bd9c7646da0232693367645061bf3ff5a6989d1b196
    /// tsc-span: _tsc.js:81979-81986
    fn check_template_literal_type(&mut self, node: NodeId) -> CheckResult<()> {
        let NodeData::TemplateLiteralType(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let spans = self.nodes_of(data.template_spans);
        for span in spans {
            let span_type = match self.data_of(span) {
                NodeData::TemplateLiteralTypeSpan(data) => data.r#type,
                _ => None,
            };
            self.check_source_element(span_type);
            if let Some(span_type) = span_type {
                let ty = self.get_type_from_type_node(span_type)?;
                let constraint = self.tables.intrinsics.template_constraint;
                self.check_type_assignable_to(
                    ty,
                    constraint,
                    Some(span_type),
                    &diagnostics::Type_0_is_not_assignable_to_type_1,
                )?;
            }
        }
        self.get_type_from_type_node(node)?;
        Ok(())
    }

    /// tsc-port: checkImportType @6.0.3
    /// tsc-hash: e300b9504ef6915d0ee8b66eee8c536bf348750ed9a5a320144d96aac474ff56
    /// tsc-span: _tsc.js:81987-81996
    ///
    /// The `assert`-deprecation row is live unless
    /// `ignoreDeprecations` is exactly `"6.0"`; the with/assert discriminator is read from
    /// ImportAttributes.token — the parser threads the consumed
    /// keyword into the node data (codegen seed). The
    /// getResolutionModeOverride grammar validation is a named escape
    /// (5.8d §9 — resolution-mode plumbing).
    fn check_import_type(&mut self, node: NodeId) -> CheckResult<()> {
        let NodeData::ImportType(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let (argument, attributes) = (data.argument, data.attributes);
        self.check_source_element(argument);
        if let Some(attributes) = attributes {
            // node.attributes.token: the parser threads the consumed
            // with/assert keyword into ImportAttributesData (the
            // source form is unrecoverable after the parse — review
            // find, PR #5).
            let token = match self.data_of(attributes) {
                NodeData::ImportAttributes(data) => data.token,
                _ => SyntaxKind::WithKeyword,
            };
            if token != SyntaxKind::WithKeyword
                && self.options.ignore_deprecations.as_deref() != Some("6.0")
            {
                self.grammar_error_on_first_token(
                    attributes,
                    &diagnostics::Import_assertions_have_been_replaced_by_import_attributes_Use_with_instead_of_assert,
                    &[],
                );
            }
            // getResolutionModeOverride (5.8d): import-type nodes are
            // TYPE context, so the resolution-mode grammar rows report
            // unconditionally (tsc checkImportType passes
            // grammarErrorOnNode straight through).
            self.get_resolution_mode_override(attributes, true)?;
        }
        self.check_type_reference_or_import(node, {
            let NodeData::ImportType(data) = self.data_of(node) else {
                unreachable!("kind/data agree");
            };
            data.type_arguments.is_some()
        })
    }

    /// tsc-port: checkNamedTupleMember @6.0.3
    /// tsc-hash: d4d925e652a06dede81d11ea41937e9285024be36e62e01e7c02ae8cf38acda8
    /// tsc-span: _tsc.js:81997-82009
    fn check_named_tuple_member(&mut self, node: NodeId) -> CheckResult<()> {
        let NodeData::NamedTupleMember(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let (dot_dot_dot, question, member_type) =
            (data.dot_dot_dot_token, data.question_token, data.r#type);
        if dot_dot_dot.is_some() && question.is_some() {
            self.grammar_error_on_node(
                node,
                &diagnostics::A_tuple_member_cannot_be_both_optional_and_rest,
                &[],
            );
        }
        if let Some(member_type) = member_type {
            match self.kind_of(member_type) {
                SyntaxKind::OptionalType => {
                    self.grammar_error_on_node(
                        member_type,
                        &diagnostics::A_labeled_tuple_element_is_declared_as_optional_with_a_question_mark_after_the_name_and_before_the_colon_rather_than_after_the_type,
                        &[],
                    );
                }
                SyntaxKind::RestType => {
                    self.grammar_error_on_node(
                        member_type,
                        &diagnostics::A_labeled_tuple_element_is_declared_as_rest_with_a_before_the_name_rather_than_before_the_type,
                        &[],
                    );
                }
                _ => {}
            }
        }
        self.check_source_element(member_type);
        self.get_type_from_type_node(node)?;
        Ok(())
    }

    /// tsc-port: checkGrammarTypeOperatorNode @6.0.3
    /// tsc-hash: 1d1ac27cc886851d1df8f00399ac752d935cdc56b0eda59d59fd918de563d38f
    /// tsc-span: _tsc.js:89894-89937
    fn check_grammar_type_operator_node(&mut self, node: NodeId) -> bool {
        let NodeData::TypeOperator(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let (operator, operand) = (data.operator, data.r#type);
        if operator == SyntaxKind::UniqueKeyword {
            let Some(operand) = operand else {
                return false;
            };
            if self.kind_of(operand) != SyntaxKind::SymbolKeyword {
                return self.grammar_error_on_node(operand, &diagnostics::_0_expected, &["symbol"]);
            }
            // walkUpParenthesizedTypes.
            let mut parent = self.parent_of(node);
            while let Some(candidate) = parent {
                if self.kind_of(candidate) != SyntaxKind::ParenthesizedType {
                    break;
                }
                parent = self.parent_of(candidate);
            }
            if parent.is_some_and(|parent| {
                self.is_in_js_file(parent)
                    && self.kind_of(parent) == SyntaxKind::JSDocTypeExpression
            }) {
                parent = parent
                    .and_then(|parent| self.get_jsdoc_host(parent))
                    .map(|host| {
                        self.single_variable_of_variable_statement(host)
                            .unwrap_or(host)
                    });
            }
            let Some(parent) = parent else {
                return false;
            };
            match self.kind_of(parent) {
                SyntaxKind::VariableDeclaration => {
                    let name = self.name_of_node(parent);
                    let Some(name) = name else { return false };
                    if self.kind_of(name) != SyntaxKind::Identifier {
                        return self.grammar_error_on_node(
                            node,
                            &diagnostics::unique_symbol_types_may_not_be_used_on_a_variable_declaration_with_a_binding_name,
                            &[],
                        );
                    }
                    let list = self.parent_of(parent);
                    let in_variable_statement = list.is_some_and(|list| {
                        self.kind_of(list) == SyntaxKind::VariableDeclarationList
                            && self.parent_of(list).is_some_and(|statement| {
                                self.kind_of(statement) == SyntaxKind::VariableStatement
                            })
                    });
                    if !in_variable_statement {
                        return self.grammar_error_on_node(
                            node,
                            &diagnostics::unique_symbol_types_are_only_allowed_on_variables_in_a_variable_statement,
                            &[],
                        );
                    }
                    let list_is_const = list.is_some_and(|list| {
                        self.node_flags(list) & tsc_types::NodeFlags::CONST.bits() != 0
                    });
                    if !list_is_const {
                        return self.grammar_error_on_node(
                            name,
                            &diagnostics::A_variable_whose_type_is_a_unique_symbol_type_must_be_const,
                            &[],
                        );
                    }
                }
                SyntaxKind::PropertyDeclaration => {
                    let source = self.binder.source_of_node(parent);
                    let is_static = tsc_binder::node_util::has_syntactic_modifier(
                        source,
                        parent,
                        tsc_types::ModifierFlags::STATIC,
                    );
                    let is_readonly =
                        tsc_binder::node_util::get_effective_modifier_flags(source, parent)
                            .intersects(tsc_types::ModifierFlags::READONLY);
                    if !is_static || !is_readonly {
                        let name = self.name_of_node(parent);
                        return self.grammar_error_on_node(
                            name.unwrap_or(parent),
                            &diagnostics::A_property_of_a_class_whose_type_is_a_unique_symbol_type_must_be_both_static_and_readonly,
                            &[],
                        );
                    }
                }
                SyntaxKind::PropertySignature => {
                    let source = self.binder.source_of_node(parent);
                    let is_readonly = tsc_binder::node_util::has_syntactic_modifier(
                        source,
                        parent,
                        tsc_types::ModifierFlags::READONLY,
                    );
                    if !is_readonly {
                        let name = self.name_of_node(parent);
                        return self.grammar_error_on_node(
                            name.unwrap_or(parent),
                            &diagnostics::A_property_of_an_interface_or_type_literal_whose_type_is_a_unique_symbol_type_must_be_readonly,
                            &[],
                        );
                    }
                }
                _ => {
                    return self.grammar_error_on_node(
                        node,
                        &diagnostics::unique_symbol_types_are_not_allowed_here,
                        &[],
                    );
                }
            }
        } else if operator == SyntaxKind::ReadonlyKeyword {
            if let Some(operand) = operand {
                if !matches!(
                    self.kind_of(operand),
                    SyntaxKind::ArrayType | SyntaxKind::TupleType
                ) {
                    return self.grammar_error_on_first_token(
                        node,
                        &diagnostics::readonly_type_modifier_is_only_permitted_on_array_and_tuple_literal_types,
                        &["symbol"],
                    );
                }
            }
        }
        false
    }

    // ---- deferred nodes ----

    /// tsc-port: checkNodeDeferred @6.0.3
    /// tsc-hash: fe303c77e683b6c4f22764158c193cce31042f720adb610369ce753c037ff01c
    /// tsc-span: _tsc.js:86899-86968
    pub(crate) fn check_node_deferred(&mut self, node: NodeId) {
        let file_root = self.binder.source_of_node(node).root;
        if !self
            .links
            .node(file_root)
            .check_flags
            .intersects(NodeCheckFlags::TYPE_CHECKED)
        {
            self.deferred_nodes
                .entry(file_root)
                .or_default()
                .insert(node);
        } else {
            debug_assert!(
                !self.deferred_nodes.contains_key(&file_root),
                "A type-checked file should have no deferred nodes."
            );
        }
    }

    /// checkDeferredNodes (86909): index iteration reproduces the JS
    /// Set's visit-inserts-during-forEach semantics — a node deferred
    /// DURING the drain is drained too.
    fn check_deferred_nodes(&mut self, root: NodeId) {
        let mut index = 0;
        loop {
            let next = self
                .deferred_nodes
                .get(&root)
                .and_then(|set| set.get_index(index).copied());
            let Some(node) = next else { break };
            self.check_deferred_node(node);
            index += 1;
        }
        self.deferred_nodes.remove(&root);
    }

    /// tsrs-native (7.4b, precision reworked by the 7.4 review): the
    /// three-signal containment test for a deferred FUNCTION-kind
    /// node — see check_deferred_node's comment for the rationale.
    /// The slot-bearing node for a call-like ancestor is usually the
    /// ancestor itself, but JSX CHILDREN hang off JsxElement /
    /// JsxFragment whose resolvedSignature lives on the OPENING node
    /// — a sibling subtree, never an ancestor of the children — so
    /// those kinds resolve through opening_element/opening_fragment
    /// (the pre-review walk listed JsxOpeningFragment directly, which
    /// is a leaf and therefore dead as an ancestor). instanceof
    /// resolutions stash on the BinaryExpression itself
    /// (operators.rs).
    fn deferred_context_call_reverted(&self, node: NodeId) -> bool {
        let is_function_kind = matches!(
            self.kind_of(node),
            SyntaxKind::FunctionExpression
                | SyntaxKind::ArrowFunction
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::MethodSignature
        );
        if !is_function_kind {
            return false;
        }
        let file_index = self.binder.file_index_of_node(node);
        let (pos, end) = {
            let raw = self.binder.source_of_node(node).arena.node(node);
            (raw.pos, raw.end)
        };
        let inside_contained =
            self.partially_checked_ranges
                .get(&file_index)
                .is_some_and(|ranges| {
                    ranges
                        .iter()
                        .any(|&(range_pos, range_end)| range_pos <= pos && end <= range_end)
                });
        if !inside_contained {
            return false;
        }
        let mut current = node;
        while let Some(parent) = self.parent_of(current) {
            let slot_node = match self.kind_of(parent) {
                SyntaxKind::CallExpression
                | SyntaxKind::NewExpression
                | SyntaxKind::TaggedTemplateExpression
                | SyntaxKind::Decorator
                | SyntaxKind::JsxOpeningElement
                | SyntaxKind::JsxSelfClosingElement
                | SyntaxKind::BinaryExpression => Some(parent),
                SyntaxKind::JsxElement => match self.data_of(parent) {
                    NodeData::JsxElement(data) => data.opening_element,
                    _ => None,
                },
                SyntaxKind::JsxFragment => match self.data_of(parent) {
                    NodeData::JsxFragment(data) => data.opening_fragment,
                    _ => None,
                },
                _ => None,
            };
            if let Some(slot_node) = slot_node {
                if matches!(
                    self.links.node(slot_node).resolved_signature,
                    crate::links::LinkSlot::Vacant
                ) && self.contained_call_resolutions.contains(&slot_node)
                {
                    return true;
                }
            }
            current = parent;
        }
        false
    }

    /// checkDeferredNode (86916), tracing elided. Every arm except
    /// TypeParameter is unreachable TODAY: the only checkNodeDeferred
    /// call site is checkTypeParameter (grep check_node_deferred) —
    /// the expression/call registrations arrive with 5.5/5.7, whose
    /// stages replace the unreachable!()s with their workers.
    fn check_deferred_node(&mut self, node: NodeId) {
        // tsrs-native (7.4b): a deferred node whose CONTEXT hangs off
        // a CONTAINED resolution cannot be checked faithfully (tsc,
        // with no failure channel, resolves those fully) — checking it
        // contextless FABRICATES implicit-any/unknown rows tsc never
        // emits (the intraExpressionInferencesJsx 7006/18046 FP face,
        // reachable once 7.4 registers trial-checked functions). The
        // test is ALL THREE signals (deferred_context_call_reverted):
        // the node sits inside an already-contained range, some
        // call-like ancestor's resolvedSignature slot is Vacant, AND
        // that Vacant was left by a containment unwind (the
        // contained_call_resolutions record) — a call that was
        // ATTEMPTED (it visited this argument) but whose sentinel the
        // containment reverted. Range-inclusion alone is too broad
        // (the first cut regressed 164 accepted identities whose
        // containment was unrelated to their context — the set-ratchet
        // caught it live); a Resolved slot (success or failure-face
        // stash) feeds contextual reads exactly like tsc, so those
        // still check; a Vacant WITHOUT the containment record is the
        // benign mid-fixpoint clear (tsc 77505 `: cached` on a
        // loop-dirty fresh frame) — fully re-resolvable, so its
        // deferred functions still check too (7.4 review fix).
        // Scope: FUNCTION kinds only — the fabrication class is
        // contextless PARAMETER typing (7006/7044/18046). Other
        // deferred kinds (assertions, calls) carry their own operands
        // and still check — the first kind-blind cut regressed a
        // deferred assertion's 2352 (subtypingWithCallSignatures3).
        if self.deferred_context_call_reverted(node) {
            self.mark_partially_checked_node(
                node,
                "deferred check under a contained call resolution (context unavailable)",
            );
            return;
        }
        let save_current_node = self.current_node;
        self.current_node = Some(node);
        self.instantiation_count = 0;
        #[cfg(debug_assertions)]
        let unwind_entry = self.unwind_snapshot();
        if let Err(err) = self.check_deferred_node_worker(node) {
            // A contained deferred check leaves this node's range
            // unverified. Keep only the comment-directive range so
            // 2578 does not report a directive whose suppression
            // target was never checked.
            self.mark_oracle_crash_range(node, err);
            if std::env::var_os("TSRS_TRACE_CONTAIN").is_some() {
                eprintln!("contained deferred @{node:?}: {err}");
            }
        }
        #[cfg(debug_assertions)]
        self.assert_unwound(&unwind_entry, node, "check_deferred_node");
        self.current_node = save_current_node;
    }

    fn check_deferred_node_worker(&mut self, node: NodeId) -> CheckResult<()> {
        match self.kind_of(node) {
            SyntaxKind::CallExpression | SyntaxKind::NewExpression => {
                // checkDeferredNode 86923-86928: the overload-failure
                // deferral re-checks the RAW arguments; contextual
                // reads see the stashed failure candidate (5.7a).
                self.resolve_untyped_call(node)?;
                Ok(())
            }
            SyntaxKind::TaggedTemplateExpression => {
                // checkDeferredNode 86923-86928: overload-failure
                // deferrals re-check the raw operands (template + type
                // arguments) against the stashed failure candidate.
                self.resolve_untyped_call(node)?;
                Ok(())
            }
            SyntaxKind::Decorator => {
                // 86923-86928: overload-failure deferrals re-check the
                // raw operands like calls.
                self.resolve_untyped_call(node)?;
                Ok(())
            }
            SyntaxKind::JsxOpeningElement => {
                // 86923-86928: an overload-failure deferral over a JSX
                // opening element re-checks the raw attributes operand
                // against the stashed failure candidate, like calls.
                self.resolve_untyped_call(node)?;
                Ok(())
            }
            SyntaxKind::FunctionExpression
            | SyntaxKind::ArrowFunction
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::MethodSignature => {
                self.check_function_expression_or_object_literal_method_deferred(node)
            }
            SyntaxKind::GetAccessor | SyntaxKind::SetAccessor => {
                self.check_accessor_declaration(node)
            }
            SyntaxKind::ClassExpression => self.check_class_expression_deferred(node),
            SyntaxKind::TypeParameter => self.check_type_parameter_deferred(node),
            SyntaxKind::TypeReference
            | SyntaxKind::ImportType
            | SyntaxKind::ExpressionWithTypeArguments => {
                let needs_deprecation_tail =
                    self.links.node(node).resolved_type.resolved().is_none();
                if let Some(type_parameters) =
                    self.get_type_parameters_for_type_reference_or_import(node)?
                {
                    self.check_type_argument_constraints(node, &type_parameters)?;
                }
                if needs_deprecation_tail {
                    let ty = self.get_type_from_type_node(node)?;
                    if !self.tables.is_error_type(ty) {
                        self.check_deprecated_type_reference_or_import(node);
                    }
                }
                Ok(())
            }
            SyntaxKind::JsxSelfClosingElement => self.check_jsx_self_closing_element_deferred(node),
            SyntaxKind::JsxElement => self.check_jsx_element_deferred(node),
            SyntaxKind::TypeAssertionExpression
            | SyntaxKind::AsExpression
            | SyntaxKind::ParenthesizedExpression => self.check_assertion_deferred(node),
            SyntaxKind::VoidExpression => {
                // checkDeferredNode's void arm (86957): checkExpression
                // of the operand — registration is live from 5.5a
                // (checkVoidExpression).
                let NodeData::VoidExpression(data) = self.data_of(node) else {
                    unreachable!("kind/data agree");
                };
                let Some(expression) = data.expression else {
                    return Ok(());
                };
                self.check_expression(expression, tsc_types::CheckMode::NORMAL)?;
                Ok(())
            }
            SyntaxKind::BinaryExpression => {
                // 86960-86964: only instanceof binaries register
                // deferrals (overload failure on [Symbol.hasInstance]).
                let is_instanceof = matches!(self.data_of(node), NodeData::BinaryExpression(data)
                if data.operator_token.is_some_and(|t| {
                    self.kind_of(t) == SyntaxKind::InstanceOfKeyword
                }));
                if is_instanceof {
                    self.resolve_untyped_call(node)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// tsc-port: checkTypeParameterDeferred @6.0.3
    /// tsc-hash: 1c07b9d8ea60523fff8b158a9833d515d943394736c9dfc43f117f6f8090cd65
    /// tsc-span: _tsc.js:81148-81170
    fn check_type_parameter_deferred(&mut self, node: NodeId) -> CheckResult<()> {
        let Some(parent) = self.parent_of(node) else {
            return Ok(());
        };
        let parent_kind = self.kind_of(parent);
        let is_alias_parent = parent_kind == SyntaxKind::TypeAliasDeclaration;
        if !(parent_kind == SyntaxKind::InterfaceDeclaration
            || parent_kind == SyntaxKind::ClassDeclaration
            || parent_kind == SyntaxKind::ClassExpression
            || is_alias_parent)
        {
            return Ok(());
        }
        let symbol = self.get_symbol_of_declaration(node)?;
        let type_parameter = self.get_declared_type_of_type_parameter(symbol);
        let modifiers = ModifierFlags::from_bits(
            self.get_type_parameter_modifiers(type_parameter).bits()
                & (ModifierFlags::IN.bits() | ModifierFlags::OUT.bits()),
        );
        if modifiers == ModifierFlags::NONE {
            return Ok(());
        }
        let parent_symbol = self.get_symbol_of_declaration(parent)?;
        let parent_declared = self.get_declared_type_of_symbol_for_variance(parent_symbol)?;
        if is_alias_parent
            && !self
                .tables
                .object_flags_of(parent_declared)
                .intersects(ObjectFlags::ANONYMOUS | ObjectFlags::MAPPED)
        {
            self.error_at(
                Some(node),
                &diagnostics::Variance_annotations_are_only_supported_in_type_aliases_for_object_function_constructor_and_mapped_types,
                &[],
            );
        } else if modifiers == ModifierFlags::IN || modifiers == ModifierFlags::OUT {
            let out = modifiers == ModifierFlags::OUT;
            let (source_marker, target_marker) = if out {
                (
                    self.marker_sub_type_for_check,
                    self.marker_super_type_for_check,
                )
            } else {
                (
                    self.marker_super_type_for_check,
                    self.marker_sub_type_for_check,
                )
            };
            let source = self.create_marker_type(parent_symbol, type_parameter, source_marker)?;
            let target = self.create_marker_type(parent_symbol, type_parameter, target_marker)?;
            let save_variance_type_parameter = self.variance_type_parameter;
            self.variance_type_parameter = Some(type_parameter);
            let result = self.check_type_assignable_to(
                source,
                target,
                Some(node),
                &diagnostics::Type_0_is_not_assignable_to_type_1_as_implied_by_variance_annotation,
            );
            self.variance_type_parameter = save_variance_type_parameter;
            result?;
        }
        Ok(())
    }

    // ---- relation reporting (the 5.4 slice) ----

    /// tsc-port: isRelatedTo @6.0.3 (the nullable-candidate substitution)
    /// tsc-hash: e700526d3ad4ff20b24e5f5218b2fe969a0745358f190ce915314e8fbe2eac9f
    /// tsc-span: _tsc.js:65185-65196
    ///
    /// A DefinitelyNonNullable source against a 2-member
    /// [nullable, X] or 3-member [nullable, nullable, X] union
    /// substitutes X for the WHOLE relation level — which is why
    /// `let v: string | undefined = 1` reports `number ↛ string`
    /// while two-real-member unions keep the union face
    /// (oracle-probed U1-U5). Verdicts are unchanged (a definitely
    /// non-nullable source relates to the union iff it relates to
    /// X); the port applies the substitution at its report entries,
    /// where tsc's in-engine reportRelationError sees the
    /// substituted pair. Nullable members sort first in union lists,
    /// matching tsc's positional probe.
    fn nullable_stripped_report_target(
        &mut self,
        source: TypeId,
        target: TypeId,
    ) -> CheckResult<TypeId> {
        if !self
            .tables
            .flags_of(source)
            .intersects(TypeFlags::DEFINITELY_NON_NULLABLE)
            || !self.tables.flags_of(target).intersects(TypeFlags::UNION)
        {
            return Ok(target);
        }
        let types = match &self.tables.type_of(target).data {
            TypeData::Union { types, .. } => types.to_vec(),
            _ => return Ok(target),
        };
        let nullable =
            |state: &Self, t: TypeId| state.tables.flags_of(t).intersects(TypeFlags::NULLABLE);
        let candidate = if types.len() == 2 && nullable(self, types[0]) {
            Some(types[1])
        } else if types.len() == 3 && nullable(self, types[0]) && nullable(self, types[1]) {
            Some(types[2])
        } else {
            None
        };
        match candidate {
            Some(candidate) if !nullable(self, candidate) => {
                self.get_normalized_type(candidate, /*writing*/ true)
            }
            _ => Ok(target),
        }
    }

    /// tsc-port: isRelatedTo @6.0.3
    /// tsc-hash: 347cb4f05f51cf2f84cf35b3506eecf7649742674a98e720ef96514cae0d718a
    /// tsc-span: _tsc.js:65147-65168
    /// tsc-port: reportErrorResults @6.0.3
    /// tsc-hash: 3eddd5747113ff18b7f684d7f85fe61a1b833cae13a3f11bbaa8219d0838a31e
    /// tsc-span: _tsc.js:65248-65253
    ///
    /// isRelatedTo's reporting closure receives the read-normalized
    /// source and write-normalized target, not the original verdict
    /// pair. The Rust verdict/report split must reconstruct that pair
    /// for every direct reporting entry: this covers fresh literals,
    /// NoInfer substitutions, and simplifiable indexed accesses such
    /// as Partial<T>[K] and Readonly<T>[K].
    pub(crate) fn normalized_relation_report_types(
        &mut self,
        original_source: TypeId,
        original_target: TypeId,
    ) -> CheckResult<(TypeId, TypeId)> {
        let mut source = self.get_normalized_type(original_source, /*writing*/ false)?;
        let target = self.get_normalized_type(original_target, /*writing*/ true)?;
        let mut target = self.nullable_stripped_report_target(source, target)?;
        // reportErrorResults 65248-65253 restores the original display
        // face after the relation-level normalization when an alias
        // symbol or a non-augmenting base is present. Without this
        // step named conditional/mapped aliases expand in the head.
        let source_has_base = self
            .get_single_base_for_non_augmenting_subtype(original_source)?
            .is_some();
        let target_has_base = self
            .get_single_base_for_non_augmenting_subtype(original_target)?
            .is_some();
        if self.tables.type_of(original_source).alias_symbol.is_some() || source_has_base {
            source = original_source;
        }
        if self.tables.type_of(original_target).alias_symbol.is_some() || target_has_base {
            target = original_target;
        }
        Ok((source, target))
    }

    /// tsc-port: checkTypeAssignableTo @6.0.3
    /// tsc-hash: c54f432c89f2f52677994a63f73b2d9e30dadfe890712c62749b4aab33e7f833
    /// tsc-span: _tsc.js:63931-63933
    ///
    /// With an error node this enters one reporting relation frame,
    /// exactly like tsc's `checkTypeRelatedTo`. A preliminary boolean
    /// probe is not equivalent: its failed cache entries can be read
    /// by nested non-reporting overload candidates during the replay
    /// and turn recursive `Maybe` results into hard failures. The
    /// no-error-node face remains the ordinary boolean query.
    pub(crate) fn check_type_assignable_to(
        &mut self,
        source: TypeId,
        target: TypeId,
        error_node: Option<NodeId>,
        head_message: &'static DiagnosticMessage,
    ) -> CheckResult<bool> {
        let (related, diagnostic) =
            self.check_type_assignable_to_worker(source, target, error_node, head_message)?;
        if let Some(diagnostic) = diagnostic {
            self.push_error_diagnostic(diagnostic);
        }
        Ok(related)
    }

    /// tsrs-native: return tsc's errorOutputContainer row as an owned
    /// diagnostic instead of publishing it through the checker sink.
    ///
    /// Run one reporting relation with an explicit diagnostic result.
    ///
    /// This is the Rust-owned equivalent of tsc's
    /// `errorOutputContainer`: the relation diagnostic is returned as
    /// data even when an identical row already exists in the program
    /// sink. Lazy global diagnostics still use the ordinary checker
    /// sink; only the relation-owned row crosses this boundary.
    pub(crate) fn capture_type_assignable_to_diagnostic(
        &mut self,
        source: TypeId,
        target: TypeId,
        error_node: NodeId,
        head_message: &'static DiagnosticMessage,
    ) -> CheckResult<(bool, Option<Diagnostic>)> {
        self.check_type_assignable_to_worker(source, target, Some(error_node), head_message)
    }

    /// `(related, diagnostic)` worker shared by the ordinary program
    /// sink and the applicability output container above.
    fn check_type_assignable_to_worker(
        &mut self,
        source: TypeId,
        target: TypeId,
        error_node: Option<NodeId>,
        head_message: &'static DiagnosticMessage,
    ) -> CheckResult<(bool, Option<Diagnostic>)> {
        let Some(error_node) = error_node else {
            return Ok((self.is_type_assignable_to(source, target)?, None));
        };
        let generic_head = std::ptr::eq(
            head_message,
            &diagnostics::Type_0_is_not_assignable_to_type_1,
        );
        let (related, output) = self.check_relation_with_error_output_at(
            source,
            target,
            crate::relate::RelationKind::Assignable,
            if generic_head {
                None
            } else {
                Some(head_message)
            },
            None,
            Some(error_node),
        )?;
        if let Some(output) = output {
            let mut diagnostic =
                self.create_error(output.error_node.or(Some(error_node)), head_message, &[]);
            diagnostic.message = output.message;
            diagnostic.related = output.related;
            // tsc logs errorInfo before returning `result !== False`.
            // In particular, a reporting relation can return true for a
            // `Maybe` verdict after publishing a useful diagnostic chain.
            return Ok((related, Some(diagnostic)));
        }
        if related {
            return Ok((true, None));
        }

        // A false relation normally owns an error chain. Retain the
        // generic face as a defensive fallback for overflow or a
        // contained checker abort, without introducing a second
        // relation walk that could mutate the cache topology.
        let (source, target) = self.normalized_relation_report_types(source, target)?;
        // An EXPLICIT tsc headMessage chains OUTERMOST
        // unconditionally (64860: errorInfo =
        // chainDiagnosticMessages(errorInfo, headMessage)) —
        // the reportUnmatchedProperty override and the 2696
        // head selection replace only the relation-level
        // GENERIC head. Our conflated signature distinguishes
        // by message identity: only the generic 2322 head
        // takes the override paths (the 5.8c class-band heads
        // 2415/2417/2420/2430 keep their code —
        // implementingAnInterfaceExtendingClassWithPrivates
        // pins the 2739→2720 silence).
        // isRelatedTo's excess-property arm (65197 →
        // hasExcessProperties) precedes the common-property
        // arm and every structural walk: a fresh object
        // literal with an unknown property reports the
        // parent-skipped 2353/2561 INSIDE the relation and no
        // head lands, for ANY head message (argument excess
        // rows are 2353 top-level too).
        if generic_head
            && self
                .tables
                .object_flags_of(source)
                .intersects(ObjectFlags::JSX_ATTRIBUTES)
            && self
                .tables
                .flags_of(target)
                .intersects(TypeFlags::INTERSECTION)
        {
            let constituents = match &self.tables.type_of(target).data {
                tsc_types::TypeData::Intersection { types } => types.to_vec(),
                _ => Vec::new(),
            };
            for constituent in constituents {
                let intrinsic = self
                    .tables
                    .type_of(constituent)
                    .symbol
                    .map(|symbol| self.binder.symbol(symbol).escaped_name.as_str())
                    .is_some_and(|name| {
                        matches!(name, "IntrinsicAttributes" | "IntrinsicClassAttributes")
                    });
                if intrinsic {
                    let Some(diagnostic) =
                        self.report_unmatched_property_head(source, constituent, error_node)?
                    else {
                        continue;
                    };
                    return Ok((related, Some(diagnostic)));
                }
            }
        }
        if let Some(diagnostic) = self.report_excess_property_head(
            source,
            target,
            error_node,
            crate::relate::RelationKind::Assignable,
        )? {
            return Ok((related, Some(diagnostic)));
        }
        // isRelatedTo's common-property arm (65208-65235)
        // precedes ALL structural elaboration and its early
        // return skips the head for ANY head message
        // (subtypingWithObjectMembers5 pins 2420→2559).
        if let Some(diagnostic) =
            self.report_no_common_properties_head(source, target, error_node)?
        {
            return Ok((related, Some(diagnostic)));
        }
        // global Object's 2696 branch lives inside
        // reportErrorResults, after structural elaboration.
        // Let the relation frame preserve missing-property
        // and incompatible-return descendants under it; the
        // old flattened approximation lost those rows.
        let global_object_source = generic_head
            && self.tables.flags_of(source).intersects(TypeFlags::OBJECT)
            && self.tables.type_of(source).symbol.is_some()
            && source == self.global_object_type()?;
        if generic_head && !global_object_source {
            if let Some(diagnostic) =
                self.report_unmatched_property_head(source, target, error_node)?
            {
                return Ok((related, Some(diagnostic)));
            }
        }
        let mut source_text = self.type_to_string_slice_with_error_enclosing(source)?;
        let mut target_text = self.type_to_string_slice_with_error_enclosing(target)?;
        if source_text == target_text {
            // getTypeNamesForErrorDisplay (50748-50756): equal
            // renders re-render fully qualified (no enclosing).
            source_text = self.get_type_name_for_error_display(source)?;
            target_text = self.get_type_name_for_error_display(target)?;
        }
        // reportRelationError 65097-65098: the GENERIC head
        // whose faces stay identical after the fully-qualified
        // re-render (unqualifiable same-name symbols — type
        // parameters, unexported namespaces) swaps to the 2719
        // "Two different types with this name exist" face. The
        // selection reads the PRE-generalization source face
        // (65066/65094-65099 ordering); explicit heads keep their
        // code.
        let head_message = if generic_head && source_text == target_text {
            &diagnostics::Type_0_is_not_assignable_to_type_1_Two_different_types_with_this_name_exist_but_they_are_unrelated
        } else {
            head_message
        };
        // reportRelationError 65068-65072: a literal source
        // generalizes to its base primitive unless the target
        // could accept singletons.
        let source_text = if !self.tables.flags_of(target).intersects(TypeFlags::NEVER)
            && self.is_literal_type(source)
            && !self.type_could_have_top_level_singleton_types(target)?
        {
            let generalized = self.get_base_type_of_literal_type(source)?;
            // 65072: the generalized source renders through
            // getTypeNameForErrorDisplay.
            self.get_type_name_for_error_display(generalized)?
        } else {
            source_text
        };
        let diagnostic = self.create_error(
            Some(error_node),
            head_message,
            &[&source_text, &target_text],
        );
        Ok((false, Some(diagnostic)))
    }

    /// tsc-port: hasExcessProperties @6.0.3 (the head-site face)
    /// tsc-hash: 2feb57fb3012195ec298b8373aae179205e425727845272eac7ef6231ed69cc7
    /// tsc-span: _tsc.js:65347-65410
    ///
    /// (The isRelatedTo gate that calls it sits at 65196-65207.)
    ///
    /// The relation engine's verdict runs the same
    /// excess_properties_worker (engine.rs) — this face re-runs it at
    /// the head site with reporting on, exactly the split tsc's
    /// reportErrors2 parameter expresses. The gate transcribes
    /// isRelatedTo's isPerformingExcessPropertyChecks at the
    /// reporting boundary (intersectionState is NONE at every
    /// check_type_assignable_to entry). The probe runs on a FRESH
    /// relation frame where tsc reports inside the failed walk's
    /// in-flight closure — the maybe-stack/budget difference cannot
    /// change the discriminant probes' verdicts (recorded deviation).
    fn report_excess_property_head(
        &mut self,
        source: TypeId,
        target: TypeId,
        error_node: NodeId,
        relation: crate::relate::RelationKind,
    ) -> CheckResult<Option<Diagnostic>> {
        if !self.is_object_literal_type(source)
            || !self
                .tables
                .object_flags_of(source)
                .intersects(ObjectFlags::FRESH_LITERAL)
        {
            return Ok(None);
        }
        let relation_count = (16_000_000 - self.relations.cache(relation).len() as i64) >> 3;
        let mut checker = crate::engine::RelationChecker {
            st: self,
            relation,
            maybe_keys: Vec::new(),
            maybe_keys_set: std::collections::HashSet::new(),
            source_stack: Vec::new(),
            target_stack: Vec::new(),
            maybe_count: 0,
            source_depth: 0,
            target_depth: 0,
            expanding_flags: tsc_types::ExpandingFlags::NONE,
            overflow: false,
            relation_count,
            error_state: Default::default(),
        };
        Ok(
            match checker.excess_properties_worker(
                source,
                target,
                /*report_errors*/ true,
                Some(error_node),
            )? {
                crate::engine::ExcessPropertyOutcome::UnknownProperty { diagnostic } => diagnostic,
                _ => None,
            },
        )
    }

    /// tsc-port: reportUnmatchedProperty @6.0.3 (the head-override
    /// half)
    /// tsc-hash: 2273740e1e468507c9fe6968bfee394b8d0511c7fcaf96b850f3ea2795413fbd
    /// tsc-span: _tsc.js:66708-66760
    ///
    /// propertiesRelatedTo reports missing REQUIRED properties before
    /// anything else and overrideNextErrorInfo suppresses the generic
    /// head: the missing-property message IS the diagnostic code
    /// (2741 single / 2739 list / 2740 list+more, related 2728 on the
    /// single property's declaration). The 5.4-slice twin runs it as
    /// a pre-head selection on failed OBJECT→OBJECT relations — the
    /// union/intersection and primitive paths never reach the
    /// properties walk and keep the generic head (oracle: unionDE = c
    /// stays 2322). tsc stamps canonicalHead (the skipped 2322) on
    /// these for compare/dedupe; elided here — no corpus observable
    /// until the T2 error machinery.
    /// tsc-port: isRelatedTo @6.0.3 (the common-property arm)
    /// tsc-hash: 21866dfda91834a7e8e842080b855cb4263b1c8e88917dd30df036aff15881e4
    /// tsc-span: _tsc.js:65208-65236
    ///
    /// The weak-type no-common-properties face: 2560 when the source
    /// is callable/constructable with a target-compatible return,
    /// else 2559. Conditions transcribe isPerformingCommonProperty
    /// Checks at the top-level inputs (relation=assignable ⇒ the
    /// comparable/unit clause holds; intersection state NONE at the
    /// call boundary); when they hold the engine's arm is exactly
    /// what failed the relation.
    fn report_no_common_properties_head(
        &mut self,
        source: TypeId,
        target: TypeId,
        error_node: NodeId,
    ) -> CheckResult<Option<Diagnostic>> {
        if !self
            .tables
            .flags_of(source)
            .intersects(TypeFlags::from_bits(
                TypeFlags::PRIMITIVE.bits()
                    | TypeFlags::OBJECT.bits()
                    | TypeFlags::INTERSECTION.bits(),
            ))
        {
            return Ok(None);
        }
        if source == self.global_object_type()? {
            return Ok(None);
        }
        // typeRelatedToSomeType reports on the BEST-MATCHING union
        // member, and the common-property arm fires inside that member
        // recursion — for a nullable union (`ImportCallOptions |
        // undefined`, the import-call options check) the object member
        // is the best match. Other union shapes keep the generic head.
        let target = if self.tables.flags_of(target).intersects(TypeFlags::UNION) {
            let members = match &self.tables.type_of(target).data {
                tsc_types::TypeData::Union { types, .. } => types.to_vec(),
                _ => Vec::new(),
            };
            let non_nullable: Vec<TypeId> = members
                .into_iter()
                .filter(|&member| !self.tables.flags_of(member).intersects(TypeFlags::NULLABLE))
                .collect();
            match non_nullable.as_slice() {
                [only] => *only,
                _ => return Ok(None),
            }
        } else {
            target
        };
        if !self
            .tables
            .flags_of(target)
            .intersects(TypeFlags::from_bits(
                TypeFlags::OBJECT.bits() | TypeFlags::INTERSECTION.bits(),
            ))
        {
            return Ok(None);
        }
        if !self.is_weak_type(target)? {
            return Ok(None);
        }
        let has_surface = !self.get_properties_of_type(source)?.is_empty()
            || self.type_has_call_or_construct_signatures(source)?;
        if !has_surface {
            return Ok(None);
        }
        if self.has_common_properties(source, target)? {
            return Ok(None);
        }
        // reportRelationError computes the display pair once at entry
        // (65066) — the weak-type rows read the same
        // getTypeNamesForErrorDisplay strings, enclosing included.
        let source_text = self.type_to_string_slice_with_error_enclosing(source)?;
        let target_text = self.type_to_string_slice_with_error_enclosing(target)?;
        let mut callable_face = false;
        for kind in [
            crate::state::SignatureKind::Call,
            crate::state::SignatureKind::Construct,
        ] {
            let signatures = self.get_signatures_of_type(source, kind)?;
            if let Some(&first) = signatures.first() {
                let return_type = self.get_return_type_of_signature(first)?;
                if self.is_type_assignable_to(return_type, target)? {
                    callable_face = true;
                    break;
                }
            }
        }
        let message = if callable_face {
            &diagnostics::Value_of_type_0_has_no_properties_in_common_with_type_1_Did_you_mean_to_call_it
        } else {
            &diagnostics::Type_0_has_no_properties_in_common_with_type_1
        };
        Ok(Some(self.create_error(
            Some(error_node),
            message,
            &[&source_text, &target_text],
        )))
    }

    /// The pre-head missing-property approximation uses only
    /// tryElaborateArrayLikeErrors' reportErrors=false verdict. The
    /// reporting face lives at its exact recursive relation site.
    fn try_elaborate_array_like_errors_without_reporting(
        &mut self,
        source: TypeId,
        target: TypeId,
    ) -> CheckResult<bool> {
        if self.tables.is_tuple_type(source) {
            let tuple_readonly = {
                let tuple_target = self.tables.reference_target(source);
                match &self.tables.type_of(tuple_target).data {
                    tsc_types::TypeData::TupleTarget(data) => data.readonly,
                    _ => false,
                }
            };
            if tuple_readonly && self.is_mutable_array_or_tuple(target)? {
                return Ok(false);
            }
            return Ok(self.is_array_type(target)? || self.tables.is_tuple_type(target));
        }
        if self.is_readonly_array_type(source)? && self.is_mutable_array_or_tuple(target)? {
            return Ok(false);
        }
        if self.tables.is_tuple_type(target) {
            return self.is_array_type(source);
        }
        Ok(true)
    }

    fn report_unmatched_property_head(
        &mut self,
        source: TypeId,
        target: TypeId,
        error_node: NodeId,
    ) -> CheckResult<Option<Diagnostic>> {
        // reportUnmatchedProperty runs over the isRelatedTo-NORMALIZED
        // pair: getNormalizedType's non-augmenting-subtype arm (64809)
        // substitutes an EMPTY single-base subclass with its base for
        // the property walk AND the missing-property displays (the
        // 2741 face of `class B extends A {}` prints 'A'), while
        // reportErrorResults keeps the ORIGINAL types for the plain
        // relation head only (65250-65253). The head-shaping caller
        // hands us the originals, so the substitution loop reruns
        // here (fixpoint, like getNormalizedType's while-true).
        let source = {
            let mut ty = source;
            while let Some(base) = self.get_single_base_for_non_augmenting_subtype(ty)? {
                ty = base;
            }
            ty
        };
        // isRelatedTo hands reportUnmatchedProperty its normalized
        // pair. Keep this defensive normalization for direct callers
        // too: required-property walks and 2741 displays must see the
        // write-normalized target.
        let target = self.get_normalized_type(target, /*writing*/ true)?;
        let target = {
            let mut ty = target;
            while let Some(base) = self.get_single_base_for_non_augmenting_subtype(ty)? {
                ty = base;
            }
            ty
        };
        // structuredTypeRelatedTo apparent-izes the source in place
        // (`source = getApparentType(source)`) — for the nonPrimitive
        // `object` that substitution is what the properties walk AND
        // the missing-property faces see (the oracle 2741 renders
        // '{}'). Primitive sources never report structurally
        // (reportStructuralErrors = reportErrors &&
        // !sourceIsPrimitive) and TYPE VARIABLES re-enter through the
        // constraint arm's NESTED isRelatedTo whose OUTER level
        // re-heads with the type-parameter face (`T extends {…}`
        // sources stay 2322) — both stay on the generic head via the
        // OBJECT|INTERSECTION gate below.
        let source = if self
            .tables
            .flags_of(source)
            .intersects(TypeFlags::NON_PRIMITIVE)
        {
            self.get_apparent_type(source)?
        } else {
            source
        };
        // Object AND intersection sources reach tsc's properties walk
        // (getUnmatchedProperties works over getPropertiesOfType);
        // unions/primitives keep the generic head (5.4 pin: unionDE =
        // c stays 2322).
        if !self
            .tables
            .flags_of(source)
            .intersects(TypeFlags::from_bits(
                TypeFlags::OBJECT.bits() | TypeFlags::INTERSECTION.bits(),
            ))
            || !self.tables.flags_of(target).intersects(TypeFlags::OBJECT)
        {
            return Ok(None);
        }
        // propertiesRelatedTo's tuple arm (66771-66774): a tuple
        // target with an array-or-tuple source takes the ARITY /
        // element-position walk — its failures report tuple-arity
        // chains under the generic relation head (or nothing at the
        // readonly early return) and never reach
        // reportUnmatchedProperty, so the missing-property override
        // must not fire for the pair (a NON-array source against a
        // tuple target falls through to the generic walk and keeps
        // the 2741/2739 faces — arityAndOrderCompatibility01's
        // 'StrNum' rows pin that half).
        if self.tables.is_tuple_type(target)
            && (self.is_array_type(source)? || self.tables.is_tuple_type(source))
        {
            return Ok(None);
        }
        // shouldReportUnmatchedPropertyError (67043-67054, gating the
        // 66879 report): a signature-shaped property-less source keeps
        // the plain relation head UNLESS the target is signature-
        // shaped in the same kind (oracle-probed: `t = () => 1`
        // against `{ f(): void }` is a headless 2322, not 2741 —
        // masked pre-9.3b2 by the fn-display curtain).
        {
            let source_calls = self
                .get_signatures_of_type(source, crate::state::SignatureKind::Call)?
                .len();
            let source_constructs = self
                .get_signatures_of_type(source, crate::state::SignatureKind::Construct)?
                .len();
            if (source_calls > 0 || source_constructs > 0)
                && self.get_properties_of_object_type_owned(source)?.is_empty()
            {
                let target_reports = (source_calls > 0
                    && !self
                        .get_signatures_of_type(target, crate::state::SignatureKind::Call)?
                        .is_empty())
                    || (source_constructs > 0
                        && !self
                            .get_signatures_of_type(target, crate::state::SignatureKind::Construct)?
                            .is_empty());
                if !target_reports {
                    return Ok(None);
                }
            }
        }
        let mut unmatched: Vec<SymbolId> = Vec::new();
        for target_prop in self.get_properties_of_type(target)? {
            let flags = self.binder.symbol(target_prop).flags;
            if flags.intersects(tsc_types::SymbolFlags::OPTIONAL)
                || self
                    .get_check_flags(target_prop)
                    .intersects(tsc_types::CheckFlags::PARTIAL)
            {
                continue;
            }
            let name = self.binder.symbol(target_prop).escaped_name.clone();
            // isStaticPrivateIdentifierProperty skip: only STATIC
            // private-identifier properties stay out of the head —
            // instance private names DO surface (privateNamesUnique-4
            // pins 2741 with '#something').
            let is_private = name.starts_with('#') || name.starts_with("__#");
            if is_private {
                let is_static = self
                    .binder
                    .symbol(target_prop)
                    .value_declaration
                    .is_some_and(|declaration| {
                        tsc_binder::node_util::has_syntactic_modifier(
                            self.binder.source_of_node(declaration),
                            declaration,
                            ModifierFlags::STATIC,
                        )
                    });
                if is_static {
                    continue;
                }
            }
            if self.get_property_of_type_full(source, &name)?.is_none() {
                unmatched.push(target_prop);
            }
        }
        if unmatched.is_empty() {
            return Ok(None);
        }
        // reportUnmatchedProperty's PRIVATE arm (66710-66724): probed
        // on the FIRST unmatched property BEFORE the props-count
        // dispatch (a #name twin beside other missing members still
        // takes this arm; a non-private FIRST unmatched skips it even
        // when a later one is private — both faces oracle-pinned). A
        // private-identifier member whose SOURCE class declares its
        // OWN #name twin reports the refers-to-a-different-member
        // chain under the PLAIN relation head (2322 row; the 18015
        // chain detail rides the unmodeled chain tail) — never a
        // missing-property head. tsc's twin lookup keys
        // getSymbolNameForPrivateIdentifier(source.symbol, desc) into
        // getPropertyOfType — only a member declared by the source
        // class itself can carry the source class's id, so the OWN
        // members table probe below is key-lookup-equivalent
        // (inherited privates carry the base class's id and never
        // match; the non-augmenting substitution above is what lets
        // an empty subclass hit its base's twin).
        let first_unmatched = unmatched[0];
        let private_description = self
            .binder
            .symbol(first_unmatched)
            .value_declaration
            .and_then(|declaration| {
                let source_file = self.binder.source_of_node(declaration);
                let name =
                    tsc_binder::node_util::get_name_of_declaration(source_file, declaration)?;
                if self.kind_of(name) != SyntaxKind::PrivateIdentifier {
                    return None;
                }
                self.escaped_text_of(Some(name)).map(str::to_owned)
            });
        if let Some(description) = private_description {
            let source_class_symbol = self.tables.type_of(source).symbol.filter(|&symbol| {
                self.binder
                    .symbol(symbol)
                    .flags
                    .intersects(tsc_types::SymbolFlags::CLASS)
            });
            if let Some(class_symbol) = source_class_symbol {
                let suffix = format!("@{description}");
                let has_own_twin = self
                    .get_members_of_symbol(class_symbol)?
                    .keys()
                    .any(|name| name.starts_with("__#") && name.ends_with(&suffix));
                if has_own_twin {
                    return Ok(None);
                }
            }
        }
        // reportUnmatchedProperty 66750: the MULTI-property head runs
        // behind tryElaborateArrayLikeErrors — a readonly-source /
        // mutable-target mismatch reports 4104 later instead (the
        // single-property 2741 arm is unconditional in tsc).
        if unmatched.len() > 1
            && !self.try_elaborate_array_like_errors_without_reporting(source, target)?
        {
            return Ok(None);
        }
        // 66735: the single-property face renders through
        // getTypeNamesForErrorDisplay — the context-sensitive
        // enclosing pass plus the equal→fully-qualified retry; the
        // multi-property 2739/2740 faces use plain typeToString
        // (66752-66757, no enclosing).
        // The unmatched-property verdict above remains keyed to the
        // original report pair. Only its source DISPLAY reconstructs
        // the structural relation's inner apparent face: branded
        // primitive intersections acquire their wrapper constituent,
        // and generic-reference aliases report the canonical
        // reference that failed. Keeping this after head selection is
        // essential — apparent-izing the verdict input would turn
        // unrelated generic 2322 rows into missing-property heads.
        let source_display = self.get_apparent_type(source)?;
        let source_display = if self
            .tables
            .object_flags_of(source_display)
            .intersects(ObjectFlags::REFERENCE)
            && self.tables.type_of(source_display).alias_symbol.is_some()
        {
            let target = self.tables.reference_target(source_display);
            let arguments = self.get_type_arguments(source_display)?;
            self.tables.create_type_reference(target, &arguments)
        } else {
            source_display
        };
        let (source_text, target_text) = if unmatched.len() == 1 {
            let source_text = self.type_to_string_slice_with_error_enclosing(source_display)?;
            let target_text = self.type_to_string_slice_with_error_enclosing(target)?;
            if source_text == target_text {
                (
                    self.get_type_name_for_error_display(source_display)?,
                    self.get_type_name_for_error_display(target)?,
                )
            } else {
                (source_text, target_text)
            }
        } else {
            (
                self.type_to_string_slice(source_display)?,
                self.type_to_string_slice(target)?,
            )
        };
        if unmatched.len() == 1 {
            let prop = unmatched[0];
            // 66736-66742: the single-property face is symbolToString
            // with WriteComputedProps — computed-name declarations
            // re-print their name node; the related 2728 reuses the
            // same string.
            let prop_name = self.missing_property_display_name(unmatched[0], true)?;
            let declaration = self.binder.symbol(prop).declarations.first().copied();
            let related = declaration
                .map(|declaration| {
                    self.related_info_for_node(
                        declaration,
                        &diagnostics::_0_is_declared_here,
                        &[&prop_name],
                    )
                })
                .into_iter()
                .collect();
            let mut diagnostic = self.create_error(
                Some(error_node),
                &diagnostics::Property_0_is_missing_in_type_1_but_required_in_type_2,
                &[&prop_name, &source_text, &target_text],
            );
            diagnostic.related = related;
            return Ok(Some(diagnostic));
        }
        // 66752-66757: the multi-property lists ride plain
        // symbolToString (no WriteComputedProps) — late-bound computed
        // names print their declaration SOURCE text verbatim.
        let mut names: Vec<String> = Vec::with_capacity(unmatched.len());
        for &prop in &unmatched {
            names.push(self.missing_property_display_name(prop, false)?);
        }
        let diagnostic = if unmatched.len() > 5 {
            let head: Vec<String> = names[..4].to_vec();
            let more = (unmatched.len() - 4).to_string();
            self.create_error(
                Some(error_node),
                &diagnostics::Type_0_is_missing_the_following_properties_from_type_1_2_and_3_more,
                &[&source_text, &target_text, &head.join(", "), &more],
            )
        } else {
            self.create_error(
                Some(error_node),
                &diagnostics::Type_0_is_missing_the_following_properties_from_type_1_2,
                &[&source_text, &target_text, &names.join(", ")],
            )
        };
        Ok(Some(diagnostic))
    }

    /// tsrs-native: the missing-property display name — private
    /// identifiers print their declaration text (`#x`), computed-name
    /// declarations follow the two symbolToString flavors, everything
    /// else unescapes like symbolToString.
    ///
    /// write_computed_props mirrors SymbolFormatFlags.WriteComputedProps
    /// (symbolToNode, 51122-51135): a computed-name value declaration
    /// re-prints its name node through the comment-stripping printer
    /// (51124-51127; entity chains normalize whitespace, strings
    /// re-quote double, numerics print the scanner's cooked value —
    /// all oracle-probed), and a declaration-less EnumLiteral /
    /// UniqueESSymbol nameType re-encloses at the name symbol's own
    /// declaration (51128-51133). WITHOUT the flag
    /// (getNameOfSymbolAsWritten, 50682-50690) late-bound computed
    /// names print declarationNameToString — SOURCE text verbatim
    /// (oracle: `other, [ B . sym ]`) — while early-bound
    /// string/number computed names ride the
    /// getNameOfSymbolFromNameType value face, which the unescape
    /// tail matches for bound names.
    pub(crate) fn missing_property_display_name(
        &mut self,
        prop: SymbolId,
        write_computed_props: bool,
    ) -> CheckResult<String> {
        let escaped = self.binder.symbol(prop).escaped_name.clone();
        if escaped.starts_with('#') {
            return Ok(escaped);
        }
        if escaped.starts_with("__#") {
            if let Some(declaration) = self.binder.symbol(prop).value_declaration {
                let source = self.binder.source_of_node(declaration);
                if let Some(name) =
                    tsc_binder::node_util::get_name_of_declaration(source, declaration)
                {
                    return Ok(tsc_binder::node_util::declaration_name_to_string(
                        source,
                        Some(name),
                    ));
                }
            }
        }
        let computed_name = self.binder.symbol(prop).value_declaration.and_then(|decl| {
            tsc_binder::node_util::get_name_of_declaration(self.binder.source_of_node(decl), decl)
                .filter(|&name| matches!(self.data_of(name), NodeData::ComputedPropertyName(_)))
        });
        if let Some(name) = computed_name {
            if write_computed_props {
                return self.computed_property_name_face_slice(name);
            }
            if self
                .links
                .symbol(prop)
                .check_flags
                .intersects(tsc_types::CheckFlags::LATE)
            {
                let source = self.binder.source_of_node(name);
                return Ok(tsc_binder::node_util::declaration_name_to_string(
                    source,
                    Some(name),
                ));
            }
        } else if write_computed_props {
            if let Some(name_type) = self.links.symbol(prop).name_type {
                if self
                    .tables
                    .flags_of(name_type)
                    .intersects(TypeFlags::ENUM_LITERAL | TypeFlags::UNIQUE_ES_SYMBOL)
                {
                    let name_symbol = self
                        .tables
                        .type_of(name_type)
                        .symbol
                        .expect("enum-literal/unique-symbol name types carry their symbol");
                    let enclosing = self.binder.symbol(name_symbol).value_declaration;
                    let face = self.symbol_expression_face_slice(name_symbol, enclosing, false)?;
                    return Ok(format!("[{face}]"));
                }
            }
        }
        Ok(self.symbol_name_as_written_slice(prop))
    }

    /// tsc-port: symbolToNode @6.0.3 (WriteComputedProps name reprint)
    /// tsc-hash: ba015cf97ede8e4493cf851a6464d86bfd06e225e4fed66cae72ea6a2d91ff41
    /// tsc-span: _tsc.js:51122-51135
    ///
    /// The 51124-51127 arm returns the declaration's computed name
    /// NODE and symbolToStringWorker prints it without a source file:
    /// entity chains re-print structurally (whitespace normalizes),
    /// string literals re-escape double-quoted (probed: `[ 'ab' ]` →
    /// `["ab"]`), numeric literals print the scanner's cooked value
    /// (probed: `[0x10]` → `[16]`), prefix-minus numerics keep the
    /// operator. Other expression shapes (templates, bigints,
    /// element-access names) use the same synthesized-expression
    /// printer leaf below.
    fn computed_property_name_face_slice(&mut self, name: NodeId) -> CheckResult<String> {
        let NodeData::ComputedPropertyName(data) = self.data_of(name) else {
            unreachable!("WriteComputedProps supplies a ComputedPropertyName node");
        };
        let expression = data
            .expression
            .expect("ComputedPropertyName carries its expression");
        let text = self.expression_text_slice(expression)?;
        Ok(format!("[{text}]"))
    }

    /// tsc-port: getLiteralText @6.0.3
    /// tsc-hash: 80004e3b921d6a73de6bfa96158bda4d14b1fc0f7515ea38e85c2aea93928326
    /// tsc-span: _tsc.js:13647-13687
    ///
    /// tsc-port: symbolToExpression @6.0.3
    /// tsc-hash: f1c7de91b82f1b2f5a3b4a2e7c1b82bd8504e06172492e073464b298e0938e03
    /// tsc-span: _tsc.js:53337-53387
    ///
    /// getDeclarationName only binds literal/signed-literal computed
    /// properties; late binding additionally admits entity/property/
    /// element-access unique-symbol names. Those are the complete
    /// constructible inputs to WriteComputedProps. Template and bigint
    /// leaves are included because the factory printer owns their
    /// synthesized spelling even when a recovery tree reaches this
    /// helper.
    fn expression_text_slice(&mut self, node: NodeId) -> CheckResult<String> {
        match self.kind_of(node) {
            SyntaxKind::TrueKeyword => return Ok("true".to_owned()),
            SyntaxKind::FalseKeyword => return Ok("false".to_owned()),
            SyntaxKind::NullKeyword => return Ok("null".to_owned()),
            SyntaxKind::ThisKeyword => return Ok("this".to_owned()),
            SyntaxKind::SuperKeyword => return Ok("super".to_owned()),
            _ => {}
        }
        match self.data_of(node).clone() {
            NodeData::Identifier(data) => {
                Ok(tsc_binder::unescape_leading_underscores(&data.escaped_text).to_owned())
            }
            NodeData::PrivateIdentifier(data) => Ok(data.text),
            NodeData::StringLiteral(data) => string_literal_name_slice(&data.text, false),
            NodeData::NumericLiteral(data) => Ok(data.text),
            NodeData::BigIntLiteral(data) => Ok(data.text),
            NodeData::NoSubstitutionTemplateLiteral(data) => {
                let raw = data
                    .raw_text
                    .unwrap_or_else(|| template_text_raw(&data.text));
                Ok(format!("`{raw}`"))
            }
            NodeData::RegularExpressionLiteral(data) => Ok(data.text),
            NodeData::PrefixUnaryExpression(data) => {
                let operator = tsc_syntax::tokens::token_to_string(data.operator)
                    .expect("PrefixUnaryExpression carries a prefix operator");
                let operand = self.expression_text_slice(
                    data.operand
                        .expect("PrefixUnaryExpression carries its operand"),
                )?;
                Ok(format!("{operator}{operand}"))
            }
            NodeData::BinaryExpression(data) => {
                // Existing-node parameter initializers admit arbitrary
                // AssignmentExpressions. This live bridge preserves
                // the printer's spaced binary/comma operator face; the
                // remaining inventory uses the recovery-safe adapter.
                let left = self.expression_text_slice(
                    data.left.expect("BinaryExpression carries its left operand"),
                )?;
                let operator = data
                    .operator_token
                    .map(|token| self.kind_of(token))
                    .and_then(tsc_syntax::tokens::token_to_string)
                    .expect("BinaryExpression carries an operator token");
                let right = self.expression_text_slice(
                    data.right
                        .expect("BinaryExpression carries its right operand"),
                )?;
                if operator == "," {
                    Ok(format!("{left}, {right}"))
                } else {
                    Ok(format!("{left} {operator} {right}"))
                }
            }
            NodeData::ParenthesizedExpression(data) => Ok(format!(
                "({})",
                self.expression_text_slice(
                    data.expression
                        .expect("ParenthesizedExpression carries its expression"),
                )?
            )),
            NodeData::PropertyAccessExpression(data) => {
                let expression = self.expression_text_slice(
                    data.expression
                        .expect("PropertyAccessExpression carries its expression"),
                )?;
                let name = self.entity_name_text_slice(
                    data.name.expect("PropertyAccessExpression carries its name"),
                )?;
                let dot = if data.question_dot_token.is_some() {
                    "?."
                } else {
                    "."
                };
                Ok(format!("{expression}{dot}{name}"))
            }
            NodeData::ElementAccessExpression(data) => {
                let expression = self.expression_text_slice(
                    data.expression
                        .expect("ElementAccessExpression carries its expression"),
                )?;
                let argument = self.expression_text_slice(
                    data.argument_expression
                        .expect("ElementAccessExpression carries its argument"),
                )?;
                let question = if data.question_dot_token.is_some() {
                    "?."
                } else {
                    ""
                };
                Ok(format!("{expression}{question}[{argument}]"))
            }
            NodeData::TemplateExpression(data) => {
                let head = data.head.expect("TemplateExpression carries its head");
                let NodeData::TemplateHead(head) = self.data_of(head).clone() else {
                    unreachable!("TemplateExpression head is a TemplateHead");
                };
                let mut text = format!(
                    "`{}",
                    head.raw_text
                        .unwrap_or_else(|| template_text_raw(&head.text))
                );
                for span in self.nodes_of(data.template_spans) {
                    let NodeData::TemplateSpan(span) = self.data_of(span).clone() else {
                        unreachable!("TemplateExpression spans contain TemplateSpan nodes");
                    };
                    let expression = self.expression_text_slice(
                        span.expression.expect("TemplateSpan carries its expression"),
                    )?;
                    let literal = span.literal.expect("TemplateSpan carries its literal");
                    let literal = match self.data_of(literal).clone() {
                        NodeData::TemplateMiddle(data) => data
                            .raw_text
                            .unwrap_or_else(|| template_text_raw(&data.text)),
                        NodeData::TemplateTail(data) => data
                            .raw_text
                            .unwrap_or_else(|| template_text_raw(&data.text)),
                        _ => unreachable!("TemplateSpan literal is TemplateMiddle/TemplateTail"),
                    };
                    text.push_str(&format!("${{{expression}}}{literal}"));
                }
                text.push('`');
                Ok(text)
            }
            _ => unreachable!(
                "bound computed names are literal, signed literal, or unique-symbol access expressions"
            ),
        }
    }

    /// The syntactic reuse visitor clones parameter/property
    /// initializers, whose grammar admits every AssignmentExpression.
    /// The dedicated clone-display printer mirrors the standard
    /// printer over that complete grammar; `None` is reserved for a
    /// malformed/recovery node and therefore arms the existing
    /// TypeNode recovery boundary.
    fn reused_initializer_expression_text_slice(&mut self, node: NodeId) -> CheckResult<String> {
        match self.display_clone_expression_text_at_line_start(node, false)? {
            Some(text) => Ok(text),
            None => {
                self.slice_reuse_had_error = true;
                Ok(String::new())
            }
        }
    }

    /// tsc-port: checkTypeComparableTo @6.0.3
    /// tsc-hash: e58eb977753b557ce9b0c944ca7602c6b1b4981cd57f5ce5d3181ab726e31d4d
    /// tsc-span: _tsc.js:63937-63939
    ///
    /// The comparable twin of check_type_assignable_to above, including
    /// the reporting-mode relation replay and its full errorInfo chain.
    pub(crate) fn check_type_comparable_to(
        &mut self,
        source: TypeId,
        target: TypeId,
        error_node: Option<NodeId>,
        head_message: &'static DiagnosticMessage,
    ) -> CheckResult<bool> {
        let original_source = source;
        let original_target = target;
        let related = self.is_type_comparable_to(source, target)?;
        if !related {
            if let Some(error_node) = error_node {
                let (source, target) = self.normalized_relation_report_types(source, target)?;
                // isRelatedTo's excess-property arm runs under the
                // comparable relation too (65353) — a fresh-literal
                // case expression reports the parent-skipped
                // 2353/2561 and the 2678 head never lands.
                if let Some(diagnostic) = self.report_excess_property_head(
                    source,
                    target,
                    error_node,
                    crate::relate::RelationKind::Comparable,
                )? {
                    self.push_error_diagnostic(diagnostic);
                    return Ok(related);
                }
                let generic_head = std::ptr::eq(
                    head_message,
                    &diagnostics::Type_0_is_not_comparable_to_type_1,
                );
                if let Ok(Some(output)) = self.relation_error_output_with_context_at(
                    original_source,
                    original_target,
                    crate::relate::RelationKind::Comparable,
                    if generic_head {
                        None
                    } else {
                        Some(head_message)
                    },
                    None,
                    Some(error_node),
                ) {
                    let mut diagnostic = self.create_error(
                        output.error_node.or(Some(error_node)),
                        head_message,
                        &[],
                    );
                    diagnostic.message = output.message;
                    diagnostic.related = output.related;
                    self.push_error_diagnostic(diagnostic);
                    return Ok(related);
                }
                let mut source_text = self.type_to_string_slice_with_error_enclosing(source)?;
                let mut target_text = self.type_to_string_slice_with_error_enclosing(target)?;
                if source_text == target_text {
                    // getTypeNamesForErrorDisplay (50748-50756): equal
                    // renders re-render fully qualified (no enclosing).
                    source_text = self.get_type_name_for_error_display(source)?;
                    target_text = self.get_type_name_for_error_display(target)?;
                }
                let source_text = if !self.tables.flags_of(target).intersects(TypeFlags::NEVER)
                    && self.is_literal_type(source)
                    && !self.type_could_have_top_level_singleton_types(target)?
                {
                    let generalized = self.get_base_type_of_literal_type(source)?;
                    // 65072: the generalized source renders through
                    // getTypeNameForErrorDisplay.
                    self.get_type_name_for_error_display(generalized)?
                } else {
                    source_text
                };
                self.error_at(
                    Some(error_node),
                    head_message,
                    &[&source_text, &target_text],
                );
            }
        }
        Ok(related)
    }

    /// tsc-port: typeCouldHaveTopLevelSingletonTypes @6.0.3
    /// tsc-hash: 30ea1344b1c8021a31ecb437af9d4a5867abd72fb6bf08c9b64d434ca6f09947
    /// tsc-span: _tsc.js:67231-67245
    pub(crate) fn type_could_have_top_level_singleton_types(
        &mut self,
        ty: TypeId,
    ) -> CheckResult<bool> {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::BOOLEAN) {
            return Ok(false);
        }
        if flags.intersects(TypeFlags::UNION | TypeFlags::INTERSECTION) {
            let types = match &self.tables.type_of(ty).data {
                TypeData::Union { types, .. } | TypeData::Intersection { types } => types.to_vec(),
                _ => unreachable!("union/intersection flag implies composite data"),
            };
            for member in types {
                if self.type_could_have_top_level_singleton_types(member)? {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        if flags.intersects(TypeFlags::INSTANTIABLE) {
            if let Some(constraint) = self.get_constraint_of_type(ty)? {
                if constraint != ty {
                    return self.type_could_have_top_level_singleton_types(constraint);
                }
            }
        }
        Ok(self.is_unit_type(ty)
            || flags.intersects(TypeFlags::TEMPLATE_LITERAL)
            || flags.intersects(TypeFlags::STRING_MAPPING))
    }

    /// tsc-port: hasNonCircularTypeParameterDefault @6.0.3
    /// tsc-hash: 92d51650cf90282ec44b35a125949970906494f09a52d28fff996338901938cc
    /// tsc-span: _tsc.js:59065-59067
    fn has_non_circular_type_parameter_default(
        &mut self,
        type_parameter: TypeId,
    ) -> CheckResult<bool> {
        let default = self.get_resolved_type_parameter_default(type_parameter)?;
        Ok(default != self.circular_constraint_type)
    }

    /// getSymbolOfDeclaration (49936) — the binder's node.symbol
    /// through getLateBoundSymbol (57770) and the getMergedSymbol
    /// chase (JS aliasing arms with the JS residual).
    /// tsc-port: getSymbolOfDeclaration @6.0.3
    /// tsc-hash: 197061af99891199274ec82eb08309cbb138441e9fcba571ac5aa6149bf1b3a0
    /// tsc-span: _tsc.js:49936-49938
    pub(crate) fn get_symbol_of_declaration(&mut self, node: NodeId) -> CheckResult<SymbolId> {
        let Some(symbol) = self.node_symbol(node) else {
            // Binder-created declarations always carry a symbol. Recovery
            // or checker-synthetic nodes without one use the same stable
            // miss sentinel as unresolved symbol lookup.
            return Ok(self.unknown_symbol);
        };
        let symbol = self.get_late_bound_symbol(symbol)?;
        Ok(self.get_merged_symbol(symbol))
    }

    /// tsc-port: getLateBoundSymbol @6.0.3
    /// tsc-hash: 5a307eb64aef32672fb0364160c3b6f3c2a40a7797ccf19bc86145d1b04c49b8
    /// tsc-span: _tsc.js:57770-57784
    ///
    /// Forcing the parent's member/export tables runs the late-binding
    /// loop, which stamps links.lateSymbol on the early "__computed"
    /// symbols; a symbol left unstamped self-resolves (tsc
    /// `links.lateSymbol ||= symbol`) — the stamp-as-self write is
    /// elided (pure memo).
    pub(crate) fn get_late_bound_symbol(&mut self, symbol: SymbolId) -> CheckResult<SymbolId> {
        let data = self.binder.symbol(symbol);
        if !data.flags.intersects(tsc_types::SymbolFlags::CLASS_MEMBER)
            || data.escaped_name != "__computed"
        {
            return Ok(symbol);
        }
        if self.links.symbol(symbol).late_symbol.is_none()
            && data
                .declarations
                .clone()
                .iter()
                .any(|&declaration| self.has_late_bindable_ast_name(declaration))
        {
            let parent = data.parent;
            if let Some(parent) = parent {
                let parent = self.get_merged_symbol(parent);
                let source = self.binder.symbol(symbol).declarations.clone();
                let is_static = source.iter().any(|&declaration| {
                    tsc_binder::node_util::has_syntactic_modifier(
                        self.binder.source_of_node(declaration),
                        declaration,
                        tsc_types::ModifierFlags::STATIC,
                    )
                });
                if is_static {
                    self.get_exports_of_symbol(parent)?;
                } else {
                    self.get_members_of_symbol(parent)?;
                }
            }
        }
        Ok(self.links.symbol(symbol).late_symbol.unwrap_or(symbol))
    }

    // ---- typeToString display port ----

    /// The error-display typeToString path: intrinsics and literal
    /// quoting, type parameters (including the ForCheck marker rule),
    /// alias/reference heads, unions/intersections, and the complete
    /// reachable nodeBuilder structural tail. Recursive helpers below
    /// preserve TypeNode kind alongside text so every factory
    /// parenthesizer decision is made before the final string join.
    /// tsc-port: typeToString @6.0.3
    /// tsc-hash: 4b587962e2fb137a31ea52c35aeba733ffb4c6d97a8c54c98d5c1f1666e73dda
    /// tsc-span: _tsc.js:50717-50747
    pub(crate) fn type_to_string_slice(&mut self, ty: TypeId) -> CheckResult<String> {
        self.type_to_string_slice_root(
            ty, /*fully_qualified*/ false, /*no_type_reduction*/ false,
        )
    }

    /// tsrs-native: state-backed adapter for
    /// typeToString(..., NodeBuilderFlags.NoTypeReduction), used by
    /// elaborateNeverIntersection to retain the original intersection
    /// face after getReducedType has cached its collapse to never.
    pub(crate) fn type_to_string_slice_no_type_reduction(
        &mut self,
        ty: TypeId,
    ) -> CheckResult<String> {
        self.type_to_string_slice_root(
            ty, /*fully_qualified*/ false, /*no_type_reduction*/ true,
        )
    }

    /// tsc-port: getTypeNameForErrorDisplay @6.0.3
    /// tsc-hash: 9e9827829d64df1cb9ed00762b4a5c872a23139bdd217fffd5c274437e7ac389
    /// tsc-span: _tsc.js:50757-50764
    ///
    /// typeToString under UseFullyQualifiedType: every symbol head
    /// follows the same symbol-chain/import-type construction as
    /// nodeBuilder, while structural shapes reuse the ordinary
    /// recursive renderer.
    pub(crate) fn get_type_name_for_error_display(&mut self, ty: TypeId) -> CheckResult<String> {
        self.type_to_string_slice_root(
            ty, /*fully_qualified*/ true, /*no_type_reduction*/ false,
        )
    }

    /// withContext's typeToString-local nodeBuilder state
    /// (_tsc.js:51205-51256). The renderer's recursive methods share
    /// the parked fields below; a root call saves/restores them so a
    /// semantic getter that re-enters typeToString receives a fresh
    /// context just like tsc.
    fn type_to_string_slice_root(
        &mut self,
        ty: TypeId,
        fully_qualified: bool,
        no_type_reduction: bool,
    ) -> CheckResult<String> {
        let saved_visited = std::mem::take(&mut self.slice_visited_types);
        let saved_infer_type_parameters = std::mem::take(&mut self.slice_infer_type_parameters);
        let saved_approximate_length = std::mem::replace(&mut self.slice_approximate_length, 0);
        let saved_max_truncation_length = std::mem::replace(
            &mut self.slice_max_truncation_length,
            if self.options.no_error_truncation == Some(true) {
                1_000_000
            } else {
                160
            },
        );
        let saved_truncating = std::mem::replace(&mut self.slice_truncating, false);
        let saved_reverse_mapped_stack = std::mem::take(&mut self.slice_reverse_mapped_stack);
        let saved_no_type_reduction =
            std::mem::replace(&mut self.slice_no_type_reduction, no_type_reduction);
        let result = self.type_to_string_slice_ex(ty, fully_qualified);
        self.slice_visited_types = saved_visited;
        self.slice_infer_type_parameters = saved_infer_type_parameters;
        self.slice_approximate_length = saved_approximate_length;
        self.slice_max_truncation_length = saved_max_truncation_length;
        self.slice_truncating = saved_truncating;
        self.slice_reverse_mapped_stack = saved_reverse_mapped_stack;
        self.slice_no_type_reduction = saved_no_type_reduction;
        result
    }

    fn type_to_string_slice_ex(
        &mut self,
        ty: TypeId,
        fully_qualified: bool,
    ) -> CheckResult<String> {
        Ok(self.type_to_string_slice_node(ty, fully_qualified)?.0)
    }

    /// tsc-port: checkTruncationLength @6.0.3
    /// tsc-hash: 487c4a58aa166fe4725c57bdefaa15d36737ad40f6c64fa1476f33bd83d24e06
    /// tsc-span: _tsc.js:51284-51287
    ///
    /// Once the accumulated
    /// nodeBuilder estimate exceeds the active budget, truncating is
    /// sticky for the rest of this typeToString call.
    fn slice_check_truncation_length(&mut self) -> bool {
        if !self.slice_truncating {
            self.slice_truncating =
                self.slice_approximate_length > self.slice_max_truncation_length;
        }
        self.slice_truncating
    }

    fn slice_add_approximate_length(&mut self, length: usize) {
        self.slice_approximate_length = self.slice_approximate_length.saturating_add(length);
    }

    /// JS string `.length`, used by nodeBuilder's estimate counters.
    fn slice_js_length(text: &str) -> usize {
        text.encode_utf16().count()
    }

    /// createAccessFromSymbolChain on the ordinary one-symbol chain
    /// increments once while selecting the root name and once again
    /// while creating that link (53204-53231).
    fn slice_add_bare_symbol_length(&mut self, name: &str) {
        self.slice_add_approximate_length(2 * (Self::slice_js_length(name) + 1));
    }

    fn slice_truncation_type_node(&self) -> (String, SliceTypeNodeKind) {
        if self.options.no_error_truncation == Some(true) {
            // NoTruncation uses `any` plus a synthetic elision
            // comment; typeToString's remove-comments printer leaves
            // the keyword face.
            ("any".to_owned(), SliceTypeNodeKind::Keyword)
        } else {
            ("...".to_owned(), SliceTypeNodeKind::Reference)
        }
    }

    fn slice_types_are_same_reference(&self, left: TypeId, right: TypeId) -> bool {
        if left == right {
            return true;
        }
        let left = self.tables.type_of(left);
        let right = self.tables.type_of(right);
        left.symbol.is_some() && left.symbol == right.symbol
            || left.alias_symbol.is_some() && left.alias_symbol == right.alias_symbol
    }

    /// tsc-port: mapToTypeNodes @6.0.3
    /// tsc-hash: a385aa0049c7141c5be2128f906eee6191ceb8679fc524625f46ebab2808d59a
    /// tsc-span: _tsc.js:52404-52472
    ///
    /// Includes its sticky
    /// truncation probes and the two-unit estimate charged before
    /// every rendered element, plus the fully-qualified
    /// same-written-name collision retry.
    fn map_to_type_string_nodes_slice(
        &mut self,
        types: &[TypeId],
        fully_qualified: bool,
        is_bare_list: bool,
    ) -> CheckResult<Vec<(String, SliceTypeNodeKind)>> {
        if types.is_empty() {
            return Ok(Vec::new());
        }
        if self.slice_check_truncation_length() {
            if !is_bare_list {
                return Ok(vec![self.slice_truncation_type_node()]);
            }
            if types.len() > 2 {
                let first = self.type_to_string_slice_node(types[0], fully_qualified)?;
                let last =
                    self.type_to_string_slice_node(types[types.len() - 1], fully_qualified)?;
                return Ok(vec![
                    first,
                    if self.options.no_error_truncation == Some(true) {
                        ("any".to_owned(), SliceTypeNodeKind::Keyword)
                    } else {
                        (
                            format!("... {} more ...", types.len() - 2),
                            SliceTypeNodeKind::Reference,
                        )
                    },
                    last,
                ]);
            }
        }
        let mut rendered = Vec::with_capacity(types.len());
        let mut seen_names: std::collections::BTreeMap<String, Vec<(TypeId, usize)>> =
            std::collections::BTreeMap::new();
        for (index, &ty) in types.iter().enumerate() {
            let ordinal = index + 1;
            if self.slice_check_truncation_length() && ordinal + 2 < types.len() - 1 {
                rendered.push(if self.options.no_error_truncation == Some(true) {
                    ("any".to_owned(), SliceTypeNodeKind::Keyword)
                } else {
                    (
                        format!("... {} more ...", types.len() - ordinal),
                        SliceTypeNodeKind::Reference,
                    )
                });
                rendered
                    .push(self.type_to_string_slice_node(types[types.len() - 1], fully_qualified)?);
                break;
            }
            self.slice_add_approximate_length(2);
            let node = self.type_to_string_slice_node(ty, fully_qualified)?;
            let result_index = rendered.len();
            if !fully_qualified && node.1 == SliceTypeNodeKind::Reference {
                let head = node
                    .0
                    .split_once('<')
                    .map_or(node.0.as_str(), |(head, _)| head);
                if tsc_syntax::is_identifier_text(head) {
                    seen_names
                        .entry(head.to_owned())
                        .or_default()
                        .push((ty, result_index));
                }
            }
            rendered.push(node);
        }
        // mapToTypeNodes' same-written-name retry (52451-52469):
        // only a heterogeneous identifier-reference group rerenders
        // under UseFullyQualifiedType. Same-symbol generic
        // instantiations are homogeneous even when arguments differ.
        for collisions in seen_names.values() {
            let Some(&(first, _)) = collisions.first() else {
                continue;
            };
            if collisions
                .iter()
                .skip(1)
                .all(|&(ty, _)| self.slice_types_are_same_reference(first, ty))
            {
                continue;
            }
            for &(ty, result_index) in collisions {
                rendered[result_index] =
                    self.type_to_string_slice_node(ty, /*fully_qualified*/ true)?;
            }
        }
        Ok(rendered)
    }

    /// symbolToTypeNode's reference head. Qualification/import roots
    /// still belong to symbolToTypeNode; its root spelling is supplied
    /// by getNameOfSymbolAsWritten below.
    fn type_reference_symbol_name_slice(
        &mut self,
        symbol: SymbolId,
        fully_qualified: bool,
    ) -> CheckResult<String> {
        Ok(self.symbol_type_face_slice(symbol, fully_qualified)?.0)
    }

    /// tsc-port: getNameOfSymbolAsWritten @6.0.3
    /// tsc-hash: 3ab46f78adf8c8b4e40a35ee7661568f015913f937ce2a575104d21c3428f9f0
    /// tsc-span: _tsc.js:55541-55588
    ///
    /// Root entity names run with InInitialEntityName. That lets a
    /// named default declaration use its written name in the same
    /// binding context. Class/function expressions borrow an assigned
    /// declaration name; truly unnamed expressions use tsc's sentinel
    /// instead of leaking `__class`/`__function`.
    // h2-7a-m-3 widening: decision-only NodeBuilder reuse anchor.
    pub(crate) fn entity_symbol_name_as_written_slice(
        &self,
        symbol: SymbolId,
        in_initial_entity_name: bool,
        use_alias_defined_outside_current_scope: bool,
        enclosing: Option<NodeId>,
    ) -> String {
        let declarations = self.binder.symbol(symbol).declarations.clone();
        if self.binder.symbol(symbol).escaped_name == tsc_types::InternalSymbolName::DEFAULT
            && !use_alias_defined_outside_current_scope
            && (!in_initial_entity_name
                || declarations.is_empty()
                || enclosing.is_some_and(|enclosing| {
                    declarations
                        .first()
                        .copied()
                        .and_then(|declaration| self.default_binding_context_slice(declaration))
                        != self.default_binding_context_slice(enclosing)
                }))
        {
            return tsc_types::InternalSymbolName::DEFAULT.to_owned();
        }

        if !declarations.is_empty() {
            let named_declaration = declarations.iter().copied().find(|&declaration| {
                node_util::get_name_of_declaration(
                    self.binder.source_of_node(declaration),
                    declaration,
                )
                .is_some()
            });
            if named_declaration.is_some() {
                return self.symbol_name_as_written_slice(symbol);
            }

            let declaration = declarations[0];
            if let Some(parent) = self.parent_of(declaration) {
                if self.kind_of(parent) == SyntaxKind::VariableDeclaration {
                    let source = self.binder.source_of_node(parent);
                    if let Some(name) = node_util::get_name_of_declaration(source, parent) {
                        return node_util::declaration_name_to_string(source, Some(name));
                    }
                }
            }
            match self.kind_of(declaration) {
                SyntaxKind::ClassExpression => return "(Anonymous class)".to_owned(),
                SyntaxKind::FunctionExpression | SyntaxKind::ArrowFunction => {
                    return "(Anonymous function)".to_owned();
                }
                _ => {}
            }
        }
        self.symbol_display_name(symbol)
    }

    /// tsc isDefaultBindingContext + findAncestor: source files and
    /// ambient modules delimit where a named default export may use
    /// its declaration spelling.
    fn default_binding_context_slice(&self, node: NodeId) -> Option<NodeId> {
        let mut current = Some(node);
        while let Some(candidate) = current {
            let source = self.binder.source_of_node(candidate);
            if self.kind_of(candidate) == SyntaxKind::SourceFile
                || node_util::is_ambient_module(source, candidate)
            {
                return Some(candidate);
            }
            current = self.parent_of(candidate);
        }
        None
    }

    /// tsc-port: getParentSymbolOfTypeParameter @6.0.3
    /// tsc-hash: c6c6439ef9269ecc33487047b46a90e24f5781fb5e1ee2548429866d84d7e57e
    /// tsc-span: _tsc.js:60123-60127
    fn parent_symbol_of_type_parameter_slice(&self, parameter: TypeId) -> Option<SymbolId> {
        let symbol = self.tables.type_of(parameter).symbol?;
        let declaration = self
            .binder
            .symbol(symbol)
            .declarations
            .iter()
            .copied()
            .find(|&declaration| self.kind_of(declaration) == SyntaxKind::TypeParameter)?;
        let parent = self.parent_of(declaration)?;
        let host = if self.kind_of(parent) == SyntaxKind::JSDocTemplateTag {
            self.effective_container_for_jsdoc_template_tag(parent)?
        } else {
            parent
        };
        self.get_symbol_of_declaration_opt(host)
    }

    /// typeReferenceToTypeNode's deliberately narrow trailing-default
    /// elision gate. tsc only applies it to the four iterable protocol
    /// references; ordinary generics (including Generator) keep every
    /// filled argument. A node-backed reference with an explicitly
    /// complete argument list also keeps its arguments.
    ///
    /// tsc-port: typeReferenceToTypeNode @6.0.3
    /// tsc-hash: 5c22abc10910aaee0aa11e8f853b69bb6a7437fa209d7fb7194627f7a692775e
    /// tsc-span: _tsc.js:52009-52033
    fn should_elide_iterable_default_arguments_slice(
        &mut self,
        ty: TypeId,
        type_parameter_count: usize,
    ) -> CheckResult<bool> {
        let target = self.tables.reference_target(ty);
        let Some(symbol) = self.tables.type_of(target).symbol else {
            return Ok(false);
        };
        // Keep the common reference-rendering path allocation-free
        // and getter-free: only one of the four exact written names
        // can enter the global-identity probe.
        enum IterableProtocol {
            Iterable,
            IterableIterator,
            AsyncIterable,
            AsyncIterableIterator,
        }
        let protocol_name = match self.binder.symbol(symbol).escaped_name.as_str() {
            "Iterable" => Some(IterableProtocol::Iterable),
            "IterableIterator" => Some(IterableProtocol::IterableIterator),
            "AsyncIterable" => Some(IterableProtocol::AsyncIterable),
            "AsyncIterableIterator" => Some(IterableProtocol::AsyncIterableIterator),
            _ => None,
        };
        let protocol_target = match protocol_name {
            Some(IterableProtocol::Iterable) => {
                self.get_global_iterable_type(/*report_errors*/ false)?
            }
            Some(IterableProtocol::IterableIterator) => {
                self.get_global_iterable_iterator_type(/*report_errors*/ false)?
            }
            Some(IterableProtocol::AsyncIterable) => {
                self.get_global_async_iterable_type(/*report_errors*/ false)?
            }
            Some(IterableProtocol::AsyncIterableIterator) => {
                self.get_global_async_iterable_iterator_type(/*report_errors*/ false)?
            }
            None => return Ok(false),
        };
        // The Rust speculation boundary can rebuild a declared
        // GenericType shell after rolling back its symbol-link
        // publication while the checker-global memo retains the
        // earlier shell. tsc has one mutable object in both slots, so
        // its `type.target === globalIterableType` identity still
        // holds. The shared merged symbol is the storage-adapter
        // identity for that otherwise-impossible duplicate shell.
        let same_protocol_target = self.is_reference_to_type(ty, protocol_target)
            || (self
                .tables
                .object_flags_of(ty)
                .intersects(ObjectFlags::REFERENCE)
                && self.tables.type_of(target).symbol.is_some()
                && self.tables.type_of(target).symbol
                    == self.tables.type_of(protocol_target).symbol);
        if !same_protocol_target {
            return Ok(false);
        }

        let Some(node) = self.links.ty(ty).deferred_node else {
            return Ok(true);
        };
        let NodeData::TypeReference(data) = self.data_of(node) else {
            return Ok(true);
        };
        Ok(self.nodes_of(data.type_arguments).len() < type_parameter_count)
    }

    /// The kind-carrying face of the slice renderer: the nodeBuilder
    /// emits factory TypeNodes and the factory's parenthesizer rules
    /// branch on the CHILD node's kind at every join, so the string
    /// slice returns the would-be node kind beside the text and the
    /// joins apply the same rules (`SliceTypeNodeKind`).
    ///
    /// tsc-port: typeToTypeNodeWorker @6.0.3 (Any arm)
    /// tsc-hash: db8cf9911d836f13e37be9d395a4d61fbfad346e6c3e3c25210441a94d986533
    /// tsc-span: _tsc.js:51338-51347
    fn type_to_string_slice_node(
        &mut self,
        ty: TypeId,
        fully_qualified: bool,
    ) -> CheckResult<(String, SliceTypeNodeKind)> {
        // typeToTypeNodeWorker 51331-51333: typeToString's default
        // builder flags do not include NoTypeReduction, so every
        // recursive display frame reduces before selecting a node
        // arm. This is observable for never-reduced intersections.
        let ty = if self.slice_no_type_reduction {
            ty
        } else {
            self.get_reduced_type(ty)?
        };
        if ty == self.marker_super_type_for_check || ty == self.marker_sub_type_for_check {
            // typeToString's type-parameter arm (51535).
            let name = self
                .variance_type_parameter
                .and_then(|tp| self.tables.type_of(tp).symbol)
                .map(|symbol| self.symbol_display_name(symbol));
            let prefix = if ty == self.marker_sub_type_for_check {
                "sub-"
            } else {
                "super-"
            };
            return Ok((
                match name {
                    Some(name) => format!("{prefix}{name}"),
                    None => "?".to_owned(),
                },
                SliceTypeNodeKind::Reference,
            ));
        }
        let flags = self.tables.flags_of(ty);
        // typeToTypeNodeWorker 51327-51330: Any-like types consult
        // aliasSymbol before the AnyKeyword fallback. This is the
        // display half of unresolved alias error types: semantics keep
        // TypeFlags::ANY while the written entity name and arguments
        // survive.
        if flags.intersects(TypeFlags::ANY) {
            let type_of = self.tables.type_of(ty);
            if let (Some(alias_symbol), alias_arguments) =
                (type_of.alias_symbol, type_of.alias_type_arguments.clone())
            {
                let name = if fully_qualified
                    || self
                        .get_check_flags(alias_symbol)
                        .intersects(CheckFlags::UNRESOLVED)
                {
                    self.get_fully_qualified_name(alias_symbol)
                } else {
                    self.symbol_display_name(alias_symbol)
                };
                return match alias_arguments {
                    Some(arguments) if !arguments.is_empty() => {
                        // The Any+alias arm uses
                        // symbolToEntityNameNode, not symbolToTypeNode:
                        // it charges only mapToTypeNodes' per-argument
                        // units, never the alias name.
                        let rendered = self
                            .map_to_type_string_nodes_slice(
                                &arguments,
                                fully_qualified,
                                /*is_bare_list*/ false,
                            )?
                            .into_iter()
                            .map(|(text, _)| text)
                            .collect::<Vec<_>>();
                        Ok((
                            format!("{name}<{}>", rendered.join(", ")),
                            SliceTypeNodeKind::Reference,
                        ))
                    }
                    _ => Ok((name, SliceTypeNodeKind::Reference)),
                };
            }
        }
        if flags.intersects(TypeFlags::TYPE_PARAMETER) {
            // isThisTypeParameter (51454-51463): the synthesized
            // thisType renders the ThisTypeNode face — `this`, never
            // the symbol name (the InObjectTypeLiteral
            // inaccessible-this tracking is declaration-emit band; the
            // error path has no tracker). No parenthesizer rule lists
            // the ThisType kind — it joins like a keyword.
            if matches!(
                self.tables.type_of(ty).data,
                TypeData::TypeParameter {
                    is_this_type: true,
                    ..
                }
            ) {
                return Ok(("this".to_owned(), SliceTypeNodeKind::Keyword));
            }
            // typeToTypeNodeWorker 51496-51512: an infer parameter is
            // a declaration only inside its conditional root's
            // extends type. The surrounding conditional arm installs
            // that root-scoped context and restores it before either
            // result branch is rendered.
            if self.slice_infer_type_parameters.contains(&ty) {
                let symbol = self
                    .tables
                    .type_of(ty)
                    .symbol
                    .expect("infer type parameters carry declaration symbols");
                let name = self.symbol_display_name(symbol);
                self.slice_add_approximate_length(Self::slice_js_length(&name) + 6);

                let constraint_text = match self.get_constraint_of_type_parameter(ty)? {
                    Some(constraint) => {
                        let inferred_constraint =
                            self.get_inferred_type_parameter_constraint(ty, true)?;
                        let is_inferred_constraint = match inferred_constraint {
                            Some(inferred_constraint) => {
                                self.is_type_identical_to(constraint, inferred_constraint)?
                            }
                            None => false,
                        };
                        if is_inferred_constraint {
                            None
                        } else {
                            self.slice_add_approximate_length(9);
                            Some(
                                self.type_to_string_slice_node(constraint, fully_qualified)?
                                    .0,
                            )
                        }
                    }
                    None => None,
                };
                let text = match constraint_text {
                    Some(constraint) => format!("infer {name} extends {constraint}"),
                    None => format!("infer {name}"),
                };
                return Ok((text, SliceTypeNodeKind::Infer));
            }
            return Ok((
                match self.tables.type_of(ty).symbol {
                    Some(symbol) => self.symbol_type_face_slice(symbol, fully_qualified)?.0,
                    None => "?".to_owned(),
                },
                SliceTypeNodeKind::Reference,
            ));
        }
        // typeToTypeNodeWorker 51495-51540 sends Reference objects
        // through typeReferenceToTypeNode BEFORE the later
        // ClassOrInterface symbol head.  A generic class/interface
        // target is both flags, and its self-reference must therefore
        // render its resolved type-parameter arguments (`C<T>`), not
        // stop at the bare symbol (`C`).  Only thisless, non-reference
        // interfaces reach the later symbol-only arm.
        //
        // Anonymous value sides (class statics, enum/value modules,
        // and mixin statics) must continue to createAnonymousTypeNode:
        // that later gate decides between `typeof X` and structural
        // expansion using getBaseTypeVariableOfClass.
        if flags.intersects(TypeFlags::OBJECT)
            && self
                .tables
                .object_flags_of(ty)
                .intersects(ObjectFlags::CLASS_OR_INTERFACE)
            && !self
                .tables
                .object_flags_of(ty)
                .intersects(ObjectFlags::REFERENCE)
        {
            if let Some(symbol) = self.tables.type_of(ty).symbol {
                return self.symbol_type_face_slice(symbol, fully_qualified);
            }
        }
        // tsc-port: typeToTypeNodeHelper @6.0.3 (the EnumLike arm)
        // tsc-hash: 22c6a7f005d1933da0b85f6ceb4faa654d8a927f8cbb782359104a7f3ff37a1a
        // tsc-span: _tsc.js:51367-51399
        //
        // EnumLike precedes the literal arms: enum-member literal
        // types print `E.A` (or the bare enum name when the member
        // type IS the declared type — the single-member collapse,
        // 51371), and the EnumLiteral-stamped declared union prints
        // `E` here BEFORE the union walk (the formatUnionTypes
        // collapse hands the declared union back — without this arm
        // it would re-enter the walk unboundedly). shouldExpandType
        // (51394) is verbosity-walk machinery the error-display slice
        // never enables; the non-identifier member face renders as a
        // `typeof E["..."]` indexed access — out of slice.
        if flags.intersects(TypeFlags::ENUM_LIKE) {
            let symbol = self
                .tables
                .type_of(ty)
                .symbol
                .expect("EnumLike types carry their declaration symbol");
            if self
                .binder
                .symbol(symbol)
                .flags
                .intersects(tsc_types::SymbolFlags::ENUM_MEMBER)
            {
                let parent = self
                    .get_parent_of_symbol(symbol)
                    .expect("enum members carry their enum parent");
                let (parent_name, parent_kind) =
                    self.symbol_type_face_slice(parent, fully_qualified)?;
                if self.get_declared_type_of_symbol_slice(parent)? == ty {
                    return Ok((parent_name, SliceTypeNodeKind::Reference));
                }
                let member_name = self.symbol_display_name(symbol);
                if tsc_syntax::is_identifier_text(&member_name) {
                    return Ok((
                        format!("{parent_name}.{member_name}"),
                        SliceTypeNodeKind::Reference,
                    ));
                }
                let member = string_literal_name_slice(&member_name, false)?;
                return match parent_kind {
                    SliceTypeNodeKind::ImportType => Ok((
                        format!("typeof {parent_name}[{member}]"),
                        SliceTypeNodeKind::IndexedAccess,
                    )),
                    SliceTypeNodeKind::Reference => Ok((
                        format!("(typeof {parent_name})[{member}]"),
                        SliceTypeNodeKind::IndexedAccess,
                    )),
                    _ => unreachable!(
                        "symbolToTypeNode returned a non-reference/import enum parent face"
                    ),
                };
            }
            return self.symbol_type_face_slice(symbol, fully_qualified);
        }
        match self.tables.type_of(ty).data.clone() {
            TypeData::Intrinsic { name, .. } => {
                // tsc-port: typeToTypeNodeWorker @6.0.3
                // tsc-span: _tsc.js:51314-51330
                //
                // The Any arm precedes the generic intrinsic arm:
                // implementation-only intrinsic names such as
                // `error` and `unresolved` therefore print through
                // the AnyKeyword face. The single `intrinsic`
                // marker used by string-mapping declarations keeps
                // its dedicated IntrinsicKeyword face.
                let text: &str = if flags.intersects(TypeFlags::ANY) {
                    self.slice_add_approximate_length(3);
                    if ty == self.tables.intrinsics.intrinsic_marker {
                        "intrinsic"
                    } else {
                        "any"
                    }
                } else if flags.intersects(TypeFlags::UNKNOWN) {
                    name
                } else if flags
                    .intersects(TypeFlags::STRING | TypeFlags::NUMBER | TypeFlags::BIG_INT)
                {
                    self.slice_add_approximate_length(6);
                    name
                } else if flags.intersects(TypeFlags::BOOLEAN) {
                    self.slice_add_approximate_length(7);
                    name
                } else if flags.intersects(TypeFlags::BOOLEAN_LITERAL) {
                    self.slice_add_approximate_length(Self::slice_js_length(name));
                    name
                } else if flags.intersects(TypeFlags::VOID) {
                    self.slice_add_approximate_length(4);
                    name
                } else if flags.intersects(TypeFlags::UNDEFINED) {
                    self.slice_add_approximate_length(9);
                    name
                } else if flags.intersects(TypeFlags::NULL) {
                    self.slice_add_approximate_length(4);
                    name
                } else if flags.intersects(TypeFlags::NEVER) {
                    self.slice_add_approximate_length(5);
                    name
                } else if flags.intersects(TypeFlags::ES_SYMBOL | TypeFlags::NON_PRIMITIVE) {
                    self.slice_add_approximate_length(6);
                    name
                } else {
                    name
                };
                Ok((text.to_owned(), SliceTypeNodeKind::Keyword))
            }
            TypeData::Literal { value } => match value {
                tsc_types::LiteralValue::String(text) => {
                    // 51401-51403: the StringLiteral face carries
                    // EmitFlags.NoAsciiEscaping, so getLiteralText
                    // runs escapeString(text, '"') WITHOUT the
                    // non-ASCII pass — `"あ"` prints raw while
                    // `"AB\r\nC"` spells its escapes (oracle-pinned).
                    // Unpaired surrogates are carried losslessly and
                    // spelled as `\uXXXX` at the Rust UTF-8 display
                    // boundary.
                    self.slice_add_approximate_length(text.len() + 2);
                    Ok((
                        format!("\"{}\"", string_literal_type_display_text(&text)),
                        SliceTypeNodeKind::Literal,
                    ))
                }
                tsc_types::LiteralValue::Number(value) => {
                    let text = tsc_types::js_number_to_string(value);
                    self.slice_add_approximate_length(Self::slice_js_length(&text));
                    Ok((text, SliceTypeNodeKind::Literal))
                }
                tsc_types::LiteralValue::BigInt(value) => {
                    // 51409-51411: pseudoBigIntToString plus the
                    // BigIntLiteral printer's `n` suffix. The pseudo
                    // value is already normalized to signed base-10.
                    let text = format!("{}n", value.to_base10_string());
                    self.slice_add_approximate_length(Self::slice_js_length(&text));
                    Ok((text, SliceTypeNodeKind::Literal))
                }
            },
            TypeData::UniqueESSymbol { .. } => {
                // 51417-51428. typeToString's DEFAULT flags include
                // AllowUniqueESSymbolType (50717) — the plain render
                // short-circuits every unique symbol to the OPERATOR
                // face `unique symbol` (probed: accessible locals and
                // type-literal members alike). Only
                // getTypeNameForErrorDisplay REPLACES the defaults
                // with bare UseFullyQualifiedType, unlocking the
                // 51419 accessible-value probe — the Value
                // symbolToTypeNode chain face (`typeof
                // Symbol.toPrimitive` / `typeof A.B.tp`; a
                // nested-literal member with no accessible chain
                // collapses to [symbol] → bare `typeof tp` — all
                // oracle-probed). reportRelationError reaches the FQ
                // flavor through its GENERALIZED render:
                // getBaseTypeOfLiteralType passes unique symbols
                // through unchanged.
                if fully_qualified {
                    let symbol = self
                        .tables
                        .type_of(ty)
                        .symbol
                        .expect("unique symbols carry their declaration symbol");
                    self.slice_add_approximate_length(6);
                    self.symbol_value_face_slice(symbol, true)
                } else {
                    self.slice_add_approximate_length(13);
                    Ok(("unique symbol".to_owned(), SliceTypeNodeKind::TypeOperator))
                }
            }
            _ => self.type_to_string_slice_structured(ty, fully_qualified),
        }
    }

    fn type_to_string_slice_structured(
        &mut self,
        ty: TypeId,
        fully_qualified: bool,
    ) -> CheckResult<(String, SliceTypeNodeKind)> {
        let type_of = self.tables.type_of(ty);
        if let (Some(alias_symbol), alias_arguments) =
            (type_of.alias_symbol, type_of.alias_type_arguments.clone())
        {
            return match alias_arguments {
                Some(arguments) if !arguments.is_empty() => {
                    // Type-argument lists never parenthesize in the
                    // slice (parenthesizeOrdinalTypeArgument wraps only
                    // a LEADING function/constructor head, 20607-20612
                    // — not a producible child).
                    //
                    // 51476-51477: the global Array symbol can also
                    // arrive through the alias head. It receives the
                    // same ArrayTypeNode sugar as the reference arm;
                    // name-based matching would incorrectly sugar a
                    // shadowing local alias, so retain symbol identity.
                    let mut rendered_nodes = self.map_to_type_string_nodes_slice(
                        &arguments,
                        fully_qualified,
                        /*is_bare_list*/ false,
                    )?;
                    if arguments.len() == 1
                        && self.binder.symbol(alias_symbol).escaped_name == "Array"
                        && self.get_global_type_symbol("Array", /*report_errors*/ false)?
                            == Some(alias_symbol)
                    {
                        let (element, kind) = rendered_nodes
                            .pop()
                            .expect("one alias argument produced one mapToTypeNodes face");
                        return Ok((
                            array_type_node_text(element, kind),
                            SliceTypeNodeKind::Array,
                        ));
                    }
                    let name = self
                        .symbol_type_face_slice(alias_symbol, fully_qualified)?
                        .0;
                    let rendered = rendered_nodes
                        .into_iter()
                        .map(|(text, _)| text)
                        .collect::<Vec<_>>();
                    Ok((
                        format!("{name}<{}>", rendered.join(", ")),
                        SliceTypeNodeKind::Reference,
                    ))
                }
                _ => self.symbol_type_face_slice(alias_symbol, fully_qualified),
            };
        }
        let flags = self.tables.flags_of(ty);
        // typeToTypeNodeHelper's keyword arm precedes the union walk:
        // the interned `true | false` pair carries TypeFlags::BOOLEAN
        // (getUnionType's boolean-pair stamp — tables mirror it) and
        // prints as the keyword, never as its members.
        if flags.intersects(TypeFlags::BOOLEAN) && flags.intersects(TypeFlags::UNION) {
            self.slice_add_approximate_length(7);
            return Ok(("boolean".to_owned(), SliceTypeNodeKind::Keyword));
        }
        if flags.intersects(TypeFlags::UNION | TypeFlags::INTERSECTION) {
            let (mut types, origin) = match &self.tables.type_of(ty).data {
                TypeData::Union { types, origin } => (types.to_vec(), *origin),
                TypeData::Intersection { types } => (types.to_vec(), None),
                _ => unreachable!("union/intersection flag implies composite data"),
            };
            let mut is_union = flags.intersects(TypeFlags::UNION);
            if let Some(origin) = origin {
                // 51542-51544: `type = type.origin` — the denormalized
                // union substitutes its ORIGIN wholesale and falls
                // through THIS arm (never back through the alias/named
                // heads above, so an origin's own alias face cannot
                // apply). Union/intersection origins re-enter the walk
                // with the origin's list (`(A | B) & (C | D)` prints
                // the syntactic shape — the M5/M6-era verdict shield
                // retired with this slice: narrowing landed at M5/M6
                // and the corpus-wide FP=0 + set-ratchet run is the
                // removal proof); keyof origins continue down the
                // substituted helper to the Index arm.
                let origin_flags = self.tables.flags_of(origin);
                if origin_flags.intersects(TypeFlags::UNION | TypeFlags::INTERSECTION) {
                    is_union = origin_flags.intersects(TypeFlags::UNION);
                    types = match &self.tables.type_of(origin).data {
                        TypeData::Union { types, .. } => types.to_vec(),
                        TypeData::Intersection { types } => types.to_vec(),
                        _ => unreachable!("union/intersection flag implies composite data"),
                    };
                } else if origin_flags.intersects(TypeFlags::INDEX) {
                    return self.index_type_to_string_slice_node(origin, fully_qualified);
                } else {
                    // typeToTypeNodeHelper substitutes the origin and
                    // ultimately Debug.fail()s for every other kind.
                    // Union construction only mints composite and keyof
                    // origins.
                    unreachable!("union origin is neither a composite nor keyof distribution");
                }
            }
            let separator = if is_union { " | " } else { " & " };
            // 51546: union member lists format for display before
            // rendering; intersections render their stored order.
            let types = if is_union {
                self.format_union_types(&types)?
            } else {
                types
            };
            // 51547-51548: a single-member list (enum-run collapse,
            // origin lists) renders the member bare with ITS OWN node
            // kind for the enclosing parenthesizer.
            if types.len() == 1 {
                return self.type_to_string_slice_node(types[0], fully_qualified);
            }
            let rendered_nodes = self.map_to_type_string_nodes_slice(
                &types,
                fully_qualified,
                /*is_bare_list*/ true,
            )?;
            let mut rendered = Vec::new();
            for (text, kind) in rendered_nodes {
                let needs_parens = if is_union {
                    union_constituent_needs_parens(kind)
                } else {
                    intersection_constituent_needs_parens(kind)
                };
                rendered.push(if needs_parens {
                    format!("({text})")
                } else {
                    text
                });
            }
            return Ok((
                rendered.join(separator),
                if is_union {
                    SliceTypeNodeKind::Union
                } else {
                    SliceTypeNodeKind::Intersection
                },
            ));
        }
        if self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::MAPPED)
        {
            return self.type_node_from_object_type_slice(ty, fully_qualified);
        }
        if self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::REFERENCE)
        {
            let target = self.tables.reference_target(ty);
            // typeReferenceToTypeNode's tuple arm (51948-51978),
            // checked before the symbol head: tuple targets are the
            // symbol-less references, and the tuple objectFlags test
            // is disjoint from the global-Array sugar identity test,
            // so running it first is unobservable against tsc's
            // dispatch order.
            if let TypeData::TupleTarget(data) = &self.tables.type_of(target).data {
                let element_flags = data.element_flags.clone();
                let labels = data.labeled_element_declarations.clone();
                let readonly = data.readonly;
                // getTypeReferenceArity: length(target.typeParameters).
                let arity = data.type_parameters.len();
                let raw_arguments = self.get_type_arguments(ty)?;
                // 51949: removeMissingType on every OPTIONAL element —
                // the eOPT missing marker never prints.
                let mut arguments = Vec::with_capacity(raw_arguments.len());
                for (i, &argument) in raw_arguments.iter().enumerate() {
                    let optional = element_flags
                        .get(i)
                        .is_some_and(|flags| flags.intersects(ElementFlags::OPTIONAL));
                    arguments.push(self.remove_missing_type(argument, optional));
                }
                // 51950-51952: an empty argument list (and the arity-0
                // slice, whose mapToTypeNodes returns undefined) falls
                // through to the empty-tuple tail; typeToString always
                // runs under IgnoreErrors ⊇ AllowEmptyTuple (50722),
                // so the error-display slice prints `[]` there.
                let argument_count = arguments.len().min(arity);
                let tuple_nodes = self.map_to_type_string_nodes_slice(
                    &arguments[..argument_count],
                    fully_qualified,
                    /*is_bare_list*/ false,
                )?;
                let mut rendered = Vec::with_capacity(tuple_nodes.len());
                for (i, (text, kind)) in tuple_nodes.into_iter().enumerate() {
                    let flags = element_flags[i];
                    let label = labels
                        .as_ref()
                        .and_then(|labels| labels.get(i).copied())
                        .flatten();
                    rendered.push(match label {
                        // 51959-51964 createNamedTupleMember: `...`
                        // for Variable elements, `?` for Optional, the
                        // Rest element type wrapped as an array. The
                        // member type itself never parenthesizes
                        // (factory 22247-22256 applies no rule).
                        Some(label) => {
                            let name = self.tuple_element_label(NodeId(label))?;
                            let dot_dot_dot = if flags.intersects(ElementFlags::VARIABLE) {
                                "..."
                            } else {
                                ""
                            };
                            let question = if flags.intersects(ElementFlags::OPTIONAL) {
                                "?"
                            } else {
                                ""
                            };
                            let member = if flags.intersects(ElementFlags::REST) {
                                array_type_node_text(text, kind)
                            } else {
                                text
                            };
                            format!("{dot_dot_dot}{name}{question}: {member}")
                        }
                        // 51966: RestTypeNode (`...T[]` for Rest,
                        // `...T` for Variadic — createRestTypeNode
                        // applies no parenthesizer) ‖ OptionalTypeNode
                        // (`T?`, postfix-parenthesized) ‖ the bare
                        // element.
                        None => {
                            if flags.intersects(ElementFlags::VARIABLE) {
                                let member = if flags.intersects(ElementFlags::REST) {
                                    array_type_node_text(text, kind)
                                } else {
                                    text
                                };
                                format!("...{member}")
                            } else if flags.intersects(ElementFlags::OPTIONAL) {
                                let member = if optional_type_operand_needs_parens(kind) {
                                    format!("({text})")
                                } else {
                                    text
                                };
                                format!("{member}?")
                            } else {
                                text
                            }
                        }
                    });
                }
                // SingleLine TupleTypeNode emission `[a, b]`;
                // 51970/51975 wrap readonly targets in the readonly
                // TypeOperator (a tuple operand never parenthesizes,
                // 20570-20576).
                let tuple = if rendered.is_empty() {
                    "[]".to_owned()
                } else {
                    format!("[{}]", rendered.join(", "))
                };
                return Ok(if readonly {
                    (format!("readonly {tuple}"), SliceTypeNodeKind::TypeOperator)
                } else {
                    (tuple, SliceTypeNodeKind::Tuple)
                });
            }
            let symbol = self
                .tables
                .type_of(target)
                .symbol
                .expect("non-tuple reference targets carry a generic symbol");
            let arguments = self.get_type_arguments(ty)?;
            // typeReferenceToTypeNode's array sugar: references to the
            // global Array/ReadonlyArray print as element sugar (the
            // identity probe is against the target globals, before the
            // ordinary symbol head. Member-resolution can append the
            // reference's `this` argument, so only the first semantic
            // type argument is consumed here — tsc does not require
            // `typeArguments.length === 1`.
            let array_name_kind = match self.binder.symbol(symbol).escaped_name.as_str() {
                "Array" => Some(false),
                "ReadonlyArray" => Some(true),
                _ => None,
            };
            let array_kind = match array_name_kind {
                Some(false)
                    if self.get_global_type_symbol("Array", /*report_errors*/ false)?
                        == Some(symbol) =>
                {
                    Some(false)
                }
                Some(true)
                    if self.get_global_type_symbol(
                        "ReadonlyArray",
                        /*report_errors*/ false,
                    )? == Some(symbol) =>
                {
                    Some(true)
                }
                _ => None,
            };
            if let (Some(readonly), Some(&element_type)) = (array_kind, arguments.first()) {
                let (element, kind) =
                    self.type_to_string_slice_node(element_type, fully_qualified)?;
                // 51945-51947: ArrayTypeNode (postfix-parenthesized
                // element) + the readonly TypeOperator for
                // ReadonlyArray (an array operand never parenthesizes,
                // 20570-20576).
                let array = array_type_node_text(element, kind);
                return Ok(if readonly {
                    (format!("readonly {array}"), SliceTypeNodeKind::TypeOperator)
                } else {
                    (array, SliceTypeNodeKind::Array)
                });
            }
            let (type_parameters, outer_type_parameter_count) =
                match &self.tables.type_of(target).data {
                    TypeData::GenericType {
                        type_parameters,
                        outer_type_parameter_count,
                        ..
                    } => (type_parameters.to_vec(), *outer_type_parameter_count),
                    _ => {
                        unreachable!("reference targets are GenericType or symbol-less TupleTarget")
                    }
                };
            let mut outer_reference: Option<String> = None;
            let mut argument_start = 0;
            while argument_start < outer_type_parameter_count {
                // TypeScript 6.0.3 passes this absent JSDoc-template
                // parent into lookupSymbolChainWorker and crashes.
                // Preserve that oracle behavior as typed control flow:
                // do not invent a class parent or a non-tsc display
                // face.
                let Some(parent) = self
                    .parent_symbol_of_type_parameter_slice(type_parameters[argument_start])
                    .filter(|_| arguments.len() >= outer_type_parameter_count)
                else {
                    return Err(CheckAbort::OracleCrash(
                        OracleCrashKind::OuterJsdocTemplateReferenceDisplay,
                    ));
                };
                let group_start = argument_start;
                argument_start += 1;
                while argument_start < outer_type_parameter_count
                    && self.parent_symbol_of_type_parameter_slice(type_parameters[argument_start])
                        == Some(parent)
                {
                    argument_start += 1;
                }
                let argument_group = &arguments[group_start..argument_start];
                if argument_group
                    .iter()
                    .copied()
                    .ne(type_parameters[group_start..argument_start].iter().copied())
                {
                    let rendered = self
                        .map_to_type_string_nodes_slice(
                            argument_group,
                            fully_qualified,
                            /*is_bare_list*/ false,
                        )?
                        .into_iter()
                        .map(|(text, _)| text)
                        .collect::<Vec<_>>();
                    let parent_name =
                        self.type_reference_symbol_name_slice(parent, fully_qualified)?;
                    let reference = format!("{parent_name}<{}>", rendered.join(", "));
                    outer_reference = Some(match outer_reference {
                        Some(root) => format!("{root}.{reference}"),
                        None => reference,
                    });
                }
            }
            let mut type_parameter_count = type_parameters.len().min(arguments.len());
            // 52009-52033: only the four iterable protocol types omit
            // a trailing run of arguments identical to their declared
            // defaults. Stop at the first absent/non-identical default;
            // Generator and every other generic are non-firing
            // siblings even when their parameter defaults are equal.
            if self.should_elide_iterable_default_arguments_slice(ty, type_parameter_count)? {
                while type_parameter_count > outer_type_parameter_count {
                    let argument = arguments[type_parameter_count - 1];
                    let parameter = type_parameters[type_parameter_count - 1];
                    let Some(default_type) = self.get_default_from_type_parameter(parameter)?
                    else {
                        break;
                    };
                    if !self.is_type_identical_to(argument, default_type)? {
                        break;
                    }
                    type_parameter_count -= 1;
                }
            }
            let argument_end = type_parameter_count.min(arguments.len());
            let argument_start = outer_type_parameter_count.min(argument_end);
            let rendered = self
                .map_to_type_string_nodes_slice(
                    &arguments[argument_start..argument_end],
                    fully_qualified,
                    /*is_bare_list*/ false,
                )?
                .into_iter()
                .map(|(text, _)| text)
                .collect::<Vec<_>>();
            let name = self.type_reference_symbol_name_slice(symbol, fully_qualified)?;
            let reference = if rendered.is_empty() {
                name
            } else {
                format!("{name}<{}>", rendered.join(", "))
            };
            return Ok((
                match outer_reference {
                    Some(root) => format!("{root}.{reference}"),
                    None => reference,
                },
                SliceTypeNodeKind::Reference,
            ));
        }
        if flags.intersects(TypeFlags::OBJECT)
            && self
                .tables
                .object_flags_of(ty)
                .intersects(ObjectFlags::ANONYMOUS)
        {
            return self.anonymous_object_type_to_string_slice(ty, fully_qualified);
        }
        if flags.intersects(TypeFlags::INDEX) {
            return self.index_type_to_string_slice_node(ty, fully_qualified);
        }
        // tsc-port: typeToTypeNodeHelper @6.0.3 (the TemplateLiteral arm)
        // tsc-hash: 6493ff308f2472547a5845eab6a4caf09dac56f0b3916dd3f5029ab1e4fa1ef7
        // tsc-span: _tsc.js:51575-51587
        //
        // createTemplateHead/Middle/Tail carry the COOKED texts; the
        // printer re-derives rawText per getLiteralText's synthesized
        // branch (template_text_raw below). Span types join bare —
        // createTemplateLiteralTypeSpan applies no parenthesizer rule
        // (22120-22126).
        if flags.intersects(TypeFlags::TEMPLATE_LITERAL) {
            let (texts, types) = match &self.tables.type_of(ty).data {
                TypeData::TemplateLiteral { texts, types } => (texts.clone(), types.clone()),
                _ => unreachable!("TEMPLATE_LITERAL flag implies TemplateLiteral data"),
            };
            let mut out = String::from("`");
            out.push_str(&template_text_utf16_raw(texts[0].units()));
            for (i, &span_type) in types.iter().enumerate() {
                out.push_str("${");
                let (text, _) = self.type_to_string_slice_node(span_type, fully_qualified)?;
                out.push_str(&text);
                out.push('}');
                out.push_str(&template_text_utf16_raw(texts[i + 1].units()));
            }
            out.push('`');
            self.slice_add_approximate_length(2);
            return Ok((out, SliceTypeNodeKind::TemplateLiteral));
        }
        // tsc-port: typeToTypeNodeHelper @6.0.3 (the StringMapping arm)
        // tsc-hash: 291aa6e7b9a0b30d8b3c92b1db3553639c530d4bef608fe985d2d89944b52aa6
        // tsc-span: _tsc.js:51588-51591
        //
        // symbolToTypeNode under the Type meaning with one type
        // argument — the intrinsic alias reference `Uppercase<T>`.
        // Type::symbol is set at creation for every string mapping
        // (createStringMappingType); the guard is a constructibility
        // gate, not a reachable face. The argument never wraps
        // (parenthesizeOrdinalTypeArgument's leading arm needs a
        // type-parametered function head, unconstructible under the
        // `S extends string` operand constraint).
        if flags.intersects(TypeFlags::STRING_MAPPING) {
            let inner = match self.tables.type_of(ty).data {
                TypeData::StringMapping { ty: inner } => inner,
                _ => unreachable!("STRING_MAPPING flag implies StringMapping data"),
            };
            let symbol = self
                .tables
                .type_of(ty)
                .symbol
                .expect("string-mapping types carry their intrinsic alias symbol");
            let (argument, _) = self.type_to_string_slice_node(inner, fully_qualified)?;
            let name = self.symbol_type_face_slice(symbol, fully_qualified)?.0;
            return Ok((format!("{name}<{argument}>"), SliceTypeNodeKind::Reference));
        }
        // tsc-port: typeToTypeNodeHelper @6.0.3 (the IndexedAccess arm)
        // tsc-hash: 68490ac5787a8d01645877e13a6f9c108604b8806d7168e7566adb32f942c760
        // tsc-span: _tsc.js:51592-51597
        //
        // createIndexedAccessTypeNode parenthesizes the OBJECT side
        // only (parenthesizeNonArrayTypeOfPostfixType, 22372-22378);
        // the index side joins bare — oracle: `(keyof T)[K]` /
        // `(T | U)[K]` vs `T[keyof T]` / `T[K][K2]`.
        if flags.intersects(TypeFlags::INDEXED_ACCESS) {
            let (object_type, index_type) = match self.tables.type_of(ty).data {
                TypeData::IndexedAccess {
                    object_type,
                    index_type,
                    ..
                } => (object_type, index_type),
                _ => unreachable!("INDEXED_ACCESS flag implies IndexedAccess data"),
            };
            let (object_text, object_kind) =
                self.type_to_string_slice_node(object_type, fully_qualified)?;
            let object = if non_array_postfix_operand_needs_parens(object_kind) {
                format!("({object_text})")
            } else {
                object_text
            };
            let (index_text, _) = self.type_to_string_slice_node(index_type, fully_qualified)?;
            self.slice_add_approximate_length(2);
            return Ok((
                format!("{object}[{index_text}]"),
                SliceTypeNodeKind::IndexedAccess,
            ));
        }
        if flags.intersects(TypeFlags::CONDITIONAL) {
            let TypeData::Conditional(data) = self.tables.type_of(ty).data.clone() else {
                unreachable!("CONDITIONAL flag implies Conditional data");
            };
            let (check_text, check_kind) =
                self.type_to_string_slice_node(data.check_type, fully_qualified)?;
            let check = if conditional_check_type_needs_parens(check_kind) {
                format!("({check_text})")
            } else {
                check_text
            };
            self.slice_add_approximate_length(15);
            // conditionalTypeToTypeNode 51642-51645: only the
            // extends branch sees this conditional root's infer
            // parameters as declarations. Nested conditional renders
            // replace this vector and restore the outer context.
            let infer_type_parameters = self
                .tables
                .conditional_root(data.root)
                .infer_type_parameters
                .to_vec();
            let saved_infer_type_parameters =
                std::mem::replace(&mut self.slice_infer_type_parameters, infer_type_parameters);
            let extends_result = self.type_to_string_slice_node(data.extends_type, fully_qualified);
            self.slice_infer_type_parameters = saved_infer_type_parameters;
            let (extends_text, extends_kind) = extends_result?;
            let extends = if extends_kind == SliceTypeNodeKind::Conditional {
                format!("({extends_text})")
            } else {
                extends_text
            };
            let true_type = self.get_true_type_from_conditional_type(ty)?;
            let false_type = self.get_false_type_from_conditional_type(ty)?;
            let (true_text, _) = self.type_to_string_slice_node(true_type, fully_qualified)?;
            let (false_text, _) = self.type_to_string_slice_node(false_type, fully_qualified)?;
            return Ok((
                format!("{check} extends {extends} ? {true_text} : {false_text}"),
                SliceTypeNodeKind::Conditional,
            ));
        }
        if flags.intersects(TypeFlags::SUBSTITUTION) {
            let TypeData::Substitution(data) = self.tables.type_of(ty).data.clone() else {
                unreachable!("SUBSTITUTION flag implies Substitution data");
            };
            if self.tables.is_no_infer_type(ty) {
                let (argument, _) =
                    self.type_to_string_slice_node(data.base_type, fully_qualified)?;
                if let Some(symbol) = self.get_global_type_symbol("NoInfer", false)? {
                    let name = self.symbol_type_face_slice(symbol, fully_qualified)?.0;
                    return Ok((format!("{name}<{argument}>"), SliceTypeNodeKind::Reference));
                }
            }
            return self.type_to_string_slice_node(data.base_type, fully_qualified);
        }
        unreachable!("typeToTypeNodeHelper exhausted every TypeFlags/TypeData arm")
    }

    /// tsc-port: createMappedTypeNodeFromType @6.0.3
    /// tsc-hash: e98a9761331ba59c7dbe487ff1629d598ffb7f4725e2855177f89b6f824abccc
    /// tsc-span: _tsc.js:51670-51724
    ///
    /// Phase 9.5a admits the declaration-preserving generic mapped face
    /// for every constructible mapped object. The modifier-preserving
    /// wrapper and non-homomorphic instantiated rewrites remain owned by
    /// 9.5b, which is also the first producer of instantiated payloads.
    fn mapped_type_to_string_slice_node(
        &mut self,
        ty: TypeId,
        fully_qualified: bool,
    ) -> CheckResult<(String, SliceTypeNodeKind)> {
        let declaration = self.mapped_type_declaration(ty);
        let NodeData::MappedType(data) = self.data_of(declaration) else {
            unreachable!("mapped payload declaration has MappedType syntax kind");
        };
        let readonly_token = data.readonly_token;
        let question_token = data.question_token;
        let has_name_type = data.name_type.is_some();

        let type_parameter = self.get_type_parameter_from_mapped_type(ty)?;
        let parameter_name = self
            .tables
            .type_of(type_parameter)
            .symbol
            .map(|symbol| self.symbol_display_name(symbol))
            .unwrap_or_else(|| "?".to_owned());
        let constraint = self.get_constraint_type_from_mapped_type(ty)?;
        let constraint = self.type_to_string_slice_ex(constraint, fully_qualified)?;
        let name_type = if has_name_type {
            match self.get_name_type_from_mapped_type(ty)? {
                Some(name_type) => Some(self.type_to_string_slice_ex(name_type, fully_qualified)?),
                None => None,
            }
        } else {
            None
        };

        let modifiers = self.get_mapped_type_modifiers(ty);
        let template = self.get_template_type_from_mapped_type(ty)?;
        let template = self.remove_missing_type(
            template,
            modifiers.intersects(tsc_types::MappedTypeModifiers::INCLUDE_OPTIONAL),
        );
        let template = self.type_to_string_slice_ex(template, fully_qualified)?;

        let readonly = match readonly_token.map(|token| self.kind_of(token)) {
            None => "",
            Some(SyntaxKind::ReadonlyKeyword) => "readonly ",
            Some(SyntaxKind::PlusToken) => "+readonly ",
            Some(SyntaxKind::MinusToken) => "-readonly ",
            Some(_) => unreachable!("parser mapped readonly token kinds are closed"),
        };
        let question = match question_token.map(|token| self.kind_of(token)) {
            None => "",
            Some(SyntaxKind::QuestionToken) => "?",
            Some(SyntaxKind::PlusToken) => "+?",
            Some(SyntaxKind::MinusToken) => "-?",
            Some(_) => unreachable!("parser mapped question token kinds are closed"),
        };
        let name = name_type
            .map(|name_type| format!(" as {name_type}"))
            .unwrap_or_default();
        self.slice_add_approximate_length(10);
        Ok((
            format!(
                "{{ {readonly}[{parameter_name} in {constraint}{name}]{question}: {template}; }}"
            ),
            SliceTypeNodeKind::TypeLiteral,
        ))
    }

    /// tsc-port: typeToTypeNodeHelper @6.0.3 (the Index arm)
    /// tsc-hash: 52a79339b35cd929e71042a217573bdcac4282a23ebfc182c37349459e88a6c6
    /// tsc-span: _tsc.js:51569-51574
    ///
    /// createTypeOperatorNode(KeyOfKeyword) parenthesizes the operand
    /// (parenthesizeOperandOfTypeOperator, 22362-22368). Reached both
    /// directly (deferred `keyof T` over a generic operand) and
    /// through the union-origin substitution (51536-51538) — origin
    /// index types share TypeData::Index.
    fn index_type_to_string_slice_node(
        &mut self,
        ty: TypeId,
        fully_qualified: bool,
    ) -> CheckResult<(String, SliceTypeNodeKind)> {
        let inner = match self.tables.type_of(ty).data {
            TypeData::Index { ty: inner, .. } => inner,
            _ => unreachable!("INDEX flag implies Index data"),
        };
        self.slice_add_approximate_length(6);
        let (text, kind) = self.type_to_string_slice_node(inner, fully_qualified)?;
        let operand = if type_operator_operand_needs_parens(kind) {
            format!("({text})")
        } else {
            text
        };
        Ok((format!("keyof {operand}"), SliceTypeNodeKind::TypeOperator))
    }

    /// tsc-port: createAnonymousTypeNode @6.0.3 (structural tail)
    /// tsc-hash: eeb2cbaf6a73cc2d146b87f84abdfc081055559279e2d3e3b98358fa8b71e0e1
    /// tsc-span: _tsc.js:51750-51812
    ///
    /// The slice renders the createTypeNodeFromObjectType tail for
    /// type-literal/object-literal shapes and symbol-less anonymous
    /// types. Every symbol special ahead of that tail — the
    /// instantiation-expression TypeQuery reuse, class/enum/value-
    /// module symbol heads, typeof-function
    /// (shouldWriteTypeOfFunctionSymbol) — renders a symbol reference
    /// or `typeof X` face instead; actual JS constructors take their
    /// exact `isJSConstructor` symbol face here. visitedTypes revisits
    /// reuse a type-literal alias when present and otherwise emit
    /// createElidedInformationPlaceholder.
    fn anonymous_object_type_to_string_slice(
        &mut self,
        ty: TypeId,
        fully_qualified: bool,
    ) -> CheckResult<(String, SliceTypeNodeKind)> {
        // InstantiationExpressionType (51755-51770): a TypeQuery can
        // be reused only while it still resolves to this exact type.
        // Failed partial instantiations rely on that identity check to
        // retain `typeof f<T>` in a surrounding constraint diagnostic;
        // the 2635 error still renders its original expression type,
        // not this filtered result.
        if self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::INSTANTIATION_EXPRESSION_TYPE)
        {
            if let Some(existing) = self.links.ty(ty).deferred_node {
                if self.kind_of(existing) == SyntaxKind::TypeQuery
                    && self.get_type_from_type_node(existing)? == ty
                {
                    if let Some(text) = self.reusable_annotation_node_text_slice(existing)? {
                        return Ok((text, SliceTypeNodeKind::TypeQuery));
                    }
                }
            }
        }
        if let Some(symbol) = self.tables.type_of(ty).symbol {
            let symbol_flags = self.binder.symbol(symbol).flags;
            // 51771: a JS constructor's anonymous object is its VALUE
            // side (the CLASS instance-side cases were intercepted by
            // the named-object arm above), so symbolToTypeNode prints
            // the `typeof C` face. Non-constructor JS functions and
            // methods continue through the structural tail exactly
            // like ordinary FUNCTION/METHOD symbols.
            if self
                .binder
                .symbol(symbol)
                .value_declaration
                .is_some_and(|declaration| self.is_js_constructor(declaration))
            {
                return self.symbol_value_face_slice(symbol, fully_qualified);
            }
            // 51771-51786 symbol routing. Actual ClassOrInterface
            // shapes took the declared-type symbol head upstream;
            // anonymous class statics and enum objects arrive here,
            // including class+ns/enum+ns value sides.
            // Function/method symbols fall THROUGH on their first visit,
            // but a recursive revisit of a nameable non-local function or
            // static method is written as `typeof f`. That second half of
            // shouldWriteTypeOfFunctionSymbol is what makes self-returning
            // signatures finite without erasing the recursive edge to
            // `any`. The isJSConstructor head is handled immediately above.
            if self.should_write_type_of_function_symbol_slice(ty, symbol)? {
                return self.symbol_value_face_slice(symbol, fully_qualified);
            }
            let named_class = symbol_flags.intersects(SymbolFlags::CLASS)
                && self.slice_base_type_variable_of_class(symbol)?.is_none();
            if named_class
                || symbol_flags.intersects(
                    tsc_types::SymbolFlags::REGULAR_ENUM | tsc_types::SymbolFlags::CONST_ENUM,
                )
            {
                // createAnonymousTypeNode 51770-51783 chooses Type
                // meaning only for a class instance side; enum objects
                // and ordinary class statics use Value meaning.
                let class_instance = symbol_flags.intersects(SymbolFlags::CLASS)
                    && (self.get_declared_type_of_symbol_slice(symbol)? == ty
                        || self
                            .tables
                            .object_flags_of(ty)
                            .intersects(ObjectFlags::IS_CLASS_INSTANCE_CLONE));
                return if class_instance {
                    self.symbol_type_face_slice(symbol, fully_qualified)
                } else {
                    self.symbol_value_face_slice(symbol, fully_qualified)
                };
            }
            // The ValueModule half of the 51779 disjunct:
            // symbolToTypeNode under the Value meaning — namespace,
            // external-module and globalThis object faces (a
            // function+namespace merge carries VALUE_MODULE and takes
            // this arm before the FUNCTION admission below, matching
            // tsc's disjunct order). isClassInstanceSide (50771)
            // requires SymbolFlags::CLASS, which cannot reach here,
            // so the meaning is always Value.
            if symbol_flags.intersects(tsc_types::SymbolFlags::VALUE_MODULE) {
                return self.symbol_value_face_slice(symbol, fully_qualified);
            }
            // Every OTHER symbol flavor is tsc's else branch —
            // createAnonymousTypeNode falls through to
            // visitAndTransformType(createTypeNodeFromObjectType)
            // (51786-51788): variable-symbol rest/widening clones and
            // the rest take the structural walk below. The 9.3b3-era
            // allowlist (TYPE_LITERAL|OBJECT_LITERAL|FUNCTION|METHOD)
            // was an over-narrow constructibility guess — the
            // object-rest types carry their VARIABLE symbol
            // (getRestType passes the binding's symbol) and were
            // display-inert behind it. Unreal-member flavors stay
            // protected by the empty-resolution shield and the JS
            // gates, not by symbol-flag allowlisting. (A blanket
            // JSON-declaration curtain here regressed 8 accepted
            // nodeModulesJson rows — direct JSON-literal members bind
            // and render correctly; the arbitrary-extensions
            // declaration-vs-JSON winner is contained at the RESOLVER
            // instead.)
        }
        if self.slice_visited_types.contains(&ty) {
            // 51786-51792: a recursive type-literal alias reuses the
            // alias head; every other revisit emits the ordinary
            // elided-information placeholder.
            if let Some(symbol) = self.tables.type_of(ty).symbol {
                if self
                    .binder
                    .symbol(symbol)
                    .flags
                    .intersects(SymbolFlags::TYPE_LITERAL)
                {
                    if let Some(&declaration) = self.binder.symbol(symbol).declarations.first() {
                        let mut node = self.parent_of(declaration);
                        while node
                            .is_some_and(|node| self.kind_of(node) == SyntaxKind::ParenthesizedType)
                        {
                            node = node.and_then(|node| self.parent_of(node));
                        }
                        if let Some(alias) = node
                            .filter(|&node| self.kind_of(node) == SyntaxKind::TypeAliasDeclaration)
                        {
                            let alias_symbol = self.get_symbol_of_declaration(alias)?;
                            return self.symbol_type_face_slice(alias_symbol, fully_qualified);
                        }
                    }
                }
            }
            return Ok(self.reverse_mapped_elision_placeholder_slice());
        }
        self.slice_visited_types.insert(ty);
        let result = self.type_node_from_object_type_slice(ty, fully_qualified);
        self.slice_visited_types.remove(&ty);
        result
    }

    /// tsc-port: shouldWriteTypeOfFunctionSymbol @6.0.3
    /// tsc-hash: c613afc58096a6ced8cbbaf0463b9eb7009d996d87842cfac380b4d1753d085a
    /// tsc-span: _tsc.js:51799-51809
    ///
    /// `typeToString` does not set UseTypeOfFunction or
    /// UseStructuralFallback, so the live admission is the visited-type
    /// arm. Keeping the declaration-shape predicate intact matters: local
    /// function expressions have no stable value name and must retain the
    /// ordinary elision fallback on recursion.
    fn should_write_type_of_function_symbol_slice(
        &mut self,
        ty: TypeId,
        symbol: SymbolId,
    ) -> CheckResult<bool> {
        if !self.slice_visited_types.contains(&ty) {
            return Ok(false);
        }
        let symbol_flags = self.binder.symbol(symbol).flags;
        let declarations = self.binder.symbol(symbol).declarations.clone();
        let is_static_method_symbol = if symbol_flags.intersects(SymbolFlags::METHOD) {
            let mut admitted = false;
            for declaration in &declarations {
                if self.is_static_element(*declaration)
                    && !self.has_late_bindable_index_signature(*declaration)?
                {
                    admitted = true;
                    break;
                }
            }
            admitted
        } else {
            false
        };
        let is_non_local_function_symbol = symbol_flags.intersects(SymbolFlags::FUNCTION)
            && (self.binder.symbol(symbol).parent.is_some()
                || declarations.iter().any(|&declaration| {
                    self.parent_of(declaration).is_some_and(|parent| {
                        matches!(
                            self.kind_of(parent),
                            SyntaxKind::SourceFile | SyntaxKind::ModuleBlock
                        )
                    })
                }));
        Ok(is_static_method_symbol || is_non_local_function_symbol)
    }

    /// tsc-port: getBaseTypeVariableOfClass @6.0.3
    /// tsc-hash: 4c17d2c29383954876ca8e8b980b1f4ea472d166adcbde14083b75ccfab8bca3
    /// tsc-span: _tsc.js:56804-56807
    ///
    /// createAnonymousTypeNode must structurally expand the static side
    /// of a mixin class. Ordinary classes keep their symbol face.
    fn slice_base_type_variable_of_class(
        &mut self,
        symbol: SymbolId,
    ) -> CheckResult<Option<TypeId>> {
        let class_type = self.get_declared_type_of_class_or_interface(symbol)?;
        let base_constructor = self.get_base_constructor_type_of_class(class_type)?;
        let flags = self.tables.flags_of(base_constructor);
        if flags.intersects(TypeFlags::TYPE_VARIABLE) {
            return Ok(Some(base_constructor));
        }
        if flags.intersects(TypeFlags::INTERSECTION) {
            let TypeData::Intersection { types } =
                self.tables.type_of(base_constructor).data.clone()
            else {
                unreachable!("intersection flag implies intersection data");
            };
            return Ok(types.iter().copied().find(|&ty| {
                self.tables
                    .flags_of(ty)
                    .intersects(TypeFlags::TYPE_VARIABLE)
            }));
        }
        Ok(None)
    }

    /// tsc-port: symbolToTypeNode @6.0.3 (error-path Type/Value slice)
    /// tsc-hash: 352e9c292fbd16c2334897be45253723d19b5f8f522d36cf226ac469b796e919
    /// tsc-span: _tsc.js:53114-53198
    ///
    /// lookupSymbolChainWorker (52943-52958) builds `[symbol]` when
    /// the context has no enclosingDeclaration and
    /// UseFullyQualifiedType is off — the error path always lands
    /// there — so the accessibility walk, lookupTypeParameterNodes
    /// (WriteTypeParametersInQualifiedName-gated) and
    /// createAccessFromSymbolChain's parent/indexed-access arms all
    /// collapse to the single-identifier face. The
    /// UseFullyQualifiedType leg runs getSymbolChain
    /// (symbol_chain_slice below): an external-module ROOT is chain[0]
    /// for the 53117 gate, so the below-root links ride as the
    /// ImportTypeNode's qualifier (createAccessFromSymbolChain with
    /// stopper 1, export-table naming) and the export= short-circuit's
    /// length-1 chain keeps the bare import face. Value meaning adds
    /// `typeof`; Type meaning does not. Other roots render the entity
    /// face over the same chain, with `typeof` only for Value. The
    /// import face's
    /// node16/nodenext resolution-mode attributes (53125-53150)
    /// and /node_modules/ specifier swap (53151-53174) read
    /// impliedNodeFormat, which the port does not model: the swap can
    /// only fire on node_modules fixtures (host-adjudicated band) and
    /// the attributes only change message text under node16 matrices —
    /// recorded T2 residue, row keys unaffected.
    fn symbol_type_face_slice(
        &mut self,
        symbol: SymbolId,
        fully_qualified: bool,
    ) -> CheckResult<(String, SliceTypeNodeKind)> {
        self.symbol_to_type_face_slice(symbol, fully_qualified, tsc_types::SymbolFlags::TYPE)
    }

    fn symbol_value_face_slice(
        &mut self,
        symbol: SymbolId,
        fully_qualified: bool,
    ) -> CheckResult<(String, SliceTypeNodeKind)> {
        self.symbol_to_type_face_slice(symbol, fully_qualified, tsc_types::SymbolFlags::VALUE)
    }

    /// serializeTypeName's context-armed symbolToTypeNode face. Unlike
    /// UseFullyQualifiedType, lookupSymbolChainWorker starts from the
    /// active enclosing declaration and therefore selects the shortest
    /// accessible alias/namespace chain, falling back to an import root
    /// only when no lexical chain names the symbol.
    fn symbol_to_type_face_at_slice(
        &mut self,
        symbol: SymbolId,
        meaning: tsc_types::SymbolFlags,
        enclosing: NodeId,
    ) -> CheckResult<(String, SliceTypeNodeKind)> {
        let chain = self
            .symbol_chain_slice(
                symbol,
                meaning,
                /*end_of_chain*/ true,
                /*yield_module_symbol*/ true,
                Some(enclosing),
            )?
            .expect("getSymbolChain with endOfChain always yields (52991-52999)");
        self.symbol_chain_to_type_face_slice(&chain, meaning, Some(enclosing))
    }

    /// createAccessFromSymbolChain's TypeNode construction over an
    /// already-selected chain (53117-53197).
    fn symbol_chain_to_type_face_slice(
        &mut self,
        chain: &[SymbolId],
        meaning: tsc_types::SymbolFlags,
        enclosing: Option<NodeId>,
    ) -> CheckResult<(String, SliceTypeNodeKind)> {
        let is_type_of = meaning == tsc_types::SymbolFlags::VALUE;
        let root = chain[0];
        if self.symbol_has_external_module_declaration(root) {
            let specifier = match enclosing {
                Some(enclosing) => self.specifier_for_module_symbol_at_slice(root, enclosing)?,
                None => self.specifier_for_module_symbol_slice(root)?,
            };
            let literal = string_literal_name_slice(&specifier, false)?;
            let type_of = if is_type_of { "typeof " } else { "" };
            self.slice_add_approximate_length(Self::slice_js_length(&specifier) + 10);
            if chain.len() == 1 {
                return Ok((
                    format!("{type_of}import({literal})"),
                    SliceTypeNodeKind::ImportType,
                ));
            }
            let mut qualifier = Vec::with_capacity(chain.len() - 1);
            for index in 1..chain.len() {
                let name = self.qualifier_symbol_name_slice(
                    chain[index - 1],
                    chain[index],
                    true,
                    enclosing,
                )?;
                self.slice_add_approximate_length(Self::slice_js_length(&name) + 1);
                qualifier.push(name);
            }
            return Ok((
                format!("{type_of}import({literal}).{}", qualifier.join(".")),
                SliceTypeNodeKind::ImportType,
            ));
        }

        let mut parts = Vec::with_capacity(chain.len());
        let root_name = self.entity_symbol_name_as_written_slice(root, true, true, enclosing);
        self.slice_add_bare_symbol_length(&root_name);
        parts.push(root_name);
        for index in 1..chain.len() {
            let name =
                self.qualifier_symbol_name_slice(chain[index - 1], chain[index], true, enclosing)?;
            self.slice_add_approximate_length(Self::slice_js_length(&name) + 1);
            parts.push(name);
        }
        let text = parts.join(".");
        Ok(if is_type_of {
            (format!("typeof {text}"), SliceTypeNodeKind::TypeQuery)
        } else {
            (text, SliceTypeNodeKind::Reference)
        })
    }

    fn symbol_to_type_face_slice(
        &mut self,
        symbol: SymbolId,
        fully_qualified: bool,
        meaning: tsc_types::SymbolFlags,
    ) -> CheckResult<(String, SliceTypeNodeKind)> {
        let is_type_of = meaning == tsc_types::SymbolFlags::VALUE;
        // 53117: some(chain[0].declarations,
        // hasNonGlobalAugmentationExternalModuleSymbol) routes the
        // import-type face. (For a module symbol the chain is always
        // [symbol]: ambient-module declarations fail the candidates
        // guard (49995 isAmbientModule) and the globals accessibility
        // probe (50329 external-module rejection), and source-file
        // declarations have no node parent — so the head-first check
        // is chain[0]-exact.)
        if self.symbol_has_external_module_declaration(symbol) {
            // 53175-53185: a length-1 chain leaves nonRootParts and
            // typeParameterNodes undefined — the face is the bare
            // ImportTypeNode, with isTypeOf exactly under Value
            // meaning.
            let specifier = self.specifier_for_module_symbol_slice(symbol)?;
            let literal = string_literal_name_slice(&specifier, false)?;
            let type_of = if is_type_of { "typeof " } else { "" };
            self.slice_add_approximate_length(Self::slice_js_length(&specifier) + 10);
            return Ok((
                format!("{type_of}import({literal})"),
                SliceTypeNodeKind::ImportType,
            ));
        }
        // lookupSymbolChainWorker 52945-52957: type parameters are
        // deliberately never expanded into a symbol chain, even when
        // UseFullyQualifiedType is set. This is what keeps two
        // unrelated same-written-name parameters as `T`/`T` for
        // getTypeNamesForErrorDisplay's 2719 branch instead of
        // inventing container-qualified faces such as `I.T`.
        if fully_qualified
            && !self
                .binder
                .symbol(symbol)
                .flags
                .intersects(tsc_types::SymbolFlags::TYPE_PARAMETER)
        {
            // yield_module_symbol TRUE — symbolToTypeNode flavor
            // (53115): the `typeof import("...")` faces REQUIRE module
            // roots.
            let chain = self
                .symbol_chain_slice(symbol, meaning, true, true, None)?
                .expect("getSymbolChain with endOfChain always yields (52991-52999)");
            let root = chain[0];
            if self.symbol_has_external_module_declaration(root) {
                let specifier = self.specifier_for_module_symbol_slice(root)?;
                let literal = string_literal_name_slice(&specifier, false)?;
                let type_of = if is_type_of { "typeof " } else { "" };
                self.slice_add_approximate_length(Self::slice_js_length(&specifier) + 10);
                // 53175-53185: the export= short-circuit (52978-52981)
                // leaves a length-1 chain — the bare ImportTypeNode.
                if chain.len() == 1 {
                    return Ok((
                        format!("{type_of}import({literal})"),
                        SliceTypeNodeKind::ImportType,
                    ));
                }
                let mut qualifier = Vec::with_capacity(chain.len() - 1);
                for index in 1..chain.len() {
                    let name = self.qualifier_symbol_name_slice(
                        chain[index - 1],
                        chain[index],
                        false,
                        self.slice_display_enclosing,
                    )?;
                    self.slice_add_approximate_length(Self::slice_js_length(&name) + 1);
                    qualifier.push(name);
                }
                let qualifier = qualifier.join(".");
                return Ok((
                    format!("{type_of}import({literal}).{qualifier}"),
                    SliceTypeNodeKind::ImportType,
                ));
            }
            // 53186-53197: the entity face over the chain —
            // getNameOfSymbolAsWritten at the root, then export-table
            // naming below it.
            let mut parts = Vec::with_capacity(chain.len());
            let root_name = self.entity_symbol_name_as_written_slice(
                root,
                true,
                false,
                self.slice_display_enclosing,
            );
            self.slice_add_bare_symbol_length(&root_name);
            parts.push(root_name);
            for index in 1..chain.len() {
                let name = self.qualifier_symbol_name_slice(
                    chain[index - 1],
                    chain[index],
                    false,
                    self.slice_display_enclosing,
                )?;
                self.slice_add_approximate_length(Self::slice_js_length(&name) + 1);
                parts.push(name);
            }
            let text = parts.join(".");
            return Ok(if is_type_of {
                (format!("typeof {text}"), SliceTypeNodeKind::TypeQuery)
            } else {
                (text, SliceTypeNodeKind::Reference)
            });
        }
        // 53186-53197 with the [symbol] chain: the bare-name face.
        let name = self.entity_symbol_name_as_written_slice(
            symbol,
            true,
            !fully_qualified,
            self.slice_display_enclosing,
        );
        self.slice_add_bare_symbol_length(&name);
        Ok(if is_type_of {
            (format!("typeof {name}"), SliceTypeNodeKind::TypeQuery)
        } else {
            (name, SliceTypeNodeKind::Reference)
        })
    }

    /// tsc-port: symbolToExpression @6.0.3 (computed-name face slice)
    /// tsc-hash: f1c7de91b82f1b2f5a3b4a2e7c1b82bd8504e06172492e073464b298e0938e03
    /// tsc-span: _tsc.js:53337-53387
    ///
    /// lookupSymbolChainWorker (52943-52958) gates the chain walk on
    /// `context.enclosingDeclaration || UseFullyQualifiedType`; the
    /// unarmed contexts collapse to the `[symbol]` bare-name face.
    /// createExpressionFromSymbolChain renders identifier links as a
    /// property-access join (canUsePropertyAccess, 53357); the
    /// module-specifier string-literal roots (53351-53355) and
    /// element-access faces over non-identifier links (53362-53385)
    /// follow the same quote stripping and synthesized-literal
    /// escaping as the factory. Link names ride
    /// getNameOfSymbolAsWritten.
    fn symbol_expression_face_slice(
        &mut self,
        symbol: SymbolId,
        enclosing: Option<NodeId>,
        fully_qualified: bool,
    ) -> CheckResult<String> {
        let chain = if enclosing.is_some() || fully_qualified {
            // yield_module_symbol FALSE — symbolToExpression passes
            // nothing (53338), including tsc's FQ retry, which still
            // rides this same entry point.
            self.symbol_chain_slice(
                symbol,
                tsc_types::SymbolFlags::VALUE,
                true,
                false,
                enclosing,
            )?
            .expect("getSymbolChain with endOfChain always yields (52991-52999)")
        } else {
            vec![symbol]
        };
        let mut expression = String::new();
        for (index, &link) in chain.iter().enumerate() {
            let mut name = self.entity_symbol_name_as_written_slice(
                link,
                index == 0,
                !fully_qualified,
                enclosing,
            );
            if index == 0 && self.symbol_has_external_module_declaration(link) {
                let specifier = self.specifier_for_module_symbol_slice(link)?;
                expression = string_literal_name_slice(&specifier, false)?;
                continue;
            }
            if index == 0 || can_use_property_access_slice(&name, self.options.emit_script_target())
            {
                expression = if index == 0 {
                    name
                } else {
                    format!("{expression}.{name}")
                };
                continue;
            }
            if name.starts_with('[') && name.ends_with(']') && name.len() >= 2 {
                name = name[1..name.len() - 1].to_owned();
            }
            let first = name.chars().next();
            let argument = if matches!(first, Some('\'') | Some('"'))
                && !self
                    .binder
                    .symbol(link)
                    .flags
                    .intersects(SymbolFlags::ENUM_MEMBER)
            {
                let single_quote = first == Some('\'');
                let literal = strip_symbol_name_quotes_slice(&name);
                string_literal_name_slice(&literal, single_quote)?
            } else {
                let numeric = crate::evaluate::js_string_to_number(&name);
                if tsc_types::js_number_to_string(numeric) == name {
                    tsc_types::js_number_to_string(numeric)
                } else {
                    name
                }
            };
            expression = format!("{expression}[{argument}]");
        }
        Ok(expression)
    }

    /// tsc-port: getSymbolChain @6.0.3 (error-path slice)
    /// tsc-hash: 8ccb0f4b99b34c677210c369edfdf15d1f0cc32eed7f57b6b153783b4808d291
    /// tsc-span: _tsc.js:52958-53016
    ///
    /// lookupSymbolChainWorker's chain builder; the enclosing
    /// declaration arms the accessibility walks (the computed-name
    /// member faces re-enclose at the property declaration,
    /// addPropertyToElementList 52265-52267 / symbolToNode
    /// 51128-51131), while the error-path type faces pass None.
    /// getContainersOfSymbol keeps the no-enclosing view — the typeof
    /// face passes enclosing=None (4563) and the expression path's
    /// module parents suppress at the fallback under the per-caller
    /// rule below, so the enclosing-fed reexportContainers
    /// (getAlternativeContainingModules) stay empty either way.
    /// yieldModuleSymbol is per caller (9.3b5 r2): symbolToTypeNode
    /// passes `!(context.flags & UseAliasDefinedOutsideCurrentScope)`
    /// = TRUE on the error path (53115) — its `typeof import("...")`
    /// faces REQUIRE module roots — while symbolToExpression (53338)
    /// and symbolToName (53316) pass NOTHING = falsy, so the
    /// module-parent suppression (52996-52998) fires there. The
    /// suppression is !endOfChain-guarded: end_of_chain tops still
    /// yield [symbol], keeping the `.expect("always yields")`
    /// contracts at both callers. The TypeLiteral/ObjectLiteral parent
    /// guard (52991-52995) is kept verbatim though the
    /// module/namespace parents this face walks cannot carry those
    /// flags. getQualifiedLeftMeaning (50291) fixes Value → Value, so
    /// the top-level Value meaning rides the whole recursion.
    // h2-7a-m-3 widening: decision-only NodeBuilder reuse anchor.
    pub(crate) fn symbol_chain_slice(
        &mut self,
        symbol: SymbolId,
        meaning: tsc_types::SymbolFlags,
        end_of_chain: bool,
        yield_module_symbol: bool,
        enclosing: Option<NodeId>,
    ) -> CheckResult<Option<Vec<SymbolId>>> {
        let mut accessible = self.accessible_symbol_chain_at_slice(symbol, meaning, enclosing)?;
        let needs_walk = match &accessible {
            None => true,
            Some(chain) => {
                let link_meaning = if chain.len() == 1 {
                    meaning
                } else {
                    Self::qualified_left_meaning(meaning)
                };
                self.needs_qualification_slice(chain[0], link_meaning, enclosing)?
            }
        };
        if needs_walk {
            let walk_from = accessible.as_ref().map_or(symbol, |chain| chain[0]);
            let parents = self.containers_of_symbol_slice(walk_from, enclosing, meaning)?;
            if !parents.is_empty() {
                // 52964-52969: parents sort by specifier shape
                // (sortByBestName) — module parents key their
                // specifier, namespace parents ride as ties (the
                // missing-specifier `return 0`, 53014).
                let mut specifiers: Vec<Option<String>> = Vec::with_capacity(parents.len());
                for &parent in &parents {
                    if self.symbol_has_external_module_declaration(parent) {
                        match self.specifier_for_module_symbol_slice(parent) {
                            Ok(specifier) => specifiers.push(Some(specifier)),
                            // tsc always produces a specifier; a
                            // curtained one can only misorder a
                            // MULTI-parent sort.
                            Err(abort) => {
                                if parents.len() > 1 {
                                    return Err(abort);
                                }
                                specifiers.push(None);
                            }
                        }
                    } else {
                        specifiers.push(None);
                    }
                }
                let mut order: Vec<usize> = (0..parents.len()).collect();
                order.sort_by(|&a, &b| match (&specifiers[a], &specifiers[b]) {
                    // pathIsRelative (5314) is false for both the
                    // host-rooted absolute paths and ambient names
                    // (standalone relative ambient modules are a 2436
                    // parse-band reject), so sortByBestName reduces to
                    // countPathComponents (45645): the separator
                    // count.
                    (Some(a), Some(b)) => {
                        let count = |s: &str| s.bytes().filter(|&byte| byte == b'/').count();
                        count(a).cmp(&count(b))
                    }
                    _ => std::cmp::Ordering::Equal,
                });
                for index in order {
                    let parent = parents[index];
                    let Some(parent_chain) = self.symbol_chain_slice(
                        parent,
                        Self::qualified_left_meaning(meaning),
                        false,
                        yield_module_symbol,
                        enclosing,
                    )?
                    else {
                        continue;
                    };
                    // 52978-52981: an export= parent whose target IS
                    // the symbol renders as the bare parent chain.
                    let export_equals = self
                        .binder
                        .symbol(parent)
                        .exports
                        .get(tsc_types::InternalSymbolName::EXPORT_EQUALS)
                        .copied();
                    if let Some(export_equals) = export_equals {
                        if self.symbol_if_same_reference_slice(export_equals, symbol)? {
                            accessible = Some(parent_chain);
                            break;
                        }
                    }
                    // 52982: parentChain.concat(accessibleSymbolChain
                    // || [getAliasForSymbolInContainer(parent, symbol)
                    // || symbol]).
                    let mut chain = parent_chain;
                    match accessible.take() {
                        Some(tail) => chain.extend(tail),
                        None => {
                            let alias = self.alias_for_symbol_in_container_slice(parent, symbol)?;
                            chain.push(alias.unwrap_or(symbol));
                        }
                    }
                    accessible = Some(chain);
                    break;
                }
            }
        }
        if accessible.is_some() {
            return Ok(accessible);
        }
        if end_of_chain
            || !self.binder.symbol(symbol).flags.intersects(
                tsc_types::SymbolFlags::TYPE_LITERAL | tsc_types::SymbolFlags::OBJECT_LITERAL,
            )
        {
            // 52996-52998: a module PARENT dies on the falsy-
            // yieldModuleSymbol paths — `x`, never `"./mod".x` — and
            // the outer end_of_chain fallback yields the bare tail.
            if !end_of_chain
                && !yield_module_symbol
                && self.symbol_has_external_module_declaration(symbol)
            {
                return Ok(None);
            }
            return Ok(Some(vec![symbol]));
        }
        Ok(None)
    }

    /// tsc-port: getQualifiedLeftMeaning @6.0.3
    /// tsc-hash: c3a93b2efde3a16cc56ac39c4a7d91e7bd2297ad3c569c10077e18e1f20f63f9
    /// tsc-span: _tsc.js:50291-50293
    fn qualified_left_meaning(meaning: tsc_types::SymbolFlags) -> tsc_types::SymbolFlags {
        if meaning == tsc_types::SymbolFlags::VALUE {
            tsc_types::SymbolFlags::VALUE
        } else {
            tsc_types::SymbolFlags::NAMESPACE
        }
    }

    /// tsc-port: getAccessibleSymbolChain @6.0.3 (error-path slice)
    /// tsc-hash: 86303c2907e872494ac8075b43923ebdd2dda7e3c0de5261e57930f45c0a8346
    /// tsc-span: _tsc.js:50294-50375
    ///
    /// The scope walk (symbol_tables_in_scope_slice below) consults
    /// each table in lexical order and the shared visited list rides
    /// across them like tsc's per-symbol visitedSymbolTables. The
    /// isPropertyOrMethodDeclarationSymbol guard (50295) cannot match
    /// this face's module/namespace/variable declaration lists, and
    /// the accessibleChainCache is a recomputation-only economy the
    /// slice skips.
    fn accessible_symbol_chain_at_slice(
        &mut self,
        symbol: SymbolId,
        meaning: tsc_types::SymbolFlags,
        enclosing: Option<NodeId>,
    ) -> CheckResult<Option<Vec<SymbolId>>> {
        let tables = self.symbol_tables_in_scope_slice(enclosing);
        let mut visited = Vec::new();
        for (table_key, table, is_local_name_lookup) in tables {
            if let Some(chain) = self.accessible_chain_from_table_slice(
                &table,
                table_key,
                symbol,
                meaning,
                /*ignore_qualification*/ false,
                is_local_name_lookup,
                &mut visited,
                enclosing,
            )? {
                return Ok(Some(chain));
            }
        }
        Ok(None)
    }

    /// tsc-port: forEachSymbolTableInScope @6.0.3 (display slice)
    /// tsc-hash: 00a3e1adb3bf462d0d1b2ab5d4b5872871757447673fc172a4eaa9407eb4541e
    /// tsc-span: _tsc.js:50227-50290
    ///
    /// Ancestor locals tables (any locals-bearing location except the
    /// global source file, 50230-50240), the merged exports view of
    /// module declarations and external/CJS source files on the way up
    /// (50242-50259; getSymbolOfDeclaration's merged table — the same
    /// view resolve_name_full's module-exports arm reads), then the
    /// `globals` tail (50284-50289). The class/interface Type-filtered
    /// members table (50260-50283) is omitted because no current reuse
    /// canary enters this helper from a member-only lookup.
    fn symbol_tables_in_scope_slice(
        &mut self,
        enclosing: Option<NodeId>,
    ) -> Vec<(ScopeTableKey, tsc_binder::SymbolTable, bool)> {
        let mut tables = Vec::new();
        let mut location = enclosing;
        while let Some(loc) = location {
            let is_global_source_file = self.kind_of(loc) == SyntaxKind::SourceFile
                && !self.binder.is_external_or_common_js_module_of_node(loc);
            if !is_global_source_file {
                if let Some(locals) = self.binder.locals_of(loc) {
                    tables.push((
                        ScopeTableKey::Locals(loc),
                        locals.clone(),
                        /*is_local_name_lookup*/ true,
                    ));
                }
            }
            match self.kind_of(loc) {
                SyntaxKind::SourceFile if is_global_source_file => {}
                SyntaxKind::SourceFile | SyntaxKind::ModuleDeclaration => {
                    if let Some(symbol) = self.binder.node_symbol(loc) {
                        let symbol = self.get_merged_symbol(symbol);
                        let exports = self.binder.symbol(symbol).exports.clone();
                        tables.push((
                            ScopeTableKey::Exports(symbol),
                            exports,
                            /*is_local_name_lookup*/ true,
                        ));
                    }
                }
                _ => {}
            }
            location = self.parent_of(loc);
        }
        tables.push((
            ScopeTableKey::Globals,
            self.globals.clone(),
            /*is_local_name_lookup*/ true,
        ));
        tables
    }

    /// getAccessibleSymbolChainFromSymbolTable (50313-50319): the
    /// visited guard is table-object identity in tsc — keyed by
    /// provenance here (ScopeTableKey).
    #[allow(clippy::too_many_arguments)]
    fn accessible_chain_from_table_slice(
        &mut self,
        table: &tsc_binder::SymbolTable,
        table_key: ScopeTableKey,
        symbol: SymbolId,
        meaning: tsc_types::SymbolFlags,
        ignore_qualification: bool,
        is_local_name_lookup: bool,
        visited: &mut Vec<ScopeTableKey>,
        enclosing: Option<NodeId>,
    ) -> CheckResult<Option<Vec<SymbolId>>> {
        if visited.contains(&table_key) {
            return Ok(None);
        }
        visited.push(table_key);
        let result = self.try_symbol_table_slice(
            table,
            table_key,
            symbol,
            meaning,
            ignore_qualification,
            is_local_name_lookup,
            visited,
            enclosing,
        );
        visited.pop();
        result
    }

    /// trySymbolTable (50331-50360): the direct hit, then per entry —
    /// in table order — the alias leg and the exportSymbol arm; an
    /// alias leg that declines (or whose candidate walk misses) falls
    /// through to the arm on the SAME entry before the next entry is
    /// seen, tsc's single forEachEntry pass. If the globals table itself
    /// misses, 50359 retries through globalThisSymbol's exports; that is
    /// what makes a shadowed script-global type name serializable as
    /// `globalThis.A`.
    #[allow(clippy::too_many_arguments)]
    fn try_symbol_table_slice(
        &mut self,
        table: &tsc_binder::SymbolTable,
        table_key: ScopeTableKey,
        symbol: SymbolId,
        meaning: tsc_types::SymbolFlags,
        ignore_qualification: bool,
        is_local_name_lookup: bool,
        visited: &mut Vec<ScopeTableKey>,
        enclosing: Option<NodeId>,
    ) -> CheckResult<Option<Vec<SymbolId>>> {
        let escaped = self.binder.symbol(symbol).escaped_name.clone();
        let direct = table.get(&escaped).copied();
        if self.symbol_chain_is_accessible_slice(
            symbol,
            direct,
            None,
            meaning,
            ignore_qualification,
            enclosing,
        )? {
            return Ok(Some(vec![symbol]));
        }
        for (name, &entry) in table.iter() {
            let alias_leg = self
                .binder
                .symbol(entry)
                .flags
                .intersects(tsc_types::SymbolFlags::ALIAS)
                && name != tsc_types::InternalSymbolName::EXPORT_EQUALS
                && name != tsc_types::InternalSymbolName::DEFAULT
                // The isUMDExportSymbol leg (50341): inside an
                // external module the UMD global alias is excluded —
                // r1 armed `enclosing` (the member faces re-enclose at
                // the property declaration), so the filter is live.
                // The useOnlyExternalAliasing half stays off: the
                // error path passes false (52959).
                && !(self.is_umd_export_symbol(entry)
                    && enclosing
                        .is_some_and(|enclosing| self.binder.is_external_module_of_node(enclosing)))
                // isNamespaceReexportDeclaration (50341): `export * as
                // ns from` — the only grammatical NamespaceExport.
                && !(is_local_name_lookup
                    && self.symbol_has_declaration_of_kind(entry, SyntaxKind::NamespaceExport))
                && (ignore_qualification
                    || !self.symbol_has_declaration_of_kind(entry, SyntaxKind::ExportSpecifier));
            if alias_leg {
                let resolved = self.resolve_alias(entry)?;
                if let Some(chain) = self.candidate_list_for_symbol_slice(
                    entry,
                    resolved,
                    symbol,
                    meaning,
                    ignore_qualification,
                    visited,
                    enclosing,
                )? {
                    return Ok(Some(chain));
                }
            }
            // The exportSymbol arm (50348-50357): a name-matching
            // local whose export slot IS the symbol (the EXPORT_VALUE
            // locals of containers.rs:566-591) yields the bare
            // [symbol] before any LATER entry's alias leg can qualify
            // it (per-entry order, probe C: `[s]`, not `[Self.s]`).
            let export_symbol = {
                let entry_symbol = self.binder.symbol(entry);
                (entry_symbol.escaped_name == escaped)
                    .then_some(entry_symbol.export_symbol)
                    .flatten()
            };
            if let Some(export_symbol) = export_symbol {
                let merged = self.get_merged_symbol(export_symbol);
                if self.symbol_chain_is_accessible_slice(
                    symbol,
                    Some(merged),
                    None,
                    meaning,
                    ignore_qualification,
                    enclosing,
                )? {
                    return Ok(Some(vec![symbol]));
                }
            }
        }
        if table_key == ScopeTableKey::Globals {
            return self.candidate_list_for_symbol_slice(
                self.global_this_symbol,
                self.global_this_symbol,
                symbol,
                meaning,
                ignore_qualification,
                visited,
                enclosing,
            );
        }
        Ok(None)
    }

    /// getCandidateListForSymbol (50361-50374): the alias itself, or
    /// the alias prepended to a chain found in its target's export
    /// table (qualification ignored inside).
    #[allow(clippy::too_many_arguments)]
    fn candidate_list_for_symbol_slice(
        &mut self,
        entry: SymbolId,
        resolved: SymbolId,
        symbol: SymbolId,
        meaning: tsc_types::SymbolFlags,
        ignore_qualification: bool,
        visited: &mut Vec<ScopeTableKey>,
        enclosing: Option<NodeId>,
    ) -> CheckResult<Option<Vec<SymbolId>>> {
        if self.symbol_chain_is_accessible_slice(
            symbol,
            Some(entry),
            Some(resolved),
            meaning,
            ignore_qualification,
            enclosing,
        )? {
            return Ok(Some(vec![entry]));
        }
        let candidate_table = self.get_exports_of_symbol(resolved)?;
        let inner = self.accessible_chain_from_table_slice(
            &candidate_table,
            ScopeTableKey::Exports(resolved),
            symbol,
            meaning,
            /*ignore_qualification*/ true,
            /*is_local_name_lookup*/ false,
            visited,
            enclosing,
        )?;
        if let Some(inner) = inner {
            if self.can_qualify_symbol_slice(
                entry,
                Self::qualified_left_meaning(meaning),
                enclosing,
            )? {
                let mut chain = vec![entry];
                chain.extend(inner);
                return Ok(Some(chain));
            }
        }
        Ok(None)
    }

    /// isAccessible (50325-50330): identity (raw or merged) against
    /// the alias-resolved view, the external-module rejection, then
    /// qualifiability.
    fn symbol_chain_is_accessible_slice(
        &mut self,
        symbol: SymbolId,
        entry: Option<SymbolId>,
        resolved: Option<SymbolId>,
        meaning: tsc_types::SymbolFlags,
        ignore_qualification: bool,
        enclosing: Option<NodeId>,
    ) -> CheckResult<bool> {
        let Some(entry) = entry else {
            return Ok(false);
        };
        let respect = resolved.unwrap_or(entry);
        if symbol != respect && self.get_merged_symbol(symbol) != self.get_merged_symbol(respect) {
            return Ok(false);
        }
        if self.symbol_has_external_module_declaration(entry) {
            return Ok(false);
        }
        if ignore_qualification {
            return Ok(true);
        }
        let merged_entry = self.get_merged_symbol(entry);
        self.can_qualify_symbol_slice(merged_entry, meaning, enclosing)
    }

    /// canQualifySymbol (50321-50324): no qualification needed, or the
    /// parent chain is itself accessible (from the same enclosing).
    fn can_qualify_symbol_slice(
        &mut self,
        entry: SymbolId,
        meaning: tsc_types::SymbolFlags,
        enclosing: Option<NodeId>,
    ) -> CheckResult<bool> {
        if !self.needs_qualification_slice(entry, meaning, enclosing)? {
            return Ok(true);
        }
        let Some(parent) = self.get_parent_of_symbol(entry) else {
            return Ok(false);
        };
        Ok(self
            .accessible_symbol_chain_at_slice(
                parent,
                Self::qualified_left_meaning(meaning),
                enclosing,
            )?
            .is_some())
    }

    /// tsc-port: needsQualification @6.0.3 (error-path slice)
    /// tsc-hash: 1bde4c0406bef43d2e90c293732295ec0503439a0784611675ed408d4cd0141d
    /// tsc-span: _tsc.js:50376-50396
    ///
    /// The scope walk decides at the FIRST table containing the name:
    /// the symbol itself ⇒ no qualification; a shadowing slot whose
    /// (alias-resolved, export-specifier declared ones excepted,
    /// 50384-50390) flags meet the meaning ⇒ qualify; other slots are
    /// walked past. getSymbolFlags' transitive-alias union collapses
    /// to the resolved symbol's flags — resolveAlias resolves chains
    /// to their non-alias tail.
    fn needs_qualification_slice(
        &mut self,
        symbol: SymbolId,
        meaning: tsc_types::SymbolFlags,
        enclosing: Option<NodeId>,
    ) -> CheckResult<bool> {
        let escaped = self.binder.symbol(symbol).escaped_name.clone();
        for (_, table, _) in self.symbol_tables_in_scope_slice(enclosing) {
            let Some(&entry) = table.get(&escaped) else {
                continue;
            };
            let entry = self.get_merged_symbol(entry);
            if entry == symbol {
                return Ok(false);
            }
            let entry_flags = self.binder.symbol(entry).flags;
            let should_resolve = entry_flags.intersects(tsc_types::SymbolFlags::ALIAS)
                && !self.symbol_has_declaration_of_kind(entry, SyntaxKind::ExportSpecifier);
            let flags = if should_resolve {
                let resolved = self.resolve_alias(entry)?;
                self.binder.symbol(resolved).flags
            } else {
                entry_flags
            };
            if flags.intersects(meaning) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// tsc-port: getContainersOfSymbol @6.0.3 (error-path slice)
    /// tsc-hash: 22e0144d0040f4fb713cbdddd579457d28490db454397361da682542484911d7
    /// tsc-span: _tsc.js:49989-50051
    ///
    /// The computed unique-symbol member face can carry a TYPE-only
    /// parent (for example `SymbolConstructor.iterator`) while the
    /// Value expression must qualify through an in-scope value whose
    /// type is that parent's declared type (`Symbol.iterator`).
    /// getWithAlternativeContainers' firstVariableMatch owns that
    /// bridge. The class-expression-assignment candidates arm
    /// (50003-50009) cannot match the admitted declarations here.
    fn containers_of_symbol_slice(
        &mut self,
        symbol: SymbolId,
        enclosing: Option<NodeId>,
        meaning: tsc_types::SymbolFlags,
    ) -> CheckResult<Vec<SymbolId>> {
        if let Some(container) = self.get_parent_of_symbol(symbol) {
            return self.with_alternative_containers_slice(
                container,
                Some(container),
                enclosing,
                meaning,
            );
        }
        let declarations = self.binder.symbol(symbol).declarations.clone();
        let mut candidates = Vec::new();
        for declaration in declarations {
            let source = self.binder.source_of_node(declaration);
            if node_util::is_ambient_module(source, declaration) {
                continue;
            }
            let Some(parent) = self.parent_of(declaration) else {
                continue;
            };
            // 49996-49998: a direct child of an external module.
            if self.is_non_global_augmentation_external_module_node(parent) {
                if let Some(parent_symbol) = self.node_symbol(parent) {
                    candidates.push(parent_symbol);
                }
                continue;
            }
            // 49999-50001: an export='d member of an ambient module.
            if self.kind_of(parent) == SyntaxKind::ModuleBlock {
                if let Some(grandparent) = self.parent_of(parent) {
                    if let Some(module_symbol) = self.node_symbol(grandparent) {
                        if self.resolve_external_module_symbol(Some(module_symbol), false)?
                            == Some(symbol)
                        {
                            candidates.push(module_symbol);
                        }
                    }
                }
            }
        }
        if candidates.is_empty() {
            return Ok(Vec::new());
        }
        // 50014: only containers that actually re-export the symbol
        // count.
        let mut containers = Vec::new();
        for candidate in candidates {
            if self
                .alias_for_symbol_in_container_slice(candidate, symbol)?
                .is_some()
            {
                containers.push(candidate);
            }
        }
        // 50015-50022: best/alternative interleave over each
        // container's expansion. additionalContainers close over the
        // OUTER getParentOfSymbol container (50048-50050) — undefined
        // on this parentless leg, so each expansion is the container
        // alone.
        let mut best = Vec::new();
        let mut alternatives = Vec::new();
        for container in containers {
            let expanded =
                self.with_alternative_containers_slice(container, None, enclosing, meaning)?;
            let mut expanded = expanded.into_iter();
            if let Some(first) = expanded.next() {
                best.push(first);
            }
            alternatives.extend(expanded);
        }
        best.extend(alternatives);
        Ok(best)
    }

    /// getWithAlternativeContainers (50023-50047), error-display
    /// slice:
    /// additionalContainers (files whose export= IS the symbol's
    /// PARENT container — the closure reads the outer `container`,
    /// 50048-50050) ahead of the container itself. firstVariableMatch
    /// precedes both when a TYPE-only object container has an in-scope
    /// Value with the identical declared type. reexportContainers,
    /// the accessible-container early return, and
    /// objectLiteralContainer remain outside this bounded face.
    fn with_alternative_containers_slice(
        &mut self,
        container: SymbolId,
        parent_container: Option<SymbolId>,
        enclosing: Option<NodeId>,
        meaning: tsc_types::SymbolFlags,
    ) -> CheckResult<Vec<SymbolId>> {
        let mut additional = Vec::new();
        if let Some(parent_container) = parent_container {
            let declarations = self.binder.symbol(container).declarations.clone();
            for declaration in declarations {
                if let Some(file_symbol) = self
                    .file_symbol_if_export_equals_container_slice(declaration, parent_container)?
                {
                    additional.push(file_symbol);
                }
            }
        }
        // 50038-50043: an interface/type-literal container cannot be
        // named as a Value expression. Find the first in-scope Value
        // whose value type is exactly the container's declared type.
        // Identity, table order, and the exact Value meaning gate are
        // all observable (`SymbolConstructor` -> global `Symbol`).
        let container_flags = self.binder.symbol(container).flags;
        let left_meaning = Self::qualified_left_meaning(meaning);
        let first_variable_match = if !container_flags.intersects(left_meaning)
            && container_flags.intersects(tsc_types::SymbolFlags::TYPE)
            && meaning == tsc_types::SymbolFlags::VALUE
        {
            let declared = self.get_declared_type_of_symbol_slice(container)?;
            if self.tables.flags_of(declared).intersects(TypeFlags::OBJECT) {
                let mut first = None;
                'tables: for (_, table, _) in self.symbol_tables_in_scope_slice(enclosing) {
                    for &candidate in table.values() {
                        if self.binder.symbol(candidate).flags.intersects(left_meaning)
                            && self.get_type_of_symbol(candidate)? == declared
                        {
                            first = Some(candidate);
                            break 'tables;
                        }
                    }
                }
                first
            } else {
                None
            }
        } else {
            None
        };
        let mut result = Vec::with_capacity(additional.len() + 2);
        if let Some(first_variable_match) = first_variable_match {
            result.push(first_variable_match);
        }
        result.extend(additional);
        result.push(container);
        Ok(result)
    }

    /// tsc-port: getFileSymbolIfFileSymbolExportEqualsContainer @6.0.3
    /// tsc-hash: 664797354015a10df710b2c342bfa160aa42af4711c31bf017f98df27ae685ad
    /// tsc-span: _tsc.js:50060-50064
    ///
    /// getExternalModuleContainer's findAncestor starts AT the
    /// declaration (a string-named module declaration is its own
    /// container); the export= read is the RAW exports table.
    fn file_symbol_if_export_equals_container_slice(
        &mut self,
        declaration: NodeId,
        container: SymbolId,
    ) -> CheckResult<Option<SymbolId>> {
        let mut current = Some(declaration);
        let mut file_symbol = None;
        while let Some(node) = current {
            if self.is_non_global_augmentation_external_module_node(node) {
                file_symbol = self.node_symbol(node);
                break;
            }
            current = self.parent_of(node);
        }
        let Some(file_symbol) = file_symbol else {
            return Ok(None);
        };
        let exported = self
            .binder
            .symbol(file_symbol)
            .exports
            .get(tsc_types::InternalSymbolName::EXPORT_EQUALS)
            .copied();
        let Some(exported) = exported else {
            return Ok(None);
        };
        Ok(self
            .symbol_if_same_reference_slice(exported, container)?
            .then_some(file_symbol))
    }

    /// tsc-port: getAliasForSymbolInContainer @6.0.3
    /// tsc-hash: 33333377bf20d625fbd2b1ed3577e8e1ff93b9385d89c1fd0818cf487e348c63
    /// tsc-span: _tsc.js:50065-50083
    fn alias_for_symbol_in_container_slice(
        &mut self,
        container: SymbolId,
        symbol: SymbolId,
    ) -> CheckResult<Option<SymbolId>> {
        if self.get_parent_of_symbol(symbol) == Some(container) {
            return Ok(Some(symbol));
        }
        // 50070: the RAW exports table's export= (not the resolved
        // view) — its same-reference target elects the container
        // itself.
        let export_equals = self
            .binder
            .symbol(container)
            .exports
            .get(tsc_types::InternalSymbolName::EXPORT_EQUALS)
            .copied();
        if let Some(export_equals) = export_equals {
            if self.symbol_if_same_reference_slice(export_equals, symbol)? {
                return Ok(Some(container));
            }
        }
        let exports = self.get_exports_of_symbol(container)?;
        let escaped = self.binder.symbol(symbol).escaped_name.clone();
        if let Some(&quick) = exports.get(&escaped) {
            if self.symbol_if_same_reference_slice(quick, symbol)? {
                return Ok(Some(quick));
            }
        }
        for (_, &exported) in exports.iter() {
            if self.symbol_if_same_reference_slice(exported, symbol)? {
                return Ok(Some(exported));
            }
        }
        Ok(None)
    }

    /// tsc-port: getSymbolIfSameReference @6.0.3 (predicate face)
    /// tsc-hash: 908084bf7d1f72b02a8256627f01987eb8cd0a6897b9c7027f0cac3f156f5d3d
    /// tsc-span: _tsc.js:50084-50088
    fn symbol_if_same_reference_slice(&mut self, s1: SymbolId, s2: SymbolId) -> CheckResult<bool> {
        let merged1 = self.get_merged_symbol(s1);
        let resolved1 = self
            .resolve_symbol_ex(Some(merged1), false)?
            .expect("resolveSymbol(Some) is Some");
        let merged2 = self.get_merged_symbol(s2);
        let resolved2 = self
            .resolve_symbol_ex(Some(merged2), false)?
            .expect("resolveSymbol(Some) is Some");
        Ok(self.get_merged_symbol(resolved1) == self.get_merged_symbol(resolved2))
    }

    /// tsc-port: createAccessFromSymbolChain @6.0.3 (below-root naming)
    /// tsc-hash: 702a651dcc1e3cb163bfbcd065fcb88ceb8714e0dd9cb8bb6b81b452f1f3e757
    /// tsc-span: _tsc.js:53199-53251
    ///
    /// A below-root link takes its NAME from the first entry of the
    /// parent's resolved export table that same-references it,
    /// skipping export= and late-bound `__@` keys (53210-53218) — NOT
    /// from the link symbol itself (oracle-probed: `export { N as M }`
    /// renders `typeof import("/b").M`; with both `export { N as M }`
    /// and `export { N }` the FIRST table entry wins regardless of the
    /// symbol's own name or the import path). The computed-name
    /// fallback (53221-53228) and the parent-members IndexedAccess
    /// face (53232-53238) need member-table parents that
    /// module/namespace/alias links never have;
    /// getNameOfSymbolAsWritten (the symbol_display_name posture)
    /// closes the misses — including alias parents, whose unresolved
    /// export table is empty (probed: `typeof M.B`).
    fn qualifier_symbol_name_slice(
        &mut self,
        parent: SymbolId,
        symbol: SymbolId,
        use_alias_defined_outside_current_scope: bool,
        enclosing: Option<NodeId>,
    ) -> CheckResult<String> {
        let exports = self.get_exports_of_symbol(parent)?;
        for (name, &exported) in exports.iter() {
            if self.symbol_if_same_reference_slice(exported, symbol)?
                && !name.starts_with("__@")
                && name != tsc_types::InternalSymbolName::EXPORT_EQUALS
            {
                return Ok(tsc_binder::unescape_leading_underscores(name).to_owned());
            }
        }
        Ok(self.entity_symbol_name_as_written_slice(
            symbol,
            false,
            use_alias_defined_outside_current_scope,
            enclosing,
        ))
    }

    fn symbol_has_declaration_of_kind(&self, symbol: SymbolId, kind: SyntaxKind) -> bool {
        self.binder
            .symbol(symbol)
            .declarations
            .iter()
            .any(|&declaration| self.kind_of(declaration) == kind)
    }

    /// tsc-port: isUMDExportSymbol @6.0.3
    /// tsc-hash: 28246d1b6ded3e56b4f91451ba3fe1ebfd5c9166d25974444417b47edf8c3cdf
    /// tsc-span: _tsc.js:17555-17557
    ///
    /// declarations[0] ONLY — not the any-declaration helper above.
    /// NamespaceExportDeclaration is `export as namespace U`; the
    /// `export * as ns from` shape is SyntaxKind::NamespaceExport,
    /// filtered one leg later.
    fn is_umd_export_symbol(&self, symbol: SymbolId) -> bool {
        self.binder
            .symbol(symbol)
            .declarations
            .first()
            .is_some_and(|&declaration| {
                self.kind_of(declaration) == SyntaxKind::NamespaceExportDeclaration
            })
    }

    /// tsc-port: hasNonGlobalAugmentationExternalModuleSymbol @6.0.3
    /// tsc-hash: 0dd109154ac5ad4bb4b4feae06275eb4af183ce33d0687c637b3d0726452aeae
    /// tsc-span: _tsc.js:50541-50543
    // h2-7a-m-3 widening: shared external-module-root predicate.
    pub(crate) fn symbol_has_external_module_declaration(&self, symbol: SymbolId) -> bool {
        self.binder
            .symbol(symbol)
            .declarations
            .iter()
            .any(|&declaration| self.is_non_global_augmentation_external_module_node(declaration))
    }

    /// The node face of the same predicate (tsc takes declarations).
    fn is_non_global_augmentation_external_module_node(&self, node: NodeId) -> bool {
        match self.data_of(node) {
            NodeData::ModuleDeclaration(data) => data
                .name
                .is_some_and(|name| matches!(self.data_of(name), NodeData::StringLiteral(_))),
            NodeData::SourceFile(_) => self.binder.is_external_or_common_js_module_of_node(node),
            _ => false,
        }
    }

    /// tsc-port: getSpecifierForModuleSymbol @6.0.3 (error-path slice)
    /// tsc-hash: 26225fda031f89922a16ce84a38d6dc09b66e0d6b9ff8e0dbfa52465739fbafc
    /// tsc-span: _tsc.js:53060-53111
    ///
    /// Without an enclosingFile the specifier is decided before the
    /// getModuleSpecifiers machinery (53076-53081): the
    /// ambientModuleSymbolRegex unquote covers ambient modules (their
    /// string-literal name) and source-file modules (`"<fileName minus
    /// extension>"`, bindSourceFileAsExternalModule, 44548-44550 —
    /// which is also why the rendered specifier is extension-free),
    /// and the fileName fallback (53080) fires exactly when the regex
    /// rejects — `declare module ""` binds `""`, whose empty body
    /// fails /^".+"$/ — reading getNonAugmentationDeclaration's
    /// source file, extension intact. The moduleName arm (53068; AMD
    /// `///<amd-module>` pragma) is unparsed by the port (zero
    /// conformance uses), and the export= file-equivalence probe
    /// (53062-53067) only re-points that moduleName read — outcome-
    /// inert here. Source-file paths (both legs) render through the
    /// host's absolute normalized form: the oracle host roots every
    /// fileName against the program cwd (program-host.mjs
    /// absoluteProgramFileName), the same posture as
    /// getFullyQualifiedName's source-file arm.
    fn specifier_for_module_symbol_slice(&self, symbol: SymbolId) -> CheckResult<String> {
        let data = self.binder.symbol(symbol);
        let escaped = &data.escaped_name;
        // ambientModuleSymbolRegex (46291): /^".+"$/.
        if escaped.len() >= 3 && escaped.starts_with('"') && escaped.ends_with('"') {
            let name = &escaped[1..escaped.len() - 1];
            let source_file_module = data
                .declarations
                .iter()
                .any(|&declaration| self.kind_of(declaration) == SyntaxKind::SourceFile);
            if source_file_module {
                return Ok(Self::normalize_program_path(
                    name,
                    &self.host_current_directory,
                ));
            }
            return Ok(name.to_owned());
        }
        // tsc-port: getNonAugmentationDeclaration @6.0.3
        // tsc-span: _tsc.js:13749-13752
        let declaration = data.declarations.iter().copied().find(|&declaration| {
            let source = self.binder.source_of_node(declaration);
            let external_augmentation = node_util::is_ambient_module(source, declaration)
                && node_util::is_module_augmentation_external(source, declaration);
            let global_augmentation =
                matches!(self.data_of(declaration), NodeData::ModuleDeclaration(_))
                    && node_util::is_global_scope_augmentation(source, declaration);
            !external_augmentation && !global_augmentation
        });
        // getSpecifierForModuleSymbol dereferences this lookup
        // unconditionally after the ambient-name leg. Preserve that
        // invariant: every valid non-ambient module symbol has a
        // non-augmentation declaration.
        let declaration = declaration
            .expect("module symbol without an ambient name has a non-augmentation declaration");
        Ok(Self::normalize_program_path(
            &self.binder.source_of_node(declaration).file_name,
            &self.host_current_directory,
        ))
    }

    /// getSpecifierForModuleSymbol with an enclosing file: source-file
    /// module roots use the shortest relative module specifier, while
    /// ambient-module names remain their declared bare spelling.
    fn specifier_for_module_symbol_at_slice(
        &self,
        symbol: SymbolId,
        enclosing: NodeId,
    ) -> CheckResult<String> {
        let source_file_module = self
            .binder
            .symbol(symbol)
            .declarations
            .iter()
            .any(|&declaration| self.kind_of(declaration) == SyntaxKind::SourceFile);
        let specifier = self.specifier_for_module_symbol_slice(symbol)?;
        if !source_file_module {
            return Ok(specifier);
        }
        let importer = &self.binder.source_of_node(enclosing).file_name;
        Ok(Self::relative_module_specifier_slice(importer, &specifier))
    }

    fn relative_module_specifier_slice(importer: &str, target: &str) -> String {
        let importer = Self::normalize_program_path(importer, "");
        let target = Self::normalize_program_path(target, "");
        let importer_dir = importer
            .rsplit_once('/')
            .map_or("", |(directory, _)| directory);
        let from: Vec<_> = importer_dir
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();
        let to: Vec<_> = target
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();
        let shared = from
            .iter()
            .zip(&to)
            .take_while(|(left, right)| left == right)
            .count();
        let mut parts = vec![".."; from.len().saturating_sub(shared)];
        parts.extend(to[shared..].iter().copied());
        let relative = parts.join("/");
        if relative.starts_with("../") || relative == ".." {
            relative
        } else {
            format!("./{relative}")
        }
    }

    /// tsrs-native: bounded consumer-side face of tsc's widened type
    /// for an empty object assigned to a checked-JS `this` property.
    /// The Rust flow currently retains the source OBJECT_LITERAL bit;
    /// tsc has widened it to `{}` before indexed access. Keeping this
    /// predicate syntax- and assignment-kind-exact prevents the
    /// object-literal index shortcut from swallowing the 7053 path.
    pub(crate) fn is_checked_js_empty_this_assignment_type(&self, ty: TypeId) -> bool {
        let Some(symbol) = self.tables.type_of(ty).symbol else {
            return false;
        };
        let Some(literal) = self.binder.symbol(symbol).value_declaration else {
            return false;
        };
        if !self.is_effectively_checked_js_node(literal) {
            return false;
        }
        if !matches!(
            self.data_of(literal),
            NodeData::ObjectLiteralExpression(data)
                if self.nodes_of(data.properties).is_empty()
        ) {
            return false;
        }
        let Some(assignment) = self.parent_of(literal) else {
            return false;
        };
        let NodeData::BinaryExpression(data) = self.data_of(assignment) else {
            return false;
        };
        if data.right != Some(literal)
            || data
                .operator_token
                .is_none_or(|operator| self.kind_of(operator) != SyntaxKind::EqualsToken)
        {
            return false;
        }
        tsc_binder::get_assignment_declaration_kind(
            self.binder.source_of_node(assignment),
            assignment,
        ) == tsc_binder::AssignmentDeclarationKind::ThisProperty
    }

    /// Generic or error-containing mapped types preserve their mapped
    /// declaration face. A concrete mapped type instead resolves its
    /// structured members and prints the resulting properties/index
    /// signatures, exactly like any other object type.
    ///
    /// tsc-port: createTypeNodeFromObjectType @6.0.3
    /// tsc-hash: 484555186ba56d6509e6571aec8c745dab9e4d23b6f3fcd7a596a09c55537294
    /// tsc-span: _tsc.js:51894-51897
    ///
    /// The remainder follows 51898-51937, but the
    /// abstract-construct intersection re-derivation (51918-51928)
    /// needs an anonymous type mixing abstract construct signatures
    /// with other members: `abstract new` is grammatical only on
    /// ConstructorType nodes (single-signature shapes, which the
    /// 51912-51916 shorthand takes first) and abstract CLASS statics
    /// render behind the `typeof C` face, so mixed shapes chiefly arise
    /// from mapped/instantiation-expression synthesis. The branch
    /// below performs tsc's abstract-signature intersection split.
    fn type_node_from_object_type_slice(
        &mut self,
        ty: TypeId,
        fully_qualified: bool,
    ) -> CheckResult<(String, SliceTypeNodeKind)> {
        if self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::MAPPED)
            && (self.is_generic_mapped_type_state(ty)? || self.links.ty(ty).mapped_contains_error)
        {
            return self.mapped_type_to_string_slice_node(ty, fully_qualified);
        }
        let members = self.resolve_structured_type_members(ty)?;
        let resolved = self.members_of(members);
        let member_table = resolved.members.clone();
        let properties = resolved.properties.clone();
        let call_signatures = resolved.call_signatures.clone();
        let construct_signatures = resolved.construct_signatures.clone();
        let index_infos = resolved.index_infos.clone();
        if properties.is_empty() && index_infos.is_empty() {
            if call_signatures.is_empty() && construct_signatures.is_empty() {
                // createTypeNodeFromObjectType's exact member-less
                // TypeLiteral face (51900-51906). Older stages
                // curtained symbol-carrying JS empties while expando
                // and JSDoc member production was incomplete; those
                // producers are live in M8, so the local admission
                // heuristic must not replace tsc's unconditional `{}`.
                self.slice_add_approximate_length(2);
                return Ok(("{}".to_owned(), SliceTypeNodeKind::TypeLiteral));
            }
            // 51907-51916: the single call/construct signature
            // shorthands (`(...) => R`, `new (...) => R`); the
            // ConstructorType helper kind renders the abstract
            // modifier from the signature flag (52530-52533).
            if call_signatures.len() == 1 && construct_signatures.is_empty() {
                let text = self.signature_to_string_slice(
                    call_signatures[0],
                    SliceSignatureKind::FunctionType,
                    None,
                    fully_qualified,
                )?;
                return Ok((text, SliceTypeNodeKind::FunctionType));
            }
            if construct_signatures.len() == 1 && call_signatures.is_empty() {
                let text = self.signature_to_string_slice(
                    construct_signatures[0],
                    SliceSignatureKind::ConstructorType,
                    None,
                    fully_qualified,
                )?;
                return Ok((text, SliceTypeNodeKind::ConstructorType));
            }
        }
        let abstract_signatures = construct_signatures
            .iter()
            .copied()
            .filter(|&signature| {
                self.signature_of(signature)
                    .flags
                    .intersects(tsc_types::SignatureFlags::ABSTRACT)
            })
            .collect::<Vec<_>>();
        if !abstract_signatures.is_empty() {
            // 51918-51928: abstract construct signatures cannot be
            // members of a TypeLiteral. Re-derive each as its
            // single-signature ConstructorType and intersect those
            // faces with one anonymous copy containing all remaining
            // members.
            let mut types = Vec::with_capacity(abstract_signatures.len() + 1);
            for signature in abstract_signatures.iter().copied() {
                types.push(self.get_or_create_type_from_signature(signature)?);
            }
            let non_abstract_constructs = construct_signatures
                .iter()
                .copied()
                .filter(|signature| !abstract_signatures.contains(signature))
                .collect::<Vec<_>>();
            let type_element_count = call_signatures.len()
                + non_abstract_constructs.len()
                + index_infos.len()
                + properties.len();
            if type_element_count != 0 {
                let source_symbol = self.tables.type_of(ty).symbol;
                let remainder = self.tables.create_type(TypeFlags::OBJECT, TypeData::Object);
                self.tables.type_mut(remainder).object_flags = ObjectFlags::ANONYMOUS;
                self.tables.type_mut(remainder).symbol = source_symbol;
                let remainder_members = self.alloc_members(crate::state::ResolvedMembers {
                    members: member_table,
                    properties,
                    call_signatures,
                    construct_signatures: non_abstract_constructs,
                    index_infos,
                });
                self.links.set_fresh_type_members(
                    remainder,
                    crate::links::LinkSlot::Resolved(remainder_members),
                );
                types.push(remainder);
            }
            let intersection =
                self.get_intersection_type(&types, tsc_types::IntersectionFlags::NONE)?;
            return self.type_to_string_slice_node(intersection, fully_qualified);
        }
        // tsc-port: createTypeNodesFromResolvedType @6.0.3
        // tsc-hash: 96050b8c4ac17267f28f5ad848b848455efd24d01d889c9513683ef40b05770e
        // tsc-span: _tsc.js:52137-52152
        //
        // createTypeNodesFromResolvedType performs this
        // probe before signatures, index infos, or properties. A
        // sticky truncating context therefore turns the entire object
        // body into the synthetic `...` property; the enclosing
        // TypeLiteral still charges its braces below.
        if self.slice_check_truncation_length() {
            self.slice_add_approximate_length(2);
            return Ok(("{ ...; }".to_owned(), SliceTypeNodeKind::TypeLiteral));
        }
        // createTypeNodesFromResolvedType (52137-52240): call
        // signatures, then construct signatures (the 52157 abstract
        // `continue` is unreachable because the re-derivation above
        // has split every abstract-bearing shape), then index
        // signatures, then properties. The leading and per-property
        // checkTruncationLength probes share the exact sticky context
        // initialized by type_to_string_slice_root.
        let mut rendered = Vec::new();
        for &signature in &call_signatures {
            rendered.push(self.signature_to_string_slice(
                signature,
                SliceSignatureKind::CallSignature,
                None,
                fully_qualified,
            )?);
        }
        for &signature in &construct_signatures {
            rendered.push(self.signature_to_string_slice(
                signature,
                SliceSignatureKind::ConstructSignature,
                None,
                fully_qualified,
            )?);
        }
        for info in &index_infos {
            rendered.push(self.index_signature_slice(info, fully_qualified)?);
        }
        for (index, &property) in properties.iter().enumerate() {
            let ordinal = index + 1;
            if self.slice_check_truncation_length() && ordinal + 2 < properties.len() - 1 {
                if self.options.no_error_truncation != Some(true) {
                    rendered.push(format!("... {} more ...", properties.len() - ordinal));
                }
                self.property_signature_slice(
                    properties[properties.len() - 1],
                    fully_qualified,
                    &mut rendered,
                )?;
                break;
            }
            self.property_signature_slice(property, fully_qualified, &mut rendered)?;
        }
        if rendered.is_empty() {
            // 52238: every property skipped -> undefined members ->
            // the member-less literal face.
            self.slice_add_approximate_length(2);
            return Ok(("{}".to_owned(), SliceTypeNodeKind::TypeLiteral));
        }
        self.slice_add_approximate_length(2);
        Ok((
            format!("{{ {}; }}", rendered.join("; ")),
            SliceTypeNodeKind::TypeLiteral,
        ))
    }

    /// tsc-port: indexInfoToIndexSignatureDeclarationHelper @6.0.3
    /// tsc-hash: 272ecb1e37223afa95dd90071374ac2c2c8985c529f7a26a9e328f020360d79c
    /// tsc-span: _tsc.js:52476-52503
    ///
    /// getNameFromIndexInfo reads the declared parameter name ("x" for
    /// synthesized infos); the AllowEmptyIndexInfoType encounteredError
    /// leg is dead under IgnoreErrors and the port's IndexInfo always
    /// carries a value type.
    fn index_signature_slice(
        &mut self,
        info: &crate::state::IndexInfo,
        fully_qualified: bool,
    ) -> CheckResult<String> {
        let name = match info.declaration {
            Some(declaration) => {
                let parameter = match self.data_of(declaration) {
                    NodeData::IndexSignature(data) => data.parameters.and_then(|parameters| {
                        self.binder.node_array(parameters).nodes.first().copied()
                    }),
                    _ => None,
                };
                let name = parameter.and_then(|parameter| match self.data_of(parameter) {
                    NodeData::Parameter(data) => data.name,
                    _ => None,
                });
                // getNameFromIndexInfo delegates to
                // declarationNameToString, so recovery/missing names
                // and binding patterns use their declaration face
                // rather than an identifier-only approximation.
                tsc_binder::node_util::declaration_name_to_string(
                    self.binder.source_of_node(declaration),
                    name,
                )
            }
            None => "x".to_owned(),
        };
        let key = self.type_to_string_slice_ex(info.key_type, fully_qualified)?;
        let value = self.type_to_string_slice_ex(info.value_type, fully_qualified)?;
        self.slice_add_approximate_length(Self::slice_js_length(&name) + 4);
        let readonly = if info.is_readonly { "readonly " } else { "" };
        Ok(format!("{readonly}[{name}: {key}]: {value}"))
    }

    /// tsc-port: createElidedInformationPlaceholder @6.0.3
    /// tsc-hash: 9fe24796b9c8dc49e718e88a66c16cac0341b79590fdec1e7b8edc49122e169f
    /// tsc-span: _tsc.js:52212-52222
    fn reverse_mapped_elision_placeholder_slice(&mut self) -> (String, SliceTypeNodeKind) {
        self.slice_add_approximate_length(3);
        if self.options.no_error_truncation == Some(true) {
            // The printer removes the synthetic `/* elided */`
            // comment from the AnyKeyword node.
            ("any".to_owned(), SliceTypeNodeKind::Keyword)
        } else {
            ("...".to_owned(), SliceTypeNodeKind::Reference)
        }
    }

    /// tsc-port: shouldUsePlaceholderForProperty @6.0.3
    /// tsc-hash: 6216ae17f4795783d5b0c85fe0c09dd1c1b7fb7ccc3940cd203890b7a4dc7822
    /// tsc-span: _tsc.js:52223-52240
    fn should_use_reverse_mapped_placeholder_slice(&self, property: SymbolId) -> bool {
        let links = self.links.symbol(property);
        if !links
            .check_flags
            .intersects(tsc_types::CheckFlags::REVERSE_MAPPED)
        {
            return false;
        }
        if self.slice_reverse_mapped_stack.contains(&property) {
            return true;
        }
        if let Some(&last) = self.slice_reverse_mapped_stack.last() {
            let property_type = self
                .links
                .symbol(last)
                .property_type
                .expect("reverse-mapped properties carry propertyType");
            if !self
                .tables
                .object_flags_of(property_type)
                .intersects(ObjectFlags::ANONYMOUS)
            {
                return true;
            }
        }
        const DEPTH: usize = 3;
        if self.slice_reverse_mapped_stack.len() < DEPTH {
            return false;
        }
        let mapped_type = links
            .mapped_type
            .expect("reverse-mapped properties carry mappedType");
        let mapped_symbol = self.tables.type_of(mapped_type).symbol;
        self.slice_reverse_mapped_stack
            .iter()
            .rev()
            .take(DEPTH)
            .all(|&stacked| {
                let mapped = self
                    .links
                    .symbol(stacked)
                    .mapped_type
                    .expect("reverse-mapped properties carry mappedType");
                self.tables.type_of(mapped).symbol == mapped_symbol
            })
    }

    /// tsc-port: addPropertyToElementList @6.0.3
    /// tsc-hash: 51ca73b16014f72c20c3b112b50304ef359bc84bf5820463afb782e4cda6e335
    /// tsc-span: _tsc.js:52241-52400
    ///
    /// The late-bound trackComputedName block is dead in the slice
    /// (typeToString's tracker cannot track symbols). Reverse-mapped
    /// properties share the root context's stack and take tsc's
    /// recursive/deep placeholder faces. A function/method-flagged
    /// property whose filtered type has no call signatures and no
    /// question token emits NOTHING (52350's early return past the
    /// emission) — transcribed as the skip arm.
    fn property_signature_slice(
        &mut self,
        property: SymbolId,
        fully_qualified: bool,
        rendered: &mut Vec<String>,
    ) -> CheckResult<()> {
        let property_is_reverse_mapped = self
            .links
            .symbol(property)
            .check_flags
            .intersects(tsc_types::CheckFlags::REVERSE_MAPPED);
        let use_reverse_mapped_placeholder =
            self.should_use_reverse_mapped_placeholder_slice(property);
        let property_type = if use_reverse_mapped_placeholder {
            self.tables.intrinsics.any
        } else {
            self.get_non_missing_type_of_symbol(property)?
        };
        let symbol_flags = self.binder.symbol(property).flags;
        let name = self.property_name_slice(property, fully_qualified)?;
        self.slice_add_approximate_length(Self::slice_js_length(&name) + 1);
        // 52268-52343: accessor properties whose write type diverges
        // (or whose class parent takes the getter/setter arms) print
        // signature faces; the same-type non-class fall-through
        // prints the plain property row (oracle-pinned:
        // `{ get p(): string; set p(v: string) }` displays
        // `{ p: string; }`).
        if symbol_flags.intersects(tsc_types::SymbolFlags::ACCESSOR) {
            let write_type = self.get_write_type_of_symbol(property)?;
            let error = self.tables.intrinsics.error;
            if property_type != error && write_type != error {
                let class_parent = self.binder.symbol(property).parent.is_some_and(|parent| {
                    self.binder
                        .symbol(parent)
                        .flags
                        .intersects(tsc_types::SymbolFlags::CLASS)
                });
                // 52272-52339: class auto-accessors either reuse real
                // getter/setter declarations or synthesize the exact
                // `get`/`set(arg)` pair when the backing
                // PropertyDeclaration carries the accessor modifier.
                let property_declaration = self
                    .binder
                    .symbol(property)
                    .declarations
                    .iter()
                    .copied()
                    .find(|&declaration| {
                        matches!(self.data_of(declaration), NodeData::PropertyDeclaration(_))
                    });
                if property_type != write_type || (class_parent && property_declaration.is_none()) {
                    // 52274-52297: the diverging pair prints one
                    // signature face per present accessor declaration,
                    // instantiated under the symbol links mapper.
                    let symbol_mapper = self.links.symbol(property).mapper;
                    let declarations = self.binder.symbol(property).declarations.clone();
                    let getter = declarations
                        .iter()
                        .copied()
                        .find(|&d| matches!(self.data_of(d), NodeData::GetAccessor(_)));
                    let setter = declarations
                        .iter()
                        .copied()
                        .find(|&d| matches!(self.data_of(d), NodeData::SetAccessor(_)));
                    if let Some(getter) = getter {
                        let mut signature = self.get_signature_from_declaration(getter)?;
                        if let Some(mapper) = symbol_mapper {
                            signature = self.instantiate_signature(signature, mapper, false)?;
                        }
                        rendered.push(self.signature_to_string_slice(
                            signature,
                            SliceSignatureKind::GetAccessor,
                            Some((&name, false)),
                            fully_qualified,
                        )?);
                    }
                    if let Some(setter) = setter {
                        let mut signature = self.get_signature_from_declaration(setter)?;
                        if let Some(mapper) = symbol_mapper {
                            signature = self.instantiate_signature(signature, mapper, false)?;
                        }
                        rendered.push(self.signature_to_string_slice(
                            signature,
                            SliceSignatureKind::SetAccessor,
                            Some((&name, false)),
                            fully_qualified,
                        )?);
                    }
                    return Ok(());
                }
                if class_parent
                    && property_declaration.is_some_and(|declaration| {
                        tsc_binder::node_util::has_syntactic_modifier(
                            self.binder.source_of_node(declaration),
                            declaration,
                            ModifierFlags::ACCESSOR,
                        )
                    })
                {
                    let read = self.type_to_string_slice_ex(property_type, fully_qualified)?;
                    let write = self.type_to_string_slice_ex(write_type, fully_qualified)?;
                    rendered.push(format!("get {name}(): {read}"));
                    rendered.push(format!("set {name}(arg: {write})"));
                    return Ok(());
                }
            }
        }
        let optional = symbol_flags.intersects(tsc_types::SymbolFlags::OPTIONAL);
        if symbol_flags
            .intersects(tsc_types::SymbolFlags::FUNCTION | tsc_types::SymbolFlags::METHOD)
            && self
                .get_properties_of_object_type_owned(property_type)?
                .is_empty()
            && !self.is_readonly_symbol(property)
        {
            let filtered = self.filter_type_with(property_type, |state, member| {
                Ok(!state
                    .tables
                    .flags_of(member)
                    .intersects(TypeFlags::UNDEFINED))
            })?;
            let signatures = self.get_signatures_of_type(filtered, SignatureKind::Call)?;
            if !signatures.is_empty() {
                // Method faces (52344-52350): one MethodSignature
                // member per call signature, the optional token on
                // each (`m?(...)`), the filtered type's undefined
                // never printing.
                for &signature in &signatures {
                    rendered.push(self.signature_to_string_slice(
                        signature,
                        SliceSignatureKind::MethodSignature,
                        Some((&name, optional)),
                        fully_qualified,
                    )?);
                }
                return Ok(());
            }
            if !optional {
                return Ok(());
            }
        }
        // serializeTypeForDeclaration → syntacticNodeBuilder.typeFromProperty
        // (53487-53507, 133921-133940): an explicit property
        // annotation is reusable in an enclosing-scoped render. The
        // question-token equivalence deliberately compares the
        // annotation against the property's undefined-stripped type.
        let mut type_text = None;
        if !use_reverse_mapped_placeholder {
            if let Some(declaration) = self.binder.symbol(property).declarations.first().copied() {
                if let Some(annotation) = self.effective_type_annotation_node(declaration) {
                    type_text = self.annotation_reuse_text_slice(
                        annotation,
                        property_type,
                        /*requires_adding_undefined*/ false,
                        self.is_optional_declaration(declaration),
                        /*is_parameter*/ false,
                    )?;
                }
            }
        }
        let type_text = match type_text {
            Some(text) => text,
            None if use_reverse_mapped_placeholder => {
                self.reverse_mapped_elision_placeholder_slice().0
            }
            None => {
                if property_is_reverse_mapped {
                    self.slice_reverse_mapped_stack.push(property);
                }
                let rendered = self.type_to_string_slice_ex(property_type, fully_qualified);
                if property_is_reverse_mapped {
                    let popped = self.slice_reverse_mapped_stack.pop();
                    debug_assert_eq!(popped, Some(property));
                }
                rendered?
            }
        };
        let readonly = if self.is_readonly_symbol(property) {
            self.slice_add_approximate_length(9);
            "readonly "
        } else {
            ""
        };
        let question = if optional { "?" } else { "" };
        rendered.push(format!("{readonly}{name}{question}: {type_text}"));
        Ok(())
    }

    /// tsrs-native: call-signature adapter for resolveCall's
    /// signatureToString(c) overload-error row.
    pub(crate) fn signature_to_string_slice_for_overload_error(
        &mut self,
        signature: SignatureId,
    ) -> CheckResult<String> {
        self.signature_to_string_slice_for_diagnostic(signature, SliceSignatureKind::CallSignature)
    }

    /// tsrs-native: signatureToString's default-flags relation-error
    /// adapter.
    ///
    /// signaturesRelatedTo passes its Call/Construct kind with no
    /// WriteArrowStyleSignature flag, so the printer emits `(...): R`
    /// / `new (...): R` rather than the corresponding function-type
    /// arrows.
    pub(crate) fn signature_to_string_slice_for_relation_error(
        &mut self,
        signature: SignatureId,
        kind: SignatureKind,
    ) -> CheckResult<String> {
        let slice_kind = match kind {
            SignatureKind::Call => SliceSignatureKind::CallSignature,
            SignatureKind::Construct => SliceSignatureKind::ConstructSignature,
        };
        self.signature_to_string_slice_for_diagnostic(signature, slice_kind)
    }

    /// tsrs-native: select the constructor-arrow printer face used by tsc's
    /// single-constructor relation fallback.
    ///
    /// tsc's single-constructor relation fallback renders both signatures
    /// with `WriteArrowStyleSignature`, even though the surrounding
    /// signaturesRelatedTo diagnostics use declaration-style signatures.
    pub(crate) fn signature_to_string_slice_for_construct_assignment_error(
        &mut self,
        signature: SignatureId,
    ) -> CheckResult<String> {
        self.signature_to_string_slice_for_diagnostic(
            signature,
            SliceSignatureKind::ConstructorType,
        )
    }

    /// Keep every standalone diagnostic render isolated from an
    /// enclosing typeToString slice. This mirrors tsc's fresh
    /// single-line writer per signatureToString call.
    fn signature_to_string_slice_for_diagnostic(
        &mut self,
        signature: SignatureId,
        kind: SliceSignatureKind,
    ) -> CheckResult<String> {
        let saved_visited = std::mem::take(&mut self.slice_visited_types);
        let saved_infer_type_parameters = std::mem::take(&mut self.slice_infer_type_parameters);
        let saved_approximate_length = std::mem::replace(&mut self.slice_approximate_length, 0);
        let saved_max_truncation_length = std::mem::replace(
            &mut self.slice_max_truncation_length,
            if self.options.no_error_truncation == Some(true) {
                1_000_000
            } else {
                160
            },
        );
        let saved_truncating = std::mem::replace(&mut self.slice_truncating, false);
        let saved_reverse_mapped_stack = std::mem::take(&mut self.slice_reverse_mapped_stack);
        let saved_no_type_reduction = std::mem::replace(&mut self.slice_no_type_reduction, false);
        let saved_enclosing = self.slice_display_enclosing.take();
        let result =
            self.signature_to_string_slice(signature, kind, None, /*fully_qualified*/ false);
        self.slice_visited_types = saved_visited;
        self.slice_infer_type_parameters = saved_infer_type_parameters;
        self.slice_approximate_length = saved_approximate_length;
        self.slice_max_truncation_length = saved_max_truncation_length;
        self.slice_truncating = saved_truncating;
        self.slice_reverse_mapped_stack = saved_reverse_mapped_stack;
        self.slice_no_type_reduction = saved_no_type_reduction;
        self.slice_display_enclosing = saved_enclosing;
        result
    }

    /// tsc-port: signatureToSignatureDeclarationHelper @6.0.3
    /// tsc-hash: 648aa8da24269c33b616fec95aa4cf725df9b6ddc0bb254ac01e456791be71c7
    /// tsc-span: _tsc.js:52504-52631
    ///
    /// Dead context legs under the error-display slice:
    /// WriteTypeArgumentsOfSignature (a signatureToString-band flag,
    /// 52515), GenerateNamesForShadowedTypeParams renaming,
    /// OmitThisParameter,
    /// SuppressAnyReturnType (52520 clears it around the parameter
    /// walk regardless), and the JSDocSignature overload-comment tail
    /// (52605-52620), whose synthetic comment is discarded by this
    /// comment-free string slice. enterNewScope's mapper is live for
    /// instantiated signatures and is parked around the worker below;
    /// nested reusable TypeReferences consult it. options.modifiers is empty at every slice call
    /// site; the ConstructorType abstract OR-in (52530-52533) reads
    /// the signature flag. The returnTypeNode ?? empty-reference
    /// fallbacks (52547) are dead — serializeReturnTypeForSignature
    /// always yields under the never-set SuppressAnyReturnType.
    fn signature_to_string_slice(
        &mut self,
        signature: SignatureId,
        kind: SliceSignatureKind,
        member_name: Option<(&str, bool)>,
        fully_qualified: bool,
    ) -> CheckResult<String> {
        let mapper = self.signature_of(signature).mapper;
        if let Some(mapper) = mapper {
            self.slice_display_mappers.push(mapper);
        }
        let result =
            self.signature_to_string_slice_worker(signature, kind, member_name, fully_qualified);
        if mapper.is_some() {
            self.slice_display_mappers.pop();
        }
        result
    }

    fn signature_to_string_slice_worker(
        &mut self,
        signature: SignatureId,
        kind: SliceSignatureKind,
        member_name: Option<(&str, bool)>,
        fully_qualified: bool,
    ) -> CheckResult<String> {
        let expanded = self.expanded_parameter_faces_slice(signature)?;
        let sig = self.signature_of(signature);
        let type_parameters = sig.type_parameters.clone();
        let declared_parameters = sig.parameters.clone();
        let this_parameter = sig.this_parameter;
        let declaration = sig.declaration;
        let is_abstract = sig.flags.intersects(tsc_types::SignatureFlags::ABSTRACT);
        // 52519-52523: a REST-flagged expanded face anywhere but last
        // falls back to the declared parameter list.
        let faces = match expanded {
            Some(faces)
                if !faces[..faces.len().saturating_sub(1)]
                    .iter()
                    .any(|face| face.rest) =>
            {
                faces
            }
            _ => {
                let mut faces = Vec::with_capacity(declared_parameters.len());
                for &parameter in &declared_parameters {
                    faces.push(self.declared_parameter_face_slice(parameter)?);
                }
                faces
            }
        };
        let mut parameter_texts = Vec::with_capacity(faces.len() + 1);
        if let Some(this_parameter) = this_parameter {
            // tryGetThisParameterDeclaration (52802-52805): the
            // declared this parameter unshifts to the front.
            let face = self.declared_parameter_face_slice(this_parameter)?;
            parameter_texts.push(self.parameter_face_to_string_slice(&face, fully_qualified)?);
        } else if let Some(declaration) =
            declaration.filter(|&declaration| self.is_in_js_file(declaration))
        {
            if let Some(this_tag) = self.first_jsdoc_tag(declaration, SyntaxKind::JSDocThisTag) {
                if let NodeData::JSDocThisTag(data) = self.data_of(this_tag) {
                    if let Some(type_expression) = data.type_expression {
                        let ty = self.get_type_from_type_node(type_expression)?;
                        let face = SliceParameterFace {
                            symbol: None,
                            declaration: None,
                            name: Some("this".to_owned()),
                            ty,
                            optional: false,
                            rest: false,
                        };
                        parameter_texts
                            .push(self.parameter_face_to_string_slice(&face, fully_qualified)?);
                    }
                }
            }
        }
        for face in &faces {
            parameter_texts.push(self.parameter_face_to_string_slice(face, fully_qualified)?);
        }
        let type_parameters_text = match &type_parameters {
            Some(parameters) if !parameters.is_empty() => {
                let mut rendered = Vec::with_capacity(parameters.len());
                for &parameter in parameters {
                    rendered.push(
                        self.type_parameter_to_declaration_slice(parameter, fully_qualified)?,
                    );
                }
                format!("<{}>", rendered.join(", "))
            }
            _ => String::new(),
        };
        let return_text =
            self.serialize_return_type_for_signature_slice(signature, fully_qualified)?;
        let parameters_text = parameter_texts.join(", ");
        let type_parameters_text = type_parameters_text.as_str();
        Ok(match kind {
            SliceSignatureKind::FunctionType => {
                format!("{type_parameters_text}({parameters_text}) => {return_text}")
            }
            SliceSignatureKind::ConstructorType => {
                let modifier = if is_abstract { "abstract " } else { "" };
                format!("{modifier}new {type_parameters_text}({parameters_text}) => {return_text}")
            }
            SliceSignatureKind::CallSignature => {
                format!("{type_parameters_text}({parameters_text}): {return_text}")
            }
            SliceSignatureKind::ConstructSignature => {
                format!("new {type_parameters_text}({parameters_text}): {return_text}")
            }
            SliceSignatureKind::MethodSignature => {
                let (name, optional) = member_name.unwrap_or(("", false));
                let question = if optional { "?" } else { "" };
                format!("{name}{question}{type_parameters_text}({parameters_text}): {return_text}")
            }
            SliceSignatureKind::GetAccessor => {
                // The accessor factories take no type parameters —
                // the grammar admits none on accessors.
                let (name, _) = member_name.unwrap_or(("", false));
                format!("get {name}({parameters_text}): {return_text}")
            }
            SliceSignatureKind::SetAccessor => {
                let (name, _) = member_name.unwrap_or(("", false));
                format!("set {name}({parameters_text})")
            }
        })
    }

    /// tsc-port: getExpandedParameters @6.0.3 (skipUnionExpanding face)
    /// tsc-hash: 43c4acbf32d5eaa48b8366c408ee5255add1639b9c48993d53c049bc18b7e6c8
    /// tsc-span: _tsc.js:57911-57960
    ///
    /// The display helper always passes skipUnionExpanding
    /// (52508-52511), so only [0] materializes and the union
    /// expansion never runs. tsc mints transient parameter symbols;
    /// the slice carries (name, type, optional, rest) faces instead —
    /// the only other consumer of the symbols is enterNewScope's fake
    /// scope, dead without an enclosingDeclaration. None = the
    /// declared parameter list (no tuple-typed rest).
    fn expanded_parameter_faces_slice(
        &mut self,
        signature: SignatureId,
    ) -> CheckResult<Option<Vec<SliceParameterFace>>> {
        let sig = self.signature_of(signature);
        if !sig
            .flags
            .intersects(tsc_types::SignatureFlags::HAS_REST_PARAMETER)
            || sig.parameters.is_empty()
        {
            return Ok(None);
        }
        let rest_index = sig.parameters.len() - 1;
        let rest_symbol = sig.parameters[rest_index];
        let prefix: Vec<SymbolId> = sig.parameters[..rest_index].to_vec();
        let rest_type = self.get_type_of_symbol(rest_symbol)?;
        if !self
            .tables
            .object_flags_of(rest_type)
            .intersects(ObjectFlags::REFERENCE)
        {
            return Ok(None);
        }
        let target = self.tables.reference_target(rest_type);
        let TypeData::TupleTarget(data) = self.tables.type_of(target).data.clone() else {
            return Ok(None);
        };
        let element_types = self.get_type_arguments(rest_type)?;
        let count = element_types.len().min(data.element_flags.len());
        // getUniqAssociatedNamesFromTupleType (57937-57959): the
        //4-arg getTupleElementLabel derives every name (labeled or
        // synthesized through the rest parameter's binding name); the
        // duplicate `_N` counter pass runs only when the target
        // carries a labels array, matching tsc's names-array gate.
        let mut names = Vec::with_capacity(count);
        for i in 0..count {
            let label = data
                .labeled_element_declarations
                .as_ref()
                .and_then(|labels| labels.get(i).copied())
                .flatten();
            names.push(self.tuple_element_label_slice(
                label.map(NodeId),
                i,
                data.element_flags[i],
                Some(rest_symbol),
            )?);
        }
        if data.labeled_element_declarations.is_some() {
            let mut unique: std::collections::HashSet<String> = std::collections::HashSet::new();
            let mut duplicates = Vec::new();
            for (i, name) in names.iter().enumerate() {
                if !unique.insert(name.clone()) {
                    duplicates.push(i);
                }
            }
            let mut counters: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            for i in duplicates {
                let base = names[i].clone();
                let mut counter = counters.get(&base).copied().unwrap_or(1);
                let mut fresh;
                loop {
                    fresh = format!("{base}_{counter}");
                    if unique.insert(fresh.clone()) {
                        break;
                    }
                    counter += 1;
                }
                names[i] = fresh.clone();
                // 57956: tsc keys the counter on the REWRITTEN name —
                // transcribed as-is.
                counters.insert(fresh, counter + 1);
            }
        }
        let mut faces = Vec::with_capacity(prefix.len() + count);
        for &parameter in &prefix {
            faces.push(self.declared_parameter_face_slice(parameter)?);
        }
        for (i, name) in names.into_iter().enumerate() {
            let flags = data.element_flags[i];
            // 57929-57931: Variable elements become rest faces,
            // Optional elements optional faces; Rest element types
            // wrap as arrays (57932).
            let rest = flags.intersects(ElementFlags::VARIABLE);
            let optional = !rest && flags.intersects(ElementFlags::OPTIONAL);
            let ty = if flags.intersects(ElementFlags::REST) {
                self.create_array_type(element_types[i], false)?
            } else {
                element_types[i]
            };
            faces.push(SliceParameterFace {
                symbol: None,
                declaration: None,
                name: Some(name),
                ty,
                optional,
                rest,
            });
        }
        Ok(Some(faces))
    }

    /// tsc-port: getTupleElementLabel @6.0.3 (4-arg synthesis face)
    /// tsc-hash: cfaef41e5163a36e33fb797ca0f1cf2445bcc1cf9453ac75b2f61681f2b472b1
    /// tsc-span: _tsc.js:78150-78157
    pub(crate) fn tuple_element_label_slice(
        &mut self,
        declaration: Option<NodeId>,
        index: usize,
        element_flags: ElementFlags,
        rest_symbol: Option<SymbolId>,
    ) -> CheckResult<String> {
        if let Some(declaration) = declaration {
            return self.tuple_element_label(declaration);
        }
        let rest_parameter = rest_symbol
            .and_then(|symbol| self.binder.symbol(symbol).value_declaration)
            .filter(|&declaration| matches!(self.data_of(declaration), NodeData::Parameter(_)));
        match rest_parameter {
            Some(parameter) => {
                self.tuple_element_label_from_binding_element_slice(parameter, index, element_flags)
            }
            None => {
                let base = rest_symbol
                    .map(|symbol| {
                        tsc_binder::unescape_leading_underscores(
                            &self.binder.symbol(symbol).escaped_name,
                        )
                        .to_owned()
                    })
                    .unwrap_or_else(|| "arg".to_owned());
                Ok(format!("{base}_{index}"))
            }
        }
    }

    /// tsc-port: getTupleElementLabelFromBindingElement @6.0.3
    /// tsc-hash: a8abed48acb2849e206d1748a97355a466b6a962706a1b417bcd041eacb3a0be
    /// tsc-span: _tsc.js:78121-78149
    ///
    /// Works over Parameter and BindingElement declarations alike
    /// (both carry name + dotDotDotToken); the escapedText reads
    /// unescape at this boundary because the labels land directly in
    /// display text (tsc unescapes at symbolName).
    fn tuple_element_label_from_binding_element_slice(
        &mut self,
        node: NodeId,
        index: usize,
        element_flags: ElementFlags,
    ) -> CheckResult<String> {
        let (name, dot_dot_dot) = match self.data_of(node) {
            NodeData::Parameter(data) => (data.name, data.dot_dot_dot_token.is_some()),
            NodeData::BindingElement(data) => (data.name, data.dot_dot_dot_token.is_some()),
            _ => (None, false),
        };
        if let Some(name) = name {
            match self.data_of(name) {
                NodeData::Identifier(data) => {
                    let text =
                        tsc_binder::unescape_leading_underscores(&data.escaped_text).to_owned();
                    if dot_dot_dot {
                        return Ok(if element_flags.intersects(ElementFlags::VARIABLE) {
                            text
                        } else {
                            format!("{text}_{index}")
                        });
                    }
                    return Ok(
                        if element_flags.intersects(ElementFlags::REQUIRED)
                            || element_flags.intersects(ElementFlags::OPTIONAL)
                        {
                            text
                        } else {
                            format!("{text}_n")
                        },
                    );
                }
                NodeData::ArrayBindingPattern(data) if dot_dot_dot => {
                    let elements = self.nodes_of(data.elements);
                    let last_is_rest = elements.last().copied().is_some_and(|last| {
                        matches!(self.data_of(last), NodeData::BindingElement(data)
                            if data.dot_dot_dot_token.is_some())
                    });
                    let element_count = elements.len() - usize::from(last_is_rest);
                    if index < element_count {
                        let element = elements[index];
                        if matches!(self.data_of(element), NodeData::BindingElement(_)) {
                            return self.tuple_element_label_from_binding_element_slice(
                                element,
                                index,
                                element_flags,
                            );
                        }
                    } else if last_is_rest {
                        let last = *elements.last().expect("last_is_rest implies non-empty");
                        return self.tuple_element_label_from_binding_element_slice(
                            last,
                            index - element_count,
                            element_flags,
                        );
                    }
                }
                _ => {}
            }
        }
        Ok(format!("arg_{index}"))
    }

    /// tsc-port: getEffectiveParameterDeclaration @6.0.3 (face builder)
    /// tsc-hash: e2d6460f51a6d6b97152d7d0e0a2de7ad9d85fd754748ccb8fbde1e29fb89a2f
    /// tsc-span: _tsc.js:52846-52880
    ///
    /// The declared-parameter half of symbolToParameterDeclaration:
    /// declaration lookup, the type read, and the rest/optional bits
    /// (isRestParameter on the declaration OR the RestParameter check
    /// flag; isOptionalParameter OR the OptionalParameter check flag).
    fn declared_parameter_face_slice(
        &mut self,
        parameter: SymbolId,
    ) -> CheckResult<SliceParameterFace> {
        let parameter_declaration = self
            .binder
            .symbol(parameter)
            .declarations
            .iter()
            .copied()
            .find(|&declaration| matches!(self.data_of(declaration), NodeData::Parameter(_)));
        let declaration = parameter_declaration.or_else(|| {
            (!self
                .binder
                .symbol(parameter)
                .flags
                .intersects(SymbolFlags::TRANSIENT))
            .then(|| {
                self.binder
                    .symbol(parameter)
                    .declarations
                    .iter()
                    .copied()
                    .find(|&declaration| self.kind_of(declaration) == SyntaxKind::JSDocParameterTag)
            })
            .flatten()
        });
        let ty = self.get_type_of_symbol(parameter)?;
        let check_flags = self.links.symbol(parameter).check_flags;
        let rest = declaration
            .is_some_and(|declaration| self.is_rest_parameter_declaration(declaration))
            || check_flags.intersects(tsc_types::CheckFlags::REST_PARAMETER);
        let optional = match declaration {
            Some(declaration) => self.is_optional_parameter_slice(declaration)?,
            None => false,
        } || check_flags.intersects(tsc_types::CheckFlags::OPTIONAL_PARAMETER);
        Ok(SliceParameterFace {
            symbol: Some(parameter),
            declaration,
            name: None,
            ty,
            optional,
            rest,
        })
    }

    /// tsc-port: symbolToParameterDeclaration @6.0.3 (render face)
    /// tsc-hash: 1852083e14ec6077c419dd8cb5fc7f552c1a3b4e26b02f96792636f55ca5cad9
    /// tsc-span: _tsc.js:52854-52911
    ///
    /// preserveModifierFlags is Constructor-kind-only (kind 177 —
    /// unreachable from the slice's member/shorthand kinds), so the
    /// modifiers leg stays empty. parameterToParameterDeclarationName:
    /// identifiers print their text (NoAsciiEscaping — the port
    /// prints raw), the QualifiedName arm is JSDoc-only
    /// (unconstructible under the no-parse policy), and binding
    /// patterns clone with initializers elided.
    fn parameter_face_to_string_slice(
        &mut self,
        face: &SliceParameterFace,
        fully_qualified: bool,
    ) -> CheckResult<String> {
        let requires_undefined = match face.declaration {
            Some(declaration) => self.requires_adding_implicit_undefined_slice(declaration)?,
            None => false,
        };
        // serializeTypeForDeclaration (53487-53509): the
        // syntacticNodeBuilder annotation arm lives behind
        // canReuseTypeNodeAnnotation's enclosing gate;
        // addUndefinedForParameter rides requiresAddingImplicitUndefined.
        let mut type_text = None;
        if let Some(declaration) = face.declaration {
            let annotation = self.effective_type_annotation_node(declaration);
            let question = self.is_optional_declaration(declaration);
            if let Some(annotation) = annotation {
                // JSDoc annotations take this same semantic path.
                // serializeExistingTypeNode resolves their type under
                // context.mapper; reprinting the raw annotation here
                // would leak an outer `T` from an instantiated
                // signature even though both the parameter symbol and
                // return type have already mapped it.
                // tsc-port: serializeTypeForDeclaration @6.0.3
                // tsc-hash: e8876735379b64ea1df7ad7b8717a20d509e85d78677f5425bc3b729f42d6d19
                // tsc-span: _tsc.js:53487-53509
                // tsc-port: serializeExistingTypeNode @6.0.3
                // tsc-hash: 433daa463f78335a63960c6658ccab7a037a667922af31e6eb4320cadafe30ff
                // tsc-span: _tsc.js:53712-53721
                type_text = self.annotation_reuse_text_slice(
                    annotation,
                    face.ty,
                    requires_undefined,
                    question,
                    /*is_parameter*/ true,
                )?;
            }
        }
        let type_text = match type_text {
            Some(text) => text,
            None => {
                let ty = if requires_undefined {
                    self.get_optional_type(face.ty, /*is_property*/ false)?
                } else {
                    face.ty
                };
                self.type_to_string_slice_ex(ty, fully_qualified)?
            }
        };
        let name_text = match &face.name {
            Some(name) => name.clone(),
            None => {
                let name_node =
                    face.declaration
                        .and_then(|declaration| match self.data_of(declaration) {
                            NodeData::Parameter(data) => data.name,
                            NodeData::JSDocParameterTag(data) => data.name,
                            _ => None,
                        });
                match name_node {
                    Some(name) => match self.data_of(name) {
                        NodeData::Identifier(data) => {
                            tsc_binder::unescape_leading_underscores(&data.escaped_text).to_owned()
                        }
                        NodeData::QualifiedName(data) => data
                            .right
                            .and_then(|right| self.identifier_text_of(right))
                            .map(tsc_binder::unescape_leading_underscores)
                            .unwrap_or_default()
                            .to_owned(),
                        NodeData::ObjectBindingPattern(_) | NodeData::ArrayBindingPattern(_) => {
                            self.binding_pattern_text_slice(name)?
                        }
                        _ => self.member_name_node_text_slice(name)?,
                    },
                    None => self.symbol_display_name(face.symbol.expect(
                        "parameter faces without declarations or synthesized names carry a symbol",
                    )),
                }
            }
        };
        let dots = if face.rest { "..." } else { "" };
        let question = if face.optional { "?" } else { "" };
        Ok(format!("{dots}{name_text}{question}: {type_text}"))
    }

    /// tsc-port: isOptionalParameter @6.0.3
    /// tsc-hash: 230cc8ce09e27fc4b9b6e370079e26817941e278127f592eca3c51ecb55ac67b
    /// tsc-span: _tsc.js:59509-59527
    ///
    /// The initializer arm reads getMinArgumentCount under
    /// (StrongArityForUntypedJS|VoidIsNonOptional), which reduces to
    /// the min-argument integer without the void-trimming loop
    /// (structural.rs's variant).
    pub(crate) fn is_optional_parameter_slice(&mut self, node: NodeId) -> CheckResult<bool> {
        let NodeData::Parameter(data) = self.data_of(node) else {
            return Ok(false);
        };
        let (question, initializer, annotation, dots) = (
            data.question_token.is_some(),
            data.initializer,
            data.r#type,
            data.dot_dot_dot_token.is_some(),
        );
        if question || self.is_optional_declaration(node) {
            return Ok(true);
        }
        if initializer.is_some() {
            let Some(parent) = self.parent_of(node) else {
                return Ok(false);
            };
            let signature = self.get_signature_from_declaration(parent)?;
            // The parser can retain an initializer on every
            // signature-declaration parameter as error-recovery syntax, not
            // only on implementation declarations. Use the shared typed AST
            // projection for `node.parent.parameters` so function/constructor
            // types and call/construct signatures preserve optional arity in
            // diagnostics too.
            let parameters = self.parameters_of_function(parent);
            let Some(parameter_index) = parameters.iter().position(|&p| p == node) else {
                return Ok(false);
            };
            return Ok(parameter_index >= self.min_argument_count_without_void_trimming(signature)?);
        }
        let parent = self.parent_of(node);
        if let Some(parent) = parent {
            if let Some(iife) = self.get_immediately_invoked_function_expression(parent) {
                // 59524: getEffectiveCallArguments — tuple spreads
                // expand per element (`(...[1, ""] as const)` counts
                // 2), so the syntactic argument list undercounts.
                let argument_count = self.get_effective_call_arguments(iife)?.len();
                let parameters = match self.data_of(parent) {
                    NodeData::FunctionExpression(data) => self.nodes_of(data.parameters),
                    NodeData::ArrowFunction(data) => self.nodes_of(data.parameters),
                    _ => Vec::new(),
                };
                let index = parameters.iter().position(|&p| p == node);
                return Ok(annotation.is_none()
                    && !dots
                    && index.is_some_and(|index| index >= argument_count));
            }
        }
        Ok(false)
    }

    /// tsc-port: requiresAddingImplicitUndefined @6.0.3
    /// tsc-hash: 0a4f62267c4e164779f61e6db1eb7e6d0ba8b59a21fe6fca9bdbce2d684aa52d
    /// tsc-span: _tsc.js:88075-88090
    ///
    /// isRequiredInitializedParameter + isOptionalUninitializedParameterProperty
    /// folded in. The parameter-property arms consult the syntactic modifier mask
    /// (accessibility/readonly/override) on the declaration.
    pub(crate) fn requires_adding_implicit_undefined_slice(
        &mut self,
        parameter: NodeId,
    ) -> CheckResult<bool> {
        let NodeData::Parameter(data) = self.data_of(parameter) else {
            return Ok(false);
        };
        let has_initializer = data.initializer.is_some();
        if !self
            .options
            .strict_option_value(self.options.strict_null_checks)
        {
            return Ok(false);
        }
        let optional = self.is_optional_parameter_slice(parameter)?;
        let source = self.binder.source_of_node(parameter);
        let parameter_property = tsc_binder::node_util::has_syntactic_modifier(
            source,
            parameter,
            tsc_types::ModifierFlags::PARAMETER_PROPERTY_MODIFIER,
        );
        let required_initialized = if optional || !has_initializer {
            false
        } else if parameter_property {
            // 88083-88085: a parameter property counts only inside a
            // function-like enclosing declaration — the reuse gates
            // only run with one set, and parameter properties sit in
            // constructors (function-like) by grammar.
            true
        } else {
            true
        };
        let optional_uninitialized_property = optional && !has_initializer && parameter_property;
        if !(required_initialized || optional_uninitialized_property) {
            return Ok(false);
        }
        // declaredParameterTypeContainsUndefined: the annotation's
        // type admits undefined already.
        if let Some(annotation) = data.r#type {
            let annotation_type = self.get_type_from_type_node(annotation)?;
            if self.some_type_is_undefined_slice(annotation_type) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// tsc someType(type, t => !!(t.flags & Undefined)) over the
    /// union-member view (the serializeExistingTypeNode 53714 probe).
    fn some_type_is_undefined_slice(&mut self, ty: TypeId) -> bool {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::UNION) {
            if let TypeData::Union { types, .. } = &self.tables.type_of(ty).data {
                return types
                    .iter()
                    .any(|&t| self.tables.flags_of(t).intersects(TypeFlags::UNDEFINED));
            }
        }
        flags.intersects(TypeFlags::UNDEFINED)
    }

    /// tsc-port: serializeReturnTypeForSignature @6.0.3
    /// tsc-hash: 894fb20ae6f5651fefa9cb149da299323b03e147cf90976c6a47ec2a9d8ad42d
    /// tsc-span: _tsc.js:53524-53556
    ///
    /// SuppressAnyReturnType is never set on the slice's contexts, so
    /// the suppress legs are dead and a node always yields. The
    /// syntactic arm first reuses an explicit annotation, then the
    /// reusable type assertion of a single return expression; the
    /// inferred arm renders the type predicate first (53548-53556).
    /// context.mapper re-instantiation of the predicate is identity
    /// here: getTypePredicateOfSignature already resolves through
    /// signature.target/mapper (narrow.rs), and enterNewScope's
    /// context.mapper IS signature.mapper, whose second application
    /// re-maps type parameters the instantiation already replaced.
    fn serialize_return_type_for_signature_slice(
        &mut self,
        signature: SignatureId,
        fully_qualified: bool,
    ) -> CheckResult<String> {
        let return_type = self.get_return_type_of_signature(signature)?;
        let declaration = self.signature_of(signature).declaration;
        if let Some(declaration) = declaration {
            let annotation = match self.data_of(declaration) {
                NodeData::FunctionDeclaration(data) => data.r#type,
                NodeData::FunctionExpression(data) => data.r#type,
                NodeData::ArrowFunction(data) => data.r#type,
                NodeData::MethodDeclaration(data) => data.r#type,
                NodeData::MethodSignature(data) => data.r#type,
                NodeData::CallSignature(data) => data.r#type,
                NodeData::ConstructSignature(data) => data.r#type,
                NodeData::FunctionType(data) => data.r#type,
                NodeData::ConstructorType(data) => data.r#type,
                NodeData::GetAccessor(data) => data.r#type,
                _ => None,
            };
            if let Some(annotation) = annotation {
                if let Some(text) = self.annotation_reuse_text_slice(
                    annotation,
                    return_type,
                    /*requires_adding_undefined*/ false,
                    /*question_equivalence*/ false,
                    /*is_parameter*/ false,
                )? {
                    return Ok(text);
                }
            }
            if let Some(text) = self.syntactic_single_return_type_text_slice(declaration)? {
                return Ok(text);
            }
        }
        if let Some(predicate) = self.get_type_predicate_of_signature(signature)? {
            return self.type_predicate_text_slice(&predicate, fully_qualified);
        }
        self.type_to_string_slice_ex(return_type, fully_qualified)
    }

    /// tsc-port: typeFromSingleReturnExpression @6.0.3 (the reusable
    /// assertion face exercised by diagnostic signature serialization).
    /// tsc-hash: 8598fa12646f02f556815a62d604714593fb45f0342beebadc7d224ff9649c37
    /// tsc-span: _tsc.js:134407-134438
    ///
    /// Keep the syntactic face separate from the semantic return type:
    /// the former preserves an assertion such as `number | string` in
    /// the enclosing function signature, while a nested relation still
    /// prints the canonical semantic union `string | number`. The full
    /// syntactic builder also derives primitive/function/literal faces;
    /// unsupported expression shapes deliberately fall through to the
    /// semantic serializer instead of duplicating that builder here.
    fn syntactic_single_return_type_text_slice(
        &mut self,
        declaration: NodeId,
    ) -> CheckResult<Option<String>> {
        if !matches!(
            self.kind_of(declaration),
            SyntaxKind::FunctionDeclaration
                | SyntaxKind::FunctionExpression
                | SyntaxKind::ArrowFunction
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::Constructor
                | SyntaxKind::GetAccessor
                | SyntaxKind::SetAccessor
        ) {
            return Ok(None);
        }
        if self.get_function_flags(declaration)
            & (crate::functions::FUNCTION_FLAGS_ASYNC | crate::functions::FUNCTION_FLAGS_GENERATOR)
            != 0
        {
            return Ok(None);
        }
        let source = self.binder.source_of_node(declaration);
        let Some(body) = node_util::body_of(source, declaration) else {
            return Ok(None);
        };
        if node_util::node_is_missing(source, Some(body)) {
            return Ok(None);
        }

        let candidate = if self.kind_of(body) == SyntaxKind::Block {
            let mut candidate = None;
            let invalid = self.for_each_return_statement(body, &mut |state, statement| {
                if state.parent_of(statement) != Some(body) {
                    candidate = None;
                    return true;
                }
                let expression = match state.data_of(statement) {
                    NodeData::ReturnStatement(data) => data.expression,
                    _ => None,
                };
                if candidate.is_some() {
                    candidate = None;
                    return true;
                }
                candidate = expression;
                false
            });
            if invalid {
                None
            } else {
                candidate
            }
        } else {
            Some(body)
        };
        let Some(mut expression) = candidate else {
            return Ok(None);
        };

        loop {
            if let Some(type_node) = self.jsdoc_type_assertion_type_node(expression) {
                return if self.is_const_type_reference_node(type_node) {
                    Ok(None)
                } else {
                    self.type_annotation_text_slice(type_node).map(Some)
                };
            }
            match self.data_of(expression) {
                NodeData::ParenthesizedExpression(data) => {
                    let Some(inner) = data.expression else {
                        return Ok(None);
                    };
                    expression = inner;
                }
                NodeData::AsExpression(data) => {
                    let Some(type_node) = data.r#type else {
                        return Ok(None);
                    };
                    return if self.is_const_type_reference_node(type_node) {
                        Ok(None)
                    } else {
                        self.type_annotation_text_slice(type_node).map(Some)
                    };
                }
                NodeData::TypeAssertionExpression(data) => {
                    let Some(type_node) = data.r#type else {
                        return Ok(None);
                    };
                    return if self.is_const_type_reference_node(type_node) {
                        Ok(None)
                    } else {
                        self.type_annotation_text_slice(type_node).map(Some)
                    };
                }
                _ => return Ok(None),
            }
        }
    }

    /// tsc-port: typePredicateToTypePredicateNodeHelper @6.0.3
    /// tsc-hash: ef7d04a8094c121ca47028327ba885afcb7a285a28adfe579ddff0335642b7f4
    /// tsc-span: _tsc.js:52840-52846
    fn type_predicate_text_slice(
        &mut self,
        predicate: &crate::narrow::TypePredicate,
        fully_qualified: bool,
    ) -> CheckResult<String> {
        use crate::narrow::TypePredicateKind;
        let asserts = matches!(
            predicate.kind,
            TypePredicateKind::AssertsThis | TypePredicateKind::AssertsIdentifier
        );
        let parameter = match predicate.kind {
            TypePredicateKind::Identifier | TypePredicateKind::AssertsIdentifier => {
                predicate.parameter_name.clone().unwrap_or_default()
            }
            TypePredicateKind::This | TypePredicateKind::AssertsThis => "this".to_owned(),
        };
        let asserts = if asserts { "asserts " } else { "" };
        match predicate.ty {
            Some(ty) => {
                let text = self.type_to_string_slice_ex(ty, fully_qualified)?;
                Ok(format!("{asserts}{parameter} is {text}"))
            }
            None => Ok(format!("{asserts}{parameter}")),
        }
    }

    /// tsc-port: typeParameterToDeclarationWithConstraint @6.0.3 (+
    /// typeParameterToDeclaration, typeToTypeNodeHelperWithPossibleReusableTypeNode)
    /// tsc-hash: 6f194529d9afac3f1f089536f4cbef76025aed4e4f96edc0fbb233acf1fcff9f
    /// tsc-span: _tsc.js:52822-52840
    ///
    /// Modifiers via getTypeParameterModifiers (67373-67376:
    /// declaration modifier union ∩ const/in/out — const and the
    /// variance pair are grammatically disjoint contexts, so the
    /// emission order is unobservable). The constraint節 rides the
    /// REUSABLE-node path (52832-52834): the declared constraint
    /// annotation prints whenever its unmapped type IS the current
    /// constraint — an instantiated (remapped) constraint fails the
    /// equality and renders structurally, which is exactly tsc's
    /// canReuseTypeNode TypeParameter-mapper rejection collapsed into
    /// one probe. Defaults NEVER reuse (52829: typeToTypeNodeHelper
    /// direct — oracle-probed: `= (A)` prints `= string`).
    fn type_parameter_to_declaration_slice(
        &mut self,
        type_parameter: TypeId,
        fully_qualified: bool,
    ) -> CheckResult<String> {
        let symbol = self
            .tables
            .type_of(type_parameter)
            .symbol
            .expect("TypeParameter types carry their declaration symbol");
        let mut modifiers = String::new();
        {
            let declarations = self.binder.symbol(symbol).declarations.clone();
            let mut has_const = false;
            let mut has_in = false;
            let mut has_out = false;
            for declaration in declarations {
                let source = self.binder.source_of_node(declaration);
                has_const |= tsc_binder::node_util::has_syntactic_modifier(
                    source,
                    declaration,
                    tsc_types::ModifierFlags::CONST,
                );
                has_in |= tsc_binder::node_util::has_syntactic_modifier(
                    source,
                    declaration,
                    tsc_types::ModifierFlags::IN,
                );
                has_out |= tsc_binder::node_util::has_syntactic_modifier(
                    source,
                    declaration,
                    tsc_types::ModifierFlags::OUT,
                );
            }
            if has_const {
                modifiers.push_str("const ");
            }
            if has_in {
                modifiers.push_str("in ");
            }
            if has_out {
                modifiers.push_str("out ");
            }
        }
        let name = self.symbol_display_name(symbol);
        let constraint = self.get_constraint_of_type_parameter(type_parameter)?;
        let constraint_text = match constraint {
            Some(constraint) => {
                let mut reused = None;
                if let Some(declaration) = self.get_constraint_declaration(type_parameter) {
                    let annotation_type = self.get_type_from_type_node(declaration)?;
                    if annotation_type == constraint {
                        reused = self.reusable_annotation_node_text_slice(declaration)?;
                    }
                }
                Some(match reused {
                    Some(text) => text,
                    None => self.type_to_string_slice_ex(constraint, fully_qualified)?,
                })
            }
            None => None,
        };
        let default = self.get_default_from_type_parameter(type_parameter)?;
        let default_text = match default {
            Some(default) => Some(self.type_to_string_slice_ex(default, fully_qualified)?),
            None => None,
        };
        let mut text = format!("{modifiers}{name}");
        if let Some(constraint_text) = constraint_text {
            text.push_str(" extends ");
            text.push_str(&constraint_text);
        }
        if let Some(default_text) = default_text {
            text.push_str(" = ");
            text.push_str(&default_text);
        }
        Ok(text)
    }

    /// tsc-port: canReuseTypeNodeAnnotation @6.0.3 (+
    /// typeNodeIsEquivalentToType 53511-53523 and the
    /// serializeExistingTypeNode addUndefined append 53712-53721)
    /// tsc-hash: edfd54626c63d3d1645a16cfcad8561dab1388e09a7278579ada789709becc6d
    /// tsc-span: _tsc.js:50932-50955
    ///
    /// The whole channel keys on the enclosingDeclaration the
    /// error-display entries set only for non-context-sensitive
    /// expression-valued symbols (getTypeNamesForErrorDisplay 50748)
    /// — without one, every annotation renders structurally (probed:
    /// declare-let sources drop parens/resolve aliases; fn-expression
    /// sources keep them). An annotation that resolves to the error
    /// type reuses unconditionally (50948-50950 — unresolved names
    /// print as written). Returns None = render structurally.
    fn annotation_reuse_text_slice(
        &mut self,
        annotation: NodeId,
        symbol_type: TypeId,
        requires_adding_undefined: bool,
        question_equivalence: bool,
        is_parameter: bool,
    ) -> CheckResult<Option<String>> {
        if self.slice_display_enclosing.is_none() {
            return Ok(None);
        }
        let annotation_type = self.get_type_from_type_node(annotation)?;
        if self.tables.is_error_type(annotation_type) {
            return self.reusable_annotation_node_text_slice(annotation);
        }
        let compared_annotation_type = if requires_adding_undefined {
            // addOptionality(annotationType, !isParameter) — the
            // strictNullChecks gate held upstream.
            self.get_optional_type(annotation_type, /*is_property*/ !is_parameter)?
        } else {
            annotation_type
        };
        let equivalent = compared_annotation_type == symbol_type
            || (question_equivalence && {
                let without_undefined =
                    self.get_type_with_facts(symbol_type, TypeFacts::NE_UNDEFINED)?;
                without_undefined == compared_annotation_type
            });
        if !equivalent {
            return Ok(None);
        }
        if !self.reference_annotation_argument_count_compatible(annotation, symbol_type)? {
            return Ok(None);
        }
        let Some(text) = self.reusable_annotation_node_text_slice(annotation)? else {
            return Ok(None);
        };
        // serializeExistingTypeNode (53712-53721): the undefined
        // union appends when the annotation itself lacks it.
        if requires_adding_undefined && !self.some_type_is_undefined_slice(annotation_type) {
            return Ok(Some(format!("{text} | undefined")));
        }
        Ok(Some(text))
    }

    /// tsc-port: existingTypeNodeIsNotReferenceOrIsReferenceWithCompatibleTypeArgumentCount @6.0.3
    /// tsc-hash: f818acd066ea9e59b4904508233bd6c6a70ce3ca8f8ae6bfc0c29da862399853
    /// tsc-span: _tsc.js:53665-53674
    fn reference_annotation_argument_count_compatible(
        &mut self,
        annotation: NodeId,
        ty: TypeId,
    ) -> CheckResult<bool> {
        if !self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::REFERENCE)
        {
            return Ok(true);
        }
        let NodeData::TypeReference(data) = self.data_of(annotation) else {
            return Ok(true);
        };
        let argument_count = self.nodes_of(data.type_arguments).len();
        let Some(symbol) = self.links.node(annotation).resolved_symbol.resolved() else {
            return Ok(true);
        };
        let declared = self.get_declared_type_of_symbol_slice(symbol)?;
        let target = self.tables.reference_target(ty);
        if declared != target {
            return Ok(true);
        }
        let type_parameters = match &self.tables.type_of(target).data {
            TypeData::GenericType {
                type_parameters, ..
            } => Some(type_parameters.clone()),
            _ => None,
        };
        Ok(argument_count >= self.get_min_type_argument_count(type_parameters.as_deref()))
    }

    /// tsc-port: getTypeNamesForErrorDisplay @6.0.3 (enclosing probe)
    /// tsc-hash: 2cb44b742f2abb8976c29d155182a513e19a7d2832c0d0cc11f93104230219d0
    /// tsc-span: _tsc.js:50748-50767
    ///
    /// symbolValueDeclarationIsContextSensitive: the source/target of
    /// a relation error render with the symbol's value declaration as
    /// enclosingDeclaration when it is an expression and NOT
    /// context-sensitive — which arms the annotation-reuse channel
    /// (oracle-probed: `let g = (x?: number) => {}` displays
    /// `(x?: number) => void` where the declare-let twin prints
    /// `(x?: number | undefined) => void`).
    pub(crate) fn slice_display_enclosing_for(&mut self, ty: TypeId) -> Option<NodeId> {
        let symbol = self.tables.type_of(ty).symbol?;
        let value_declaration = self.binder.symbol(symbol).value_declaration?;
        let source = self.binder.source_of_node(value_declaration);
        if !tsc_binder::node_util::is_expression_node(source, value_declaration) {
            return None;
        }
        if self.is_context_sensitive(value_declaration) {
            return None;
        }
        Some(value_declaration)
    }

    /// tsrs-native: the enclosing-scoped render for one relation-error
    /// side (the state-parked face of getTypeNamesForErrorDisplay's
    /// per-side typeToString(type, valueDeclaration) call); the
    /// enclosing restores across the Err unwind (CheckAbort rides
    /// `?` past the reset otherwise).
    pub(crate) fn type_to_string_slice_with_error_enclosing(
        &mut self,
        ty: TypeId,
    ) -> CheckResult<String> {
        let enclosing = self.slice_display_enclosing_for(ty);
        let saved = std::mem::replace(&mut self.slice_display_enclosing, enclosing);
        let result = self.type_to_string_slice(ty);
        self.slice_display_enclosing = saved;
        result
    }

    /// tsrs-native: explicit-enclosing adapter for tsc's
    /// `typeToString(type, enclosingDeclaration)` calls. The parked
    /// nodeBuilder context is restored on both success and
    /// CheckAbort unwind.
    pub(crate) fn type_to_string_slice_at(
        &mut self,
        ty: TypeId,
        enclosing: NodeId,
    ) -> CheckResult<String> {
        let saved = self.slice_display_enclosing.replace(enclosing);
        let result = self.type_to_string_slice(ty);
        self.slice_display_enclosing = saved;
        result
    }

    /// tsc-port: tryReuseExistingTypeNode @6.0.3 (bounded printer)
    /// tsc-hash: dd3b6d1408c0a1685cfb3e3d107db34a442f77ff62ee7d0e9b42945b063b6cf7
    /// tsc-span: _tsc.js:133283-133292
    ///
    /// The reuse path prints a CLONE of the annotation through the
    /// printer (visitExistingNodeTreeSymbols → factory.cloneNode):
    /// synthesized literals lose source spellings (0x10 prints its
    /// cooked text 16, string literals re-quote double — both
    /// oracle-probed), type-literal members re-join with `; `, and
    /// everything else keeps its structure — parentheses, union
    /// order, alias spellings. The visitor's tracker-driven rewrites
    /// are observable: inaccessible entity names recover through the
    /// semantic serializer, dynamic computed declarations can be
    /// removed, and missing declaration annotations become `any`.
    /// The printer below covers every valid TypeNode/JSDoc TypeNode
    /// shape admitted by tryReuseExistingTypeNode.
    fn reusable_annotation_node_text_slice(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        if !self.can_reuse_existing_type_node_slice(node)? {
            return Ok(None);
        }
        Ok(self
            .reused_type_node_boundary_face_slice(node)?
            .map(|face| face.text))
    }

    /// The type-node printer behind the reuse faces: the standard
    /// printer's emission for cloned annotation ASTs, including import,
    /// mapped, conditional, infer, template, binding-pattern, and
    /// JSDoc-lowered shapes. Invalid-but-checker-visible FunctionType
    /// and TypeElement parameters can carry initializers; their clone
    /// path stays distinct from semantic signature synthesis.
    ///
    /// JSDoc nodes do not print in their source grammar here.
    /// visitExistingNodeTreeSymbolsWorker lowers them to ordinary
    /// TypeNodes before the cloned tree reaches the printer: `*` /
    /// namepaths become `any`, `?` becomes `unknown`, nullable and
    /// optional wrappers become unions, non-null wrappers disappear,
    /// and variadics become arrays.
    /// tsrs-native: string-face adapter over the exact existing-TypeNode
    /// visitor and standard-printer ledger blocks below.
    pub(crate) fn type_annotation_text_slice(&mut self, node: NodeId) -> CheckResult<String> {
        Ok(self.type_annotation_face_slice(node)?.text)
    }

    /// The actual transformed node produced by
    /// visitExistingNodeTreeSymbolsWorker, carried as printer text plus
    /// its POST-TRANSFORM factory kind. Semantic recovery and
    /// serializeTypeName can change the kind, so this cannot be
    /// reconstructed from the original AST after printing.
    fn type_annotation_text_and_kind_slice(
        &mut self,
        node: NodeId,
    ) -> CheckResult<(String, SliceTypeNodeKind)> {
        let face = self.type_annotation_face_slice(node)?;
        Ok((face.text, face.kind))
    }

    fn type_annotation_face_slice(&mut self, node: NodeId) -> CheckResult<SliceTypeNodeFace> {
        if self.slice_reuse_visit_depth == 0 {
            return match self.reused_type_node_boundary_face_slice(node)? {
                Some(face) => Ok(face),
                None => self.semantic_existing_type_node_face_slice(node),
            };
        }
        self.visit_type_annotation_face_slice(node)
    }

    /// tsc-port: createRecoveryBoundary @6.0.3
    /// tsc-hash: 458d082bf43dddc458d7af70e8d8ab78cc5d7e3e2d7614a2df953f48682a4cfe
    /// tsc-span: _tsc.js:52612-52673
    ///
    /// tsc-port: tryReuseExistingTypeNode @6.0.3
    /// tsc-hash: 1597d2da65389394062d7e4fef36939ca2d42afba2d661505d1fc104ef7e7f41
    /// tsc-span: _tsc.js:133293-133317
    ///
    /// Each tryReuseExistingTypeNode owns a fresh boundary. Saving the
    /// parked cell/depth makes semantic serialization re-entry an
    /// independent nested boundary and restores both fields on Err.
    fn reused_type_node_boundary_face_slice(
        &mut self,
        node: NodeId,
    ) -> CheckResult<Option<SliceTypeNodeFace>> {
        let saved_had_error = std::mem::replace(&mut self.slice_reuse_had_error, false);
        let saved_depth = std::mem::replace(&mut self.slice_reuse_visit_depth, 0);
        let result = self.visit_type_annotation_face_slice(node);
        let had_error = self.slice_reuse_had_error;
        self.slice_reuse_had_error = saved_had_error;
        self.slice_reuse_visit_depth = saved_depth;
        result.map(|face| (!had_error).then_some(face))
    }

    fn visit_type_annotation_face_slice(&mut self, node: NodeId) -> CheckResult<SliceTypeNodeFace> {
        // visitExistingNodeTreeSymbols returns the existing node
        // immediately once a sibling armed the shared boundary.
        if self.slice_reuse_had_error {
            return Ok(SliceTypeNodeFace::new(
                String::new(),
                self.type_annotation_node_kind_slice(node),
            ));
        }
        self.slice_reuse_visit_depth += 1;
        let result = self.type_annotation_face_worker_slice(node);
        self.slice_reuse_visit_depth -= 1;
        let mut face = result?;
        if self.slice_reuse_had_error && self.kind_of(node) != SyntaxKind::TypePredicate {
            // startRecoveryScope's closure clears the boundary before
            // serializeExistingTypeNode rebuilds the current TypeNode.
            self.slice_reuse_had_error = false;
            face = self.semantic_existing_type_node_face_slice(node)?;
        }
        Ok(face)
    }

    fn type_annotation_face_worker_slice(
        &mut self,
        node: NodeId,
    ) -> CheckResult<SliceTypeNodeFace> {
        if self.is_empty_jsdoc_type_reference_slice(node) {
            return Ok(SliceTypeNodeFace::new(
                "any".to_owned(),
                SliceTypeNodeKind::Keyword,
            ));
        }
        if let Some(index) = self.jsdoc_index_signature_text_slice(node)? {
            return Ok(SliceTypeNodeFace::new(
                index,
                SliceTypeNodeKind::TypeLiteral,
            ));
        }
        if self.kind_of(node) == SyntaxKind::ThisType
            && !self.can_reuse_existing_type_node_slice(node)?
        {
            return self.semantic_existing_type_node_face_slice(node);
        }
        match self.data_of(node).clone() {
            NodeData::TypeReference(_) => match self.try_visit_type_reference_face_slice(node)? {
                Some(face) => Ok(face),
                None => {
                    self.slice_reuse_had_error = true;
                    Ok(SliceTypeNodeFace::new(
                        String::new(),
                        SliceTypeNodeKind::Reference,
                    ))
                }
            },
            NodeData::TypeQuery(_) => match self.try_visit_type_query_face_slice(node)? {
                Some(face) => Ok(face),
                None => {
                    self.slice_reuse_had_error = true;
                    Ok(SliceTypeNodeFace::new(
                        String::new(),
                        SliceTypeNodeKind::TypeQuery,
                    ))
                }
            },
            NodeData::ImportType(data) => {
                if self.literal_import_type_has_assert_attributes_slice(&data)
                    || !self.can_reuse_existing_type_node_slice(node)?
                {
                    self.semantic_existing_type_node_face_slice(node)
                } else {
                    Ok(SliceTypeNodeFace::new(
                        self.type_annotation_text_slice_raw(node)?,
                        SliceTypeNodeKind::ImportType,
                    ))
                }
            }
            NodeData::TypeOperator(data)
                if data.operator == SyntaxKind::UniqueKeyword
                    && data
                        .r#type
                        .is_some_and(|inner| self.kind_of(inner) == SyntaxKind::SymbolKeyword)
                    && !self.can_reuse_existing_type_node_slice(node)? =>
            {
                self.semantic_existing_type_node_face_slice(node)
            }
            _ => {
                let text = self.type_annotation_text_slice_raw(node)?;
                let kind = self.type_annotation_node_kind_slice(node);
                Ok(SliceTypeNodeFace {
                    text,
                    kind,
                    has_type_parameters: self.generic_function_or_constructor_type_node_slice(node),
                })
            }
        }
    }

    /// tsc-port: tryVisitSimpleTypeNode @6.0.3
    /// tsc-hash: a9055f86215bdbd7003f32be0b2dc25e3cf71b6913cbc4b97bb858033d233f1d
    /// tsc-span: _tsc.js:133316-133332
    ///
    /// tsc-port: tryVisitIndexedAccess @6.0.3
    /// tsc-hash: 4f284aaa5552320115ad8e5e2c2f6b1f482593d4015236e45dcdf533a3c779aa
    /// tsc-span: _tsc.js:133333-133339
    ///
    /// tsc-port: tryVisitKeyOf @6.0.3
    /// tsc-hash: 1af02b6f4e1a58d7e4d91d75bd9c731db04ff875c42d90ae37aab672ece1db2f
    /// tsc-span: _tsc.js:133340-133347
    ///
    /// tsc-port: tryVisitTypeQuery @6.0.3
    /// tsc-hash: 0ea38917ef4438f9065f4c7f904e3df7be0a26dc60e934e6eed5519105b32ff3
    /// tsc-span: _tsc.js:133348-133366
    ///
    /// tsc-port: tryVisitTypeReference @6.0.3
    /// tsc-hash: f20025033699cce2ad6ed94d59dde522079781878049462bbdd695b8f78694a8
    /// tsc-span: _tsc.js:133367-133391
    ///
    /// The simple-node path intentionally strips parentheses only when
    /// the inner node is TypeReference, TypeQuery, IndexedAccess, or
    /// keyof. It also bypasses that child's recovery wrapper: a failed
    /// name serialization therefore recovers the enclosing indexed
    /// access/keyof node as one semantic unit.
    fn try_visit_simple_type_node_face_slice(
        &mut self,
        node: NodeId,
    ) -> CheckResult<Option<SliceTypeNodeFace>> {
        let inner = self.skip_type_parentheses_slice(node);
        match self.data_of(inner) {
            NodeData::TypeReference(_) => self.try_visit_type_reference_face_slice(inner),
            NodeData::TypeQuery(_) => self.try_visit_type_query_face_slice(inner),
            NodeData::IndexedAccessType(_) => self.try_visit_indexed_access_face_slice(inner),
            NodeData::TypeOperator(data) if data.operator == SyntaxKind::KeyOfKeyword => {
                self.try_visit_keyof_face_slice(inner)
            }
            _ => self.visit_type_annotation_face_slice(node).map(Some),
        }
    }

    fn skip_type_parentheses_slice(&self, mut node: NodeId) -> NodeId {
        while let NodeData::ParenthesizedType(data) = self.data_of(node) {
            let Some(inner) = data.r#type else {
                break;
            };
            node = inner;
        }
        node
    }

    fn try_visit_indexed_access_face_slice(
        &mut self,
        node: NodeId,
    ) -> CheckResult<Option<SliceTypeNodeFace>> {
        let NodeData::IndexedAccessType(data) = self.data_of(node).clone() else {
            unreachable!("tryVisitIndexedAccess receives IndexedAccessType");
        };
        let object_node = data
            .object_type
            .expect("IndexedAccessType carries its object type");
        let Some(object_face) = self.try_visit_simple_type_node_face_slice(object_node)? else {
            return Ok(None);
        };
        let index_node = data
            .index_type
            .expect("IndexedAccessType carries its index type");
        let index_face = self.visit_type_annotation_face_slice(index_node)?;
        let object = if non_array_postfix_operand_needs_parens(object_face.kind) {
            format!("({})", object_face.text)
        } else {
            object_face.text
        };
        Ok(Some(SliceTypeNodeFace::new(
            format!("{object}[{}]", index_face.text),
            SliceTypeNodeKind::IndexedAccess,
        )))
    }

    fn try_visit_keyof_face_slice(
        &mut self,
        node: NodeId,
    ) -> CheckResult<Option<SliceTypeNodeFace>> {
        let NodeData::TypeOperator(data) = self.data_of(node).clone() else {
            unreachable!("tryVisitKeyOf receives TypeOperator");
        };
        debug_assert_eq!(data.operator, SyntaxKind::KeyOfKeyword);
        let operand_node = data.r#type.expect("keyof carries its operand");
        let Some(operand_face) = self.try_visit_simple_type_node_face_slice(operand_node)? else {
            return Ok(None);
        };
        let operand = if type_operator_operand_needs_parens(operand_face.kind) {
            format!("({})", operand_face.text)
        } else {
            operand_face.text
        };
        Ok(Some(SliceTypeNodeFace::new(
            format!("keyof {operand}"),
            SliceTypeNodeKind::TypeOperator,
        )))
    }

    fn try_visit_type_query_face_slice(
        &mut self,
        node: NodeId,
    ) -> CheckResult<Option<SliceTypeNodeFace>> {
        let NodeData::TypeQuery(data) = self.data_of(node).clone() else {
            unreachable!("tryVisitTypeQuery receives TypeQuery");
        };
        let expr_name = data
            .expr_name
            .expect("TypeQuery carries its expression name");
        if self.reused_entity_name_introduces_error_slice(expr_name, SymbolFlags::VALUE)? {
            return Ok(self
                .serialize_reused_type_name_slice(expr_name, SymbolFlags::VALUE, &[])?
                .map(|(text, kind)| SliceTypeNodeFace::new(text, kind)));
        }
        let rendered = self.type_argument_nodes_text_slice(self.nodes_of(data.type_arguments))?;
        let name = self.entity_name_text_slice(expr_name)?;
        let text = if rendered.is_empty() {
            format!("typeof {name}")
        } else {
            format!("typeof {name}<{}>", rendered.join(", "))
        };
        Ok(Some(SliceTypeNodeFace::new(
            text,
            SliceTypeNodeKind::TypeQuery,
        )))
    }

    fn try_visit_type_reference_face_slice(
        &mut self,
        node: NodeId,
    ) -> CheckResult<Option<SliceTypeNodeFace>> {
        let NodeData::TypeReference(data) = self.data_of(node).clone() else {
            unreachable!("tryVisitTypeReference receives TypeReference");
        };
        if !self.can_reuse_existing_type_node_slice(node)? {
            return Ok(None);
        }
        let type_name = data.type_name.expect("TypeReference carries its type name");
        let meaning = self.type_reference_entity_meaning_slice(type_name);
        let introduces_error =
            self.reused_entity_name_introduces_error_slice(type_name, meaning)?;
        // tsc visits typeArguments before branching on introducesError.
        let rendered = self.type_argument_nodes_text_slice(self.nodes_of(data.type_arguments))?;
        if introduces_error {
            return Ok(self
                .serialize_reused_type_name_slice(type_name, SymbolFlags::TYPE, &rendered)?
                .map(|(text, kind)| SliceTypeNodeFace::new(text, kind)));
        }
        let name = self.entity_name_text_slice(type_name)?;
        let text = if rendered.is_empty() {
            name
        } else {
            format!("{name}<{}>", rendered.join(", "))
        };
        Ok(Some(SliceTypeNodeFace::new(
            text,
            SliceTypeNodeKind::Reference,
        )))
    }

    fn type_annotation_text_slice_raw(&mut self, node: NodeId) -> CheckResult<String> {
        // visitExistingNodeTreeSymbolsWorker's two TypeReference
        // special cases precede the ordinary TypeReference visitor.
        if self.is_empty_jsdoc_type_reference_slice(node) {
            return Ok("any".to_owned());
        }
        if let Some(index) = self.jsdoc_index_signature_text_slice(node)? {
            return Ok(index);
        }
        // canReuseTypeNode rejects JSDoc references with an intended
        // TypeScript type. tryReuseExistingTypeNode then replaces that
        // subtree through serializeExistingTypeNode/typeToTypeNodeHelper
        // while preserving the reusable parent structure.
        if self.is_jsdoc_type_reference(node) {
            if let Some(intended) = self.get_intended_type_from_jsdoc_type_reference(node)? {
                return self.type_to_string_slice_ex(intended, /*fully_qualified*/ false);
            }
        }
        // Keyword type nodes are kind-distinguished tokens.
        match self.kind_of(node) {
            SyntaxKind::StringKeyword => return Ok("string".to_owned()),
            SyntaxKind::NumberKeyword => return Ok("number".to_owned()),
            SyntaxKind::BooleanKeyword => return Ok("boolean".to_owned()),
            SyntaxKind::AnyKeyword => return Ok("any".to_owned()),
            SyntaxKind::UnknownKeyword => return Ok("unknown".to_owned()),
            SyntaxKind::VoidKeyword => return Ok("void".to_owned()),
            SyntaxKind::UndefinedKeyword => return Ok("undefined".to_owned()),
            SyntaxKind::NeverKeyword => return Ok("never".to_owned()),
            SyntaxKind::ObjectKeyword => return Ok("object".to_owned()),
            SyntaxKind::SymbolKeyword => return Ok("symbol".to_owned()),
            SyntaxKind::BigIntKeyword => return Ok("bigint".to_owned()),
            SyntaxKind::IntrinsicKeyword => return Ok("intrinsic".to_owned()),
            SyntaxKind::ThisType => return Ok("this".to_owned()),
            _ => {}
        }
        match self.data_of(node).clone() {
            // tsc-port: visitExistingNodeTreeSymbolsWorker's JSDoc
            // lowering @6.0.3.
            // tsc-hash: 8b4acd6f23476915bdfabab514bac75c2ea60ed2d25b510088a3f55c78028978
            // tsc-span: _tsc.js:133393-133484
            NodeData::JSDocTypeExpression(data) => self.type_annotation_text_slice(
                data.r#type
                    .expect("JSDocTypeExpression carries its type node"),
            ),
            NodeData::JSDocAllType(_) | NodeData::JSDocNamepathType(_) => Ok("any".to_owned()),
            NodeData::JSDocUnknownType(_) => Ok("unknown".to_owned()),
            NodeData::JSDocNullableType(data) => {
                let inner = data.r#type.expect("JSDocNullableType carries its type");
                let (inner_text, inner_kind) = self.visited_type_node_text_slice(inner)?;
                let inner_text = if union_constituent_needs_parens(inner_kind) {
                    format!("({inner_text})")
                } else {
                    inner_text
                };
                Ok(format!("{inner_text} | null"))
            }
            NodeData::JSDocOptionalType(data) => {
                let inner = data.r#type.expect("JSDocOptionalType carries its type");
                let (inner_text, inner_kind) = self.visited_type_node_text_slice(inner)?;
                let inner_text = if union_constituent_needs_parens(inner_kind) {
                    format!("({inner_text})")
                } else {
                    inner_text
                };
                Ok(format!("{inner_text} | undefined"))
            }
            NodeData::JSDocNonNullableType(data) => self.type_annotation_text_slice(
                data.r#type.expect("JSDocNonNullableType carries its type"),
            ),
            NodeData::JSDocVariadicType(data) => {
                let inner = data.r#type.expect("JSDocVariadicType carries its type");
                let (inner_text, inner_kind) = self.visited_type_node_text_slice(inner)?;
                Ok(array_type_node_text(inner_text, inner_kind))
            }
            NodeData::JSDocFunctionType(data) => self.with_reused_node_scope_slice(node, |state| {
                state.jsdoc_function_type_text_slice(node, data)
            }),
            NodeData::JSDocTypeLiteral(data) => self.jsdoc_type_literal_text_slice(node, data),
            NodeData::ParenthesizedType(data) => {
                let inner = data.r#type.expect("ParenthesizedType carries its type");
                Ok(format!("({})", self.type_annotation_text_slice(inner)?))
            }
            NodeData::TypeReference(_) => {
                unreachable!("TypeReference is handled by type_annotation_text_and_kind_slice")
            }
            NodeData::UnionType(data) => {
                let mut rendered = Vec::new();
                for member in self.nodes_of(data.types) {
                    let (text, kind) = self.visited_type_node_text_slice(member)?;
                    rendered.push(if union_constituent_needs_parens(kind) {
                        format!("({text})")
                    } else {
                        text
                    });
                }
                Ok(rendered.join(" | "))
            }
            NodeData::IntersectionType(data) => {
                let mut rendered = Vec::new();
                for member in self.nodes_of(data.types) {
                    let (text, kind) = self.visited_type_node_text_slice(member)?;
                    rendered.push(if intersection_constituent_needs_parens(kind) {
                        format!("({text})")
                    } else {
                        text
                    });
                }
                Ok(rendered.join(" & "))
            }
            NodeData::ArrayType(data) => {
                let element = data
                    .element_type
                    .expect("ArrayType carries its element type");
                let (element_text, element_kind) = self.visited_type_node_text_slice(element)?;
                Ok(array_type_node_text(element_text, element_kind))
            }
            NodeData::TupleType(data) => {
                let mut rendered = Vec::new();
                for element in self.nodes_of(data.elements) {
                    rendered.push(self.type_annotation_text_slice(element)?);
                }
                Ok(format!("[{}]", rendered.join(", ")))
            }
            NodeData::NamedTupleMember(data) => {
                let dots = if data.dot_dot_dot_token.is_some() {
                    "..."
                } else {
                    ""
                };
                let name = self.entity_name_text_slice(
                    data.name.expect("NamedTupleMember carries its name"),
                )?;
                let question = if data.question_token.is_some() {
                    "?"
                } else {
                    ""
                };
                let ty = self.type_annotation_text_slice(
                    data.r#type.expect("NamedTupleMember carries its type"),
                )?;
                Ok(format!("{dots}{name}{question}: {ty}"))
            }
            NodeData::OptionalType(data) => {
                let inner = data.r#type.expect("OptionalType carries its type");
                let (text, kind) = self.visited_type_node_text_slice(inner)?;
                let text = if optional_type_operand_needs_parens(kind) {
                    format!("({text})")
                } else {
                    text
                };
                Ok(format!("{text}?"))
            }
            NodeData::RestType(data) => {
                let inner = data.r#type.expect("RestType carries its type");
                Ok(format!("...{}", self.type_annotation_text_slice(inner)?))
            }
            NodeData::TypeOperator(data) => {
                if data.operator == SyntaxKind::KeyOfKeyword {
                    return match self.try_visit_keyof_face_slice(node)? {
                        Some(face) => Ok(face.text),
                        None => {
                            self.slice_reuse_had_error = true;
                            Ok(String::new())
                        }
                    };
                }
                let operator = match data.operator {
                    SyntaxKind::ReadonlyKeyword => "readonly",
                    SyntaxKind::UniqueKeyword => "unique",
                    _ => unreachable!("TypeOperator carries keyof/readonly/unique"),
                };
                let inner = data.r#type.expect("TypeOperator carries its operand");
                let (text, kind) = self.visited_type_node_text_slice(inner)?;
                let needs_parens = if data.operator == SyntaxKind::ReadonlyKeyword {
                    readonly_type_operator_operand_needs_parens(kind)
                } else {
                    type_operator_operand_needs_parens(kind)
                };
                let text = if needs_parens {
                    format!("({text})")
                } else {
                    text
                };
                Ok(format!("{operator} {text}"))
            }
            NodeData::TypeQuery(_) => {
                unreachable!("TypeQuery is handled by type_annotation_text_and_kind_slice")
            }
            NodeData::IndexedAccessType(_) => {
                match self.try_visit_indexed_access_face_slice(node)? {
                    Some(face) => Ok(face.text),
                    None => {
                        self.slice_reuse_had_error = true;
                        Ok(String::new())
                    }
                }
            }
            NodeData::LiteralType(data) => self.literal_type_node_text_slice(
                data.literal.expect("LiteralType carries its literal"),
            ),
            NodeData::TypePredicate(data) => {
                let asserts = if data.asserts_modifier.is_some() {
                    "asserts "
                } else {
                    ""
                };
                let parameter_name = data
                    .parameter_name
                    .expect("TypePredicate carries its parameter name");
                let parameter = if self.kind_of(parameter_name) == SyntaxKind::ThisType {
                    "this".to_owned()
                } else {
                    if self.reused_entity_name_introduces_error_slice(
                        parameter_name,
                        SymbolFlags::VALUE,
                    )? {
                        // TypePredicate is the one TypeNode that does
                        // not recover its own scope. Its enclosing
                        // FunctionType/JSDocFunctionType consumes this
                        // armed boundary and is rebuilt semantically.
                        self.slice_reuse_had_error = true;
                    }
                    self.entity_name_text_slice(parameter_name)?
                };
                match data.r#type {
                    Some(ty) => Ok(format!(
                        "{asserts}{parameter} is {}",
                        self.type_annotation_text_slice(ty)?
                    )),
                    None => Ok(format!("{asserts}{parameter}")),
                }
            }
            NodeData::FunctionType(data) => self.with_reused_node_scope_slice(node, |state| {
                let type_parameters =
                    state.type_parameter_nodes_text_slice(state.nodes_of(data.type_parameters))?;
                let parameters =
                    state.parameter_nodes_text_slice(state.nodes_of(data.parameters))?;
                let ret = match data.r#type {
                    Some(ret) => state.type_annotation_text_slice(ret)?,
                    None => "any".to_owned(),
                };
                Ok(format!("{type_parameters}({parameters}) => {ret}"))
            }),
            NodeData::ConstructorType(data) => self.with_reused_node_scope_slice(node, |state| {
                let is_abstract = {
                    let source = state.binder.source_of_node(node);
                    tsc_binder::node_util::has_syntactic_modifier(
                        source,
                        node,
                        tsc_types::ModifierFlags::ABSTRACT,
                    )
                };
                let modifier = if is_abstract { "abstract " } else { "" };
                let type_parameters =
                    state.type_parameter_nodes_text_slice(state.nodes_of(data.type_parameters))?;
                let parameters =
                    state.parameter_nodes_text_slice(state.nodes_of(data.parameters))?;
                let ret = match data.r#type {
                    Some(ret) => state.type_annotation_text_slice(ret)?,
                    None => "any".to_owned(),
                };
                Ok(format!(
                    "{modifier}new {type_parameters}({parameters}) => {ret}"
                ))
            }),
            NodeData::TypeLiteral(data) => {
                let members = self.nodes_of(data.members);
                if members.is_empty() {
                    return Ok("{}".to_owned());
                }
                let mut rendered = Vec::with_capacity(members.len());
                for member_node in members {
                    if let Some(member) = self.type_literal_member_text_slice(member_node)? {
                        rendered.push(if self.type_literal_member_has_body_slice(member_node) {
                            member
                        } else {
                            format!("{member};")
                        });
                    }
                }
                if rendered.is_empty() {
                    return Ok("{}".to_owned());
                }
                Ok(format!("{{ {} }}", rendered.join(" ")))
            }
            NodeData::ConditionalType(data) => {
                let check_node = data
                    .check_type
                    .expect("ConditionalType carries its check type");
                let (mut check, check_kind) = self.visited_type_node_text_slice(check_node)?;
                if conditional_check_type_needs_parens(check_kind) {
                    check = format!("({check})");
                }
                // The inferred type parameters enter scope only for
                // extendsType and trueType; checkType and falseType
                // remain in the outer nodeBuilder scope.
                let (extends, when_true) = self.with_reused_node_scope_slice(node, |state| {
                    let extends_node = data
                        .extends_type
                        .expect("ConditionalType carries its extends type");
                    let (mut extends, extends_kind) =
                        state.visited_type_node_text_slice(extends_node)?;
                    if extends_kind == SliceTypeNodeKind::Conditional {
                        extends = format!("({extends})");
                    }
                    let when_true = state.type_annotation_text_slice(
                        data.true_type
                            .expect("ConditionalType carries its true type"),
                    )?;
                    Ok((extends, when_true))
                })?;
                let when_false = self.type_annotation_text_slice(
                    data.false_type
                        .expect("ConditionalType carries its false type"),
                )?;
                Ok(format!(
                    "{check} extends {extends} ? {when_true} : {when_false}"
                ))
            }
            NodeData::InferType(data) => {
                let parameter = data
                    .type_parameter
                    .expect("InferType carries its type parameter");
                Ok(format!(
                    "infer {}",
                    self.type_parameter_node_text_slice(parameter)?
                ))
            }
            NodeData::MappedType(data) => self.with_reused_node_scope_slice(node, |state| {
                let readonly = mapped_modifier_text_slice(
                    data.readonly_token.map(|token| state.kind_of(token)),
                    "readonly ",
                );
                let parameter = data
                    .type_parameter
                    .expect("MappedType carries its type parameter");
                let NodeData::TypeParameter(parameter_data) = state.data_of(parameter).clone()
                else {
                    unreachable!("MappedType type_parameter is a TypeParameter node");
                };
                let name = state.entity_name_text_slice(
                    parameter_data
                        .name
                        .expect("mapped type parameter carries its name"),
                )?;
                let constraint = state.type_annotation_text_slice(
                    parameter_data
                        .constraint
                        .expect("mapped type parameter carries its in-type"),
                )?;
                let name_type = match data.name_type {
                    Some(name_type) => {
                        format!(" as {}", state.type_annotation_text_slice(name_type)?)
                    }
                    None => String::new(),
                };
                let question = mapped_modifier_text_slice(
                    data.question_token.map(|token| state.kind_of(token)),
                    "?",
                );
                let value = match data.r#type {
                    Some(value) => state.type_annotation_text_slice(value)?,
                    None => String::new(),
                };
                let mut members = Vec::new();
                for member_node in state.nodes_of(data.members) {
                    if let Some(member) = state.type_literal_member_text_slice(member_node)? {
                        members.push((
                            member_node,
                            if state.type_literal_member_has_body_slice(member_node) {
                                member
                            } else {
                                format!("{member};")
                            },
                        ));
                    }
                }
                // emitMappedType always writes `: ` and the mapped
                // field's trailing semicolon and then one space in its
                // forced-SingleLine branch. Recovery members themselves
                // use ListFormat PreserveLines: same-line siblings touch,
                // while a source line event contributes only the current
                // display-writer indentation; the closing brace follows
                // the same last-member/end-line rule.
                let mut tail = " ".to_owned();
                let mut previous = None;
                for (member_node, member) in &members {
                    let line_event = match previous {
                        Some(previous) => matches!(
                            (
                                state.display_clone_end_line(previous),
                                state.display_clone_start_line(*member_node),
                            ),
                            (Some(previous), Some(member)) if previous != member
                        ),
                        None => matches!(
                            (
                                state.display_clone_start_line(node),
                                state.display_clone_start_line(*member_node),
                            ),
                            (Some(parent), Some(member)) if parent != member
                        ),
                    };
                    if line_event {
                        tail.push_str(&state.display_clone_line_indent());
                    }
                    tail.push_str(member);
                    previous = Some(*member_node);
                }
                if let Some(last) = previous {
                    if matches!(
                        (
                            state.display_clone_end_line(node),
                            state.display_clone_end_line(last),
                        ),
                        (Some(parent), Some(member)) if parent != member
                    ) {
                        tail.push_str(&state.display_clone_line_indent());
                    }
                }
                Ok(format!(
                    "{{ {readonly}[{name} in {constraint}{name_type}]{question}: {value};{tail}}}"
                ))
            }),
            NodeData::ImportType(data) => {
                if self.literal_import_type_has_assert_attributes_slice(&data) {
                    return self.semantic_existing_type_node_text_slice(node);
                }
                let argument = self.type_annotation_text_slice(
                    data.argument.expect("ImportType carries its argument type"),
                )?;
                let attributes = match data.attributes {
                    Some(attributes) => {
                        format!(", {}", self.import_attributes_text_slice(attributes)?)
                    }
                    None => String::new(),
                };
                let qualifier = match data.qualifier {
                    Some(qualifier) => {
                        format!(".{}", self.entity_name_text_slice(qualifier)?)
                    }
                    None => String::new(),
                };
                let arguments = self.nodes_of(data.type_arguments);
                let type_arguments = if arguments.is_empty() {
                    String::new()
                } else {
                    let rendered = self.type_argument_nodes_text_slice(arguments)?;
                    format!("<{}>", rendered.join(", "))
                };
                let type_of = if data.is_type_of { "typeof " } else { "" };
                Ok(format!(
                    "{type_of}import({argument}{attributes}){qualifier}{type_arguments}"
                ))
            }
            NodeData::TemplateLiteralType(data) => {
                let head = data.head.expect("TemplateLiteralType carries its head");
                let head_text = match self.data_of(head) {
                    NodeData::TemplateHead(head_data) => head_data
                        .raw_text
                        .clone()
                        .unwrap_or_else(|| template_text_raw(&head_data.text)),
                    _ => unreachable!("TemplateLiteralType head is a TemplateHead node"),
                };
                let mut text = format!("`{head_text}");
                for span in self.nodes_of(data.template_spans) {
                    let NodeData::TemplateLiteralTypeSpan(span_data) = self.data_of(span).clone()
                    else {
                        unreachable!(
                            "TemplateLiteralType template_spans contain TemplateLiteralTypeSpan"
                        );
                    };
                    let ty = self.type_annotation_text_slice(
                        span_data
                            .r#type
                            .expect("TemplateLiteralTypeSpan carries its type"),
                    )?;
                    let literal = span_data
                        .literal
                        .expect("TemplateLiteralTypeSpan carries its literal");
                    let literal_text = match self.data_of(literal) {
                        NodeData::TemplateMiddle(data) => data
                            .raw_text
                            .clone()
                            .unwrap_or_else(|| template_text_raw(&data.text)),
                        NodeData::TemplateTail(data) => data
                            .raw_text
                            .clone()
                            .unwrap_or_else(|| template_text_raw(&data.text)),
                        _ => unreachable!(
                            "TemplateLiteralTypeSpan literal is TemplateMiddle/TemplateTail"
                        ),
                    };
                    text.push_str(&format!("${{{ty}}}{literal_text}"));
                }
                text.push('`');
                Ok(text)
            }
            _ => unreachable!(
                "tryReuseExistingTypeNode supplied a non-TypeNode to the type-node printer"
            ),
        }
    }

    /// visitExistingNodeTreeSymbolsWorker's historically-JSDoc
    /// empty-name TypeReference rewrite (133428-133430). The branch
    /// itself is deliberately NOT NodeFlags::JSDoc-gated.
    fn is_empty_jsdoc_type_reference_slice(&self, node: NodeId) -> bool {
        matches!(
            self.data_of(node),
            NodeData::TypeReference(data)
                if data.type_name.is_some_and(|name| {
                    matches!(
                        self.data_of(name),
                        NodeData::Identifier(identifier) if identifier.escaped_text.is_empty()
                    )
                })
        )
    }

    /// visitExistingNodeTreeSymbolsWorker's historically-JSDoc
    /// `Object<string|number, V>` lowering (133431-133445).
    /// isJSDocIndexSignature is structural and does not require the
    /// TypeReference itself to carry NodeFlags::JSDoc.
    fn jsdoc_index_signature_text_slice(&mut self, node: NodeId) -> CheckResult<Option<String>> {
        let NodeData::TypeReference(data) = self.data_of(node).clone() else {
            return Ok(None);
        };
        let Some(name) = data.type_name else {
            return Ok(None);
        };
        if self.identifier_text_of(name) != Some("Object") {
            return Ok(None);
        }
        let arguments = self.nodes_of(data.type_arguments);
        if arguments.len() != 2
            || !matches!(
                self.kind_of(arguments[0]),
                SyntaxKind::StringKeyword | SyntaxKind::NumberKeyword
            )
        {
            return Ok(None);
        }
        let key = self.type_annotation_text_slice(arguments[0])?;
        let value = self.type_annotation_text_slice(arguments[1])?;
        Ok(Some(format!("{{ [x: {key}]: {value}; }}")))
    }

    /// createRecoveryBoundary's TypeNode recovery:
    /// resolver.serializeExistingTypeNode → typeToTypeNodeHelper over
    /// the semantic type, in the SAME nodeBuilder context.
    fn semantic_existing_type_node_text_and_kind_slice(
        &mut self,
        node: NodeId,
    ) -> CheckResult<(String, SliceTypeNodeKind)> {
        let face = self.semantic_existing_type_node_face_slice(node)?;
        Ok((face.text, face.kind))
    }

    fn semantic_existing_type_node_face_slice(
        &mut self,
        node: NodeId,
    ) -> CheckResult<SliceTypeNodeFace> {
        let ty = self.get_type_from_type_node(node)?;
        self.semantic_type_node_face_slice(ty)
    }

    fn semantic_type_node_face_slice(&mut self, mut ty: TypeId) -> CheckResult<SliceTypeNodeFace> {
        if let Some(&mapper) = self.slice_display_mappers.last() {
            ty = self.instantiate_type(ty, Some(mapper))?;
        }
        let (text, kind) = self.type_to_string_slice_node(ty, /*fully_qualified*/ false)?;
        let signature_kind = match kind {
            SliceTypeNodeKind::FunctionType => Some(SignatureKind::Call),
            SliceTypeNodeKind::ConstructorType => Some(SignatureKind::Construct),
            _ => None,
        };
        let has_type_parameters = match signature_kind {
            Some(signature_kind) => self
                .get_signatures_of_type(ty, signature_kind)?
                .first()
                .is_some_and(|&signature| {
                    self.signature_of(signature)
                        .type_parameters
                        .as_ref()
                        .is_some_and(|parameters| !parameters.is_empty())
                }),
            None => false,
        };
        Ok(SliceTypeNodeFace {
            text,
            kind,
            has_type_parameters,
        })
    }

    fn semantic_existing_type_node_text_slice(&mut self, node: NodeId) -> CheckResult<String> {
        Ok(self
            .semantic_existing_type_node_text_and_kind_slice(node)?
            .0)
    }

    /// enterNewScope's enclosing-declaration component for reused
    /// signature and mapped nodes. The original AST already owns the
    /// parameter/type-parameter locals, so parking that declaration is
    /// the native equivalent of nodeBuilder's synthesized fake scope.
    fn with_reused_node_scope_slice<T>(
        &mut self,
        node: NodeId,
        op: impl FnOnce(&mut Self) -> CheckResult<T>,
    ) -> CheckResult<T> {
        let saved = self.slice_display_enclosing.replace(node);
        let result = op(self);
        self.slice_display_enclosing = saved;
        result
    }

    /// tsc-port: serializeTypeName @6.0.3.
    /// tsc-hash: df4a76962d3a7605e7ad28b17db185ce5908de4271994b98a0e436257ce89990
    /// tsc-span: _tsc.js:53656-53674
    ///
    /// tryVisitTypeReference/tryVisitTypeQuery's serializeTypeName
    /// recovery (133357-133388): resolve the semantic target in its
    /// original scope, then let the enclosing-aware symbol-chain walk
    /// choose the shortest usable spelling. Already-visited type
    /// arguments remain syntactically reused as the override list.
    fn serialize_reused_type_name_slice(
        &mut self,
        name: NodeId,
        meaning: SymbolFlags,
        type_arguments: &[String],
    ) -> CheckResult<Option<(String, SliceTypeNodeKind)>> {
        let Some(enclosing) = self.slice_display_enclosing else {
            return Ok(None);
        };
        // serializeTypeName does not set dontResolveAlias: the semantic
        // target is passed to symbolToTypeNode, so the destination scope
        // can select a differently named alias for that target.
        let Some(symbol) = self.resolve_entity_name_ex(
            name, meaning, /*ignore_errors*/ true, None, /*dont_resolve_alias*/ false,
        )?
        else {
            return Ok(None);
        };
        if !self.symbol_is_accessible_with_containers_slice(
            symbol,
            meaning,
            enclosing,
            symbol,
            &mut Vec::new(),
        )? {
            return Ok(None);
        }
        let symbol = if self
            .binder
            .symbol(symbol)
            .flags
            .intersects(SymbolFlags::ALIAS)
        {
            self.resolve_alias(symbol)?
        } else {
            symbol
        };
        let (mut text, kind) = self.symbol_to_type_face_at_slice(symbol, meaning, enclosing)?;
        if !type_arguments.is_empty() {
            text.push('<');
            text.push_str(&type_arguments.join(", "));
            text.push('>');
        }
        Ok(Some((text, kind)))
    }

    /// tsc-port: trackExistingEntityName @6.0.3.
    /// tsc-hash: 209b12123fd836edaefcaef413f04659f4e3b998dac70ab139b159a0125e85ed
    /// tsc-span: _tsc.js:53555-53655
    ///
    /// Like tsc, resolution and comparison use the
    /// LEFTMOST identifier; the cloned node itself retains the full
    /// qualified spelling. A mismatch arms the enclosing TypeNode's
    /// semantic recovery boundary.
    fn reused_entity_name_introduces_error_slice(
        &mut self,
        name: NodeId,
        meaning: SymbolFlags,
    ) -> CheckResult<bool> {
        let first = self.first_identifier(name);
        // trackExistingEntityName's JS-only early error: `exports`,
        // expression-form `module.exports`, and type-position
        // `module.exports` are export plumbing rather than reusable
        // lexical entity names.
        if self.is_in_js_file(name) && self.is_js_exports_entity_name_slice(first) {
            return Ok(true);
        }
        // `this` identifiers bind through their this-container rather
        // than resolveEntityName. An inaccessible container symbol
        // marks the surrounding TypeNode recovery boundary.
        if self.is_this_identifier(first) {
            let source = self.binder.source_of_node(first);
            let container = node_util::get_this_container(
                source, first, /*include_arrow_functions*/ false,
            );
            return match container {
                Some(container) => {
                    Ok(!self.this_container_is_accessible_slice(container, first, meaning)?)
                }
                None => Ok(false),
            };
        }
        let Some(enclosing) = self.slice_display_enclosing else {
            return Ok(false);
        };
        let original =
            self.resolve_entity_name_ex(first, meaning, /*ignore_errors*/ true, None, true)?;
        if original.is_some_and(|symbol| {
            self.binder
                .symbol(symbol)
                .flags
                .intersects(SymbolFlags::TYPE_PARAMETER)
        }) {
            return Ok(false);
        }
        let original =
            original.map(|symbol| self.get_export_symbol_of_value_symbol_if_exported(symbol));
        let at_enclosing = self.resolve_entity_name_ex(
            first,
            meaning,
            /*ignore_errors*/ true,
            Some(enclosing),
            true,
        )?;
        if at_enclosing == Some(self.unknown_symbol) {
            return Ok(true);
        }
        let symbol = match (original, at_enclosing) {
            (Some(_), None) => return Ok(true),
            (Some(original), Some(at_enclosing)) => {
                let at_enclosing = self.get_export_symbol_of_value_symbol_if_exported(at_enclosing);
                if !self.symbol_if_same_reference_slice(at_enclosing, original)? {
                    return Ok(true);
                }
                Some(at_enclosing)
            }
            (_, at_enclosing) => at_enclosing,
        };
        let Some(symbol) = symbol else {
            return Ok(false);
        };
        let symbol_data = self.binder.symbol(symbol);
        if symbol_data
            .flags
            .intersects(SymbolFlags::FUNCTION_SCOPED_VARIABLE)
            && symbol_data.value_declaration.is_some_and(|declaration| {
                node_util::is_part_of_parameter_declaration(
                    self.binder.source_of_node(declaration),
                    declaration,
                ) || self.kind_of(declaration) == SyntaxKind::JSDocParameterTag
            })
        {
            return Ok(false);
        }
        if !symbol_data.flags.intersects(SymbolFlags::TYPE_PARAMETER)
            && !self.is_reused_declaration_name_slice(name)
            && !self.symbol_is_accessible_with_containers_slice(
                symbol,
                meaning,
                enclosing,
                symbol,
                &mut Vec::new(),
            )?
        {
            return Ok(true);
        }
        Ok(false)
    }

    /// isSymbolAccessible(getSymbolOfDeclaration(getThisContainer))
    /// includes getContainersOfSymbol recursion. For class elements,
    /// a named accessible class therefore suffices even though the
    /// member itself is not lexical; an anonymous class has no such
    /// container chain and forces semantic recovery.
    fn this_container_is_accessible_slice(
        &mut self,
        container: NodeId,
        location: NodeId,
        meaning: SymbolFlags,
    ) -> CheckResult<bool> {
        let mut candidate = container;
        if matches!(
            self.kind_of(container),
            SyntaxKind::PropertyDeclaration
                | SyntaxKind::PropertySignature
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::MethodSignature
                | SyntaxKind::Constructor
                | SyntaxKind::GetAccessor
                | SyntaxKind::SetAccessor
                | SyntaxKind::ClassStaticBlockDeclaration
        ) {
            if let Some(parent) = self.parent_of(container) {
                let named_class = match self.data_of(parent) {
                    NodeData::ClassDeclaration(data) => data.name.is_some(),
                    NodeData::ClassExpression(data) => data.name.is_some(),
                    _ => false,
                };
                if named_class {
                    candidate = parent;
                } else if matches!(
                    self.kind_of(parent),
                    SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
                ) {
                    return Ok(false);
                }
            }
        }
        let Some(symbol) = self.node_symbol(candidate) else {
            return Ok(false);
        };
        self.symbol_is_accessible_with_containers_slice(
            symbol,
            meaning,
            location,
            symbol,
            &mut Vec::new(),
        )
    }

    /// tsc-port: isAnySymbolAccessible @6.0.3 (boolean adapter).
    /// tsc-hash: 196ddf5926730f5e6f16ff4f2a7d59e1abf506c39cfc64d9ff90bd1a065f6cb1
    /// tsc-span: _tsc.js:50450-50498
    fn symbol_is_accessible_with_containers_slice(
        &mut self,
        symbol: SymbolId,
        meaning: SymbolFlags,
        enclosing: NodeId,
        initial_symbol: SymbolId,
        seen: &mut Vec<SymbolId>,
    ) -> CheckResult<bool> {
        if seen.contains(&symbol) {
            return Ok(false);
        }
        seen.push(symbol);
        if let Some(chain) =
            self.accessible_symbol_chain_at_slice(symbol, meaning, Some(enclosing))?
        {
            if self.symbol_has_visible_declarations_slice(chain[0]) {
                return Ok(true);
            }
        }
        if self.symbol_has_external_module_declaration(symbol) {
            return Ok(true);
        }
        let containers = self.containers_of_symbol_slice(symbol, Some(enclosing), meaning)?;
        let parent_meaning = if symbol == initial_symbol {
            Self::qualified_left_meaning(meaning)
        } else {
            meaning
        };
        for container in containers {
            if self.symbol_is_accessible_with_containers_slice(
                container,
                parent_meaning,
                enclosing,
                initial_symbol,
                seen,
            )? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// tsc-port: hasVisibleDeclarations @6.0.3
    /// tsc-hash: 3a5941173ae711a2e4bd9bf466cb674b521a0cb7cbd8c97fffdd7ac817dc4a6b
    /// tsc-span: _tsc.js:50544-50594
    ///
    /// Identifier-only synthetic declarations are ignored exactly like
    /// tsc. A declaration that is not directly visible can still be made
    /// visible through its import/variable statement; because this caller
    /// passes `false`, the alias-painting side effect is omitted while the
    /// exact acceptance decision is preserved.
    fn symbol_has_visible_declarations_slice(&self, symbol: SymbolId) -> bool {
        let symbol_flags = self.binder.symbol(symbol).flags;
        self.binder
            .symbol(symbol)
            .declarations
            .iter()
            .copied()
            .filter(|&declaration| self.kind_of(declaration) != SyntaxKind::Identifier)
            .all(|declaration| {
                self.reused_symbol_declaration_is_visible_slice(symbol_flags, declaration)
            })
    }

    fn reused_symbol_declaration_is_visible_slice(
        &self,
        symbol_flags: SymbolFlags,
        declaration: NodeId,
    ) -> bool {
        if self.reused_declaration_is_visible_slice(declaration) {
            return true;
        }
        let source = self.binder.source_of_node(declaration);
        if let Some(import_syntax) = self.reused_any_import_syntax_slice(declaration) {
            if !node_util::has_syntactic_modifier(source, import_syntax, ModifierFlags::EXPORT)
                && self
                    .parent_of(import_syntax)
                    .is_some_and(|parent| self.reused_declaration_is_visible_slice(parent))
            {
                return true;
            }
        }
        if self.kind_of(declaration) == SyntaxKind::VariableDeclaration {
            let variable_statement = self
                .parent_of(declaration)
                .and_then(|parent| self.parent_of(parent));
            if variable_statement.is_some_and(|statement| {
                self.kind_of(statement) == SyntaxKind::VariableStatement
                    && !node_util::has_syntactic_modifier(source, statement, ModifierFlags::EXPORT)
                    && self
                        .parent_of(statement)
                        .is_some_and(|parent| self.reused_declaration_is_visible_slice(parent))
            }) {
                return true;
            }
        }
        if self.is_late_visibility_painted_statement_slice(declaration)
            && !node_util::has_syntactic_modifier(source, declaration, ModifierFlags::EXPORT)
            && self
                .parent_of(declaration)
                .is_some_and(|parent| self.reused_declaration_is_visible_slice(parent))
        {
            return true;
        }
        if self.kind_of(declaration) != SyntaxKind::BindingElement {
            return false;
        }
        if symbol_flags.intersects(SymbolFlags::ALIAS) && self.is_in_js_file(declaration) {
            let variable_declaration = self
                .parent_of(declaration)
                .and_then(|parent| self.parent_of(parent));
            let variable_statement = variable_declaration.and_then(|variable| {
                self.parent_of(variable)
                    .and_then(|parent| self.parent_of(parent))
            });
            if variable_declaration
                .is_some_and(|variable| self.kind_of(variable) == SyntaxKind::VariableDeclaration)
                && variable_statement.is_some_and(|statement| {
                    self.kind_of(statement) == SyntaxKind::VariableStatement
                        && !node_util::has_syntactic_modifier(
                            source,
                            statement,
                            ModifierFlags::EXPORT,
                        )
                        && self
                            .parent_of(statement)
                            .is_some_and(|parent| self.reused_declaration_is_visible_slice(parent))
                })
            {
                return true;
            }
        }
        if symbol_flags.intersects(SymbolFlags::BLOCK_SCOPED_VARIABLE) {
            let Some(root) = node_util::walk_up_binding_elements_and_patterns(source, declaration)
            else {
                return false;
            };
            if self.kind_of(root) == SyntaxKind::Parameter {
                return false;
            }
            let Some(variable_statement) = self
                .parent_of(root)
                .and_then(|parent| self.parent_of(parent))
            else {
                return false;
            };
            if self.kind_of(variable_statement) != SyntaxKind::VariableStatement {
                return false;
            }
            if node_util::has_syntactic_modifier(source, variable_statement, ModifierFlags::EXPORT)
            {
                return true;
            }
            return self
                .parent_of(variable_statement)
                .is_some_and(|parent| self.reused_declaration_is_visible_slice(parent));
        }
        false
    }

    /// tsc-port: getAnyImportSyntax @6.0.3.
    /// tsc-hash: 4bbd19cf79821054af5d7cde72570b4cead4f046a0487abc67e2dcf9e5dd85df
    /// tsc-span: _tsc.js:48481-48492
    fn reused_any_import_syntax_slice(&self, declaration: NodeId) -> Option<NodeId> {
        match self.kind_of(declaration) {
            SyntaxKind::ImportEqualsDeclaration => Some(declaration),
            SyntaxKind::ImportClause => self.parent_of(declaration),
            SyntaxKind::NamespaceImport => self
                .parent_of(declaration)
                .and_then(|parent| self.parent_of(parent)),
            SyntaxKind::ImportSpecifier => self
                .parent_of(declaration)
                .and_then(|parent| self.parent_of(parent))
                .and_then(|parent| self.parent_of(parent)),
            _ => None,
        }
    }

    /// tsc-port: isLateVisibilityPaintedStatement @6.0.3.
    /// tsc-hash: f287e28aeb22f22f5a56296876740f2c7312d47d6633ef1143e8e8b3effc4dd7
    /// tsc-span: _tsc.js:13819-13834
    fn is_late_visibility_painted_statement_slice(&self, node: NodeId) -> bool {
        matches!(
            self.kind_of(node),
            SyntaxKind::ImportDeclaration
                | SyntaxKind::ImportEqualsDeclaration
                | SyntaxKind::VariableStatement
                | SyntaxKind::ClassDeclaration
                | SyntaxKind::FunctionDeclaration
                | SyntaxKind::ModuleDeclaration
                | SyntaxKind::TypeAliasDeclaration
                | SyntaxKind::InterfaceDeclaration
                | SyntaxKind::EnumDeclaration
        )
    }

    /// tsc-port: isDeclarationVisible @6.0.3.
    /// tsc-hash: b569e8243cf2db9de0dbec7462f29fa1e70f4b94405adb5a134b6571d4c8fbeb
    /// tsc-span: _tsc.js:55589-55674
    pub(crate) fn reused_declaration_is_visible_slice(&self, declaration: NodeId) -> bool {
        match self.kind_of(declaration) {
            SyntaxKind::JSDocCallbackTag
            | SyntaxKind::JSDocTypedefTag
            | SyntaxKind::JSDocEnumTag => self
                .parent_of(declaration)
                .and_then(|parent| self.parent_of(parent))
                .and_then(|parent| self.parent_of(parent))
                .is_some_and(|parent| self.kind_of(parent) == SyntaxKind::SourceFile),
            SyntaxKind::BindingElement => self
                .parent_of(declaration)
                .and_then(|parent| self.parent_of(parent))
                .is_some_and(|parent| self.reused_declaration_is_visible_slice(parent)),
            SyntaxKind::VariableDeclaration
            | SyntaxKind::ModuleDeclaration
            | SyntaxKind::ClassDeclaration
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::EnumDeclaration
            | SyntaxKind::ImportEqualsDeclaration => {
                let source = self.binder.source_of_node(declaration);
                if self.kind_of(declaration) == SyntaxKind::VariableDeclaration {
                    let empty_pattern = match self.data_of(declaration) {
                        NodeData::VariableDeclaration(data) => {
                            data.name.is_some_and(|name| match self.data_of(name) {
                                NodeData::ObjectBindingPattern(data) => {
                                    self.nodes_of(data.elements).is_empty()
                                }
                                NodeData::ArrayBindingPattern(data) => {
                                    self.nodes_of(data.elements).is_empty()
                                }
                                _ => false,
                            })
                        }
                        _ => false,
                    };
                    if empty_pattern {
                        return false;
                    }
                }
                if node_util::is_ambient_module(source, declaration)
                    && node_util::is_module_augmentation_external(source, declaration)
                {
                    return true;
                }
                let Some(container) = self.reused_declaration_container_slice(declaration) else {
                    return false;
                };
                let exported = node_util::get_combined_modifier_flags(source, declaration)
                    .intersects(ModifierFlags::EXPORT);
                let ambient_nested = self.kind_of(declaration)
                    != SyntaxKind::ImportEqualsDeclaration
                    && self.kind_of(container) != SyntaxKind::SourceFile
                    && self.node_flags(container) & tsc_types::NodeFlags::AMBIENT.bits() != 0;
                if !exported && !ambient_nested {
                    return self.kind_of(container) == SyntaxKind::SourceFile
                        && !self
                            .binder
                            .is_external_or_common_js_module_of_node(container);
                }
                self.reused_declaration_is_visible_slice(container)
            }
            SyntaxKind::PropertyDeclaration
            | SyntaxKind::PropertySignature
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::MethodSignature => {
                let source = self.binder.source_of_node(declaration);
                if node_util::get_effective_modifier_flags(source, declaration)
                    .intersects(ModifierFlags::PRIVATE | ModifierFlags::PROTECTED)
                {
                    return false;
                }
                self.parent_of(declaration)
                    .is_some_and(|parent| self.reused_declaration_is_visible_slice(parent))
            }
            SyntaxKind::Constructor
            | SyntaxKind::ConstructSignature
            | SyntaxKind::CallSignature
            | SyntaxKind::IndexSignature
            | SyntaxKind::Parameter
            | SyntaxKind::ModuleBlock
            | SyntaxKind::FunctionType
            | SyntaxKind::ConstructorType
            | SyntaxKind::TypeLiteral
            | SyntaxKind::TypeReference
            | SyntaxKind::ArrayType
            | SyntaxKind::TupleType
            | SyntaxKind::UnionType
            | SyntaxKind::IntersectionType
            | SyntaxKind::ParenthesizedType
            | SyntaxKind::NamedTupleMember => self
                .parent_of(declaration)
                .is_some_and(|parent| self.reused_declaration_is_visible_slice(parent)),
            SyntaxKind::TypeParameter
            | SyntaxKind::SourceFile
            | SyntaxKind::NamespaceExportDeclaration => true,
            SyntaxKind::ImportClause
            | SyntaxKind::NamespaceImport
            | SyntaxKind::ImportSpecifier
            | SyntaxKind::ExportAssignment
            | SyntaxKind::FunctionExpression
            | SyntaxKind::ArrowFunction => false,
            _ => false,
        }
    }

    /// tsc-port: getDeclarationContainer @6.0.3.
    /// tsc-hash: 3d4b993da842ea191877ffad47fb0c8045a3d1086066350235a4992e74413283
    /// tsc-span: _tsc.js:55784-55798
    fn reused_declaration_container_slice(&self, declaration: NodeId) -> Option<NodeId> {
        let source = self.binder.source_of_node(declaration);
        let root = node_util::get_root_declaration(source, declaration);
        let mut current = Some(root);
        while let Some(node) = current {
            match self.kind_of(node) {
                SyntaxKind::VariableDeclaration
                | SyntaxKind::VariableDeclarationList
                | SyntaxKind::ImportSpecifier
                | SyntaxKind::NamedImports
                | SyntaxKind::NamespaceImport
                | SyntaxKind::ImportClause => current = self.parent_of(node),
                _ => return self.parent_of(node),
            }
        }
        None
    }

    fn is_reused_declaration_name_slice(&self, name: NodeId) -> bool {
        let Some(parent) = self.parent_of(name) else {
            return false;
        };
        match self.data_of(parent) {
            NodeData::TypeParameter(data) => data.name == Some(name),
            NodeData::Parameter(data) => data.name == Some(name),
            NodeData::PropertySignature(data) => data.name == Some(name),
            NodeData::MethodSignature(data) => data.name == Some(name),
            NodeData::InferType(data) => data.type_parameter == Some(name),
            _ => false,
        }
    }

    fn is_js_exports_entity_name_slice(&self, leftmost: NodeId) -> bool {
        let source = self.binder.source_of_node(leftmost);
        if tsc_binder::assignment::is_exports_identifier(source, leftmost) {
            return true;
        }
        let Some(parent) = self.parent_of(leftmost) else {
            return false;
        };
        if tsc_binder::assignment::is_module_exports_access_expression(source, parent) {
            return true;
        }
        matches!(
            self.data_of(parent),
            NodeData::QualifiedName(data)
                if data.left.is_some_and(|left| self.identifier_text_of(left) == Some("module"))
                    && data.right.is_some_and(|right| {
                        tsc_binder::assignment::is_exports_identifier(source, right)
                    })
        )
    }

    fn type_reference_entity_meaning_slice(&self, name: NodeId) -> SymbolFlags {
        if matches!(
            self.kind_of(name),
            SyntaxKind::QualifiedName | SyntaxKind::PropertyAccessExpression
        ) {
            SymbolFlags::NAMESPACE
        } else {
            SymbolFlags::TYPE
        }
    }

    /// isLiteralImportTypeNode + the visitor's AssertKeyword
    /// markError branch (133518-133522).
    fn literal_import_type_has_assert_attributes_slice(&self, data: &ImportTypeData) -> bool {
        let literal_argument = data.argument.is_some_and(|argument| {
            matches!(
                self.data_of(argument),
                NodeData::LiteralType(literal)
                    if literal.literal.is_some_and(|literal| {
                        self.kind_of(literal) == SyntaxKind::StringLiteral
                    })
            )
        });
        literal_argument
            && data.attributes.is_some_and(|attributes| {
                matches!(
                    self.data_of(attributes),
                    NodeData::ImportAttributes(attributes)
                        if attributes.token == SyntaxKind::AssertKeyword
                )
            })
    }

    /// canReuseTypeNode's JS literal-import fallback guards
    /// (53683-53694). A value-only export used as a type, or a generic
    /// whose required arguments were autofilled by JSDoc resolution,
    /// must be rebuilt from its semantic type instead of cloning the
    /// source ImportTypeNode.
    fn can_reuse_literal_import_type_slice(
        &mut self,
        node: NodeId,
        data: &ImportTypeData,
    ) -> CheckResult<bool> {
        if !self.is_in_js_file(node) {
            return Ok(true);
        }
        let is_literal = data.argument.is_some_and(|argument| {
            matches!(
                self.data_of(argument),
                NodeData::LiteralType(literal)
                    if literal.literal.is_some_and(|literal| {
                        self.kind_of(literal) == SyntaxKind::StringLiteral
                    })
            )
        });
        if !is_literal {
            return Ok(true);
        }
        // getTypeFromImportTypeNode initializes resolvedSymbol before
        // canReuseTypeNode reads the link.
        let _ = self.get_type_from_type_node(node)?;
        let Some(symbol) = self.links.node(node).resolved_symbol.resolved() else {
            return Ok(true);
        };
        if !data.is_type_of
            && !self
                .binder
                .symbol(symbol)
                .flags
                .intersects(SymbolFlags::TYPE)
        {
            return Ok(false);
        }
        let parameters = self.get_local_type_parameters_of_class_or_interface_or_type_alias(symbol);
        Ok(self.nodes_of(data.type_arguments).len()
            >= self.get_min_type_argument_count(Some(&parameters)))
    }

    /// tsc-port: canReuseTypeNode @6.0.3.
    /// tsc-hash: 01c9a8dfeb77eef59be644334bdb51546fe769387f732ff932cf1cb5637bad4f
    /// tsc-span: _tsc.js:53675-53702
    ///
    /// The root reuse
    /// boundary calls this once; recursive visitor calls are restricted
    /// to the worker branches that tsc actually probes (TypeReference,
    /// literal ImportType, ThisType, and `unique symbol`). Ordinary
    /// child nodes therefore do not pay a semantic type read.
    fn can_reuse_existing_type_node_slice(&mut self, node: NodeId) -> CheckResult<bool> {
        let mut context_type = None;
        if let Some(&mapper) = self.slice_display_mappers.last() {
            let ty = self.get_type_from_type_node(node)?;
            if self.instantiate_type(ty, Some(mapper))? != ty {
                return Ok(false);
            }
            context_type = Some(ty);
        }
        match self.data_of(node).clone() {
            NodeData::ImportType(data) => {
                return self.can_reuse_literal_import_type_slice(node, &data);
            }
            NodeData::TypeReference(_) => {
                if self.is_const_type_reference_node(node) {
                    return Ok(false);
                }
                let ty = match context_type {
                    Some(ty) => ty,
                    None => self.get_type_from_type_node(node)?,
                };
                let Some(symbol) = self.links.node(node).resolved_symbol.resolved() else {
                    return Ok(false);
                };
                if self
                    .binder
                    .symbol(symbol)
                    .flags
                    .intersects(SymbolFlags::TYPE_PARAMETER)
                {
                    return Ok(true);
                }
                if self.is_jsdoc_type_reference(node) {
                    return Ok(
                        self.reference_annotation_argument_count_compatible(node, ty)?
                            && self
                                .get_intended_type_from_jsdoc_type_reference(node)?
                                .is_none()
                            && self
                                .binder
                                .symbol(symbol)
                                .flags
                                .intersects(SymbolFlags::TYPE),
                    );
                }
            }
            NodeData::TypeOperator(data)
                if data.operator == SyntaxKind::UniqueKeyword
                    && data
                        .r#type
                        .is_some_and(|inner| self.kind_of(inner) == SyntaxKind::SymbolKeyword) =>
            {
                return Ok(self
                    .slice_display_enclosing
                    .is_some_and(|enclosing| self.is_node_descendant_of(node, enclosing)));
            }
            _ => {}
        }
        Ok(true)
    }

    /// A visited child as the same `(TypeNode text, transformed kind)`
    /// pair the factory parenthesizer observes. Every container join
    /// consumes this pair rather than inferring precedence from text.
    fn visited_type_node_text_slice(
        &mut self,
        node: NodeId,
    ) -> CheckResult<(String, SliceTypeNodeKind)> {
        self.type_annotation_text_and_kind_slice(node)
    }

    /// Kind carried beside a reused annotation's text for the factory
    /// parenthesizer decisions made by JSDoc lowering. This mirrors
    /// the ordinary TypeNode kind produced by
    /// visitExistingNodeTreeSymbolsWorker, rather than the original
    /// JSDoc wrapper kind.
    fn type_annotation_node_kind_slice(&self, node: NodeId) -> SliceTypeNodeKind {
        match self.kind_of(node) {
            SyntaxKind::JSDocTypeExpression | SyntaxKind::JSDocNonNullableType => {
                let inner = match self.data_of(node) {
                    NodeData::JSDocTypeExpression(data) => data.r#type,
                    NodeData::JSDocNonNullableType(data) => data.r#type,
                    _ => None,
                };
                inner
                    .map(|inner| self.type_annotation_node_kind_slice(inner))
                    .unwrap_or(SliceTypeNodeKind::Keyword)
            }
            SyntaxKind::JSDocAllType
            | SyntaxKind::JSDocUnknownType
            | SyntaxKind::JSDocNamepathType => SliceTypeNodeKind::Keyword,
            SyntaxKind::JSDocNullableType | SyntaxKind::JSDocOptionalType => {
                SliceTypeNodeKind::Union
            }
            SyntaxKind::JSDocVariadicType | SyntaxKind::ArrayType => SliceTypeNodeKind::Array,
            SyntaxKind::JSDocTypeLiteral | SyntaxKind::TypeLiteral => {
                SliceTypeNodeKind::TypeLiteral
            }
            SyntaxKind::JSDocFunctionType => {
                if node_util::is_jsdoc_construct_signature(self.binder.source_of_node(node), node) {
                    SliceTypeNodeKind::ConstructorType
                } else {
                    SliceTypeNodeKind::FunctionType
                }
            }
            SyntaxKind::FunctionType => SliceTypeNodeKind::FunctionType,
            SyntaxKind::ConstructorType => SliceTypeNodeKind::ConstructorType,
            SyntaxKind::UnionType => SliceTypeNodeKind::Union,
            SyntaxKind::IntersectionType => SliceTypeNodeKind::Intersection,
            SyntaxKind::TypeOperator => SliceTypeNodeKind::TypeOperator,
            SyntaxKind::TypeQuery => SliceTypeNodeKind::TypeQuery,
            SyntaxKind::ImportType => SliceTypeNodeKind::ImportType,
            SyntaxKind::TupleType => SliceTypeNodeKind::Tuple,
            SyntaxKind::TemplateLiteralType => SliceTypeNodeKind::TemplateLiteral,
            SyntaxKind::IndexedAccessType => SliceTypeNodeKind::IndexedAccess,
            SyntaxKind::ConditionalType => SliceTypeNodeKind::Conditional,
            SyntaxKind::InferType => SliceTypeNodeKind::Infer,
            SyntaxKind::LiteralType => SliceTypeNodeKind::Literal,
            _ => SliceTypeNodeKind::Reference,
        }
    }

    /// tsc-port: visitExistingNodeTreeSymbolsWorker @6.0.3
    /// tsc-hash: d43fea9b24f553ed46ba34a6722ab90374b8011af16ca7e3b469feaef68fbd62
    /// tsc-span: _tsc.js:133446-133481
    ///
    /// JSDoc function parameters are deliberately renamed by the
    /// syntactic builder (`argN`, `args`, with `this` preserved), and
    /// a leading `new` pseudo-parameter supplies a constructor return
    /// type instead of appearing in the emitted parameter list.
    fn jsdoc_function_type_text_slice(
        &mut self,
        node: NodeId,
        data: JSDocFunctionTypeData,
    ) -> CheckResult<String> {
        let construct =
            node_util::is_jsdoc_construct_signature(self.binder.source_of_node(node), node);
        let type_parameters =
            self.type_parameter_nodes_text_slice(self.nodes_of(data.type_parameters))?;
        let mut return_from_new = None;
        let mut parameters = Vec::new();
        for (index, parameter) in self.nodes_of(data.parameters).into_iter().enumerate() {
            let NodeData::Parameter(parameter_data) = self.data_of(parameter).clone() else {
                unreachable!("JSDocFunctionType parameters are Parameter nodes");
            };
            let original_name = parameter_data
                .name
                .and_then(|name| self.identifier_text_of(name))
                .map(str::to_owned);
            if construct && original_name.as_deref() == Some("new") {
                return_from_new = parameter_data.r#type;
                continue;
            }
            let rest = parameter_data.dot_dot_dot_token.is_some()
                || parameter_data
                    .r#type
                    .is_some_and(|ty| self.kind_of(ty) == SyntaxKind::JSDocVariadicType);
            let name = if original_name.as_deref() == Some("this") {
                "this".to_owned()
            } else if rest {
                "args".to_owned()
            } else {
                format!("arg{index}")
            };
            let dots = if rest { "..." } else { "" };
            let question = if parameter_data.question_token.is_some() {
                "?"
            } else {
                ""
            };
            let mut text = format!("{dots}{name}{question}");
            if let Some(annotation) = parameter_data.r#type {
                text.push_str(": ");
                text.push_str(&self.type_annotation_text_slice(annotation)?);
            }
            parameters.push(text);
        }
        let return_type = return_from_new.or(data.r#type);
        let return_text = match return_type {
            Some(return_type) => self.type_annotation_text_slice(return_type)?,
            None => "any".to_owned(),
        };
        if construct {
            Ok(format!(
                "new {type_parameters}({}) => {return_text}",
                parameters.join(", ")
            ))
        } else {
            Ok(format!(
                "{type_parameters}({}) => {return_text}",
                parameters.join(", ")
            ))
        }
    }

    /// tsc-port: visitExistingNodeTreeSymbolsWorker @6.0.3
    /// tsc-hash: ba4ec70ca9817c23c31d5cbfcae6cf9a1ee2778cbcab1e89399b4c7caa5c0030
    /// tsc-span: _tsc.js:133412-133427
    ///
    /// The visitor synthesizes an ordinary TypeLiteral of property
    /// signatures. Bracketed tags and optional JSDoc types both set
    /// `?`; a missing annotation becomes `any`.
    fn jsdoc_type_literal_text_slice(
        &mut self,
        node: NodeId,
        data: JSDocTypeLiteralData,
    ) -> CheckResult<String> {
        let properties = self.nodes_of(data.js_doc_property_tags);
        if properties.is_empty() {
            return Ok("{}".to_owned());
        }
        let literal_type = self.get_type_from_type_node(node)?;
        let mut rendered = Vec::with_capacity(properties.len());
        for property in properties {
            let NodeData::JSDocPropertyTag(property_data) = self.data_of(property).clone() else {
                unreachable!("JSDocTypeLiteral property lists contain JSDocPropertyTag nodes");
            };
            let name_node = property_data
                .name
                .expect("JSDocPropertyTag carries its property name");
            let name_node = match self.data_of(name_node) {
                NodeData::Identifier(_) => name_node,
                NodeData::JSDocMemberName(data) => data
                    .right
                    .expect("JSDocMemberName carries its right-hand property name"),
                _ => name_node,
            };
            let name = self.entity_name_text_slice(name_node)?;
            let annotation = property_data.type_expression.and_then(|expression| {
                match self.data_of(expression) {
                    NodeData::JSDocTypeExpression(data) => data.r#type,
                    _ => None,
                }
            });
            let optional = property_data.is_bracketed
                || annotation.is_some_and(|ty| self.kind_of(ty) == SyntaxKind::JSDocOptionalType);

            // resolver.getJsDocPropertyOverride: if resolving the
            // enclosing literal changed this property's type, render
            // that resolved face; otherwise visit the written JSDoc
            // annotation.
            let property_type = self.get_type_of_property_of_type(literal_type, &name)?;
            let annotation_type = match annotation {
                Some(annotation) => Some(self.get_type_from_type_node(annotation)?),
                None => None,
            };
            let type_text = if let (Some(property_type), Some(annotation_type)) =
                (property_type, annotation_type)
            {
                if property_type != annotation_type {
                    self.type_to_string_slice(property_type)?
                } else {
                    self.type_annotation_text_slice(
                        annotation.expect("matched Some annotation_type"),
                    )?
                }
            } else if let Some(annotation) = annotation {
                self.type_annotation_text_slice(annotation)?
            } else {
                "any".to_owned()
            };
            rendered.push(format!(
                "{name}{}: {type_text}",
                if optional { "?" } else { "" }
            ));
        }
        Ok(format!("{{ {}; }}", rendered.join("; ")))
    }

    /// tsc-port: emitImportTypeNodeAttributes @6.0.3
    /// tsc-hash: c2028e6703acc1bf61520b1e7d35939630903c14182894f429a798dc396a8486
    /// tsc-span: _tsc.js:119305-119315
    ///
    /// tsc-port: emitImportAttribute @6.0.3
    /// tsc-hash: a0edb1f08aefc12e25f1d599c3cf2229a8048fc7d56e28b1e0785d33726c6b6f
    /// tsc-span: _tsc.js:119322-119332
    fn import_attributes_text_slice(&mut self, node: NodeId) -> CheckResult<String> {
        let NodeData::ImportAttributes(data) = self.data_of(node).clone() else {
            unreachable!("ImportType attributes is an ImportAttributes node");
        };
        let keyword = tsc_syntax::tokens::token_to_string(data.token)
            .expect("ImportAttributes token is with/assert");
        let mut rendered = Vec::new();
        for element in self.nodes_of(data.elements) {
            let NodeData::ImportAttribute(data) = self.data_of(element).clone() else {
                unreachable!("ImportAttributes elements contain ImportAttribute nodes");
            };
            let name = self.member_name_node_text_slice(
                data.name.expect("ImportAttribute carries its name"),
            )?;
            let value =
                self.expression_text_slice(data.value.expect("ImportAttribute carries its value"))?;
            rendered.push(format!("{name}: {value}"));
        }
        let elements = if rendered.is_empty() {
            "{}".to_owned()
        } else {
            format!("{{ {} }}", rendered.join(", "))
        };
        Ok(format!("{{ {keyword}: {elements} }}"))
    }

    /// Entity names in reused annotations: Identifier / QualifiedName
    /// dots / the property-access spellings type queries carry.
    fn entity_name_text_slice(&mut self, node: NodeId) -> CheckResult<String> {
        match self.data_of(node).clone() {
            NodeData::Identifier(data) => {
                Ok(tsc_binder::unescape_leading_underscores(&data.escaped_text).to_owned())
            }
            NodeData::PrivateIdentifier(data) => Ok(data.text),
            NodeData::QualifiedName(data) => {
                let left = self.entity_name_text_slice(
                    data.left.expect("QualifiedName carries its left side"),
                )?;
                let right = self.entity_name_text_slice(
                    data.right.expect("QualifiedName carries its right side"),
                )?;
                Ok(format!("{left}.{right}"))
            }
            NodeData::PropertyAccessExpression(data) => {
                let left = self.entity_name_text_slice(
                    data.expression
                        .expect("PropertyAccessExpression carries its expression"),
                )?;
                let right = self.entity_name_text_slice(
                    data.name
                        .expect("PropertyAccessExpression carries its name"),
                )?;
                Ok(format!("{left}.{right}"))
            }
            NodeData::JSDocMemberName(data) => {
                let left = self.entity_name_text_slice(
                    data.left.expect("JSDocMemberName carries its left side"),
                )?;
                let right = self.entity_name_text_slice(
                    data.right.expect("JSDocMemberName carries its right side"),
                )?;
                Ok(format!("{left}.{right}"))
            }
            _ => unreachable!("entity-name printer received a non-entity-name node"),
        }
    }

    /// LiteralTypeNode literal faces: synthesized clones print cooked
    /// numeric text and double-quoted strings (oracle-probed Q01/Q02).
    fn literal_type_node_text_slice(&mut self, literal: NodeId) -> CheckResult<String> {
        match self.kind_of(literal) {
            SyntaxKind::TrueKeyword => return Ok("true".to_owned()),
            SyntaxKind::FalseKeyword => return Ok("false".to_owned()),
            SyntaxKind::NullKeyword => return Ok("null".to_owned()),
            _ => {}
        }
        match self.data_of(literal).clone() {
            NodeData::StringLiteral(data) => string_literal_name_slice(&data.text, false),
            NodeData::NumericLiteral(data) => Ok(data.text.clone()),
            // getLiteralText's BigIntLiteral arm emits the cloned
            // node's scanner-cooked text verbatim (hex remains hex;
            // binary/octal are already normalized by the scanner).
            NodeData::BigIntLiteral(data) => Ok(data.text),
            NodeData::PrefixUnaryExpression(data) => {
                let operator = match data.operator {
                    SyntaxKind::MinusToken => "-",
                    SyntaxKind::PlusToken => "+",
                    _ => unreachable!("LiteralType prefix expressions use +/-"),
                };
                let operand = data
                    .operand
                    .expect("PrefixUnaryExpression carries its literal operand");
                Ok(format!(
                    "{operator}{}",
                    self.literal_type_node_text_slice(operand)?
                ))
            }
            _ => unreachable!("LiteralType carries literal or +/- literal nodes"),
        }
    }

    /// factory parenthesizeTypeArguments (20607-20617): only the
    /// leading argument is wrapped, and only when it is a generic
    /// function/constructor TypeNode. The visitor-lowered JSDoc
    /// function shape follows the same ordinary-node rule.
    /// tsrs-native: Rust vector adapter over the cloned TypeNode printer.
    pub(crate) fn type_argument_nodes_text_slice(
        &mut self,
        nodes: Vec<NodeId>,
    ) -> CheckResult<Vec<String>> {
        let mut rendered = Vec::with_capacity(nodes.len());
        for (index, node) in nodes.into_iter().enumerate() {
            let face = self.type_annotation_face_slice(node)?;
            let mut text = face.text;
            if index == 0
                && matches!(
                    face.kind,
                    SliceTypeNodeKind::FunctionType | SliceTypeNodeKind::ConstructorType
                )
                && face.has_type_parameters
            {
                text = format!("({text})");
            }
            rendered.push(text);
        }
        Ok(rendered)
    }

    fn generic_function_or_constructor_type_node_slice(&self, node: NodeId) -> bool {
        match self.data_of(node) {
            NodeData::FunctionType(data) => data.type_parameters.is_some(),
            NodeData::ConstructorType(data) => data.type_parameters.is_some(),
            NodeData::JSDocFunctionType(data) => data.type_parameters.is_some(),
            NodeData::JSDocTypeExpression(data) => data
                .r#type
                .is_some_and(|inner| self.generic_function_or_constructor_type_node_slice(inner)),
            _ => false,
        }
    }

    /// Type-parameter declaration NODES inside reused annotations
    /// (`(x: <T>(y: T) => T)` shapes): name / constraint / default
    /// print from the AST.
    /// tsrs-native: Rust node-list adapter over the cloned TypeNode printer.
    pub(crate) fn type_parameter_nodes_text_slice(
        &mut self,
        nodes: Vec<NodeId>,
    ) -> CheckResult<String> {
        if nodes.is_empty() {
            return Ok(String::new());
        }
        let mut rendered = Vec::with_capacity(nodes.len());
        for node in nodes {
            rendered.push(self.type_parameter_node_text_slice(node)?);
        }
        Ok(format!("<{}>", rendered.join(", ")))
    }

    fn type_parameter_node_text_slice(&mut self, node: NodeId) -> CheckResult<String> {
        let NodeData::TypeParameter(data) = self.data_of(node).clone() else {
            unreachable!("type-parameter lists contain TypeParameter nodes");
        };
        let mut text = String::new();
        for modifier in self.nodes_of(data.modifiers) {
            if matches!(self.data_of(modifier), NodeData::Decorator(_)) {
                continue;
            }
            let token = tsc_syntax::tokens::token_to_string(self.kind_of(modifier))
                .expect("type-parameter modifiers are keyword tokens");
            text.push_str(token);
            text.push(' ');
        }
        text.push_str(
            &self.entity_name_text_slice(
                data.name
                    .expect("TypeParameter carries its declaration name"),
            )?,
        );
        if let Some(constraint) = data.constraint {
            text.push_str(" extends ");
            text.push_str(&self.type_annotation_text_slice(constraint)?);
        }
        if let Some(default) = data.r#default {
            text.push_str(" = ");
            text.push_str(&self.type_annotation_text_slice(default)?);
        }
        Ok(text)
    }

    /// Parameter declaration NODES inside reused annotations: the
    /// printer's `[...]name[?][: type]` face. The visitor's
    /// isFunctionLike/isParameter missing-type branch synthesizes
    /// `any` when no initializer supplies a type-bearing face.
    /// tsrs-native: Rust node-list adapter into the body-printer compartment.
    pub(crate) fn parameter_nodes_text_slice(&mut self, nodes: Vec<NodeId>) -> CheckResult<String> {
        match self.display_clone_parameter_nodes_text(nodes)? {
            Some(text) => Ok(text),
            None => {
                self.slice_reuse_had_error = true;
                Ok(String::new())
            }
        }
    }

    fn modifier_nodes_text_slice(&self, modifiers: Option<NodeArrayId>) -> String {
        let mut rendered = Vec::new();
        for modifier in self.nodes_of(modifiers) {
            if matches!(self.data_of(modifier), NodeData::Decorator(_)) {
                continue;
            }
            if let Some(token) = tsc_syntax::tokens::token_to_string(self.kind_of(modifier)) {
                rendered.push(token);
            }
        }
        if rendered.is_empty() {
            String::new()
        } else {
            format!("{} ", rendered.join(" "))
        }
    }

    fn type_literal_member_has_body_slice(&self, member: NodeId) -> bool {
        match self.data_of(member) {
            NodeData::GetAccessor(data) => data.body.is_some(),
            NodeData::SetAccessor(data) => data.body.is_some(),
            _ => false,
        }
    }

    /// Type-literal MEMBER nodes inside reused annotations, printed
    /// with the single-line `; ` joins (oracle-probed C07:
    /// `{ a: (number) }` renders `{ a: (number); }`).
    /// tsrs-native: one-member string-face adapter over the cloned printer.
    pub(crate) fn type_literal_member_text_slice(
        &mut self,
        member: NodeId,
    ) -> CheckResult<Option<String>> {
        if matches!(
            self.kind_of(member),
            SyntaxKind::MethodSignature
                | SyntaxKind::CallSignature
                | SyntaxKind::ConstructSignature
                | SyntaxKind::IndexSignature
                | SyntaxKind::GetAccessor
                | SyntaxKind::SetAccessor
        ) {
            return self.with_reused_node_scope_slice(member, |state| {
                state.type_literal_member_text_slice_worker(member)
            });
        }
        self.type_literal_member_text_slice_worker(member)
    }

    fn type_literal_member_text_slice_worker(
        &mut self,
        member: NodeId,
    ) -> CheckResult<Option<String>> {
        // 133537-133543: in this error-display context
        // shouldRemoveDeclaration is true for every unresolved dynamic
        // name. Late-bindable computed names survive and are visited.
        let source = self.binder.source_of_node(member);
        let computed_name = node_util::get_name_of_declaration(source, member)
            .is_some_and(|name| self.kind_of(name) == SyntaxKind::ComputedPropertyName);
        if computed_name
            && node_util::has_dynamic_name(source, member)
            && !self.has_bindable_name(member)?
        {
            return Ok(None);
        }

        match self.data_of(member).clone() {
            NodeData::PropertySignature(data) => {
                let modifiers = self.modifier_nodes_text_slice(data.modifiers);
                let name = self.member_name_node_text_slice(
                    data.name.expect("PropertySignature carries its name"),
                )?;
                let question = if data.question_token.is_some() {
                    "?"
                } else {
                    ""
                };
                let mut text = format!("{modifiers}{name}{question}");
                if let Some(annotation) = data.r#type {
                    text.push_str(": ");
                    text.push_str(&self.type_annotation_text_slice(annotation)?);
                } else if data.initializer.is_none() {
                    text.push_str(": any");
                }
                Ok(Some(text))
            }
            NodeData::MethodSignature(data) => {
                let modifiers = self.modifier_nodes_text_slice(data.modifiers);
                let name = self.member_name_node_text_slice(
                    data.name.expect("MethodSignature carries its name"),
                )?;
                let question = if data.question_token.is_some() {
                    "?"
                } else {
                    ""
                };
                let type_parameters =
                    self.type_parameter_nodes_text_slice(self.nodes_of(data.type_parameters))?;
                let parameters = self.parameter_nodes_text_slice(self.nodes_of(data.parameters))?;
                let mut text =
                    format!("{modifiers}{name}{question}{type_parameters}({parameters})");
                if let Some(annotation) = data.r#type {
                    text.push_str(": ");
                    text.push_str(&self.type_annotation_text_slice(annotation)?);
                } else {
                    text.push_str(": any");
                }
                Ok(Some(text))
            }
            NodeData::CallSignature(data) => {
                let type_parameters =
                    self.type_parameter_nodes_text_slice(self.nodes_of(data.type_parameters))?;
                let parameters = self.parameter_nodes_text_slice(self.nodes_of(data.parameters))?;
                let mut text = format!("{type_parameters}({parameters})");
                if let Some(annotation) = data.r#type {
                    text.push_str(": ");
                    text.push_str(&self.type_annotation_text_slice(annotation)?);
                } else {
                    text.push_str(": any");
                }
                Ok(Some(text))
            }
            NodeData::ConstructSignature(data) => {
                let type_parameters =
                    self.type_parameter_nodes_text_slice(self.nodes_of(data.type_parameters))?;
                let parameters = self.parameter_nodes_text_slice(self.nodes_of(data.parameters))?;
                let mut text = format!("new {type_parameters}({parameters})");
                if let Some(annotation) = data.r#type {
                    text.push_str(": ");
                    text.push_str(&self.type_annotation_text_slice(annotation)?);
                } else {
                    text.push_str(": any");
                }
                Ok(Some(text))
            }
            NodeData::IndexSignature(data) => {
                let modifiers = self.modifier_nodes_text_slice(data.modifiers);
                let parameters = self.parameter_nodes_text_slice(self.nodes_of(data.parameters))?;
                let mut text = format!("{modifiers}[{parameters}]");
                if let Some(annotation) = data.r#type {
                    text.push_str(": ");
                    text.push_str(&self.type_annotation_text_slice(annotation)?);
                } else {
                    text.push_str(": any");
                }
                Ok(Some(text))
            }
            NodeData::GetAccessor(data) => {
                let modifiers = self.modifier_nodes_text_slice(data.modifiers);
                let name = self.member_name_node_text_slice(
                    data.name.expect("GetAccessor carries its name"),
                )?;
                let type_parameters =
                    self.type_parameter_nodes_text_slice(self.nodes_of(data.type_parameters))?;
                let parameters = self.parameter_nodes_text_slice(self.nodes_of(data.parameters))?;
                let mut text = format!("{modifiers}get {name}{type_parameters}({parameters})");
                if let Some(annotation) = data.r#type {
                    text.push_str(": ");
                    text.push_str(&self.type_annotation_text_slice(annotation)?);
                } else {
                    text.push_str(": any");
                }
                if let Some(body) = data.body {
                    let Some(body) = self.display_clone_function_body_text(body)? else {
                        self.slice_reuse_had_error = true;
                        return Ok(None);
                    };
                    text.push(' ');
                    text.push_str(&body);
                }
                Ok(Some(text))
            }
            NodeData::SetAccessor(data) => {
                let modifiers = self.modifier_nodes_text_slice(data.modifiers);
                let name = self.member_name_node_text_slice(
                    data.name.expect("SetAccessor carries its name"),
                )?;
                let type_parameters =
                    self.type_parameter_nodes_text_slice(self.nodes_of(data.type_parameters))?;
                let parameters = self.parameter_nodes_text_slice(self.nodes_of(data.parameters))?;
                let annotation = match data.r#type {
                    Some(annotation) => self.type_annotation_text_slice(annotation)?,
                    None => "any".to_owned(),
                };
                let mut text =
                    format!("{modifiers}set {name}{type_parameters}({parameters}): {annotation}");
                if let Some(body) = data.body {
                    let Some(body) = self.display_clone_function_body_text(body)? else {
                        self.slice_reuse_had_error = true;
                        return Ok(None);
                    };
                    text.push(' ');
                    text.push_str(&body);
                }
                Ok(Some(text))
            }
            _ => unreachable!("TypeLiteral members contain only TypeElement nodes"),
        }
    }

    /// Member/binding property NAMES inside reused nodes: identifier,
    /// quoted string (double — clones), numeric text, and the
    /// trackExistingEntityName/serializeTypeOfExpression recovery for
    /// computed entity names.
    /// tsrs-native: string-face adapter over the exact property-name
    /// and entity-name ledger blocks.
    pub(crate) fn member_name_node_text_slice(&mut self, name: NodeId) -> CheckResult<String> {
        match self.data_of(name).clone() {
            NodeData::Identifier(data) => {
                Ok(tsc_binder::unescape_leading_underscores(&data.escaped_text).to_owned())
            }
            NodeData::PrivateIdentifier(data) => Ok(data.text),
            NodeData::StringLiteral(data) => string_literal_name_slice(&data.text, false),
            NodeData::NumericLiteral(data) => Ok(data.text.clone()),
            NodeData::ComputedPropertyName(data) => {
                let expression = data
                    .expression
                    .expect("ComputedPropertyName carries its expression");
                self.reused_computed_property_name_text_slice(expression)
            }
            _ => unreachable!("property names use identifier/private/literal/computed nodes"),
        }
    }

    /// visitExistingNodeTreeSymbolsWorker's computed-name branch
    /// (133564-133599). A name which resolves differently from the
    /// enclosing display scope is replaced by the literal face of its
    /// expression type, or by evaluateEntityNameExpression's value.
    fn reused_computed_property_name_text_slice(
        &mut self,
        expression: NodeId,
    ) -> CheckResult<String> {
        if !self.is_entity_name_expression(expression)
            || !self.reused_entity_name_introduces_error_slice(expression, SymbolFlags::VALUE)?
        {
            return self.reused_computed_property_expression_text_slice(expression);
        }

        let expression_type =
            self.check_expression_cached(expression, tsc_types::CheckMode::NORMAL)?;
        let expression_type = self
            .tables
            .get_regular_type_of_literal_type(expression_type);
        let literal = match &self.tables.type_of(expression_type).data {
            TypeData::Literal {
                value: tsc_types::LiteralValue::String(value),
            } => {
                let Some(value_utf8) = value.to_utf8() else {
                    return Ok(format!("[{}]", string_literal_name_text(value, false)?));
                };
                Some(EvalValue::Str(value_utf8))
            }
            TypeData::Literal {
                value: tsc_types::LiteralValue::Number(value),
            } => Some(EvalValue::Num(*value)),
            _ => {
                self.evaluate(expression, self.slice_display_enclosing)?
                    .value
            }
        };
        match literal {
            Some(EvalValue::Str(value)) => {
                if tsc_syntax::is_identifier_text_for_target(
                    &value,
                    self.options.emit_script_target(),
                ) {
                    Ok(value)
                } else {
                    Ok(format!("[{}]", string_literal_name_slice(&value, false)?))
                }
            }
            Some(EvalValue::Num(value)) => {
                let value = tsc_types::js_number_to_string(value);
                if value.starts_with('-') {
                    Ok(format!("[{value}]"))
                } else {
                    Ok(value)
                }
            }
            None => self.reused_computed_property_expression_text_slice(expression),
        }
    }

    fn reused_computed_property_expression_text_slice(
        &mut self,
        expression: NodeId,
    ) -> CheckResult<String> {
        match self.display_clone_computed_property_expression_text(expression)? {
            Some(text) => Ok(text),
            None => {
                self.slice_reuse_had_error = true;
                Ok(String::new())
            }
        }
    }

    /// tsc-port: parameterToParameterDeclarationName @6.0.3 (binding face)
    /// tsc-hash: 44f35dfdb10907de5255a8afcf28645007b1953c6aef8352dc742faa73a0804e
    /// tsc-span: _tsc.js:52880-52911
    ///
    /// cloneBindingName elides initializers and single-lines the
    /// emission; the printer pads object-pattern braces (`{ a, b }`)
    /// but not array patterns (`[a, b]`); omitted elements print
    /// empty (`[, x]`). Computed entity names share the visitor's
    /// tracker/recovery rewrite above.
    fn binding_pattern_text_slice(&mut self, pattern: NodeId) -> CheckResult<String> {
        self.binding_pattern_text_slice_worker(pattern, false)
    }

    fn binding_pattern_text_slice_worker(
        &mut self,
        pattern: NodeId,
        preserve_initializers: bool,
    ) -> CheckResult<String> {
        match self.data_of(pattern).clone() {
            NodeData::ObjectBindingPattern(data) => {
                let (elements, has_trailing_comma) = match data.elements {
                    Some(elements) => {
                        let elements = self.binder.node_array(elements);
                        (elements.nodes.clone(), elements.has_trailing_comma)
                    }
                    None => (Vec::new(), false),
                };
                if elements.is_empty() {
                    return Ok("{}".to_owned());
                }
                let mut rendered = Vec::with_capacity(elements.len());
                for element in elements {
                    rendered.push(self.binding_element_text_slice(element, preserve_initializers)?);
                }
                let mut contents = rendered.join(", ");
                if has_trailing_comma {
                    contents.push(',');
                }
                Ok(format!("{{ {contents} }}"))
            }
            NodeData::ArrayBindingPattern(data) => {
                let (elements, has_trailing_comma) = match data.elements {
                    Some(elements) => {
                        let elements = self.binder.node_array(elements);
                        (elements.nodes.clone(), elements.has_trailing_comma)
                    }
                    None => (Vec::new(), false),
                };
                let mut rendered = Vec::with_capacity(elements.len());
                for element in elements {
                    rendered.push(self.binding_element_text_slice(element, preserve_initializers)?);
                }
                let mut contents = rendered.join(", ");
                if has_trailing_comma {
                    contents.push(',');
                }
                Ok(format!("[{contents}]"))
            }
            _ => unreachable!("cloneBindingName receives an object/array binding pattern"),
        }
    }

    fn binding_element_text_slice(
        &mut self,
        element: NodeId,
        preserve_initializers: bool,
    ) -> CheckResult<String> {
        match self.data_of(element).clone() {
            NodeData::OmittedExpression(_) => Ok(String::new()),
            NodeData::BindingElement(data) => {
                let dots = if data.dot_dot_dot_token.is_some() {
                    "..."
                } else {
                    ""
                };
                let property = match data.property_name {
                    Some(property_name) => {
                        format!("{}: ", self.member_name_node_text_slice(property_name)?)
                    }
                    None => String::new(),
                };
                let name_node = data.name.expect("BindingElement carries its binding name");
                let name = match self.data_of(name_node) {
                    NodeData::Identifier(data) => {
                        tsc_binder::unescape_leading_underscores(&data.escaped_text).to_owned()
                    }
                    NodeData::ObjectBindingPattern(_) | NodeData::ArrayBindingPattern(_) => {
                        self.binding_pattern_text_slice_worker(name_node, preserve_initializers)?
                    }
                    _ => self.member_name_node_text_slice(name_node)?,
                };
                let initializer = if preserve_initializers {
                    match data.initializer {
                        Some(initializer) => {
                            format!(
                                " = {}",
                                self.reused_initializer_expression_text_slice(initializer)?
                            )
                        }
                        None => String::new(),
                    }
                } else {
                    String::new()
                };
                Ok(format!("{dots}{property}{name}{initializer}"))
            }
            _ => unreachable!("binding arrays contain BindingElement/OmittedExpression nodes"),
        }
    }

    /// tsc-port: getPropertyNameNodeForSymbol @6.0.3
    /// tsc-hash: c1c3578eec910db69573311722f0d3fb5b95881f3bcad46ac3fafdf5d402e4a6
    /// tsc-span: _tsc.js:53411-53442
    ///
    /// (createPropertyNameNodeForIdentifierOrLiteral, 19208-19212, is
    /// the free-fn tail below.)
    ///
    /// Hash-private names (getClonedHashPrivateName) stay out of
    /// slice. tsc classifies computed/element-access names through
    /// checkExpression's StringLike; the slice reads the late-bound
    /// nameType's flags instead — identical for the literal-typed keys
    /// late binding produces, and the display walk cannot re-enter
    /// checkExpression (recorded deviation).
    fn property_name_slice(
        &mut self,
        property: SymbolId,
        fully_qualified: bool,
    ) -> CheckResult<String> {
        if let Some(value_declaration) = self.binder.symbol(property).value_declaration {
            let name = tsc_binder::node_util::get_name_of_declaration(
                self.binder.source_of_node(value_declaration),
                value_declaration,
            );
            if let Some(name) = name {
                if let NodeData::PrivateIdentifier(data) = self.data_of(name) {
                    // getClonedHashPrivateName (55445-55449): private
                    // property names are cloned before nameType/raw-name
                    // processing and retain their `#` face.
                    return Ok(data.text.clone());
                }
            }
        }
        let declarations = self.binder.symbol(property).declarations.clone();
        let name_type = self.links.symbol(property).name_type;
        let name_type_flags = name_type.map(|name_type| self.tables.flags_of(name_type));
        let string_named = !declarations.is_empty()
            && declarations
                .iter()
                .all(|&declaration| self.declaration_is_string_named(declaration, name_type_flags));
        let single_quote = !declarations.is_empty()
            && declarations
                .iter()
                .all(|&declaration| self.declaration_is_single_quoted_string_named(declaration));
        let is_method = self
            .binder
            .symbol(property)
            .flags
            .intersects(tsc_types::SymbolFlags::METHOD);
        if let Some(name_type) = name_type {
            let flags = self.tables.flags_of(name_type);
            if flags.intersects(TypeFlags::STRING_LITERAL | TypeFlags::NUMBER_LITERAL) {
                let name = match &self.tables.type_of(name_type).data {
                    TypeData::Literal { value } => match value {
                        tsc_types::LiteralValue::String(text) => {
                            let Some(text_utf8) = text.to_utf8() else {
                                return string_literal_name_text(text, single_quote);
                            };
                            text_utf8
                        }
                        tsc_types::LiteralValue::Number(value) => {
                            tsc_types::js_number_to_string(*value)
                        }
                        tsc_types::LiteralValue::BigInt(_) => {
                            unreachable!("string/number literal flags imply string/number value")
                        }
                    },
                    _ => unreachable!("literal flags imply literal data"),
                };
                if !tsc_syntax::is_identifier_text(&name)
                    && (string_named || !crate::evaluate::is_numeric_literal_name(&name))
                {
                    return string_literal_name_slice(&name, single_quote);
                }
                if crate::evaluate::is_numeric_literal_name(&name) && name.starts_with('-') {
                    // 53434: negative numeric names print as the
                    // computed `[-N]` face (prefix-minus numeric).
                    return Ok(format!("[{name}]"));
                }
                return identifier_or_literal_name_slice(
                    &name,
                    string_named,
                    single_quote,
                    is_method,
                );
            }
            if flags.intersects(TypeFlags::UNIQUE_ES_SYMBOL) {
                // 53439-53441: createComputedPropertyName(
                // symbolToExpression(nameType.symbol, Value)).
                // addPropertyToElementList arms the context with the
                // property's OWN declaration (52265-52267) before the
                // name renders, so accessibility from that declaration
                // decides the face: lexically reachable symbols print
                // bare (`[sym]` — oracle: top-level declarations),
                // container-qualified ones print the chain (`[B.sym]`
                // — oracle: namespace-nested declarations, no FQ retry
                // involved), and the getTypeNameForErrorDisplay retry
                // arms the walk even without an enclosing (52946).
                let name_symbol = self
                    .tables
                    .type_of(name_type)
                    .symbol
                    .expect("unique symbols carry their declaration symbol");
                let enclosing = self
                    .binder
                    .symbol(property)
                    .value_declaration
                    .or_else(|| declarations.first().copied());
                let face =
                    self.symbol_expression_face_slice(name_symbol, enclosing, fully_qualified)?;
                return Ok(format!("[{face}]"));
            }
        }
        let raw =
            tsc_binder::unescape_leading_underscores(&self.binder.symbol(property).escaped_name)
                .to_owned();
        identifier_or_literal_name_slice(&raw, string_named, single_quote, is_method)
    }

    /// tsc-port: isStringNamed @6.0.3 (slice face)
    /// tsc-hash: c000f08977999a9f153126ccfb4e5b4c8721c5e160a361bd941308799c3c657d
    /// tsc-span: _tsc.js:53388-53402
    // h2-7a-m-3 widening: shared property-name classification.
    pub(crate) fn declaration_is_string_named(
        &self,
        declaration: NodeId,
        name_type_flags: Option<TypeFlags>,
    ) -> bool {
        let name = tsc_binder::node_util::get_name_of_declaration(
            self.binder.source_of_node(declaration),
            declaration,
        );
        let Some(name) = name else {
            return false;
        };
        match self.data_of(name) {
            NodeData::StringLiteral(_) => true,
            // checkExpression(name.expression) StringLike in tsc; the
            // slice substitutes the late-bound nameType (see
            // property_name_slice).
            NodeData::ComputedPropertyName(_) | NodeData::ElementAccessExpression(_) => {
                name_type_flags.is_some_and(|flags| flags.intersects(TypeFlags::STRING_LIKE))
            }
            _ => false,
        }
    }

    /// tsc-port: isSingleQuotedStringNamed @6.0.3
    /// tsc-hash: a1cfaf3bb4dfc1e20d532883c41dc2ed9d730618cb43b9184a022875a3013093
    /// tsc-span: _tsc.js:53403-53410
    ///
    /// The parser never synthesizes string names, so the
    /// name.singleQuote half is dead; the source-text probe reads the
    /// literal's closing quote (trivia-immune, unterminated literals
    /// cannot late-bind a member).
    // h2-7a-m-3 widening: shared source-quote classification.
    pub(crate) fn declaration_is_single_quoted_string_named(&self, declaration: NodeId) -> bool {
        let source = self.binder.source_of_node(declaration);
        let Some(name) = tsc_binder::node_util::get_name_of_declaration(source, declaration) else {
            return false;
        };
        if !matches!(self.data_of(name), NodeData::StringLiteral(_)) {
            return false;
        }
        let end = source.arena.node(name).end as usize;
        end > 0 && source.text().as_bytes().get(end - 1) == Some(&b'\'')
    }

    /// tsc-port: formatUnionTypes @6.0.3 (error-display face)
    /// tsc-hash: bb658f102c7d7e506fd2bcdd6e4d963929fd8f222f257e9ee119203618797547
    /// tsc-span: _tsc.js:55474-55498
    ///
    /// The nodeBuilder formats union members before rendering (51546):
    /// nullable members re-append at the tail (null before undefined —
    /// the eOPT missing marker re-appends as plain `undefined`), and a
    /// consecutive member run matching an enum-like base's full list
    /// collapses to the base (`true | false` → `boolean`; enum-member
    /// runs → the enum). `expandingEnum` is a verbosity-walk input the
    /// error-display slice never sets, so the collapse probe runs for
    /// every non-nullable member (the shipped `t.flags | EnumLike`
    /// disjunct is always-true by construction).
    fn format_union_types(&mut self, types: &[TypeId]) -> CheckResult<Vec<TypeId>> {
        let mut result = Vec::new();
        let mut combined = TypeFlags::from_bits(0);
        let mut i = 0;
        while i < types.len() {
            let t = types[i];
            let t_flags = self.tables.flags_of(t);
            combined = TypeFlags::from_bits(combined.bits() | t_flags.bits());
            if !t_flags.intersects(TypeFlags::NULLABLE) {
                let base = if t_flags.intersects(TypeFlags::BOOLEAN_LITERAL) {
                    self.tables.intrinsics.boolean
                } else {
                    self.get_base_type_of_enum_like_type(t)?
                };
                if self.tables.flags_of(base).intersects(TypeFlags::UNION) {
                    let base_types = match &self.tables.type_of(base).data {
                        TypeData::Union { types, .. } => types.to_vec(),
                        _ => Vec::new(),
                    };
                    let count = base_types.len();
                    if count > 0 && i + count <= types.len() {
                        let run_last = self
                            .tables
                            .get_regular_type_of_literal_type(types[i + count - 1]);
                        let base_last = self
                            .tables
                            .get_regular_type_of_literal_type(base_types[count - 1]);
                        if run_last == base_last {
                            result.push(base);
                            i += count;
                            continue;
                        }
                    }
                }
                result.push(t);
            }
            i += 1;
        }
        if combined.intersects(TypeFlags::NULL) {
            result.push(self.tables.intrinsics.null);
        }
        if combined.intersects(TypeFlags::UNDEFINED) {
            result.push(self.tables.intrinsics.undefined);
        }
        Ok(result)
    }

    /// tsc-port: getTupleElementLabel @6.0.3 (declaration arm)
    /// tsc-hash: cfaef41e5163a36e33fb797ca0f1cf2445bcc1cf9453ac75b2f61681f2b472b1
    /// tsc-span: _tsc.js:78150-78157
    ///
    /// The renderer only reaches the declaration arm (51958 gates on a
    /// present label; the label-less overload half synthesizes
    /// signature-hint names). tsc Debug.asserts the label name IS an
    /// Identifier — a pattern-named label throws in shipped tsc, so the
    /// local expect deliberately preserves that invariant. The call-site
    /// unescapeLeadingUnderscores (51961) is folded in.
    pub(crate) fn tuple_element_label(&self, declaration: NodeId) -> CheckResult<String> {
        // h2-7a-m-3 widening
        let name = match self.data_of(declaration) {
            NodeData::NamedTupleMember(data) => data.name,
            NodeData::Parameter(data) => data.name,
            _ => None,
        };
        let text = name
            .and_then(|name| self.identifier_text(name))
            .expect("getTupleElementLabel Debug.asserts an Identifier label");
        Ok(tsc_binder::unescape_leading_underscores(text).to_owned())
    }
}

/// forEachSymbolTableInScope table identity for the accessibility
/// walk's visited guard — tsc keys by table OBJECT identity
/// (pushIfUnique over the table references, 50315); the slice keys by
/// provenance: locals by their owning node, export views by their
/// owning symbol, and the globals tail.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ScopeTableKey {
    Globals,
    Exports(SymbolId),
    Locals(NodeId),
}

struct SliceTypeNodeFace {
    text: String,
    kind: SliceTypeNodeKind,
    /// Factory `parenthesizeTypeArguments` additionally inspects
    /// whether a leading Function/Constructor TypeNode is generic;
    /// kind alone cannot carry that post-transform fact.
    has_type_parameters: bool,
}

impl SliceTypeNodeFace {
    fn new(text: String, kind: SliceTypeNodeKind) -> Self {
        Self {
            text,
            kind,
            has_type_parameters: false,
        }
    }
}

/// The would-be TypeNode kind of a slice rendering. The factory's
/// parenthesizer rules (_tsc.js 20540-20617) branch on the child
/// node's KIND at each join; the string renderer carries the kind
/// beside the text so the joins below apply the same rules. Only
/// kinds the slice can produce are listed, including InferType from
/// reused JSDoc variadics.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SliceTypeNodeKind {
    /// KeywordTypeNode — intrinsics; no reachable rule wraps one.
    Keyword,
    /// LiteralTypeNode — string/number literal displays.
    Literal,
    /// TypeReferenceNode — symbol heads, alias/reference `Name<...>`,
    /// type parameters and the variance markers.
    Reference,
    /// TypeLiteralNode — the member-less `{}`.
    TypeLiteral,
    Union,
    Intersection,
    /// TypeOperatorNode — `keyof T`, `readonly T[]`, `readonly [...]`.
    TypeOperator,
    /// TypeQueryNode — `typeof C` (class statics / enum objects).
    TypeQuery,
    /// ImportTypeNode — `typeof import("...")` module value faces
    /// (the `typeof` head is the node's own isTypeOf flag, so the
    /// kind is ImportType, not TypeQuery; no parenthesizer rule
    /// lists the kind, 20540-20606).
    ImportType,
    /// ArrayTypeNode — `T[]`.
    Array,
    /// TupleTypeNode — `[...]`.
    Tuple,
    /// TemplateLiteralTypeNode — `` `a${T}b` ``; no parenthesizer
    /// rule lists the kind (20540-20606), so the face never wraps.
    TemplateLiteral,
    /// IndexedAccessTypeNode — `T[K]`; no parenthesizer rule lists
    /// the kind (the node's own OBJECT side applies the postfix rule
    /// at creation, 22372-22378), so the face never wraps.
    IndexedAccess,
    /// ConditionalTypeNode — `T extends U ? X : Y`.
    Conditional,
    /// InferTypeNode — `infer T`; postfix TypeNodes always wrap it.
    Infer,
    /// FunctionTypeNode — `(...) => R` (the signature rung).
    FunctionType,
    /// ConstructorTypeNode — `new (...) => R` / `abstract new ...`.
    ConstructorType,
}

/// signatureToSignatureDeclarationHelper's kind argument, restricted
/// to the faces the display slice produces (52504-52631). The
/// Constructor / FunctionDeclaration / FunctionExpression / Arrow /
/// JSDocFunctionType / IndexSignature / MethodDeclaration kinds ride
/// declaration-emit bands the slice never enters.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SliceSignatureKind {
    FunctionType,
    ConstructorType,
    CallSignature,
    ConstructSignature,
    MethodSignature,
    GetAccessor,
    SetAccessor,
}

/// One rendered parameter: the declared symbol face (declaration-
/// carrying) or a tuple-expanded transient face
/// (getExpandedParameters' created symbols, 57923-57934 — carried as
/// fields instead of minted symbols; nothing outside the render
/// observes them).
struct SliceParameterFace {
    symbol: Option<SymbolId>,
    declaration: Option<NodeId>,
    /// Synthesized label for expanded faces (already unescaped).
    name: Option<String>,
    ty: TypeId,
    optional: bool,
    rest: bool,
}

/// tsc factory printer spelling for a mapped-type modifier token. The
/// parser stores only the leading `+`/`-` token for prefixed modifiers;
/// the readonly/question keyword is implicit in the MappedType field.
fn mapped_modifier_text_slice(token: Option<SyntaxKind>, plain: &str) -> String {
    match token {
        None => String::new(),
        Some(SyntaxKind::ReadonlyKeyword | SyntaxKind::QuestionToken) => plain.to_owned(),
        Some(SyntaxKind::PlusToken) => format!("+{plain}"),
        Some(SyntaxKind::MinusToken) => format!("-{plain}"),
        Some(_) => unreachable!("MappedType modifiers use readonly/question/+/- tokens"),
    }
}

/// tsc canUsePropertyAccess (19293-19299): `charCodeAt` tests one
/// UTF-16 CODE UNIT (or the unit after `#`) at the active language
/// version. In particular, an astral identifier starts with a high
/// surrogate here and therefore requires element access.
// h2-7a-m-3 widening: shared property-access spelling decision.
pub(crate) fn can_use_property_access_slice(
    name: &str,
    language_version: tsc_types::ScriptTarget,
) -> bool {
    let mut units = name.encode_utf16();
    let Some(first) = units.next() else {
        return false;
    };
    let start = if first == u16::from(b'#') {
        let Some(after_hash) = units.next() else {
            return false;
        };
        after_hash
    } else {
        first
    };
    char::from_u32(u32::from(start)).is_some_and(|start| {
        tsc_syntax::is_identifier_text_for_target(&format!("{start}x"), language_version)
    })
}

/// createExpressionFromSymbolChain's stripQuotes + `/\\./g` unescape
/// (53368-53371).
fn strip_symbol_name_quotes_slice(name: &str) -> String {
    let body = if name.len() >= 2 {
        &name[1..name.len() - 1]
    } else {
        ""
    };
    let mut chars = body.chars();
    let mut out = String::with_capacity(body.len());
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(escaped) = chars.next() {
                out.push(escaped);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// tsc-port: parenthesizeConstituentTypeOfUnionType @6.0.3 (kind test)
/// tsc-hash: 6a071b4a7c2eebb30005580cc9d725278da358dddc6e0a5a2543d51c9b33f0c3
/// tsc-span: _tsc.js:20540-20548
///
/// The fall-through (parenthesizeCheckTypeOfConditionalType,
/// 20585-20593) wraps function/constructor/conditional heads — the
/// signature rung produces the first two.
fn union_constituent_needs_parens(kind: SliceTypeNodeKind) -> bool {
    matches!(
        kind,
        SliceTypeNodeKind::Union
            | SliceTypeNodeKind::Intersection
            | SliceTypeNodeKind::FunctionType
            | SliceTypeNodeKind::ConstructorType
            | SliceTypeNodeKind::Conditional
    )
}

fn conditional_check_type_needs_parens(kind: SliceTypeNodeKind) -> bool {
    matches!(
        kind,
        SliceTypeNodeKind::FunctionType
            | SliceTypeNodeKind::ConstructorType
            | SliceTypeNodeKind::Conditional
    )
}

/// tsc-port: parenthesizeConstituentTypeOfIntersectionType @6.0.3 (kind test)
/// tsc-hash: f1132158c9dd447d9a5c54e06ca76ce42b477379ecdb7c41c266f6cc4ce44e5f
/// tsc-span: _tsc.js:20552-20559
fn intersection_constituent_needs_parens(kind: SliceTypeNodeKind) -> bool {
    matches!(
        kind,
        SliceTypeNodeKind::Union | SliceTypeNodeKind::Intersection
    ) || union_constituent_needs_parens(kind)
}

/// tsc-port: parenthesizeOperandOfTypeOperator @6.0.3 (kind test)
/// tsc-hash: fba31fe4d809aaac1d32866edcb0a1d7266daef9c5d24199487d7ae6aed17f9a
/// tsc-span: _tsc.js:20563-20569
fn type_operator_operand_needs_parens(kind: SliceTypeNodeKind) -> bool {
    matches!(kind, SliceTypeNodeKind::Intersection) || intersection_constituent_needs_parens(kind)
}

fn readonly_type_operator_operand_needs_parens(kind: SliceTypeNodeKind) -> bool {
    matches!(kind, SliceTypeNodeKind::TypeOperator) || type_operator_operand_needs_parens(kind)
}

/// tsc-port: parenthesizeNonArrayTypeOfPostfixType @6.0.3 (kind test)
/// tsc-hash: 90b6701d51af1b9f1122f0d5ffcc9febe951cdae5b1430df8dfcb37781993928
/// tsc-span: _tsc.js:20577-20585
///
/// The infer arm is observable for a reused JSDoc variadic over
/// InferType; the typeof arm wraps the TypeQuery face and the operand
/// fall-through supplies the intersection/union wraps.
fn non_array_postfix_operand_needs_parens(kind: SliceTypeNodeKind) -> bool {
    matches!(
        kind,
        SliceTypeNodeKind::Infer | SliceTypeNodeKind::TypeOperator | SliceTypeNodeKind::TypeQuery
    ) || type_operator_operand_needs_parens(kind)
}

/// tsc-port: parenthesizeTypeOfOptionalType @6.0.3 (kind test)
/// tsc-hash: fb05d98073b5129ffea157859cde639e822501ab06e62214b80b3e4a15071c41
/// tsc-span: _tsc.js:20603-20606
///
/// hasJSDocPostfixQuestion walks JSDoc type-node shapes the slice
/// cannot render — the postfix rule is the whole reachable test.
fn optional_type_operand_needs_parens(kind: SliceTypeNodeKind) -> bool {
    non_array_postfix_operand_needs_parens(kind)
}

/// tsc-port: getLiteralText @6.0.3 (synthesized template branch)
/// tsc-hash: e09a970bf93f42fa341190e5980f0adbc970e1d809299edf94e843729db22090
/// tsc-span: _tsc.js:13660-13677
///
/// The nodeBuilder's template heads carry cooked text and no rawText,
/// and typeToTypeNodeHelper sets no NoAsciiEscaping emit flag on them
/// (contrast the StringLiteral arm, 51401-51403), so the printer
/// derives rawText = escapeTemplateSubstitution(escapeNonAsciiString
/// (text, backtick)). The `` ` ``/`${`/`}` delimiters are the callers'
/// (the template arm concatenation).
fn template_text_raw(text: &str) -> String {
    template_text_utf16_raw(&text.encode_utf16().collect::<Vec<_>>())
}

/// escapeString(backtick) followed by escapeNonAsciiString, over the
/// printer's native UTF-16 code-unit domain. Keeping this join in one
/// pass preserves unpaired surrogates while retaining the exact
/// escapedCharsMap/null-lookahead behavior.
fn template_text_utf16_raw(units: &[u16]) -> String {
    let mut out = String::with_capacity(units.len());
    let mut index = 0usize;
    while index < units.len() {
        let unit = units[index];
        match unit {
            0x000D if units.get(index + 1) == Some(&0x000A) => {
                out.push_str("\\r\\n");
                index += 2;
                continue;
            }
            0x005C => out.push_str("\\\\"),
            0x0060 => out.push_str("\\`"),
            0 => {
                if units
                    .get(index + 1)
                    .is_some_and(|next| (b'0' as u16..=b'9' as u16).contains(next))
                {
                    out.push_str("\\x00");
                } else {
                    out.push_str("\\0");
                }
            }
            0x0009 => out.push_str("\\t"),
            0x0008 => out.push_str("\\b"),
            0x000B => out.push_str("\\v"),
            0x000C => out.push_str("\\f"),
            0x000D => out.push_str("\\r"),
            0x2028 => out.push_str("\\u2028"),
            0x2029 => out.push_str("\\u2029"),
            0x0085 => out.push_str("\\u0085"),
            0x0000..=0x001F if unit != 0x000A => {
                out.push_str(&encode_utf16_escape_sequence(unit));
            }
            0x0080..=0xFFFF => out.push_str(&encode_utf16_escape_sequence(unit)),
            _ => out.push(char::from_u32(u32::from(unit)).expect("ASCII code unit is a scalar")),
        }
        index += 1;
    }
    escape_template_substitution(&out)
}

/// encodeUtf16EscapeSequence (16296-16300, folded into
/// template_text_utf16_raw): uppercase hex, four digits.
fn encode_utf16_escape_sequence(unit: u16) -> String {
    format!("\\u{unit:04X}")
}

/// tsc-port: escapeString @6.0.3 (doubleQuote flavor)
/// tsc-hash: a41f6d5932395df14118761cfc227d8ad3266e0e2f3133c4ec5857ff7e0b4d2d
/// tsc-span: _tsc.js:16311-16314
///
/// doubleQuoteEscapedCharsRegExp = backslash, `"`, the FULL C0 range
/// (`\n`/`\t` included — unlike the backtick class), U+2028/U+2029/
/// U+0085; escapedCharsMap first (lowercase u-escapes), NUL digit
/// lookahead, then the UPPERCASE 4-hex fallback. Non-ASCII passes
/// through raw — the StringLiteral face sets NoAsciiEscaping.
pub(crate) fn string_literal_type_display_text(text: &tsc_types::TemplateText) -> String {
    let units = text.units();
    let mut out = String::new();
    let mut index = 0usize;
    while index < units.len() {
        let unit = units[index];
        match unit {
            0x005C => out.push_str("\\\\"),
            0x0022 => out.push_str("\\\""),
            0 => {
                if units
                    .get(index + 1)
                    .is_some_and(|next| (b'0' as u16..=b'9' as u16).contains(next))
                {
                    out.push_str("\\x00");
                } else {
                    out.push_str("\\0");
                }
            }
            0x0009 => out.push_str("\\t"),
            0x000B => out.push_str("\\v"),
            0x000C => out.push_str("\\f"),
            0x0008 => out.push_str("\\b"),
            0x000D => out.push_str("\\r"),
            0x000A => out.push_str("\\n"),
            0x2028 => out.push_str("\\u2028"),
            0x2029 => out.push_str("\\u2029"),
            0x0085 => out.push_str("\\u0085"),
            0x0001..=0x001F => out.push_str(&encode_utf16_escape_sequence(unit)),
            0xD800..=0xDBFF
                if units
                    .get(index + 1)
                    .is_some_and(|next| (0xDC00..=0xDFFF).contains(next)) =>
            {
                let high = u32::from(unit - 0xD800);
                let low = u32::from(units[index + 1] - 0xDC00);
                let scalar = 0x10000 + (high << 10) + low;
                out.push(char::from_u32(scalar).expect("valid surrogate pair"));
                index += 1;
            }
            0xD800..=0xDFFF => out.push_str(&encode_utf16_escape_sequence(unit)),
            _ => out.push(char::from_u32(u32::from(unit)).expect("BMP scalar")),
        }
        index += 1;
    }
    out
}

/// tsc-port: escapeTemplateSubstitution @6.0.3
/// tsc-hash: f078436145475a9ae2bec1c683c638bb1e8161d02d10f155a9088dc65faf678d
/// tsc-span: _tsc.js:16263-16266
fn escape_template_substitution(s: &str) -> String {
    s.replace("${", "\\${")
}

/// tsc-port: createArrayTypeNode @6.0.3 (string form)
/// tsc-hash: 71e29dc77eaa156837ba89b71ffc6b028e29a3da6e605952ea80b7443b0a38aa
/// tsc-span: _tsc.js:22229-22234
fn array_type_node_text(element: String, kind: SliceTypeNodeKind) -> String {
    if non_array_postfix_operand_needs_parens(kind) {
        format!("({element})[]")
    } else {
        format!("{element}[]")
    }
}

/// tsc-port: createPropertyNameNodeForIdentifierOrLiteral @6.0.3
/// tsc-hash: eda75843cb64ba3fbbfba1505f7caa40165242100f8be7821f1fa8f9889022c4
/// tsc-span: _tsc.js:19208-19212
///
/// The numeric face prints `(+name).toString()` (factory
/// createNumericLiteral over the coerced value); the string face is
/// the printer's quoted literal.
fn identifier_or_literal_name_slice(
    name: &str,
    string_named: bool,
    single_quote: bool,
    is_method: bool,
) -> CheckResult<String> {
    let is_method_named_new = is_method && name == "new";
    if !is_method_named_new && tsc_syntax::is_identifier_text(name) {
        return Ok(name.to_owned());
    }
    if !string_named
        && !is_method_named_new
        && crate::evaluate::is_numeric_literal_name(name)
        && crate::evaluate::js_string_to_number(name) >= 0.0
    {
        return Ok(tsc_types::js_number_to_string(
            crate::evaluate::js_string_to_number(name),
        ));
    }
    string_literal_name_slice(name, single_quote)
}

/// tsc-port: getLiteralText @6.0.3
/// tsc-hash: d0aba9b2b5367875618a7bcb2548b8bccc629c113db8cf17f3acd5d5b4710b48
/// tsc-span: _tsc.js:13647-13658
///
/// tsc-port: escapeNonAsciiString @6.0.3
/// tsc-hash: 021cee3d2e7b0591c8fe7962bb2634f8ff87967a886a64b6daae36983f2e230e
/// tsc-span: _tsc.js:16316-16319
///
/// Node-builder string names are synthesized and do not carry
/// NoAsciiEscaping. The printer therefore applies quote-sensitive
/// `escapeString` and then escapes every non-ASCII UTF-16 code unit.
/// Iterating code units (rather than Rust scalar values) preserves the
/// exact surrogate-pair spelling for astral characters.
pub(crate) fn string_literal_name_slice(name: &str, single_quote: bool) -> CheckResult<String> {
    string_literal_name_text(&tsc_types::TemplateText::from_utf8(name), single_quote)
}

fn string_literal_name_text(
    name: &tsc_types::TemplateText,
    single_quote: bool,
) -> CheckResult<String> {
    let quote = if single_quote { '\'' } else { '"' };
    let units = name.units();
    let mut escaped = String::new();
    for (index, &unit) in units.iter().enumerate() {
        let quote_unit = quote as u16;
        match unit {
            0 if units
                .get(index + 1)
                .is_some_and(|next| (b'0' as u16..=b'9' as u16).contains(next)) =>
            {
                escaped.push_str("\\x00");
            }
            0 => escaped.push_str("\\0"),
            0x0008 => escaped.push_str("\\b"),
            0x0009 => escaped.push_str("\\t"),
            0x000B => escaped.push_str("\\v"),
            0x000C => escaped.push_str("\\f"),
            0x000D => escaped.push_str("\\r"),
            0x000A => escaped.push_str("\\n"),
            0x005C => escaped.push_str("\\\\"),
            0x2028 => escaped.push_str("\\u2028"),
            0x2029 => escaped.push_str("\\u2029"),
            0x0085 => escaped.push_str("\\u0085"),
            value if value == quote_unit => {
                escaped.push('\\');
                escaped.push(quote);
            }
            0x0001..=0x001F | 0x0080..=0xFFFF => {
                escaped.push_str(&encode_utf16_escape_sequence(unit));
            }
            _ => escaped.push(
                char::from_u32(u32::from(unit)).expect("ASCII UTF-16 unit is a scalar value"),
            ),
        }
    }
    Ok(format!("{quote}{escaped}{quote}"))
}

impl crate::declaration_emit::DeclarationEmitAccessibilityPrimitives for CheckerState<'_> {
    fn declaration_emit_accessible_symbol_chain(
        &mut self,
        symbol: SymbolId,
        meaning: SymbolFlags,
        enclosing: Option<NodeId>,
    ) -> CheckResult<Option<Vec<SymbolId>>> {
        self.accessible_symbol_chain_at_slice(symbol, meaning, enclosing)
    }

    fn declaration_emit_containers_of_symbol(
        &mut self,
        symbol: SymbolId,
        enclosing: Option<NodeId>,
        meaning: SymbolFlags,
    ) -> CheckResult<Vec<SymbolId>> {
        self.containers_of_symbol_slice(symbol, enclosing, meaning)
    }
}

#[cfg(test)]
#[path = "../tests/unit/check/tests.rs"]
mod tests;

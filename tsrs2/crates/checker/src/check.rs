//! The check driver (M4 5.4): checkSourceFileWorker's two-phase pass —
//! eager statements IN SOURCE ORDER, then the deferred-node drain —
//! plus the first live statement-position checks (type parameter
//! lists and the 2636 variance-annotation probe).
//!
//! Dispatch discipline: checkSourceElementWorker's switch is ported
//! with the FULL kind list. An Unsupported unwind abandons the CURRENT
//! element's remaining checks only (an honest FN) — the driver
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

use tsrs2_binder::{node_util, SymbolId};
use tsrs2_diags::{gen as diagnostics, DiagnosticCategory, DiagnosticMessage};
use tsrs2_syntax::{for_each_child, NodeData, NodeId, SyntaxKind};
use tsrs2_types::{
    CheckFlags, ElementFlags, ModifierFlags, NodeCheckFlags, ObjectFlags, SymbolFlags, TypeData,
    TypeFacts, TypeFlags, TypeId, UnionReduction,
};

use crate::state::{CheckResult2, CheckerState, SignatureId, SignatureKind, Unsupported};

/// Debug-only unwind census (the unsupported-unwind invariant):
/// every transient stack an element check may push must be back at
/// its ENTRY depth when the element completes — Ok or Err alike —
/// and no `Resolving` sentinel may stay open across elements. A
/// deeper stack or a leaked sentinel after an Unsupported unwind is
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
    variance_handler_stack: usize,
    class_interface_declared_in_progress: usize,
    type_parameter_defaults_in_progress: usize,
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
            variance_handler_stack: self.variance_handler_stack.len(),
            class_interface_declared_in_progress: self.class_interface_declared_in_progress.len(),
            type_parameter_defaults_in_progress: self.type_parameter_defaults_in_progress.len(),
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
            "unsupported-unwind invariant violated after {boundary} of {node:?} \
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
        // The 6.6f flag registry is same-file-scoped like the report
        // faces that consult it.
        self.flow_inert_answer_nodes.clear();
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
        // the exportsChecked once-guard). Unsupported containment
        // matches check_source_element's element boundary.
        if self.binder.is_external_or_common_js_module_of_node(root) {
            if let Err(err) = self.check_external_module_exports(root) {
                // The exports walk spans the whole module — a
                // contained run leaves an unknown subset unchecked, so
                // the file's comment-directive exemption (2578) must
                // see the gap (S8).
                self.mark_partially_checked_node(root, err.reason.clone());
                if std::env::var_os("TSRS_TRACE_CONTAIN").is_some() {
                    eprintln!("contained @{root:?}: {}", err.reason);
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
                variance_handler_stack: 0,
                class_interface_declared_in_progress: 0,
                type_parameter_defaults_in_progress: 0,
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
                "unsupported-unwind invariant violated at the end of file {root:?}"
            );
        }
        self.links
            .or_node_check_flags(self.speculation_depth, root, NodeCheckFlags::TYPE_CHECKED);
    }

    /// tsc-port: skipTypeCheckingWorker @6.0.3
    /// tsc-hash: 8dcc4a08f5b94c3c9ada5b6c1e86885714d7db12c71cbf857ca88531632bd0c3
    /// tsc-span: _tsc.js:18877-18903
    ///
    /// The state-local arm represents skipLibCheck for declaration
    /// files. The public program driver owns the source pragmas and
    /// applies the @ts-nocheck/checkJs-off arms before entering this
    /// method, so skipped files produce neither file diagnostics nor
    /// shared global diagnostics.
    fn skip_type_checking(&self, root: NodeId) -> bool {
        self.options.skip_lib_check == Some(true)
            && self.binder.source_of_node(root).is_declaration_file
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
        self.node_flags(root) & tsrs2_types::NodeFlags::AMBIENT.bits() != 0
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
                    .map(|list| self.node_flags(list) & tsrs2_types::NodeFlags::BLOCK_SCOPED.bits())
                    .unwrap_or(0),
                _ => 0,
            }
        } else {
            0
        };
        let using_kinds = (
            tsrs2_types::NodeFlags::USING.bits(),
            tsrs2_types::NodeFlags::AWAIT_USING.bits(),
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
            self.node_flags(parent) & tsrs2_types::NodeFlags::AMBIENT.bits() != 0
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
            let modifier_text = tsrs2_syntax::tokens::token_to_string(modifier_kind).unwrap_or("?");
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
                        && self.node_flags(node) & tsrs2_types::NodeFlags::AMBIENT.bits() == 0
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
                            self.node_flags(list) & tsrs2_types::NodeFlags::USING.bits() != 0
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
        if self.node_flags(node) & tsrs2_types::NodeFlags::AMBIENT.bits() == 0 {
            return;
        }
        let parent = self.parent_of(node);
        let parent_kind = parent.map(|parent| self.kind_of(parent));
        let parent_is_function_like_or_accessor = parent_kind.is_some_and(|kind| {
            tsrs2_binder::node_util::is_function_like_kind(kind)
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
        // Unsupported containment boundary: tsc has no failure channel
        // here; an Err abandons this element's remaining checks (FN)
        // and the caller's loop continues. TSRS_TRACE_CONTAIN=1 prints
        // the swallowed reasons (debug aid).
        if let Err(err) = self.check_source_element_worker(node) {
            self.mark_partially_checked_node(node, err.reason.clone());
            if std::env::var_os("TSRS_TRACE_CONTAIN").is_some() {
                eprintln!("contained @{node:?}: {}", err.reason);
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
    fn check_source_element_unreachable(&mut self, node: NodeId) -> CheckResult2<bool> {
        if !tsrs2_binder::node_util::is_potentially_executable_node(
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
                    if !tsrs2_binder::node_util::is_potentially_executable_node(
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
                    if !tsrs2_binder::node_util::is_potentially_executable_node(
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
        let start = tsrs2_syntax::skip_trivia(
            &self.binder.source_of_node(start_node).text,
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
    fn is_source_element_unreachable(&mut self, node: NodeId) -> CheckResult2<bool> {
        if self.node_flags(node) & tsrs2_types::NodeFlags::UNREACHABLE.bits() != 0 {
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
    fn check_source_element_worker(&mut self, node: NodeId) -> CheckResult2<()> {
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
    fn check_jsdoc_type_alias_tag(&mut self, node: NodeId) -> CheckResult2<()> {
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
    fn check_jsdoc_template_tag(&mut self, node: NodeId) -> CheckResult2<()> {
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
    fn check_jsdoc_type_tag(&mut self, node: NodeId) -> CheckResult2<()> {
        let NodeData::JSDocTypeTag(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        self.check_source_element(data.type_expression);
        Ok(())
    }

    /// tsc-port: checkJSDocSatisfiesTag @6.0.3.
    /// tsc-hash: 06ba243cd86ac0b5ccf0af74f1537067992744abd80f186d85d8ae427648070a
    /// tsc-span: _tsc.js:82811-82823
    fn check_jsdoc_satisfies_tag(&mut self, node: NodeId) -> CheckResult2<()> {
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
    fn check_jsdoc_link_like_tag(&mut self, node: NodeId) -> CheckResult2<()> {
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
    ) -> CheckResult2<Option<SymbolId>> {
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
    fn check_jsdoc_property_like_tag(&mut self, node: NodeId) -> CheckResult2<()> {
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
    fn check_jsdoc_function_type(&mut self, node: NodeId) -> CheckResult2<()> {
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
    fn check_jsdoc_this_tag(&mut self, node: NodeId) -> CheckResult2<()> {
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
    fn check_jsdoc_import_tag(&mut self, node: NodeId) -> CheckResult2<()> {
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
    fn check_jsdoc_implements_tag(&mut self, node: NodeId) -> CheckResult2<()> {
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
    fn check_jsdoc_augments_tag(&mut self, node: NodeId) -> CheckResult2<()> {
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
    fn check_jsdoc_accessibility_modifier(&mut self, node: NodeId) -> CheckResult2<()> {
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
    fn check_jsdoc_variadic_type(&mut self, node: NodeId) -> CheckResult2<()> {
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
    fn check_jsdoc_type_is_in_js_file(&mut self, node: NodeId) -> CheckResult2<()> {
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
    pub(crate) fn check_unmatched_jsdoc_parameters(&mut self, node: NodeId) -> CheckResult2<()> {
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
    ) -> CheckResult2<bool> {
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
    pub(crate) fn check_block(&mut self, node: NodeId) -> CheckResult2<()> {
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
    fn check_expression_statement(&mut self, node: NodeId) -> CheckResult2<()> {
        self.check_grammar_statement_in_ambient_context(node);
        let NodeData::ExpressionStatement(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let Some(expression) = data.expression else {
            return Ok(());
        };
        self.check_expression(expression, tsrs2_types::CheckMode::NORMAL)?;
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
    fn check_type_parameter(&mut self, node: NodeId) -> CheckResult2<()> {
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
    pub(crate) fn check_type_parameters(&mut self, declarations: &[NodeId]) -> CheckResult2<()> {
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
                    let text = tsrs2_binder::node_util::declaration_name_to_string(
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
    ) -> CheckResult2<()> {
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
    fn check_interface_declaration(&mut self, node: NodeId) -> CheckResult2<()> {
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
                tsrs2_binder::node_util::is_entity_name_expression(source, expression)
                    && !tsrs2_binder::node_util::is_optional_chain(source, expression)
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
    fn check_type_alias_declaration(&mut self, node: NodeId) -> CheckResult2<()> {
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
    pub(crate) fn check_type_reference_node(&mut self, node: NodeId) -> CheckResult2<()> {
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
                    let start = tsrs2_syntax::skip_trivia(&source.text, type_name_end);
                    (source.text.as_bytes().get(start) == Some(&b'.')).then(|| {
                        source
                            .line_map
                            .byte_to_utf16
                            .get(start)
                            .copied()
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
    ) -> CheckResult2<()> {
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
            let name = tsrs2_binder::unescape_leading_underscores(
                &self.binder.symbol(symbol).escaped_name,
            )
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

    fn is_self_referential_type_alias_reference(&mut self, node: NodeId) -> CheckResult2<bool> {
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
    ) -> CheckResult2<Option<Vec<TypeId>>> {
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
            .intersects(tsrs2_types::SymbolFlags::TYPE_ALIAS)
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
    ) -> CheckResult2<bool> {
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
    fn check_type_query(&mut self, node: NodeId) -> CheckResult2<()> {
        self.get_type_from_type_query_node(node)?;
        Ok(())
    }

    /// tsc-port: checkTypeLiteral @6.0.3
    /// tsc-hash: af0e82a9973f07ca63af60ceec2148cc5efff3b06708128338038bda9f5c6cf2
    /// tsc-span: _tsc.js:81841-81850
    ///
    /// addLazyDiagnostic = eager identity: the lazy block's forcing +
    /// index-constraint + duplicate checks run inline (class.rs seed).
    fn check_type_literal(&mut self, node: NodeId) -> CheckResult2<()> {
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
    fn check_array_type(&mut self, node: NodeId) -> CheckResult2<()> {
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
    fn check_tuple_type(&mut self, node: NodeId) -> CheckResult2<()> {
        let NodeData::TupleType(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let elements = self.nodes_of(data.elements);
        let mut seen_optional_element = false;
        let mut seen_rest_element = false;
        for &element in &elements {
            let mut flags = self.get_tuple_element_flags(element);
            if flags.intersects(tsrs2_types::ElementFlags::VARIADIC) {
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
                                .intersects(tsrs2_types::ElementFlags::REST)
                    {
                        flags |= tsrs2_types::ElementFlags::REST;
                    }
                }
            }
            if flags.intersects(tsrs2_types::ElementFlags::REST) {
                if seen_rest_element {
                    self.grammar_error_on_node(
                        element,
                        &diagnostics::A_rest_element_cannot_follow_another_rest_element,
                        &[],
                    );
                    break;
                }
                seen_rest_element = true;
            } else if flags.intersects(tsrs2_types::ElementFlags::OPTIONAL) {
                if seen_rest_element {
                    self.grammar_error_on_node(
                        element,
                        &diagnostics::An_optional_element_cannot_follow_a_rest_element,
                        &[],
                    );
                    break;
                }
                seen_optional_element = true;
            } else if flags.intersects(tsrs2_types::ElementFlags::REQUIRED) && seen_optional_element
            {
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
    fn tuple_combined_flags(&self, ty: TypeId) -> tsrs2_types::ElementFlags {
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
                .fold(tsrs2_types::ElementFlags::from_bits(0), |acc, &flags| {
                    acc | flags
                }),
            _ => tsrs2_types::ElementFlags::from_bits(0),
        }
    }

    /// tsc-port: checkUnionOrIntersectionType @6.0.3
    /// tsc-hash: fb99110bb4ec225868bfc2a8215247de48be9c3b4c2e50d4283b5bafc74da82b
    /// tsc-span: _tsc.js:81889-81892
    fn check_union_or_intersection_type(&mut self, node: NodeId) -> CheckResult2<()> {
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
    fn check_indexed_access_type(&mut self, node: NodeId) -> CheckResult2<()> {
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
    fn check_mapped_type(&mut self, node: NodeId) -> CheckResult2<()> {
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
    fn check_this_type(&mut self, node: NodeId) -> CheckResult2<()> {
        self.get_type_from_this_type_node(node)?;
        Ok(())
    }

    /// tsc-port: checkTypeOperator @6.0.3
    /// tsc-hash: 887ed97e8defb9d4edfae94a11eec1b2fd95836cc3f6a620fc0ed3ff07edc620
    /// tsc-span: _tsc.js:81950-81953
    fn check_type_operator(&mut self, node: NodeId) -> CheckResult2<()> {
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
    fn check_conditional_type(&mut self, node: NodeId) -> CheckResult2<()> {
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
    fn check_infer_type(&mut self, node: NodeId) -> CheckResult2<()> {
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
    fn check_template_literal_type(&mut self, node: NodeId) -> CheckResult2<()> {
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
    /// The `assert`-deprecation row is LIVE (ignoreDeprecations is
    /// absent, §13); the with/assert discriminator is read from
    /// ImportAttributes.token — the parser threads the consumed
    /// keyword into the node data (codegen seed). The
    /// getResolutionModeOverride grammar validation is a named escape
    /// (5.8d §9 — resolution-mode plumbing).
    fn check_import_type(&mut self, node: NodeId) -> CheckResult2<()> {
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
            if token != SyntaxKind::WithKeyword {
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
    fn check_named_tuple_member(&mut self, node: NodeId) -> CheckResult2<()> {
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
                        self.node_flags(list) & tsrs2_types::NodeFlags::CONST.bits() != 0
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
                    let is_static = tsrs2_binder::node_util::has_syntactic_modifier(
                        source,
                        parent,
                        tsrs2_types::ModifierFlags::STATIC,
                    );
                    let is_readonly =
                        tsrs2_binder::node_util::get_effective_modifier_flags(source, parent)
                            .intersects(tsrs2_types::ModifierFlags::READONLY);
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
                    let is_readonly = tsrs2_binder::node_util::has_syntactic_modifier(
                        source,
                        parent,
                        tsrs2_types::ModifierFlags::READONLY,
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
            // unverified — record it so the comment-directive
            // exemption (2578) does not report a directive whose
            // suppression target was never checked (S8).
            self.mark_partially_checked_node(node, err.reason.clone());
            if std::env::var_os("TSRS_TRACE_CONTAIN").is_some() {
                eprintln!("contained deferred @{node:?}: {}", err.reason);
            }
        }
        #[cfg(debug_assertions)]
        self.assert_unwound(&unwind_entry, node, "check_deferred_node");
        self.current_node = save_current_node;
    }

    fn check_deferred_node_worker(&mut self, node: NodeId) -> CheckResult2<()> {
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
                self.check_expression(expression, tsrs2_types::CheckMode::NORMAL)?;
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
    fn check_type_parameter_deferred(&mut self, node: NodeId) -> CheckResult2<()> {
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
    ) -> CheckResult2<TypeId> {
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
    ) -> CheckResult2<(TypeId, TypeId)> {
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
    /// The verdict probe remains separate, but a failed diagnostic
    /// call replays checkTypeRelatedTo in reporting mode. That replay
    /// performs per-level read/write normalization and returns tsc's
    /// errorInfo, relatedInfo, incompatibleStack collapse, literal
    /// generalization, and TypeParameter-target elaboration under the
    /// supplied head. Caller-specific head overrides below still run
    /// before the common reporting walk.
    pub(crate) fn check_type_assignable_to(
        &mut self,
        source: TypeId,
        target: TypeId,
        error_node: Option<NodeId>,
        head_message: &'static DiagnosticMessage,
    ) -> CheckResult2<bool> {
        let original_source = source;
        let original_target = target;
        let related = self.is_type_assignable_to(source, target)?;
        if !related {
            if let Some(error_node) = error_node {
                // reportErrorResults (65248-65253) receives the
                // getNormalizedType pair produced by isRelatedTo.
                // The helper also applies the 65185 nullable-candidate
                // substitution after normalization. This is
                // report-only: isTypeAssignableTo above still owns the
                // verdict.
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
                let generic_head = std::ptr::eq(
                    head_message,
                    &diagnostics::Type_0_is_not_assignable_to_type_1,
                );
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
                        tsrs2_types::TypeData::Intersection { types } => types.to_vec(),
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
                        if intrinsic
                            && self.report_unmatched_property_head(
                                source,
                                constituent,
                                error_node,
                            )?
                        {
                            return Ok(related);
                        }
                    }
                }
                if self.report_excess_property_head(
                    source,
                    target,
                    error_node,
                    crate::relate::RelationKind::Assignable,
                )? {
                    return Ok(related);
                }
                // isRelatedTo's common-property arm (65208-65235)
                // precedes ALL structural elaboration and its early
                // return skips the head for ANY head message
                // (subtypingWithObjectMembers5 pins 2420→2559).
                if self.report_no_common_properties_head(source, target, error_node)? {
                    return Ok(related);
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
                if generic_head
                    && !global_object_source
                    && self.report_unmatched_property_head(source, target, error_node)?
                {
                    return Ok(related);
                }
                // Reporting is a refinement of the already-failed
                // verdict. A still-unimplemented descendant may
                // suppress only the nested chain, never this known
                // parent diagnostic.
                if let Ok(Some(output)) = self.relation_error_output_with_context(
                    original_source,
                    original_target,
                    crate::relate::RelationKind::Assignable,
                    if generic_head {
                        None
                    } else {
                        Some(head_message)
                    },
                    None,
                ) {
                    let mut diagnostic = self.create_error(Some(error_node), head_message, &[]);
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
                self.error_at(
                    Some(error_node),
                    head_message,
                    &[&source_text, &target_text],
                );
            }
        }
        Ok(related)
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
    ) -> CheckResult2<bool> {
        if !self.is_object_literal_type(source)
            || !self
                .tables
                .object_flags_of(source)
                .intersects(ObjectFlags::FRESH_LITERAL)
        {
            return Ok(false);
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
            expanding_flags: tsrs2_types::ExpandingFlags::NONE,
            overflow: false,
            relation_count,
            error_state: Default::default(),
        };
        Ok(matches!(
            checker.excess_properties_worker(
                source,
                target,
                /*report_errors*/ true,
                Some(error_node),
            )?,
            crate::engine::ExcessPropertyOutcome::UnknownProperty
        ))
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
    ) -> CheckResult2<bool> {
        if !self
            .tables
            .flags_of(source)
            .intersects(TypeFlags::from_bits(
                TypeFlags::PRIMITIVE.bits()
                    | TypeFlags::OBJECT.bits()
                    | TypeFlags::INTERSECTION.bits(),
            ))
        {
            return Ok(false);
        }
        if source == self.global_object_type()? {
            return Ok(false);
        }
        // typeRelatedToSomeType reports on the BEST-MATCHING union
        // member, and the common-property arm fires inside that member
        // recursion — for a nullable union (`ImportCallOptions |
        // undefined`, the import-call options check) the object member
        // is the best match. Other union shapes keep the generic head.
        let target = if self.tables.flags_of(target).intersects(TypeFlags::UNION) {
            let members = match &self.tables.type_of(target).data {
                tsrs2_types::TypeData::Union { types, .. } => types.to_vec(),
                _ => Vec::new(),
            };
            let non_nullable: Vec<TypeId> = members
                .into_iter()
                .filter(|&member| !self.tables.flags_of(member).intersects(TypeFlags::NULLABLE))
                .collect();
            match non_nullable.as_slice() {
                [only] => *only,
                _ => return Ok(false),
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
            return Ok(false);
        }
        if !self.is_weak_type(target)? {
            return Ok(false);
        }
        let has_surface = !self.get_properties_of_type(source)?.is_empty()
            || self.type_has_call_or_construct_signatures(source)?;
        if !has_surface {
            return Ok(false);
        }
        if self.has_common_properties(source, target)? {
            return Ok(false);
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
        self.error_at(Some(error_node), message, &[&source_text, &target_text]);
        Ok(true)
    }

    /// The pre-head missing-property approximation uses only
    /// tryElaborateArrayLikeErrors' reportErrors=false verdict. The
    /// reporting face lives at its exact recursive relation site.
    fn try_elaborate_array_like_errors_without_reporting(
        &mut self,
        source: TypeId,
        target: TypeId,
    ) -> CheckResult2<bool> {
        if self.tables.is_tuple_type(source) {
            let tuple_readonly = {
                let tuple_target = self.tables.reference_target(source);
                match &self.tables.type_of(tuple_target).data {
                    tsrs2_types::TypeData::TupleTarget(data) => data.readonly,
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
    ) -> CheckResult2<bool> {
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
            return Ok(false);
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
            return Ok(false);
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
                    return Ok(false);
                }
            }
        }
        let mut unmatched: Vec<SymbolId> = Vec::new();
        for target_prop in self.get_properties_of_type(target)? {
            let flags = self.binder.symbol(target_prop).flags;
            if flags.intersects(tsrs2_types::SymbolFlags::OPTIONAL)
                || self
                    .get_check_flags(target_prop)
                    .intersects(tsrs2_types::CheckFlags::PARTIAL)
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
                        tsrs2_binder::node_util::has_syntactic_modifier(
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
            return Ok(false);
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
                    tsrs2_binder::node_util::get_name_of_declaration(source_file, declaration)?;
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
                    .intersects(tsrs2_types::SymbolFlags::CLASS)
            });
            if let Some(class_symbol) = source_class_symbol {
                let suffix = format!("@{description}");
                let has_own_twin = self
                    .get_members_of_symbol(class_symbol)?
                    .keys()
                    .any(|name| name.starts_with("__#") && name.ends_with(&suffix));
                if has_own_twin {
                    return Ok(false);
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
            return Ok(false);
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
            self.error_at_with_related(
                Some(error_node),
                &diagnostics::Property_0_is_missing_in_type_1_but_required_in_type_2,
                &[&prop_name, &source_text, &target_text],
                related,
            );
            return Ok(true);
        }
        // 66752-66757: the multi-property lists ride plain
        // symbolToString (no WriteComputedProps) — late-bound computed
        // names print their declaration SOURCE text verbatim.
        let mut names: Vec<String> = Vec::with_capacity(unmatched.len());
        for &prop in &unmatched {
            names.push(self.missing_property_display_name(prop, false)?);
        }
        if unmatched.len() > 5 {
            let head: Vec<String> = names[..4].to_vec();
            let more = (unmatched.len() - 4).to_string();
            self.error_at(
                Some(error_node),
                &diagnostics::Type_0_is_missing_the_following_properties_from_type_1_2_and_3_more,
                &[&source_text, &target_text, &head.join(", "), &more],
            );
        } else {
            self.error_at(
                Some(error_node),
                &diagnostics::Type_0_is_missing_the_following_properties_from_type_1_2,
                &[&source_text, &target_text, &names.join(", ")],
            );
        }
        Ok(true)
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
    ) -> CheckResult2<String> {
        let escaped = self.binder.symbol(prop).escaped_name.clone();
        if escaped.starts_with('#') {
            return Ok(escaped);
        }
        if escaped.starts_with("__#") {
            if let Some(declaration) = self.binder.symbol(prop).value_declaration {
                let source = self.binder.source_of_node(declaration);
                if let Some(name) =
                    tsrs2_binder::node_util::get_name_of_declaration(source, declaration)
                {
                    return Ok(tsrs2_binder::node_util::declaration_name_to_string(
                        source,
                        Some(name),
                    ));
                }
            }
        }
        let computed_name = self.binder.symbol(prop).value_declaration.and_then(|decl| {
            tsrs2_binder::node_util::get_name_of_declaration(self.binder.source_of_node(decl), decl)
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
                .intersects(tsrs2_types::CheckFlags::LATE)
            {
                let source = self.binder.source_of_node(name);
                return Ok(tsrs2_binder::node_util::declaration_name_to_string(
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
    /// element-access names) stay behind the curtain.
    fn computed_property_name_face_slice(&mut self, name: NodeId) -> CheckResult2<String> {
        let curtain =
            || Unsupported::new("typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)");
        let NodeData::ComputedPropertyName(data) = self.data_of(name) else {
            return Err(curtain());
        };
        let expression = data.expression.ok_or_else(curtain)?;
        let text = match self.data_of(expression).clone() {
            NodeData::StringLiteral(data) => string_literal_name_slice(&data.text, false)?,
            NodeData::NumericLiteral(data) => data.text.clone(),
            NodeData::PrefixUnaryExpression(data)
                if data.operator == SyntaxKind::MinusToken
                    && data.operand.is_some_and(|operand| {
                        matches!(self.data_of(operand), NodeData::NumericLiteral(_))
                    }) =>
            {
                let NodeData::NumericLiteral(operand) =
                    self.data_of(data.operand.expect("guarded above")).clone()
                else {
                    unreachable!("guarded above");
                };
                format!("-{}", operand.text)
            }
            _ => self.entity_name_text_slice(expression)?,
        };
        Ok(format!("[{text}]"))
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
    ) -> CheckResult2<bool> {
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
                if self.report_excess_property_head(
                    source,
                    target,
                    error_node,
                    crate::relate::RelationKind::Comparable,
                )? {
                    return Ok(related);
                }
                let generic_head = std::ptr::eq(
                    head_message,
                    &diagnostics::Type_0_is_not_comparable_to_type_1,
                );
                if let Ok(Some(output)) = self.relation_error_output_with_context(
                    original_source,
                    original_target,
                    crate::relate::RelationKind::Comparable,
                    if generic_head {
                        None
                    } else {
                        Some(head_message)
                    },
                    None,
                ) {
                    let mut diagnostic = self.create_error(Some(error_node), head_message, &[]);
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
    ) -> CheckResult2<bool> {
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
    ) -> CheckResult2<bool> {
        let default = self.get_resolved_type_parameter_default(type_parameter)?;
        Ok(default != self.circular_constraint_type)
    }

    /// getSymbolOfDeclaration (49936) — the binder's node.symbol
    /// through getLateBoundSymbol (57770) and the getMergedSymbol
    /// chase (JS aliasing arms with the JS residual).
    /// tsc-port: getSymbolOfDeclaration @6.0.3
    /// tsc-hash: 197061af99891199274ec82eb08309cbb138441e9fcba571ac5aa6149bf1b3a0
    /// tsc-span: _tsc.js:49936-49938
    pub(crate) fn get_symbol_of_declaration(&mut self, node: NodeId) -> CheckResult2<SymbolId> {
        let symbol = self.node_symbol(node).ok_or_else(|| {
            Unsupported::new("declaration without a bound symbol (parse-recovery tree)")
        })?;
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
    pub(crate) fn get_late_bound_symbol(&mut self, symbol: SymbolId) -> CheckResult2<SymbolId> {
        let data = self.binder.symbol(symbol);
        if !data
            .flags
            .intersects(tsrs2_types::SymbolFlags::CLASS_MEMBER)
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
                    tsrs2_binder::node_util::has_syntactic_modifier(
                        self.binder.source_of_node(declaration),
                        declaration,
                        tsrs2_types::ModifierFlags::STATIC,
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

    // ---- typeToString (the 5.4 display slice) ----

    /// The typeToString arms 5.4's two report sites can prove exact:
    /// intrinsics (intrinsicName), string/number literal quoting,
    /// type parameters incl. the ForCheck marker rule (51535 —
    /// `super-`/`sub-` + varianceTypeParameter's name, `?` without
    /// one), alias-stamped instantiations (`Name<args>`), generic
    /// class/interface references (`Name<args>`, with the nodeBuilder
    /// array sugar `T[]`/`readonly T[]`), and unions/intersections in
    /// interned order. Everything else — qualification, tuples,
    /// anonymous shapes, enum members — is nodeBuilder work (T2/M8)
    /// and unwinds Unsupported so the caller drops the diagnostic
    /// instead of mis-printing it.
    /// tsc-port: typeToString @6.0.3
    /// tsc-hash: 4b587962e2fb137a31ea52c35aeba733ffb4c6d97a8c54c98d5c1f1666e73dda
    /// tsc-span: _tsc.js:50717-50747
    pub(crate) fn type_to_string_slice(&mut self, ty: TypeId) -> CheckResult2<String> {
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
    ) -> CheckResult2<String> {
        self.type_to_string_slice_root(
            ty, /*fully_qualified*/ false, /*no_type_reduction*/ true,
        )
    }

    /// tsc-port: getTypeNameForErrorDisplay @6.0.3
    /// tsc-hash: 9e9827829d64df1cb9ed00762b4a5c872a23139bdd217fffd5c274437e7ac389
    /// tsc-span: _tsc.js:50757-50764
    ///
    /// typeToString under UseFullyQualifiedType — the bounded slice:
    /// every symbol head qualifies through getFullyQualifiedName
    /// (import-specifier sugar is a T2 nuance under the display
    /// curtain); shapes outside the slice keep escalating to the
    /// structured tail's tagged escapes.
    pub(crate) fn get_type_name_for_error_display(&mut self, ty: TypeId) -> CheckResult2<String> {
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
    ) -> CheckResult2<String> {
        let saved_visited = std::mem::take(&mut self.slice_visited_types);
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
        let saved_no_type_reduction =
            std::mem::replace(&mut self.slice_no_type_reduction, no_type_reduction);
        let result = self.type_to_string_slice_ex(ty, fully_qualified);
        self.slice_visited_types = saved_visited;
        self.slice_approximate_length = saved_approximate_length;
        self.slice_max_truncation_length = saved_max_truncation_length;
        self.slice_truncating = saved_truncating;
        self.slice_no_type_reduction = saved_no_type_reduction;
        result
    }

    fn type_to_string_slice_ex(
        &mut self,
        ty: TypeId,
        fully_qualified: bool,
    ) -> CheckResult2<String> {
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
    ) -> CheckResult2<Vec<(String, SliceTypeNodeKind)>> {
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
                if tsrs2_syntax::is_identifier_text(head) {
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

    /// tsc getNameOfSymbolAsWritten's anonymous class/function face.
    /// A class/function expression assigned directly to a variable
    /// borrows the variable declaration's written name; otherwise an
    /// unnamed expression uses the anonymous sentinel. Named symbols
    /// keep the slice's existing qualified/unqualified spelling, so
    /// synthetic `__class`/`__function` names are never user-facing.
    fn type_reference_symbol_name_slice(
        &mut self,
        symbol: SymbolId,
        fully_qualified: bool,
    ) -> CheckResult2<String> {
        let declarations = self.binder.symbol(symbol).declarations.clone();
        // getNameOfSymbolAsWritten's default-export exception: at the
        // initial, unqualified entity-name position, a declaration-backed
        // `default` symbol uses its written declaration name while the
        // display context remains in the same default-binding scope.
        // Anonymous default declarations have no name and retain `default`.
        if !fully_qualified
            && self.binder.symbol(symbol).escaped_name == tsrs2_types::InternalSymbolName::DEFAULT
            && self.slice_display_enclosing.is_none_or(|enclosing| {
                declarations
                    .first()
                    .copied()
                    .and_then(|declaration| self.default_binding_context_slice(declaration))
                    == self.default_binding_context_slice(enclosing)
            })
        {
            for &declaration in &declarations {
                let source = self.binder.source_of_node(declaration);
                if let Some(name) = node_util::get_name_of_declaration(source, declaration) {
                    let name = node_util::declaration_name_to_string(source, Some(name));
                    self.slice_add_bare_symbol_length(&name);
                    return Ok(name);
                }
            }
        }
        if let Some(&declaration) = declarations.first() {
            match self.kind_of(declaration) {
                SyntaxKind::ClassExpression
                | SyntaxKind::FunctionExpression
                | SyntaxKind::ArrowFunction => {
                    let source = self.binder.source_of_node(declaration);
                    if let Some(name) = node_util::get_name_of_declaration(source, declaration) {
                        let name = node_util::declaration_name_to_string(source, Some(name));
                        self.slice_add_bare_symbol_length(&name);
                        return Ok(name);
                    }
                    let name = if self.kind_of(declaration) == SyntaxKind::ClassExpression {
                        "(Anonymous class)".to_owned()
                    } else {
                        "(Anonymous function)".to_owned()
                    };
                    self.slice_add_bare_symbol_length(&name);
                    return Ok(name);
                }
                _ => {}
            }
        }
        Ok(self.symbol_type_face_slice(symbol, fully_qualified)?.0)
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
    fn should_elide_iterable_default_arguments_slice(
        &mut self,
        ty: TypeId,
        type_parameter_count: usize,
    ) -> CheckResult2<bool> {
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
        if !self.is_reference_to_type(ty, protocol_target) {
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
    ) -> CheckResult2<(String, SliceTypeNodeKind)> {
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
            return Ok((
                match self.tables.type_of(ty).symbol {
                    Some(symbol) => self.symbol_type_face_slice(symbol, fully_qualified)?.0,
                    None => "?".to_owned(),
                },
                SliceTypeNodeKind::Reference,
            ));
        }
        // Named object types (interface/class/enum declared shapes)
        // print their symbol name — the nodeBuilder's symbol reference
        // without qualification (lib types like Date flow into 2344
        // args; anonymous __type shapes stay out of slice). The
        // VALUE side of the same symbols (class statics `typeof C`,
        // enum objects `typeof E` — createAnonymousTypeNode's
        // class/enum specials, 51771-51781) renders symbolToTypeNode
        // under the Value meaning: the `typeof` query face
        // (isClassInstanceSide keys the split — the declared type IS
        // the instance side).
        if flags.intersects(TypeFlags::OBJECT | TypeFlags::ENUM) {
            if let Some(symbol) = self.tables.type_of(ty).symbol {
                let symbol_flags = self.binder.symbol(symbol).flags;
                if symbol_flags.intersects(
                    tsrs2_types::SymbolFlags::CLASS
                        | tsrs2_types::SymbolFlags::INTERFACE
                        | tsrs2_types::SymbolFlags::REGULAR_ENUM
                        | tsrs2_types::SymbolFlags::CONST_ENUM,
                ) && !self
                    .tables
                    .object_flags_of(ty)
                    .intersects(ObjectFlags::REFERENCE)
                {
                    let name = self.type_reference_symbol_name_slice(symbol, fully_qualified)?;
                    if self.get_declared_type_of_symbol_slice(symbol)? != ty
                        && symbol_flags.intersects(
                            tsrs2_types::SymbolFlags::CLASS
                                | tsrs2_types::SymbolFlags::REGULAR_ENUM
                                | tsrs2_types::SymbolFlags::CONST_ENUM
                                | tsrs2_types::SymbolFlags::VALUE_MODULE,
                        )
                    {
                        // The VALUE_MODULE disjunct: a merged
                        // interface+namespace VALUE side is an
                        // anonymous object whose symbol carries
                        // INTERFACE|VALUE_MODULE — tsc routes it
                        // through createAnonymousTypeNode's 51779
                        // ValueModule arm to the `typeof X` face
                        // (oracle-probed), not the interface's plain
                        // reference name.
                        return Ok((format!("typeof {name}"), SliceTypeNodeKind::TypeQuery));
                    }
                    return Ok((name, SliceTypeNodeKind::Reference));
                }
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
            let Some(symbol) = self.tables.type_of(ty).symbol else {
                return Err(Unsupported::new(
                    "typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)",
                ));
            };
            if self
                .binder
                .symbol(symbol)
                .flags
                .intersects(tsrs2_types::SymbolFlags::ENUM_MEMBER)
            {
                let Some(parent) = self.get_parent_of_symbol(symbol) else {
                    return Err(Unsupported::new(
                        "typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)",
                    ));
                };
                let parent_name = self.symbol_type_face_slice(parent, fully_qualified)?.0;
                if self.get_declared_type_of_symbol_slice(parent)? == ty {
                    return Ok((parent_name, SliceTypeNodeKind::Reference));
                }
                let member_name = self.symbol_display_name(symbol);
                if tsrs2_syntax::is_identifier_text(&member_name) {
                    return Ok((
                        format!("{parent_name}.{member_name}"),
                        SliceTypeNodeKind::Reference,
                    ));
                }
                return Err(Unsupported::new(
                    "typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)",
                ));
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
                tsrs2_types::LiteralValue::String(text) => {
                    // 51401-51403: the StringLiteral face carries
                    // EmitFlags.NoAsciiEscaping, so getLiteralText
                    // runs escapeString(text, '"') WITHOUT the
                    // non-ASCII pass — `"あ"` prints raw while
                    // `"AB\r\nC"` spells its escapes (oracle-pinned).
                    // (The string-literal domain's unpaired-surrogate
                    // gap is the recorded 9.3b4-r1 D1a census
                    // candidate; LiteralValue::String cannot carry
                    // one.)
                    self.slice_add_approximate_length(Self::slice_js_length(&text) + 2);
                    Ok((
                        format!("\"{}\"", string_literal_type_display_text(&text)),
                        SliceTypeNodeKind::Literal,
                    ))
                }
                tsrs2_types::LiteralValue::Number(value) => {
                    let text = tsrs2_types::js_number_to_string(value);
                    self.slice_add_approximate_length(Self::slice_js_length(&text));
                    Ok((text, SliceTypeNodeKind::Literal))
                }
                _ => Err(Unsupported::new(
                    "literal display beyond plain strings/numbers (nodeBuilder, T2/M8)",
                )),
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

    /// tsrs-native: the origin verdict shield's transitive
    /// INSTANTIABLE probe — a type variable anywhere inside the
    /// origin's composite shape (nested unions/intersections and
    /// their own origins included) puts the relation in the
    /// cross-product band the port cannot verdict yet. Iterative
    /// (50k-chain corpus fixtures); only composite member lists are
    /// walked, so object members cannot cycle through here, and the
    /// seen-set bounds re-visits.
    fn composite_shape_contains_instantiable(&self, types: &[TypeId]) -> bool {
        let mut stack: Vec<TypeId> = types.to_vec();
        let mut seen: std::collections::HashSet<TypeId> = std::collections::HashSet::new();
        while let Some(ty) = stack.pop() {
            if !seen.insert(ty) {
                continue;
            }
            let flags = self.tables.flags_of(ty);
            if flags.intersects(TypeFlags::INSTANTIABLE) {
                return true;
            }
            if flags.intersects(TypeFlags::UNION | TypeFlags::INTERSECTION) {
                match &self.tables.type_of(ty).data {
                    TypeData::Union { types, origin } => {
                        stack.extend(types.iter().copied());
                        if let Some(origin) = origin {
                            stack.push(*origin);
                        }
                    }
                    TypeData::Intersection { types } => stack.extend(types.iter().copied()),
                    _ => {}
                }
            }
        }
        false
    }

    fn type_to_string_slice_structured(
        &mut self,
        ty: TypeId,
        fully_qualified: bool,
    ) -> CheckResult2<(String, SliceTypeNodeKind)> {
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
                        && self.get_global_type_symbol("Array", /*report_errors*/ false)
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
                    // NARROWED verdict shield: origins whose shape
                    // contains INSTANTIABLE constituents are the
                    // cross-product relation band where the port's
                    // verdict is NOT yet faithful — `T & U ⊆ (A | B) &
                    // T & U` holds in tsc through a normalized-
                    // intersection path the port lacks (each
                    // constituent relates individually here, and `T &
                    // U ⊆ 2` passes standalone but fails inside the
                    // intersection-target walk; FP-gate catch #8).
                    // The probe is TRANSITIVE through nested
                    // composites and their origins: a type variable
                    // wrapped in a named union member (`type N<T, U> =
                    // (T & U) | 4` inside `N<T, U> & (A | B)`) rides
                    // the same unfaithful verdict band as a direct
                    // member — the direct-member probe let it through
                    // and fabricated a 2322 tsc does not report
                    // (9.3b5 review r1). Rendering those origins would
                    // report the wrong verdicts, so the shield stays
                    // EXACTLY for them until the relation producer
                    // lands (9.9x/M8 owner); concrete-typed origins
                    // (the interface cross products) render.
                    if self.composite_shape_contains_instantiable(&types) {
                        return Err(Unsupported::new(
                            "origin display over instantiable members (cross-product relation verdict dependency, M8)",
                        ));
                    }
                } else if origin_flags.intersects(TypeFlags::INDEX) {
                    return self.index_type_to_string_slice_node(origin, fully_qualified);
                } else {
                    // No other origin kind is minted today (union
                    // denormalizations and keyof distributions); keep
                    // the curtain rather than a fresh panic claim.
                    return Err(Unsupported::new(
                        "origin display beyond union/intersection/keyof origins (nodeBuilder tail, M8)",
                    ));
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
            let Some(symbol) = self.tables.type_of(target).symbol else {
                // Non-tuple symbol-less reference targets are not
                // minted today (reference targets are GenericType or
                // TupleTarget — see the arity match below); the shape
                // stays behind the structured tail's curtain rather
                // than a fresh panic claim.
                return Err(Unsupported::new(
                    "typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)",
                ));
            };
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
                    if self.get_global_type_symbol("Array", /*report_errors*/ false)
                        == Some(symbol) =>
                {
                    Some(false)
                }
                Some(true)
                    if self
                        .get_global_type_symbol("ReadonlyArray", /*report_errors*/ false)
                        == Some(symbol) =>
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
                let Some(parent) = self
                    .parent_symbol_of_type_parameter_slice(type_parameters[argument_start])
                    .filter(|_| arguments.len() >= outer_type_parameter_count)
                else {
                    return Err(Unsupported::new(
                        "reference display with outer type parameters (nodeBuilder, T2 M8)",
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
            let Some(symbol) = self.tables.type_of(ty).symbol else {
                return Err(Unsupported::new(
                    "typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)",
                ));
            };
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
            let (extends_text, extends_kind) =
                self.type_to_string_slice_node(data.extends_type, fully_qualified)?;
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
                if let Some(symbol) = self.get_global_type_symbol("NoInfer", false) {
                    let name = self.symbol_type_face_slice(symbol, fully_qualified)?.0;
                    return Ok((format!("{name}<{argument}>"), SliceTypeNodeKind::Reference));
                }
            }
            return self.type_to_string_slice_node(data.base_type, fully_qualified);
        }
        Err(Unsupported::new(
            "typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)",
        ))
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
    ) -> CheckResult2<(String, SliceTypeNodeKind)> {
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
            modifiers.intersects(tsrs2_types::MappedTypeModifiers::INCLUDE_OPTIONAL),
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
    ) -> CheckResult2<(String, SliceTypeNodeKind)> {
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
    /// or `typeof X` face instead and stays behind the curtain for
    /// later 9.3b rungs; actual JS constructors take their exact
    /// `isJSConstructor` symbol face here. The visitedTypes revisit
    /// faces (getTypeAliasForTypeLiteral / `...` elision) likewise.
    fn anonymous_object_type_to_string_slice(
        &mut self,
        ty: TypeId,
        fully_qualified: bool,
    ) -> CheckResult2<(String, SliceTypeNodeKind)> {
        // InstantiationExpressionType (51755-51770): the TypeQuery
        // syntactic-reuse leg needs an enclosing-armed context (the
        // 9.3b probes established the reuse channel is inert for
        // error display) and the visitedTypes placeholder is the
        // recursion guard below — the error path renders these
        // STRUCTURALLY through the ordinary symbol routing
        // (oracle: 2635 prints `{ (): number; g<U>(): U; }`).
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
            // 51771-51786 symbol routing: the Class arm (51773) and
            // the Enum half of the 51779 disjunct are intercepted by
            // the named-object arm upstream (class statics and enum
            // objects — merged class+ns/enum+ns value sides included,
            // since the CLASS/ENUM symbol flag routes them there
            // first), so those flags cannot arrive at this gate; the
            // curtain stays as the constructibility guard rather than
            // a fresh unreachable claim. Function/method symbols fall
            // THROUGH to the structural tail on the error path:
            // shouldWriteTypeOfFunctionSymbol (51789-51795) requires
            // UseTypeOfFunction or a revisit, and typeToString sets
            // neither (oracle-probed: top-level, local, namespace-
            // parented declarations and expressions all render
            // structurally on first visit; the revisit face stays
            // behind the slice_visited_types curtain below). The
            // isJSConstructor head is handled immediately above.
            if symbol_flags.intersects(
                tsrs2_types::SymbolFlags::CLASS
                    | tsrs2_types::SymbolFlags::REGULAR_ENUM
                    | tsrs2_types::SymbolFlags::CONST_ENUM,
            ) {
                return Err(Unsupported::new(
                    "typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)",
                ));
            }
            // The ValueModule half of the 51779 disjunct:
            // symbolToTypeNode under the Value meaning — namespace,
            // external-module and globalThis object faces (a
            // function+namespace merge carries VALUE_MODULE and takes
            // this arm before the FUNCTION admission below, matching
            // tsc's disjunct order). isClassInstanceSide (50771)
            // requires SymbolFlags::CLASS, which cannot reach here,
            // so the meaning is always Value.
            if symbol_flags.intersects(tsrs2_types::SymbolFlags::VALUE_MODULE) {
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
            return Err(Unsupported::new(
                "typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)",
            ));
        }
        self.slice_visited_types.insert(ty);
        let result = self.type_node_from_object_type_slice(ty, fully_qualified);
        self.slice_visited_types.remove(&ty);
        result
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
    ) -> CheckResult2<(String, SliceTypeNodeKind)> {
        self.symbol_to_type_face_slice(symbol, fully_qualified, tsrs2_types::SymbolFlags::TYPE)
    }

    fn symbol_value_face_slice(
        &mut self,
        symbol: SymbolId,
        fully_qualified: bool,
    ) -> CheckResult2<(String, SliceTypeNodeKind)> {
        self.symbol_to_type_face_slice(symbol, fully_qualified, tsrs2_types::SymbolFlags::VALUE)
    }

    fn symbol_to_type_face_slice(
        &mut self,
        symbol: SymbolId,
        fully_qualified: bool,
        meaning: tsrs2_types::SymbolFlags,
    ) -> CheckResult2<(String, SliceTypeNodeKind)> {
        let is_type_of = meaning == tsrs2_types::SymbolFlags::VALUE;
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
                .intersects(tsrs2_types::SymbolFlags::TYPE_PARAMETER)
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
                    let name = self.qualifier_symbol_name_slice(chain[index - 1], chain[index])?;
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
            // getNameOfSymbolAsWritten at the root (the slice's
            // symbol_display_name posture), then export-table naming
            // below it.
            let mut parts = Vec::with_capacity(chain.len());
            let root_name = self.symbol_display_name(root);
            self.slice_add_bare_symbol_length(&root_name);
            parts.push(root_name);
            for index in 1..chain.len() {
                let name = self.qualifier_symbol_name_slice(chain[index - 1], chain[index])?;
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
        let name = self.symbol_display_name(symbol);
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
    /// module-specifier string-literal roots (53351-53355) and the
    /// element-access faces over non-identifier links (53362-53385)
    /// stay behind the curtain. Link names ride
    /// getNameOfSymbolAsWritten — the slice's symbol_display_name
    /// posture.
    fn symbol_expression_face_slice(
        &mut self,
        symbol: SymbolId,
        enclosing: Option<NodeId>,
        fully_qualified: bool,
    ) -> CheckResult2<String> {
        let curtain =
            || Unsupported::new("typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)");
        let chain = if enclosing.is_some() || fully_qualified {
            // yield_module_symbol FALSE — symbolToExpression passes
            // nothing (53338), including tsc's FQ retry, which still
            // rides this same entry point.
            self.symbol_chain_slice(
                symbol,
                tsrs2_types::SymbolFlags::VALUE,
                true,
                false,
                enclosing,
            )?
            .expect("getSymbolChain with endOfChain always yields (52991-52999)")
        } else {
            vec![symbol]
        };
        if self.symbol_has_external_module_declaration(chain[0]) {
            return Err(curtain());
        }
        let mut parts = Vec::with_capacity(chain.len());
        for &link in &chain {
            let name = self.symbol_display_name(link);
            if !tsrs2_syntax::is_identifier_text(&name) {
                return Err(curtain());
            }
            parts.push(name);
        }
        Ok(parts.join("."))
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
    fn symbol_chain_slice(
        &mut self,
        symbol: SymbolId,
        meaning: tsrs2_types::SymbolFlags,
        end_of_chain: bool,
        yield_module_symbol: bool,
        enclosing: Option<NodeId>,
    ) -> CheckResult2<Option<Vec<SymbolId>>> {
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
                            Err(unsupported) => {
                                if parents.len() > 1 {
                                    return Err(unsupported);
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
                        .get(tsrs2_types::InternalSymbolName::EXPORT_EQUALS)
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
                tsrs2_types::SymbolFlags::TYPE_LITERAL | tsrs2_types::SymbolFlags::OBJECT_LITERAL,
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
    fn qualified_left_meaning(meaning: tsrs2_types::SymbolFlags) -> tsrs2_types::SymbolFlags {
        if meaning == tsrs2_types::SymbolFlags::VALUE {
            tsrs2_types::SymbolFlags::VALUE
        } else {
            tsrs2_types::SymbolFlags::NAMESPACE
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
        meaning: tsrs2_types::SymbolFlags,
        enclosing: Option<NodeId>,
    ) -> CheckResult2<Option<Vec<SymbolId>>> {
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
    /// members table (50260-50283) is omitted: every armed entry point
    /// here carries the Value meaning, the filtered table holds only
    /// Type-meaning member symbols, class members are never
    /// Alias-flagged, and exportSymbol links never occur on
    /// class/interface members (declareModuleMember-only) — no
    /// trySymbolTable leg can fire on it.
    fn symbol_tables_in_scope_slice(
        &mut self,
        enclosing: Option<NodeId>,
    ) -> Vec<(ScopeTableKey, tsrs2_binder::SymbolTable, bool)> {
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
        table: &tsrs2_binder::SymbolTable,
        table_key: ScopeTableKey,
        symbol: SymbolId,
        meaning: tsrs2_types::SymbolFlags,
        ignore_qualification: bool,
        is_local_name_lookup: bool,
        visited: &mut Vec<ScopeTableKey>,
        enclosing: Option<NodeId>,
    ) -> CheckResult2<Option<Vec<SymbolId>>> {
        if visited.contains(&table_key) {
            return Ok(None);
        }
        visited.push(table_key);
        let result = self.try_symbol_table_slice(
            table,
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
    /// seen, tsc's single forEachEntry pass. The globals-tail
    /// globalThis probe (50359) stays omitted: the member faces
    /// re-enclose at the property declaration (52265-52267), where a
    /// script-global's direct hit precedes the tail, and `unique
    /// symbol` requires `const` — a script-global const is not a
    /// `globalThis` property, so no computed-name face can require the
    /// `globalThis.s` spelling (probe D, driver.mjs 6.0.3 2026-07-24:
    /// a module-local `s` shadowing a script-global `s` still prints
    /// '[s]', related 2728 at the script declaration).
    #[allow(clippy::too_many_arguments)]
    fn try_symbol_table_slice(
        &mut self,
        table: &tsrs2_binder::SymbolTable,
        symbol: SymbolId,
        meaning: tsrs2_types::SymbolFlags,
        ignore_qualification: bool,
        is_local_name_lookup: bool,
        visited: &mut Vec<ScopeTableKey>,
        enclosing: Option<NodeId>,
    ) -> CheckResult2<Option<Vec<SymbolId>>> {
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
                .intersects(tsrs2_types::SymbolFlags::ALIAS)
                && name != tsrs2_types::InternalSymbolName::EXPORT_EQUALS
                && name != tsrs2_types::InternalSymbolName::DEFAULT
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
        meaning: tsrs2_types::SymbolFlags,
        ignore_qualification: bool,
        visited: &mut Vec<ScopeTableKey>,
        enclosing: Option<NodeId>,
    ) -> CheckResult2<Option<Vec<SymbolId>>> {
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
        meaning: tsrs2_types::SymbolFlags,
        ignore_qualification: bool,
        enclosing: Option<NodeId>,
    ) -> CheckResult2<bool> {
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
        meaning: tsrs2_types::SymbolFlags,
        enclosing: Option<NodeId>,
    ) -> CheckResult2<bool> {
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
        meaning: tsrs2_types::SymbolFlags,
        enclosing: Option<NodeId>,
    ) -> CheckResult2<bool> {
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
            let should_resolve = entry_flags.intersects(tsrs2_types::SymbolFlags::ALIAS)
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
        meaning: tsrs2_types::SymbolFlags,
    ) -> CheckResult2<Vec<SymbolId>> {
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
        meaning: tsrs2_types::SymbolFlags,
    ) -> CheckResult2<Vec<SymbolId>> {
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
            && container_flags.intersects(tsrs2_types::SymbolFlags::TYPE)
            && meaning == tsrs2_types::SymbolFlags::VALUE
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
    ) -> CheckResult2<Option<SymbolId>> {
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
            .get(tsrs2_types::InternalSymbolName::EXPORT_EQUALS)
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
    ) -> CheckResult2<Option<SymbolId>> {
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
            .get(tsrs2_types::InternalSymbolName::EXPORT_EQUALS)
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
    fn symbol_if_same_reference_slice(&mut self, s1: SymbolId, s2: SymbolId) -> CheckResult2<bool> {
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
    ) -> CheckResult2<String> {
        let exports = self.get_exports_of_symbol(parent)?;
        for (name, &exported) in exports.iter() {
            if self.symbol_if_same_reference_slice(exported, symbol)?
                && !name.starts_with("__@")
                && name != tsrs2_types::InternalSymbolName::EXPORT_EQUALS
            {
                return Ok(tsrs2_binder::unescape_leading_underscores(name).to_owned());
            }
        }
        Ok(self.symbol_display_name(symbol))
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
    fn symbol_has_external_module_declaration(&self, symbol: SymbolId) -> bool {
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
    fn specifier_for_module_symbol_slice(&self, symbol: SymbolId) -> CheckResult2<String> {
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
        match declaration {
            Some(declaration) => Ok(Self::normalize_program_path(
                &self.binder.source_of_node(declaration).file_name,
                &self.host_current_directory,
            )),
            // tsc dereferences the find() unconditionally —
            // augmentation-only symbols stay behind the curtain.
            None => Err(Unsupported::new(
                "module specifier without a non-augmentation declaration (nodeBuilder, T2/M8)",
            )),
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
        tsrs2_binder::get_assignment_declaration_kind(
            self.binder.source_of_node(assignment),
            assignment,
        ) == tsrs2_binder::AssignmentDeclarationKind::ThisProperty
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
    /// render behind the `typeof C` face, so the mix only arises from
    /// M8-band synthesis (mapped/instantiation-expression shapes) and
    /// stays behind the curtain with them.
    fn type_node_from_object_type_slice(
        &mut self,
        ty: TypeId,
        fully_qualified: bool,
    ) -> CheckResult2<(String, SliceTypeNodeKind)> {
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
        if construct_signatures.iter().any(|&signature| {
            self.signature_of(signature)
                .flags
                .intersects(tsrs2_types::SignatureFlags::ABSTRACT)
        }) {
            // The 51918-51928 re-derivation (see the header note).
            return Err(Unsupported::new(
                "typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)",
            ));
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
        // `continue` is unreachable while the re-derivation above
        // curtains every abstract-bearing shape), then index
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
    ) -> CheckResult2<String> {
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
                match name.and_then(|name| self.identifier_text(name)) {
                    Some(text) => tsrs2_binder::unescape_leading_underscores(text).to_owned(),
                    None => {
                        return Err(Unsupported::new(
                            "typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)",
                        ))
                    }
                }
            }
            None => "x".to_owned(),
        };
        let key = self.type_to_string_slice_ex(info.key_type, fully_qualified)?;
        let value = self.type_to_string_slice_ex(info.value_type, fully_qualified)?;
        self.slice_add_approximate_length(Self::slice_js_length(&name) + 4);
        let readonly = if info.is_readonly { "readonly " } else { "" };
        Ok(format!("{readonly}[{name}: {key}]: {value}"))
    }

    /// tsc-port: addPropertyToElementList @6.0.3
    /// tsc-hash: 51ca73b16014f72c20c3b112b50304ef359bc84bf5820463afb782e4cda6e335
    /// tsc-span: _tsc.js:52241-52400
    ///
    /// The late-bound trackComputedName block is dead in the slice
    /// (typeToString's tracker cannot track symbols); reverse-mapped
    /// properties ride the shouldUsePlaceholderForProperty machinery
    /// and the accessor/method faces are signature rungs — all out of
    /// slice. A function/method-flagged property whose filtered type
    /// has no call signatures and no question token emits NOTHING
    /// (52350's early return past the emission) — transcribed as the
    /// skip arm.
    fn property_signature_slice(
        &mut self,
        property: SymbolId,
        fully_qualified: bool,
        rendered: &mut Vec<String>,
    ) -> CheckResult2<()> {
        if self
            .links
            .symbol(property)
            .check_flags
            .intersects(tsrs2_types::CheckFlags::REVERSE_MAPPED)
        {
            return Err(Unsupported::new(
                "typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)",
            ));
        }
        let property_type = self.get_non_missing_type_of_symbol(property)?;
        let symbol_flags = self.binder.symbol(property).flags;
        let name = self.property_name_slice(property, fully_qualified)?;
        self.slice_add_approximate_length(Self::slice_js_length(&name) + 1);
        // 52268-52343: accessor properties whose write type diverges
        // (or whose class parent takes the getter/setter arms) print
        // signature faces; the same-type non-class fall-through
        // prints the plain property row (oracle-pinned:
        // `{ get p(): string; set p(v: string) }` displays
        // `{ p: string; }`).
        if symbol_flags.intersects(tsrs2_types::SymbolFlags::ACCESSOR) {
            let write_type = self.get_write_type_of_symbol(property)?;
            let error = self.tables.intrinsics.error;
            if property_type != error && write_type != error {
                let class_parent = self.binder.symbol(property).parent.is_some_and(|parent| {
                    self.binder
                        .symbol(parent)
                        .flags
                        .intersects(tsrs2_types::SymbolFlags::CLASS)
                });
                // 52273: the class-parent disjunct reads a
                // PropertyDeclaration among the accessor's
                // declarations (`accessor x` auto-accessor fields).
                // Class-parented accessor symbols cannot reach an
                // admitted anonymous display today — spreads drop
                // prototype accessors (probed: `{ ...classInstance }`
                // resolves member-less) and Pick/Omit shapes are
                // mapped types (M8) — so both class arms (52274 and
                // the 52298 accessor-modifier fake pair) stay behind
                // the curtain with a class-parent test instead of a
                // per-arm transcription.
                if class_parent {
                    return Err(Unsupported::new(
                        "typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)",
                    ));
                }
                if property_type != write_type {
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
            }
        }
        let optional = symbol_flags.intersects(tsrs2_types::SymbolFlags::OPTIONAL);
        if symbol_flags
            .intersects(tsrs2_types::SymbolFlags::FUNCTION | tsrs2_types::SymbolFlags::METHOD)
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
        let type_text = self.type_to_string_slice_ex(property_type, fully_qualified)?;
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
    ) -> CheckResult2<String> {
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
    ) -> CheckResult2<String> {
        let slice_kind = match kind {
            SignatureKind::Call => SliceSignatureKind::CallSignature,
            SignatureKind::Construct => SliceSignatureKind::ConstructSignature,
        };
        self.signature_to_string_slice_for_diagnostic(signature, slice_kind)
    }

    /// Keep every standalone diagnostic render isolated from an
    /// enclosing typeToString slice. This mirrors tsc's fresh
    /// single-line writer per signatureToString call.
    fn signature_to_string_slice_for_diagnostic(
        &mut self,
        signature: SignatureId,
        kind: SliceSignatureKind,
    ) -> CheckResult2<String> {
        let saved_visited = std::mem::take(&mut self.slice_visited_types);
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
        let saved_no_type_reduction = std::mem::replace(&mut self.slice_no_type_reduction, false);
        let saved_enclosing = self.slice_display_enclosing.take();
        let result =
            self.signature_to_string_slice(signature, kind, None, /*fully_qualified*/ false);
        self.slice_visited_types = saved_visited;
        self.slice_approximate_length = saved_approximate_length;
        self.slice_max_truncation_length = saved_max_truncation_length;
        self.slice_truncating = saved_truncating;
        self.slice_no_type_reduction = saved_no_type_reduction;
        self.slice_display_enclosing = saved_enclosing;
        result
    }

    /// tsc-port: signatureToSignatureDeclarationHelper @6.0.3
    /// tsc-hash: 648aa8da24269c33b616fec95aa4cf725df9b6ddc0bb254ac01e456791be71c7
    /// tsc-span: _tsc.js:52504-52631
    ///
    /// Dead context legs under the error-display slice, all keyed on
    /// state typeToString never carries: WriteTypeArgumentsOfSignature
    /// (a signatureToString-band flag, 52515), enterNewScope's fake
    /// scopes and GenerateNamesForShadowedTypeParams renaming (both
    /// need an enclosingDeclaration/flag bit — typeToString passes
    /// AllowUniqueESSymbolType|UseAliasDefinedOutsideCurrentScope
    /// only, and the slice's enclosing field feeds nothing but the
    /// annotation-reuse gates), OmitThisParameter,
    /// SuppressAnyReturnType (52520 clears it around the parameter
    /// walk regardless), and the JSDocSignature overload-comment tail
    /// (52605-52620), whose synthetic comment is discarded by this
    /// comment-free string slice. options.modifiers is empty at every slice call
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
    ) -> CheckResult2<String> {
        let expanded = self.expanded_parameter_faces_slice(signature)?;
        let sig = self.signature_of(signature);
        let type_parameters = sig.type_parameters.clone();
        let declared_parameters = sig.parameters.clone();
        let this_parameter = sig.this_parameter;
        let declaration = sig.declaration;
        let is_abstract = sig.flags.intersects(tsrs2_types::SignatureFlags::ABSTRACT);
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
    ) -> CheckResult2<Option<Vec<SliceParameterFace>>> {
        let sig = self.signature_of(signature);
        if !sig
            .flags
            .intersects(tsrs2_types::SignatureFlags::HAS_REST_PARAMETER)
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
    fn tuple_element_label_slice(
        &mut self,
        declaration: Option<NodeId>,
        index: usize,
        element_flags: ElementFlags,
        rest_symbol: Option<SymbolId>,
    ) -> CheckResult2<String> {
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
                        tsrs2_binder::unescape_leading_underscores(
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
    ) -> CheckResult2<String> {
        let (name, dot_dot_dot) = match self.data_of(node) {
            NodeData::Parameter(data) => (data.name, data.dot_dot_dot_token.is_some()),
            NodeData::BindingElement(data) => (data.name, data.dot_dot_dot_token.is_some()),
            _ => (None, false),
        };
        if let Some(name) = name {
            match self.data_of(name) {
                NodeData::Identifier(data) => {
                    let text =
                        tsrs2_binder::unescape_leading_underscores(&data.escaped_text).to_owned();
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
    ) -> CheckResult2<SliceParameterFace> {
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
            || check_flags.intersects(tsrs2_types::CheckFlags::REST_PARAMETER);
        let optional = match declaration {
            Some(declaration) => self.is_optional_parameter_slice(declaration)?,
            None => false,
        } || check_flags.intersects(tsrs2_types::CheckFlags::OPTIONAL_PARAMETER);
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
    ) -> CheckResult2<String> {
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
                type_text =
                    if self.node_flags(annotation) & tsrs2_types::NodeFlags::JS_DOC.bits() != 0 {
                        self.reusable_annotation_node_text_slice(annotation)?
                    } else {
                        self.annotation_reuse_text_slice(
                            annotation,
                            face.ty,
                            requires_undefined,
                            question,
                            /*is_parameter*/ true,
                        )?
                    };
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
        let name_text =
            match &face.name {
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
                                tsrs2_binder::unescape_leading_underscores(&data.escaped_text)
                                    .to_owned()
                            }
                            NodeData::QualifiedName(data) => data
                                .right
                                .and_then(|right| self.identifier_text_of(right))
                                .map(tsrs2_binder::unescape_leading_underscores)
                                .unwrap_or_default()
                                .to_owned(),
                            NodeData::ObjectBindingPattern(_)
                            | NodeData::ArrayBindingPattern(_) => {
                                self.binding_pattern_text_slice(name)?
                            }
                            _ => return Err(Unsupported::new(
                                "typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)",
                            )),
                        },
                        None => match face.symbol {
                            Some(symbol) => self.symbol_display_name(symbol),
                            None => return Err(Unsupported::new(
                                "typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)",
                            )),
                        },
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
    fn is_optional_parameter_slice(&mut self, node: NodeId) -> CheckResult2<bool> {
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
            let parameters = match self.data_of(parent) {
                NodeData::FunctionDeclaration(data) => self.nodes_of(data.parameters),
                NodeData::FunctionExpression(data) => self.nodes_of(data.parameters),
                NodeData::ArrowFunction(data) => self.nodes_of(data.parameters),
                NodeData::MethodDeclaration(data) => self.nodes_of(data.parameters),
                NodeData::Constructor(data) => self.nodes_of(data.parameters),
                NodeData::GetAccessor(data) => self.nodes_of(data.parameters),
                NodeData::SetAccessor(data) => self.nodes_of(data.parameters),
                _ => Vec::new(),
            };
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
    fn requires_adding_implicit_undefined_slice(
        &mut self,
        parameter: NodeId,
    ) -> CheckResult2<bool> {
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
        let parameter_property = tsrs2_binder::node_util::has_syntactic_modifier(
            source,
            parameter,
            tsrs2_types::ModifierFlags::PARAMETER_PROPERTY_MODIFIER,
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
    /// syntactic arm rides the annotation-reuse gate; the inferred
    /// arm renders the type predicate first (53548-53556).
    /// context.mapper re-instantiation of the predicate is identity
    /// here: getTypePredicateOfSignature already resolves through
    /// signature.target/mapper (narrow.rs), and enterNewScope's
    /// context.mapper IS signature.mapper, whose second application
    /// re-maps type parameters the instantiation already replaced.
    fn serialize_return_type_for_signature_slice(
        &mut self,
        signature: SignatureId,
        fully_qualified: bool,
    ) -> CheckResult2<String> {
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
        }
        if let Some(predicate) = self.get_type_predicate_of_signature(signature)? {
            return self.type_predicate_text_slice(&predicate, fully_qualified);
        }
        self.type_to_string_slice_ex(return_type, fully_qualified)
    }

    /// tsc-port: typePredicateToTypePredicateNodeHelper @6.0.3
    /// tsc-hash: ef7d04a8094c121ca47028327ba885afcb7a285a28adfe579ddff0335642b7f4
    /// tsc-span: _tsc.js:52840-52846
    fn type_predicate_text_slice(
        &mut self,
        predicate: &crate::narrow::TypePredicate,
        fully_qualified: bool,
    ) -> CheckResult2<String> {
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
    ) -> CheckResult2<String> {
        let symbol = self.tables.type_of(type_parameter).symbol;
        let Some(symbol) = symbol else {
            return Err(Unsupported::new(
                "typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)",
            ));
        };
        let mut modifiers = String::new();
        {
            let declarations = self.binder.symbol(symbol).declarations.clone();
            let mut has_const = false;
            let mut has_in = false;
            let mut has_out = false;
            for declaration in declarations {
                let source = self.binder.source_of_node(declaration);
                has_const |= tsrs2_binder::node_util::has_syntactic_modifier(
                    source,
                    declaration,
                    tsrs2_types::ModifierFlags::CONST,
                );
                has_in |= tsrs2_binder::node_util::has_syntactic_modifier(
                    source,
                    declaration,
                    tsrs2_types::ModifierFlags::IN,
                );
                has_out |= tsrs2_binder::node_util::has_syntactic_modifier(
                    source,
                    declaration,
                    tsrs2_types::ModifierFlags::OUT,
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
    ) -> CheckResult2<Option<String>> {
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
    ) -> CheckResult2<bool> {
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
        if !tsrs2_binder::node_util::is_expression_node(source, value_declaration) {
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
    /// enclosing restores across the Err unwind (Unsupported rides
    /// `?` past the reset otherwise).
    pub(crate) fn type_to_string_slice_with_error_enclosing(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<String> {
        let enclosing = self.slice_display_enclosing_for(ty);
        let saved = std::mem::replace(&mut self.slice_display_enclosing, enclosing);
        let result = self.type_to_string_slice(ty);
        self.slice_display_enclosing = saved;
        result
    }

    /// tsrs-native: explicit-enclosing adapter for tsc's
    /// `typeToString(type, enclosingDeclaration)` calls. The parked
    /// nodeBuilder context is restored on both success and
    /// Unsupported unwind.
    pub(crate) fn type_to_string_slice_at(
        &mut self,
        ty: TypeId,
        enclosing: NodeId,
    ) -> CheckResult2<String> {
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
    /// order, alias spellings. The visitor's rewrites are
    /// tracker-driven and dead on the error path. A node kind the
    /// bounded printer cannot render faithfully Errs — the row stays
    /// curtained rather than emitting divergent text.
    fn reusable_annotation_node_text_slice(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<Option<String>> {
        Ok(Some(self.type_annotation_text_slice(node)?))
    }

    /// The bounded type-node printer behind the reuse faces: the
    /// standard printer's emission for cloned annotation ASTs.
    /// Initializer-free by construction (type positions); Errs on the
    /// kinds whose emission the slice has not needed yet (import
    /// types, mapped/conditional/infer shapes, JSDoc nodes).
    fn type_annotation_text_slice(&mut self, node: NodeId) -> CheckResult2<String> {
        let curtain =
            || Unsupported::new("typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)");
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
            NodeData::ParenthesizedType(data) => {
                let inner = data.r#type.ok_or_else(curtain)?;
                Ok(format!("({})", self.type_annotation_text_slice(inner)?))
            }
            NodeData::TypeReference(data) => {
                let name = self.entity_name_text_slice(data.type_name.ok_or_else(curtain)?)?;
                let arguments = self.nodes_of(data.type_arguments);
                if arguments.is_empty() {
                    return Ok(name);
                }
                let mut rendered = Vec::with_capacity(arguments.len());
                for argument in arguments {
                    rendered.push(self.type_annotation_text_slice(argument)?);
                }
                Ok(format!("{name}<{}>", rendered.join(", ")))
            }
            NodeData::UnionType(data) => {
                let mut rendered = Vec::new();
                for member in self.nodes_of(data.types) {
                    rendered.push(self.type_annotation_text_slice(member)?);
                }
                Ok(rendered.join(" | "))
            }
            NodeData::IntersectionType(data) => {
                let mut rendered = Vec::new();
                for member in self.nodes_of(data.types) {
                    rendered.push(self.type_annotation_text_slice(member)?);
                }
                Ok(rendered.join(" & "))
            }
            NodeData::ArrayType(data) => {
                let element = data.element_type.ok_or_else(curtain)?;
                Ok(format!("{}[]", self.type_annotation_text_slice(element)?))
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
                let name = self.entity_name_text_slice(data.name.ok_or_else(curtain)?)?;
                let question = if data.question_token.is_some() {
                    "?"
                } else {
                    ""
                };
                let ty = self.type_annotation_text_slice(data.r#type.ok_or_else(curtain)?)?;
                Ok(format!("{dots}{name}{question}: {ty}"))
            }
            NodeData::OptionalType(data) => {
                let inner = data.r#type.ok_or_else(curtain)?;
                Ok(format!("{}?", self.type_annotation_text_slice(inner)?))
            }
            NodeData::RestType(data) => {
                let inner = data.r#type.ok_or_else(curtain)?;
                Ok(format!("...{}", self.type_annotation_text_slice(inner)?))
            }
            NodeData::TypeOperator(data) => {
                let operator = match data.operator {
                    SyntaxKind::KeyOfKeyword => "keyof",
                    SyntaxKind::ReadonlyKeyword => "readonly",
                    SyntaxKind::UniqueKeyword => "unique",
                    _ => return Err(curtain()),
                };
                let inner = data.r#type.ok_or_else(curtain)?;
                Ok(format!(
                    "{operator} {}",
                    self.type_annotation_text_slice(inner)?
                ))
            }
            NodeData::TypeQuery(data) => {
                let name = self.entity_name_text_slice(data.expr_name.ok_or_else(curtain)?)?;
                let arguments = self.nodes_of(data.type_arguments);
                if arguments.is_empty() {
                    return Ok(format!("typeof {name}"));
                }
                let mut rendered = Vec::with_capacity(arguments.len());
                for argument in arguments {
                    rendered.push(self.type_annotation_text_slice(argument)?);
                }
                Ok(format!("typeof {name}<{}>", rendered.join(", ")))
            }
            NodeData::IndexedAccessType(data) => {
                let object =
                    self.type_annotation_text_slice(data.object_type.ok_or_else(curtain)?)?;
                let index =
                    self.type_annotation_text_slice(data.index_type.ok_or_else(curtain)?)?;
                Ok(format!("{object}[{index}]"))
            }
            NodeData::LiteralType(data) => {
                self.literal_type_node_text_slice(data.literal.ok_or_else(curtain)?)
            }
            NodeData::TypePredicate(data) => {
                let asserts = if data.asserts_modifier.is_some() {
                    "asserts "
                } else {
                    ""
                };
                let parameter_name = data.parameter_name.ok_or_else(curtain)?;
                let parameter = if self.kind_of(parameter_name) == SyntaxKind::ThisType {
                    "this".to_owned()
                } else {
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
            NodeData::FunctionType(data) => {
                let type_parameters =
                    self.type_parameter_nodes_text_slice(self.nodes_of(data.type_parameters))?;
                let parameters = self.parameter_nodes_text_slice(self.nodes_of(data.parameters))?;
                let ret = self.type_annotation_text_slice(data.r#type.ok_or_else(curtain)?)?;
                Ok(format!("{type_parameters}({parameters}) => {ret}"))
            }
            NodeData::ConstructorType(data) => {
                let is_abstract = {
                    let source = self.binder.source_of_node(node);
                    tsrs2_binder::node_util::has_syntactic_modifier(
                        source,
                        node,
                        tsrs2_types::ModifierFlags::ABSTRACT,
                    )
                };
                let modifier = if is_abstract { "abstract " } else { "" };
                let type_parameters =
                    self.type_parameter_nodes_text_slice(self.nodes_of(data.type_parameters))?;
                let parameters = self.parameter_nodes_text_slice(self.nodes_of(data.parameters))?;
                let ret = self.type_annotation_text_slice(data.r#type.ok_or_else(curtain)?)?;
                Ok(format!(
                    "{modifier}new {type_parameters}({parameters}) => {ret}"
                ))
            }
            NodeData::TypeLiteral(data) => {
                let members = self.nodes_of(data.members);
                if members.is_empty() {
                    return Ok("{}".to_owned());
                }
                let mut rendered = Vec::with_capacity(members.len());
                for member in members {
                    rendered.push(self.type_literal_member_text_slice(member)?);
                }
                Ok(format!("{{ {}; }}", rendered.join("; ")))
            }
            NodeData::TemplateLiteralType(data) => {
                let head = data.head.ok_or_else(curtain)?;
                let head_text = match self.data_of(head) {
                    NodeData::TemplateHead(head_data) => head_data
                        .raw_text
                        .clone()
                        .unwrap_or_else(|| head_data.text.clone()),
                    _ => return Err(curtain()),
                };
                let mut text = format!("`{head_text}");
                for span in self.nodes_of(data.template_spans) {
                    let NodeData::TemplateLiteralTypeSpan(span_data) = self.data_of(span).clone()
                    else {
                        return Err(curtain());
                    };
                    let ty =
                        self.type_annotation_text_slice(span_data.r#type.ok_or_else(curtain)?)?;
                    let literal = span_data.literal.ok_or_else(curtain)?;
                    let literal_text = match self.data_of(literal) {
                        NodeData::TemplateMiddle(data) => {
                            data.raw_text.clone().unwrap_or_else(|| data.text.clone())
                        }
                        NodeData::TemplateTail(data) => {
                            data.raw_text.clone().unwrap_or_else(|| data.text.clone())
                        }
                        _ => return Err(curtain()),
                    };
                    text.push_str(&format!("${{{ty}}}{literal_text}"));
                }
                text.push('`');
                if text.is_ascii() {
                    Ok(text)
                } else {
                    Err(curtain())
                }
            }
            _ => Err(curtain()),
        }
    }

    /// Entity names in reused annotations: Identifier / QualifiedName
    /// dots / the property-access spellings type queries carry.
    fn entity_name_text_slice(&mut self, node: NodeId) -> CheckResult2<String> {
        let curtain =
            || Unsupported::new("typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)");
        match self.data_of(node).clone() {
            NodeData::Identifier(data) => {
                Ok(tsrs2_binder::unescape_leading_underscores(&data.escaped_text).to_owned())
            }
            NodeData::QualifiedName(data) => {
                let left = self.entity_name_text_slice(data.left.ok_or_else(curtain)?)?;
                let right = self.entity_name_text_slice(data.right.ok_or_else(curtain)?)?;
                Ok(format!("{left}.{right}"))
            }
            NodeData::PropertyAccessExpression(data) => {
                let left = self.entity_name_text_slice(data.expression.ok_or_else(curtain)?)?;
                let right = self.entity_name_text_slice(data.name.ok_or_else(curtain)?)?;
                Ok(format!("{left}.{right}"))
            }
            _ => Err(curtain()),
        }
    }

    /// LiteralTypeNode literal faces: synthesized clones print cooked
    /// numeric text and double-quoted strings (oracle-probed Q01/Q02).
    fn literal_type_node_text_slice(&mut self, literal: NodeId) -> CheckResult2<String> {
        let curtain =
            || Unsupported::new("typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)");
        match self.kind_of(literal) {
            SyntaxKind::TrueKeyword => return Ok("true".to_owned()),
            SyntaxKind::FalseKeyword => return Ok("false".to_owned()),
            SyntaxKind::NullKeyword => return Ok("null".to_owned()),
            _ => {}
        }
        match self.data_of(literal).clone() {
            NodeData::StringLiteral(data) => string_literal_name_slice(&data.text, false),
            NodeData::NumericLiteral(data) => Ok(data.text.clone()),
            NodeData::BigIntLiteral(data) => {
                let text = &data.text;
                if text.ends_with('n') && text[..text.len() - 1].bytes().all(|b| b.is_ascii_digit())
                {
                    Ok(text.clone())
                } else {
                    Err(curtain())
                }
            }
            NodeData::PrefixUnaryExpression(data) => {
                let operator = match data.operator {
                    SyntaxKind::MinusToken => "-",
                    SyntaxKind::PlusToken => "+",
                    _ => return Err(curtain()),
                };
                let operand = data.operand.ok_or_else(curtain)?;
                Ok(format!(
                    "{operator}{}",
                    self.literal_type_node_text_slice(operand)?
                ))
            }
            _ => Err(curtain()),
        }
    }

    /// Type-parameter declaration NODES inside reused annotations
    /// (`(x: <T>(y: T) => T)` shapes): name / constraint / default
    /// print from the AST.
    fn type_parameter_nodes_text_slice(&mut self, nodes: Vec<NodeId>) -> CheckResult2<String> {
        if nodes.is_empty() {
            return Ok(String::new());
        }
        let curtain =
            || Unsupported::new("typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)");
        let mut rendered = Vec::with_capacity(nodes.len());
        for node in nodes {
            let NodeData::TypeParameter(data) = self.data_of(node).clone() else {
                return Err(curtain());
            };
            let source = self.binder.source_of_node(node);
            let mut text = String::new();
            if tsrs2_binder::node_util::has_syntactic_modifier(
                source,
                node,
                tsrs2_types::ModifierFlags::CONST,
            ) {
                text.push_str("const ");
            }
            if tsrs2_binder::node_util::has_syntactic_modifier(
                source,
                node,
                tsrs2_types::ModifierFlags::IN,
            ) {
                text.push_str("in ");
            }
            if tsrs2_binder::node_util::has_syntactic_modifier(
                source,
                node,
                tsrs2_types::ModifierFlags::OUT,
            ) {
                text.push_str("out ");
            }
            text.push_str(&self.entity_name_text_slice(data.name.ok_or_else(curtain)?)?);
            if let Some(constraint) = data.constraint {
                text.push_str(" extends ");
                text.push_str(&self.type_annotation_text_slice(constraint)?);
            }
            if let Some(default) = data.r#default {
                text.push_str(" = ");
                text.push_str(&self.type_annotation_text_slice(default)?);
            }
            rendered.push(text);
        }
        Ok(format!("<{}>", rendered.join(", ")))
    }

    /// Parameter declaration NODES inside reused annotations: the
    /// printer's `[...]name[?][: type]` face (initializers cannot
    /// appear in type positions).
    fn parameter_nodes_text_slice(&mut self, nodes: Vec<NodeId>) -> CheckResult2<String> {
        let curtain =
            || Unsupported::new("typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)");
        let mut rendered = Vec::with_capacity(nodes.len());
        for node in nodes {
            let NodeData::Parameter(data) = self.data_of(node).clone() else {
                return Err(curtain());
            };
            let dots = if data.dot_dot_dot_token.is_some() {
                "..."
            } else {
                ""
            };
            let name_node = data.name.ok_or_else(curtain)?;
            let name = match self.data_of(name_node) {
                NodeData::Identifier(data) => {
                    tsrs2_binder::unescape_leading_underscores(&data.escaped_text).to_owned()
                }
                NodeData::ObjectBindingPattern(_) | NodeData::ArrayBindingPattern(_) => {
                    self.binding_pattern_text_slice(name_node)?
                }
                _ => return Err(curtain()),
            };
            let question = if data.question_token.is_some() {
                "?"
            } else {
                ""
            };
            let mut text = format!("{dots}{name}{question}");
            if let Some(annotation) = data.r#type {
                text.push_str(": ");
                text.push_str(&self.type_annotation_text_slice(annotation)?);
            }
            rendered.push(text);
        }
        Ok(rendered.join(", "))
    }

    /// Type-literal MEMBER nodes inside reused annotations, printed
    /// with the single-line `; ` joins (oracle-probed C07:
    /// `{ a: (number) }` renders `{ a: (number); }`).
    fn type_literal_member_text_slice(&mut self, member: NodeId) -> CheckResult2<String> {
        let curtain =
            || Unsupported::new("typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)");
        match self.data_of(member).clone() {
            NodeData::PropertySignature(data) => {
                let source = self.binder.source_of_node(member);
                let readonly = if tsrs2_binder::node_util::has_syntactic_modifier(
                    source,
                    member,
                    tsrs2_types::ModifierFlags::READONLY,
                ) {
                    "readonly "
                } else {
                    ""
                };
                let name = self.member_name_node_text_slice(data.name.ok_or_else(curtain)?)?;
                let question = if data.question_token.is_some() {
                    "?"
                } else {
                    ""
                };
                let mut text = format!("{readonly}{name}{question}");
                if let Some(annotation) = data.r#type {
                    text.push_str(": ");
                    text.push_str(&self.type_annotation_text_slice(annotation)?);
                }
                Ok(text)
            }
            NodeData::MethodSignature(data) => {
                let name = self.member_name_node_text_slice(data.name.ok_or_else(curtain)?)?;
                let question = if data.question_token.is_some() {
                    "?"
                } else {
                    ""
                };
                let type_parameters =
                    self.type_parameter_nodes_text_slice(self.nodes_of(data.type_parameters))?;
                let parameters = self.parameter_nodes_text_slice(self.nodes_of(data.parameters))?;
                let mut text = format!("{name}{question}{type_parameters}({parameters})");
                if let Some(annotation) = data.r#type {
                    text.push_str(": ");
                    text.push_str(&self.type_annotation_text_slice(annotation)?);
                }
                Ok(text)
            }
            NodeData::CallSignature(data) => {
                let type_parameters =
                    self.type_parameter_nodes_text_slice(self.nodes_of(data.type_parameters))?;
                let parameters = self.parameter_nodes_text_slice(self.nodes_of(data.parameters))?;
                let mut text = format!("{type_parameters}({parameters})");
                if let Some(annotation) = data.r#type {
                    text.push_str(": ");
                    text.push_str(&self.type_annotation_text_slice(annotation)?);
                }
                Ok(text)
            }
            NodeData::ConstructSignature(data) => {
                let type_parameters =
                    self.type_parameter_nodes_text_slice(self.nodes_of(data.type_parameters))?;
                let parameters = self.parameter_nodes_text_slice(self.nodes_of(data.parameters))?;
                let mut text = format!("new {type_parameters}({parameters})");
                if let Some(annotation) = data.r#type {
                    text.push_str(": ");
                    text.push_str(&self.type_annotation_text_slice(annotation)?);
                }
                Ok(text)
            }
            NodeData::IndexSignature(data) => {
                let source = self.binder.source_of_node(member);
                let readonly = if tsrs2_binder::node_util::has_syntactic_modifier(
                    source,
                    member,
                    tsrs2_types::ModifierFlags::READONLY,
                ) {
                    "readonly "
                } else {
                    ""
                };
                let parameters = self.parameter_nodes_text_slice(self.nodes_of(data.parameters))?;
                let mut text = format!("{readonly}[{parameters}]");
                if let Some(annotation) = data.r#type {
                    text.push_str(": ");
                    text.push_str(&self.type_annotation_text_slice(annotation)?);
                }
                Ok(text)
            }
            NodeData::GetAccessor(data) => {
                let name = self.member_name_node_text_slice(data.name.ok_or_else(curtain)?)?;
                let parameters = self.parameter_nodes_text_slice(self.nodes_of(data.parameters))?;
                let mut text = format!("get {name}({parameters})");
                if let Some(annotation) = data.r#type {
                    text.push_str(": ");
                    text.push_str(&self.type_annotation_text_slice(annotation)?);
                }
                Ok(text)
            }
            NodeData::SetAccessor(data) => {
                let name = self.member_name_node_text_slice(data.name.ok_or_else(curtain)?)?;
                let parameters = self.parameter_nodes_text_slice(self.nodes_of(data.parameters))?;
                Ok(format!("set {name}({parameters})"))
            }
            _ => Err(curtain()),
        }
    }

    /// Member/binding property NAMES inside reused nodes: identifier,
    /// quoted string (double — clones), numeric text, computed
    /// `[entity]`.
    fn member_name_node_text_slice(&mut self, name: NodeId) -> CheckResult2<String> {
        let curtain =
            || Unsupported::new("typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)");
        match self.data_of(name).clone() {
            NodeData::Identifier(data) => {
                Ok(tsrs2_binder::unescape_leading_underscores(&data.escaped_text).to_owned())
            }
            NodeData::StringLiteral(data) => string_literal_name_slice(&data.text, false),
            NodeData::NumericLiteral(data) => Ok(data.text.clone()),
            NodeData::ComputedPropertyName(data) => {
                let expression = data.expression.ok_or_else(curtain)?;
                Ok(format!("[{}]", self.entity_name_text_slice(expression)?))
            }
            _ => Err(curtain()),
        }
    }

    /// tsc-port: parameterToParameterDeclarationName @6.0.3 (binding face)
    /// tsc-hash: 44f35dfdb10907de5255a8afcf28645007b1953c6aef8352dc742faa73a0804e
    /// tsc-span: _tsc.js:52880-52911
    ///
    /// cloneBindingName elides initializers and single-lines the
    /// emission; the printer pads object-pattern braces (`{ a, b }`)
    /// but not array patterns (`[a, b]`); omitted elements print
    /// empty (`[, x]`); trackComputedName is tracker-dead.
    fn binding_pattern_text_slice(&mut self, pattern: NodeId) -> CheckResult2<String> {
        let curtain =
            || Unsupported::new("typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)");
        match self.data_of(pattern).clone() {
            NodeData::ObjectBindingPattern(data) => {
                let elements = self.nodes_of(data.elements);
                if elements.is_empty() {
                    return Ok("{}".to_owned());
                }
                let mut rendered = Vec::with_capacity(elements.len());
                for element in elements {
                    rendered.push(self.binding_element_text_slice(element)?);
                }
                Ok(format!("{{ {} }}", rendered.join(", ")))
            }
            NodeData::ArrayBindingPattern(data) => {
                let elements = self.nodes_of(data.elements);
                let mut rendered = Vec::with_capacity(elements.len());
                for element in elements {
                    rendered.push(self.binding_element_text_slice(element)?);
                }
                Ok(format!("[{}]", rendered.join(", ")))
            }
            _ => Err(curtain()),
        }
    }

    fn binding_element_text_slice(&mut self, element: NodeId) -> CheckResult2<String> {
        let curtain =
            || Unsupported::new("typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)");
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
                let name_node = data.name.ok_or_else(curtain)?;
                let name = match self.data_of(name_node) {
                    NodeData::Identifier(data) => {
                        tsrs2_binder::unescape_leading_underscores(&data.escaped_text).to_owned()
                    }
                    NodeData::ObjectBindingPattern(_) | NodeData::ArrayBindingPattern(_) => {
                        self.binding_pattern_text_slice(name_node)?
                    }
                    _ => return Err(curtain()),
                };
                Ok(format!("{dots}{property}{name}"))
            }
            _ => Err(curtain()),
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
    ) -> CheckResult2<String> {
        if let Some(value_declaration) = self.binder.symbol(property).value_declaration {
            let name = tsrs2_binder::node_util::get_name_of_declaration(
                self.binder.source_of_node(value_declaration),
                value_declaration,
            );
            if let Some(name) = name {
                if matches!(self.data_of(name), NodeData::PrivateIdentifier(_)) {
                    return Err(Unsupported::new(
                        "typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)",
                    ));
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
            .intersects(tsrs2_types::SymbolFlags::METHOD);
        if let Some(name_type) = name_type {
            let flags = self.tables.flags_of(name_type);
            if flags.intersects(TypeFlags::STRING_LITERAL | TypeFlags::NUMBER_LITERAL) {
                let name = match &self.tables.type_of(name_type).data {
                    TypeData::Literal { value } => match value {
                        tsrs2_types::LiteralValue::String(text) => text.clone(),
                        tsrs2_types::LiteralValue::Number(value) => {
                            tsrs2_types::js_number_to_string(*value)
                        }
                        tsrs2_types::LiteralValue::BigInt(_) => {
                            unreachable!("string/number literal flags imply string/number value")
                        }
                    },
                    _ => unreachable!("literal flags imply literal data"),
                };
                if !tsrs2_syntax::is_identifier_text(&name)
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
            tsrs2_binder::unescape_leading_underscores(&self.binder.symbol(property).escaped_name)
                .to_owned();
        identifier_or_literal_name_slice(&raw, string_named, single_quote, is_method)
    }

    /// tsc-port: isStringNamed @6.0.3 (slice face)
    /// tsc-hash: c000f08977999a9f153126ccfb4e5b4c8721c5e160a361bd941308799c3c657d
    /// tsc-span: _tsc.js:53388-53402
    fn declaration_is_string_named(
        &self,
        declaration: NodeId,
        name_type_flags: Option<TypeFlags>,
    ) -> bool {
        let name = tsrs2_binder::node_util::get_name_of_declaration(
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
    fn declaration_is_single_quoted_string_named(&self, declaration: NodeId) -> bool {
        let source = self.binder.source_of_node(declaration);
        let Some(name) = tsrs2_binder::node_util::get_name_of_declaration(source, declaration)
        else {
            return false;
        };
        if !matches!(self.data_of(name), NodeData::StringLiteral(_)) {
            return false;
        }
        let end = source.arena.node(name).end as usize;
        end > 0 && source.text.as_bytes().get(end - 1) == Some(&b'\'')
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
    fn format_union_types(&mut self, types: &[TypeId]) -> CheckResult2<Vec<TypeId>> {
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
    /// Identifier — a pattern-named label would throw in shipped tsc,
    /// so the shape stays curtained instead of panicking here. The
    /// call-site unescapeLeadingUnderscores (51961) is folded in.
    fn tuple_element_label(&self, declaration: NodeId) -> CheckResult2<String> {
        let name = match self.data_of(declaration) {
            NodeData::NamedTupleMember(data) => data.name,
            NodeData::Parameter(data) => data.name,
            _ => None,
        };
        match name.and_then(|name| self.identifier_text(name)) {
            Some(text) => Ok(tsrs2_binder::unescape_leading_underscores(text).to_owned()),
            None => Err(Unsupported::new(
                "typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)",
            )),
        }
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

/// The would-be TypeNode kind of a slice rendering. The factory's
/// parenthesizer rules (_tsc.js 20540-20617) branch on the child
/// node's KIND at each join; the string renderer carries the kind
/// beside the text so the joins below apply the same rules. Only
/// kinds the slice can produce are listed — the parenthesizer arms
/// for the rest (infer heads) land with their shapes.
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

/// tsc-port: parenthesizeNonArrayTypeOfPostfixType @6.0.3 (kind test)
/// tsc-hash: 90b6701d51af1b9f1122f0d5ffcc9febe951cdae5b1430df8dfcb37781993928
/// tsc-span: _tsc.js:20577-20585
///
/// The infer arm wraps a kind the slice cannot produce; the typeof
/// arm wraps the TypeQuery face and the operand fall-through supplies
/// the intersection/union wraps.
fn non_array_postfix_operand_needs_parens(kind: SliceTypeNodeKind) -> bool {
    matches!(
        kind,
        SliceTypeNodeKind::TypeOperator | SliceTypeNodeKind::TypeQuery
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
#[cfg(test)]
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
fn string_literal_type_display_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    for (index, &c) in chars.iter().enumerate() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\0' => {
                if chars.get(index + 1).is_some_and(char::is_ascii_digit) {
                    out.push_str("\\x00");
                } else {
                    out.push_str("\\0");
                }
            }
            '\t' => out.push_str("\\t"),
            '\u{000B}' => out.push_str("\\v"),
            '\u{000C}' => out.push_str("\\f"),
            '\u{0008}' => out.push_str("\\b"),
            '\r' => out.push_str("\\r"),
            '\n' => out.push_str("\\n"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            '\u{0085}' => out.push_str("\\u0085"),
            '\u{0001}'..='\u{001F}' => out.push_str(&encode_utf16_escape_sequence(c as u16)),
            _ => out.push(c),
        }
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
) -> CheckResult2<String> {
    let is_method_named_new = is_method && name == "new";
    if !is_method_named_new && tsrs2_syntax::is_identifier_text(name) {
        return Ok(name.to_owned());
    }
    if !string_named
        && !is_method_named_new
        && crate::evaluate::is_numeric_literal_name(name)
        && crate::evaluate::js_string_to_number(name) >= 0.0
    {
        return Ok(tsrs2_types::js_number_to_string(
            crate::evaluate::js_string_to_number(name),
        ));
    }
    string_literal_name_slice(name, single_quote)
}

/// The printer's string-literal face (property names, import-type
/// specifiers), bounded like the literal display arm: plain ASCII
/// without escapes; anything needing escapeString's rewriting stays
/// behind the curtain.
fn string_literal_name_slice(name: &str, single_quote: bool) -> CheckResult2<String> {
    let quote = if single_quote { '\'' } else { '"' };
    if name
        .chars()
        .all(|c| c.is_ascii() && !c.is_ascii_control() && c != quote && c != '\\')
    {
        Ok(format!("{quote}{name}{quote}"))
    } else {
        Err(Unsupported::new(
            "literal display beyond plain strings/numbers (nodeBuilder, T2/M8)",
        ))
    }
}

#[cfg(test)]
mod tests {
    use tsrs2_types::{CompilerOptions, ScriptTarget};

    use crate::state::test_support::{
        with_program_state, with_program_state_allow_parse_diagnostics,
    };
    use crate::state::CheckerState;

    /// Drive the check driver over a single-file program and return
    /// the checker sink as (code, start, length, head message) rows.
    fn checked_diags(text: &str) -> Vec<(u32, u32, u32, String)> {
        checked_diags_with(text, &CompilerOptions::default())
    }

    fn checked_diags_with(text: &str, options: &CompilerOptions) -> Vec<(u32, u32, u32, String)> {
        checked_file_diags_with("a.ts", text, options)
    }

    fn checked_file_diags_with(
        file_name: &str,
        text: &str,
        options: &CompilerOptions,
    ) -> Vec<(u32, u32, u32, String)> {
        with_program_state(&[(file_name, text)], options, |state| {
            state.check_source_file(0);
            diag_rows(state)
        })
    }

    /// Parser-owned JSDoc diagnostics are stored separately from ordinary
    /// parse diagnostics and the checker sink. The Program driver merges
    /// this stream only for checked JavaScript files.
    fn jsdoc_parse_diag_rows(
        file_name: &str,
        text: &str,
        options: &CompilerOptions,
    ) -> Vec<(u32, u32, u32, String)> {
        with_program_state(&[(file_name, text)], options, |state| {
            state
                .binder
                .source(0)
                .js_doc_diagnostics
                .iter()
                .filter(|diagnostic| {
                    diagnostic.category() == tsrs2_diags::DiagnosticCategory::Error
                })
                .map(|diagnostic| {
                    (
                        diagnostic.code(),
                        diagnostic.start.unwrap_or(u32::MAX),
                        diagnostic.length.unwrap_or(u32::MAX),
                        diagnostic.message_text().to_owned(),
                    )
                })
                .collect()
        })
    }

    fn diag_rows(state: &CheckerState) -> Vec<(u32, u32, u32, String)> {
        state
            .diagnostics
            .iter()
            // File-less program diagnostics (the lazy missing-global
            // 2318 band these no-lib fixtures trip on Array probes)
            // are excluded from per-file output — same rule as
            // check_program's assembly.
            .filter(|diag| {
                diag.file_name.is_some()
                    && diag.category() == tsrs2_diags::DiagnosticCategory::Error
            })
            .map(|diag| {
                (
                    diag.code(),
                    diag.start.unwrap_or(u32::MAX),
                    diag.length.unwrap_or(u32::MAX),
                    diag.message_text().to_owned(),
                )
            })
            .collect()
    }

    fn checked_chain_codes(text: &str) -> Vec<Vec<u32>> {
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
            state.check_source_file(0);
            state
                .diagnostics
                .iter()
                .filter(|diagnostic| {
                    diagnostic.file_name.is_some()
                        && diagnostic.category() == tsrs2_diags::DiagnosticCategory::Error
                })
                .map(|diagnostic| {
                    fn visit(chain: &tsrs2_diags::MessageChain, codes: &mut Vec<u32>) {
                        codes.push(chain.code);
                        for next in &chain.next {
                            visit(next, codes);
                        }
                    }

                    let mut codes = Vec::new();
                    visit(&diagnostic.message, &mut codes);
                    codes
                })
                .collect()
        })
    }

    #[test]
    fn eager_unused_callback_precedes_source_file_collision_drains() {
        let text = "export {}; const WeakMap = 1; class C { #x = 1; }";
        let options = CompilerOptions {
            no_unused_locals: Some(true),
            target: Some(ScriptTarget::ES2015.bits()),
            ..CompilerOptions::default()
        };
        with_program_state(&[("a.ts", text)], &options, |state| {
            state.check_source_file(0);
            let relevant = state
                .diagnostics
                .iter()
                .filter_map(|diagnostic| {
                    matches!(diagnostic.code(), 6133 | 6196 | 18027).then_some(diagnostic.code())
                })
                .collect::<Vec<_>>();
            let collision = relevant
                .iter()
                .position(|&code| code == 18027)
                .expect("WeakMap collision");
            assert!(
                relevant[..collision]
                    .iter()
                    .any(|&code| matches!(code, 6133 | 6196)),
                "{relevant:?}"
            );
            assert!(
                relevant[collision + 1..]
                    .iter()
                    .all(|&code| !matches!(code, 6133 | 6196)),
                "{relevant:?}"
            );
        });
    }

    #[test]
    fn no_infer_relation_reports_use_the_write_normalized_target() {
        let rows = checked_diags(
            "type NoInfer<T> = intrinsic;\n\
             declare function assertEqual<T>(actual: T, expected: NoInfer<T>): boolean;\n\
             const g = { x: 3, y: 2 };\n\
             assertEqual(g, { x: 3 });\n\
             declare function invoke<T, R>(func: (value: T) => R, value: NoInfer<T>): R;\n\
             declare function test(value: { x: number }): number;\n\
             invoke(test, { x: 1, y: 2 });\n",
        );
        let messages = rows
            .into_iter()
            .filter(|row| matches!(row.0, 2345 | 2353))
            .map(|row| (row.0, row.3))
            .collect::<Vec<_>>();
        assert_eq!(
            messages,
            [
                (
                    2345,
                    "Argument of type '{ x: number; }' is not assignable to parameter of type '{ x: number; y: number; }'."
                        .to_owned(),
                ),
                (
                    2353,
                    "Object literal may only specify known properties, and 'y' does not exist in type '{ x: number; }'."
                        .to_owned(),
                ),
            ]
        );
    }

    #[test]
    fn relation_reports_use_normalized_pair_then_restore_alias_faces() {
        let options = CompilerOptions {
            strict_null_checks: Some(true),
            ..CompilerOptions::default()
        };
        let messages = checked_diags_with(
            "type Partial<T> = { [P in keyof T]?: T[P] };\n\
             type Readonly<T> = { readonly [P in keyof T]: T[P] };\n\
             type Named<T> = T & {};\n\
             function read<T>(x: T, p: Partial<T>, k: keyof T) { x[k] = p[k]; }\n\
             function write<T, U extends T>(x: T, r: Readonly<U>, k: keyof T) { r[k] = x[k]; }\n\
             function alias<T>(x: T, n: Named<T>) { n = x; }\n",
            &options,
        )
        .into_iter()
        .filter(|row| row.0 == 2322)
        .map(|row| row.3)
        .collect::<Vec<_>>();
        assert_eq!(
            messages,
            [
                "Type 'T[keyof T] | undefined' is not assignable to type 'T[keyof T]'.",
                "Type 'T[keyof T]' is not assignable to type 'U[keyof T]'.",
                "Type 'T' is not assignable to type 'Named<T>'.",
            ]
        );
    }

    #[test]
    fn report_only_refinement_does_not_erase_variadic_key_assignment() {
        let rows = checked_diags(
            "function f<T extends string[]>(k: keyof [1, 2, ...T]) {\n\
                 k = '2';\n\
             }\n",
        );
        assert_eq!(
            rows.into_iter()
                .filter(|row| row.0 == 2322)
                .collect::<Vec<_>>(),
            [(
                2322,
                56,
                1,
                "Type 'string' is not assignable to type 'keyof [1, 2, ...T]'.".to_owned(),
            )]
        );
    }

    #[test]
    fn unresolved_type_aliases_keep_written_return_faces_in_relation_reports() {
        let options = CompilerOptions {
            strict_null_checks: Some(true),
            ..CompilerOptions::default()
        };
        let messages = checked_diags_with(
            "let a: () => Missing = null;\n\
             let b: () => Missing.Scope<string> = null;\n",
            &options,
        )
        .into_iter()
        .filter(|row| row.0 == 2322)
        .map(|row| row.3)
        .collect::<Vec<_>>();
        assert_eq!(
            messages,
            [
                "Type 'null' is not assignable to type '() => Missing'.",
                "Type 'null' is not assignable to type '() => Missing.Scope<string>'.",
            ]
        );
    }

    #[test]
    fn duplicate_recovered_type_parameter_uses_the_missing_name_face() {
        let text = "type T<in in> = T;\n";
        with_program_state_allow_parse_diagnostics(
            &[("a.ts", text)],
            &CompilerOptions::default(),
            |state| {
                state.check_source_file(0);
                let diagnostic = state
                    .diagnostics
                    .iter()
                    .find(|diagnostic| diagnostic.code() == 2300)
                    .expect("duplicate recovered type parameter");
                assert_eq!(
                    diagnostic.message_text(),
                    "Duplicate identifier '(Missing)'."
                );
            },
        );
    }

    #[test]
    fn circularity_and_unassigned_property_diagnostics_use_written_names() {
        let options = CompilerOptions {
            strict: Some(true),
            target: Some(ScriptTarget::ES2015.bits()),
            ..CompilerOptions::default()
        };
        let rows = checked_diags_with(
            "class A {\n\
                 #foo = this.#bar;\n\
                 #bar = this.#foo;\n\
                 [\"#baz\"] = this[\"#baz\"];\n\
             }\n\
             class B {\n\
                 #d: number;\n\
                 constructor() {\n\
                     this.#d;\n\
                     this.#d = 1;\n\
                 }\n\
             }\n",
            &options,
        );
        let messages = rows
            .into_iter()
            .filter(|row| matches!(row.0, 7022 | 2565))
            .map(|row| (row.0, row.3))
            .collect::<Vec<_>>();
        assert_eq!(
            messages,
            [
                (
                    7022,
                    "'#foo' implicitly has type 'any' because it does not have a type annotation and is referenced directly or indirectly in its own initializer.".to_owned(),
                ),
                (
                    7022,
                    "'#bar' implicitly has type 'any' because it does not have a type annotation and is referenced directly or indirectly in its own initializer.".to_owned(),
                ),
                (
                    7022,
                    "'[\"#baz\"]' implicitly has type 'any' because it does not have a type annotation and is referenced directly or indirectly in its own initializer.".to_owned(),
                ),
                (
                    2565,
                    "Property '#d' is used before being assigned.".to_owned(),
                ),
            ]
        );
    }

    // ---- checked-JS checkJSDocTypeAliasTag AST path ----

    #[test]
    fn jsdoc_typedef_template_before_properties_reports_8021_on_the_name() {
        let text = "/**\n\
                    * @typedef Oops\n\
                    * @template T\n\
                    * @property {T} value\n\
                    */\n\
                    const host = {};\n";
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(true),
            ..CompilerOptions::default()
        };
        let rows: Vec<_> = checked_file_diags_with("a.js", text, &options)
            .into_iter()
            .filter(|row| row.0 == 8021)
            .collect();
        assert_eq!(
            rows,
            [(
                8021,
                text.find("Oops").unwrap() as u32,
                4,
                "JSDoc '@typedef' tag should either have a type annotation or be followed by '@property' or '@member' tags.".to_owned(),
            )]
        );
    }

    #[test]
    fn jsdoc_typedef_type_and_property_siblings_do_not_report_8021() {
        let text = "/** @typedef {(x: number) => string} Explicit */\n\
                    /**\n\
                    * @typedef ObjectLike\n\
                    * @property {number} value\n\
                    */\n\
                    /**\n\
                    * @typedef Nested\n\
                    * @property {Object} child\n\
                    * @template T\n\
                    * @property {T} child.value\n\
                    */\n\
                    const host = {};\n";
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(true),
            ..CompilerOptions::default()
        };
        assert!(checked_file_diags_with("a.js", text, &options)
            .into_iter()
            .all(|row| row.0 != 8021));
    }

    #[test]
    fn jsdoc_value_references_use_the_initializer_expando_symbol() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(false),
            target: Some(ScriptTarget::ES2015.bits()),
            ..CompilerOptions::default()
        };
        let class_text = "var Outer = class O {\n\
                              m(x, y) { }\n\
                          }\n\
                          Outer.Inner = class I {\n\
                              n(a, b) { }\n\
                          }\n\
                          /** @type {Outer} */\n\
                          var outer\n\
                          outer.m\n\
                          /** @type {Outer.Inner} */\n\
                          var inner\n\
                          inner.n\n";
        let function_text = "var Outer = function O() {\n\
                                 this.y = 2\n\
                             }\n\
                             Outer.Inner = class I {\n\
                                 constructor() { this.x = 1 }\n\
                             }\n\
                             /** @type {Outer} */\n\
                             var outer\n\
                             outer.y\n\
                             /** @type {Outer.Inner} */\n\
                             var inner\n\
                             inner.x\n";
        for text in [class_text, function_text] {
            let diagnostics = checked_file_diags_with("a.js", text, &options);
            assert!(
                diagnostics.iter().all(|row| row.0 != 2339),
                "JSDoc value references must expose initializer instance members: {diagnostics:?}"
            );
        }
    }

    // ---- checked-JS reportImplicitAny through materialized JSDoc ----

    #[test]
    fn jsdoc_implicit_any_honors_ts_check_and_ast_spans() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(false),
            no_implicit_any: Some(true),
            ..CompilerOptions::default()
        };
        let text = "// @ts-check\n\
                    /** @type {Function} */\n\
                    const x = a => a;\n\
                    /** @type {function (number)} */\n\
                    const y = n => n;\n";
        let diagnostics = checked_file_diags_with("a.js", text, &options);
        assert!(diagnostics.iter().any(|row| {
            row.0 == 7006
                && row.1 == text.find("a =>").expect("plain Function parameter") as u32
                && row.2 == 1
        }));
        let function_type = "function (number)";
        assert!(diagnostics.iter().any(|row| {
            row.0 == 7014
                && row.1 == text.find(function_type).expect("JSDoc function type") as u32
                && row.2 == function_type.len() as u32
        }));
    }

    // ---- checkUnmatchedJSDocParameters through materialized tags ----

    #[test]
    fn jsdoc_unmatched_parameters_preserve_owner_spans_and_nested_boundaries() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "/** @param {string} s */\n\
                    var one = function (s) {}, two = function (untyped) {};\n\
                    /**\n\
                     * @param {object} xyz\n\
                     * @param {number} xyz.bar.p\n\
                     */\n\
                    function qualified(xyz) {}\n\
                    /** @param {number?[]} a */\n\
                    function recovered(a) {}\n";
        let diagnostics = checked_file_diags_with("a.js", text, &options)
            .into_iter()
            .filter(|row| matches!(row.0, 8024 | 8032))
            .collect::<Vec<_>>();
        assert_eq!(
            diagnostics,
            [
                (
                    8024,
                    text.find("s */").expect("shared unmatched tag") as u32,
                    1,
                    "JSDoc '@param' tag has name 's', but there is no parameter with that name."
                        .to_owned(),
                ),
                (
                    8032,
                    text.find("xyz.bar.p").expect("qualified tag") as u32,
                    "xyz.bar.p".len() as u32,
                    "Qualified name 'xyz.bar.p' is not allowed without a leading '@param {object} xyz.bar'."
                        .to_owned(),
                ),
                (
                    8024,
                    text.find("?[]").expect("JSDoc type recovery") as u32,
                    0,
                    "JSDoc '@param' tag has name '', but there is no parameter with that name."
                        .to_owned(),
                ),
            ]
        );
    }

    #[test]
    fn jsdoc_unmatched_parameters_do_not_escape_nested_or_arguments_faces() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "/**\n\
                     * @param {Object} obj\n\
                     * @param {string} obj.value\n\
                     */\n\
                    function nested({ value }) {}\n\
                    /** @param {number} missing */\n\
                    function argumentsOwner(x) { return arguments; }\n";
        let diagnostics = checked_file_diags_with("a.js", text, &options);
        assert!(
            diagnostics.iter().all(|row| !matches!(row.0, 8024 | 8032)),
            "{diagnostics:?}"
        );
    }

    // ---- M8-P19 checkUnmatchedJSDocParameters arguments branch ----

    #[test]
    fn jsdoc_arguments_owner_reports_8029_for_the_last_non_array_parameter() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(true),
            ..CompilerOptions::default()
        };
        let text = "/** @param {string} first */\n\
                    function concat() { return arguments.length; }\n";
        let rows: Vec<_> = checked_file_diags_with("a.js", text, &options)
            .into_iter()
            .filter(|row| row.0 == 8029)
            .collect();
        assert_eq!(
            rows,
            [(
                8029,
                text.find("first").unwrap() as u32,
                5,
                "JSDoc '@param' tag has name 'first', but there is no parameter with that name. It would match 'arguments' if it had an array type.".to_owned(),
            )]
        );
    }

    #[test]
    fn jsdoc_arguments_owner_preserves_array_match_and_binding_siblings() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(true),
            ..CompilerOptions::default()
        };
        let text = "/**\n\
                     * @param {string} ignored\n\
                     * @param {...string} values\n\
                     */\n\
                    function variadic() { return arguments; }\n\
                    /** @param {string[]} values */\n\
                    function array() { return arguments; }\n\
                    /** @param {string} present */\n\
                    function matching(present) { return arguments; }\n\
                    /** @param {string} excluded */\n\
                    function binding({ value }) { return arguments; }\n";
        // `isArrayType` is a type-identity query. Supply the global
        // Array declaration that the ordinary Program lib prefix owns;
        // a no-lib fixture intentionally treats `string[]` as an error
        // type and tsc reports TS8029 for that world.
        with_program_state(
            &[
                (
                    "lib.d.ts",
                    "interface Array<T> { readonly length: number; }\n",
                ),
                ("a.js", text),
            ],
            &options,
            |state| {
                state.check_source_file(1);
                assert!(state
                    .diagnostics
                    .iter()
                    .all(|diagnostic| diagnostic.code() != 8029));
            },
        );
    }

    // ---- M8-P20 parseTypedefTag duplicate type child ----

    #[test]
    fn jsdoc_typedef_duplicate_type_reports_8033_with_detached_related() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "/**\n * @typedef Name\n * @type {string}\n * @type {Oops}\n */";
        with_program_state(&[("a.js", text)], &options, |state| {
            let diagnostic = state
                .binder
                .source(0)
                .js_doc_diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code() == 8033)
                .expect("TS8033");
            assert_eq!((diagnostic.start, diagnostic.length), (Some(54), Some(1)));
            assert_eq!(
                diagnostic.message_text(),
                "A JSDoc '@typedef' comment may not contain multiple '@type' tags."
            );
            assert_eq!(diagnostic.related.len(), 1);
            let related = &diagnostic.related[0];
            assert_eq!(related.file_name.as_deref(), Some("a.js"));
            assert_eq!((related.start, related.length), (Some(0), Some(0)));
            assert_eq!(related.message.code, 8034);
            assert_eq!(related.message.text, "The tag was first specified here.");
        });
    }

    #[test]
    fn jsdoc_typedef_duplicate_type_preserves_explicit_type_sibling() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "class C {\n\
                    /**\n\
                     * @typedef {C~A} C~B\n\
                     * @typedef {object} C~A\n\
                     */\n\
                    /** @param {C~A} o */\n\
                    constructor(o) {}\n\
                    }\n";
        assert!(jsdoc_parse_diag_rows("a.js", text, &options)
            .into_iter()
            .all(|row| row.0 != 8033));
    }

    // ---- M8-P21 invalid template child tags ----

    #[test]
    fn jsdoc_callback_overload_and_nested_property_report_8039() {
        let fixture = include_str!(
            "../../../ts-tests/tests/cases/conformance/jsdoc/templateInsideCallback.ts"
        );
        let text = fixture
            .split_once("// @filename: templateInsideCallback.js\n")
            .expect("fixture file section")
            .1;
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(true),
            ..CompilerOptions::default()
        };
        with_program_state(&[("templateInsideCallback.js", text)], &options, |state| {
            let rows = state
                .binder
                .source(0)
                .js_doc_diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 8039)
                .map(|diagnostic| {
                    (
                        diagnostic.start,
                        diagnostic.length,
                        diagnostic.message_text(),
                        diagnostic.related.len(),
                    )
                })
                .collect::<Vec<_>>();
            assert_eq!(
                rows,
                [
                    (
                        Some(104),
                        Some(8),
                        "A JSDoc '@template' tag may not follow a '@typedef', '@callback', or '@overload' tag",
                        0,
                    ),
                    (
                        Some(299),
                        Some(8),
                        "A JSDoc '@template' tag may not follow a '@typedef', '@callback', or '@overload' tag",
                        0,
                    ),
                    (
                        Some(370),
                        Some(8),
                        "A JSDoc '@template' tag may not follow a '@typedef', '@callback', or '@overload' tag",
                        0,
                    ),
                    (
                        Some(496),
                        Some(8),
                        "A JSDoc '@template' tag may not follow a '@typedef', '@callback', or '@overload' tag",
                        0,
                    ),
                ]
            );
        });
    }

    #[test]
    fn jsdoc_invalid_template_preserves_frozen_overload_sibling() {
        let fixture =
            include_str!("../../../ts-tests/tests/cases/conformance/jsdoc/overloadTag2.ts");
        let text = fixture
            .split_once("// @filename: overloadTag2.js\n")
            .expect("fixture file section")
            .1;
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(true),
            ..CompilerOptions::default()
        };
        assert!(jsdoc_parse_diag_rows("overloadTag2.js", text, &options)
            .into_iter()
            .all(|row| row.0 != 8039));
    }

    // ---- M7 8.1m JSDoc unique-symbol property grammar ----

    #[test]
    fn jsdoc_unique_symbol_properties_require_static_and_effective_readonly() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "class C {\n\
                      /** @type {unique symbol} */\n\
                      static missingReadonly;\n\
                      /**\n\
                       * @type {unique symbol}\n\
                       * @readonly\n\
                       */\n\
                      instance;\n\
                      /** @type {unique symbol}\n\
                       * @readonly */\n\
                      static valid;\n\
                      /** prose `@type {unique symbol}` */\n\
                      static prose;\n\
                      /** @type {unique symbolic} */\n\
                      static other;\n\
                    }\n";
        with_program_state(&[("a.js", text)], &options, |state| {
            state.check_source_file(0);
            let diagnostics = state
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 1331)
                .map(|diagnostic| {
                    (
                        diagnostic.start.expect("TS1331 start"),
                        diagnostic.length.expect("TS1331 length"),
                    )
                })
                .collect::<Vec<_>>();
            let expected = ["missingReadonly", "instance"].map(|name| {
                (
                    text.find(name).expect("property name") as u32,
                    name.len() as u32,
                )
            });
            assert_eq!(diagnostics, expected);
        });
    }

    // ---- M7 8.1n JSDoc parameter type-argument grammar ----

    #[test]
    fn jsdoc_parameter_dot_type_arguments_report_empty_and_trailing_comma() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "/** @param {C.<>} x\n\
                     * @param {C.<number,>} y */\n\
                    function f(x, y) {}\n";
        with_program_state(&[("a.js", text)], &options, |state| {
            state.check_source_file(0);
            let diagnostics = state
                .diagnostics
                .iter()
                .filter(|diagnostic| matches!(diagnostic.code(), 1009 | 1099))
                .map(|diagnostic| {
                    (
                        diagnostic.code(),
                        diagnostic.start.expect("grammar diagnostic start"),
                        diagnostic.length.expect("grammar diagnostic length"),
                    )
                })
                .collect::<Vec<_>>();
            let empty = text.find("<>").expect("empty type arguments") as u32;
            let comma = text.find(",>").expect("trailing comma") as u32;
            assert_eq!(diagnostics, [(1099, empty, 2), (1009, comma, 1)]);
        });
    }

    #[test]
    fn jsdoc_parameter_type_arguments_reject_other_comment_faces() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "/** @return {(Array.<> | null)} */\n\
                    function returns() {}\n\
                    /** prose `@param {C.<>} x` */\n\
                    function prose(x) {}\n\
                    /** @parameter {C.<number,>} x */\n\
                    function otherTag(x) {}\n\
                    const text = \"/** @param {C.<>} x */\";\n\
                    /** @param {C.<number>} x */\n\
                    function valid(x) {}\n";
        let diagnostics = jsdoc_parse_diag_rows("a.js", text, &options);
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| !matches!(diagnostic.0, 1009 | 1099)),
            "non-parameter/valid faces must not produce type-argument grammar diagnostics: \
             {diagnostics:?}"
        );
    }

    // ---- M7 8.1p JSDoc template-modifier grammar ----

    #[test]
    fn jsdoc_template_modifiers_follow_effective_host_grammar() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "/**\n\
                     * @template const T\n\
                     * @typedef {[T]} X\n\
                     */\n\
                    /** @template private T */\n\
                    function f() {}\n\
                    /** @template in T */\n\
                    function g() {}\n";
        with_program_state(&[("a.js", text)], &options, |state| {
            state.check_source_file(0);
            let diagnostics = state
                .diagnostics
                .iter()
                .filter(|diagnostic| matches!(diagnostic.code(), 1273 | 1274 | 1277))
                .map(|diagnostic| {
                    (
                        diagnostic.code(),
                        diagnostic.start.expect("template modifier start"),
                        diagnostic.length.expect("template modifier length"),
                        diagnostic.message_text().to_owned(),
                    )
                })
                .collect::<Vec<_>>();
            assert_eq!(
                diagnostics,
                [
                    (
                        1277,
                        text.find("const").expect("const modifier") as u32,
                        "const".len() as u32,
                        "'const' modifier can only appear on a type parameter of a function, method or class"
                            .to_owned(),
                    ),
                    (
                        1273,
                        text.find("private").expect("private modifier") as u32,
                        "private".len() as u32,
                        "'private' modifier cannot appear on a type parameter".to_owned(),
                    ),
                    (
                        1274,
                        text.find("@template in").expect("variance tag") as u32
                            + "@template ".len() as u32,
                        "in".len() as u32,
                        "'in' modifier can only appear on a type parameter of a class, interface or type alias"
                            .to_owned(),
                    ),
                ]
            );
        });
    }

    #[test]
    fn jsdoc_template_modifiers_preserve_valid_and_non_tag_faces() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "/** @template const T */\n\
                    class C {}\n\
                    /** @template in T\n\
                     * @typedef {Object} In */\n\
                    /** @template out T\n\
                     * @typedef {Object} Out */\n\
                    /** @template T */\n\
                    function valid() {}\n\
                    /** prose `@template private T` */\n\
                    function prose() {}\n\
                    /** @templates private T */\n\
                    function otherTag() {}\n\
                    const text = \"/** @template private T */\";\n\
                    /** @template privateish T */\n\
                    function boundary() {}\n";
        let diagnostics = checked_file_diags_with("a.js", text, &options);
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| !matches!(diagnostic.0, 1273 | 1274 | 1277)),
            "valid/non-tag faces must not produce template-modifier grammar diagnostics: \
             {diagnostics:?}"
        );
    }

    // ---- M7 8.1q JSDoc satisfies-tag duplicate grammar ----

    #[test]
    fn jsdoc_satisfies_duplicates_report_every_tag_after_the_first_per_host() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "/** @satisfies {number}\n\
                     * @satisfies {number} */\n\
                    const first = 1;\n\
                    /** @satisfies {number} */\n\
                    const second = /** @satisfies {number} */ (1);\n\
                    /** @satisfies {number}\n\
                     * @satisfies {number}\n\
                     * @satisfies {number} */\n\
                    const third = 1;\n";
        with_program_state(&[("a.js", text)], &options, |state| {
            state.check_source_file(0);
            let tags = text
                .match_indices("@satisfies")
                .map(|(start, _)| ((start + 1) as u32, "satisfies".len() as u32))
                .collect::<Vec<_>>();
            // Only the declaration-level and inline comments for `second`
            // collapse onto one effective initializer host. getAllJSDocTags
            // orders the inline tag first, so the declaration tag is the
            // duplicate reported by tsc.
            let expected = [tags[2]];
            let diagnostics = state
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 1223)
                .map(|diagnostic| {
                    (
                        diagnostic.start.expect("duplicate tag start"),
                        diagnostic.length.expect("duplicate tag length"),
                        diagnostic.message_text().to_owned(),
                    )
                })
                .collect::<Vec<_>>();
            assert_eq!(
                diagnostics,
                expected
                    .map(|(start, length)| (
                        start,
                        length,
                        "'satisfies' tag already specified.".to_owned(),
                    ))
                    .to_vec()
            );
        });
    }

    #[test]
    fn jsdoc_satisfies_duplicates_preserve_distinct_hosts_and_non_tags() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "/** @satisfies {number} */\n\
                    const first = 1;\n\
                    const inline = /** @satisfies {number} */ (1);\n\
                    /** @satisfies {number} */\n\
                    const left = 1, right = /** @satisfies {number} */ (2);\n\
                    /** prose `@satisfies {number}` */\n\
                    const prose = 1;\n\
                    /** @satisfiesElse {number} */\n\
                    const boundary = 1;\n\
                    const text = \"/** @satisfies {number} */\";\n\
                    /* @satisfies {number} */\n\
                    const ordinary = 1;\n";
        let diagnostics = checked_file_diags_with("a.js", text, &options);
        assert!(
            diagnostics.iter().all(|diagnostic| diagnostic.0 != 1223),
            "distinct-host/non-tag faces must not produce duplicate-tag diagnostics: \
             {diagnostics:?}"
        );
    }

    // ---- M7 8.1t JSDoc variadic-parameter grammar ----

    #[test]
    fn jsdoc_variadic_types_require_the_final_host_parameter() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "/**\n\
                     * @param {...?number} e\n\
                     * @param {...number?} f\n\
                     * @param {...number!?} g\n\
                     * @param {...number?!} h\n\
                     * @param {...number[]} i\n\
                     * @param {...number![]?} j\n\
                     * @param {...number?[]!} k\n\
                     * @param {...number} m\n\
                     */\n\
                    function f(e, f, g, h, i, j, k, m) {}\n";
        with_program_state(&[("a.js", text)], &options, |state| {
            state.check_source_file(0);
            let diagnostics = state
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 1014)
                .map(|diagnostic| {
                    (
                        diagnostic.start.expect("TS1014 start"),
                        diagnostic.length.expect("TS1014 length"),
                        diagnostic.message_text().to_owned(),
                    )
                })
                .collect::<Vec<_>>();
            let expected = [
                "...?number",
                "...number?",
                "...number!?",
                "...number[]",
                "...number![]?",
            ]
            .map(|variadic| {
                (
                    text.find(variadic).expect("variadic type") as u32,
                    variadic.len() as u32,
                    "A rest parameter must be last in a parameter list.".to_owned(),
                )
            });
            assert_eq!(diagnostics, expected);
        });
    }

    #[test]
    fn jsdoc_variadic_types_preserve_last_malformed_and_non_tag_faces() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "/** @param {...number} value */\n\
                    function last(value) {}\n\
                    /** @param {...number?!} bad\n\
                     * @param {...number?[]!} alsoBad\n\
                     * @param {number} final */\n\
                    function malformed(bad, alsoBad, final) {}\n\
                    /** prose `@param {...number} value` */\n\
                    function prose(value) {}\n\
                    /** @parameter {...number} value */\n\
                    function other(value) {}\n";
        let diagnostics = checked_file_diags_with("a.js", text, &options);
        assert!(
            diagnostics.iter().all(|diagnostic| diagnostic.0 != 1014),
            "last/malformed/non-tag faces must not produce TS1014: {diagnostics:?}"
        );
    }

    // ---- M7 8.1u JSDoc effective optional-parameter grammar ----

    #[test]
    fn jsdoc_optional_parameters_reject_a_following_required_parameter() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "/**\n\
                     * @param {number} a\n\
                     * @param {number} [b]\n\
                     * @param {number} c\n\
                     */\n\
                    function first(a, b, c) {}\n\
                    /**\n\
                     * @param {string=} `args`\n\
                     * @param `bwarg` {?number?}\n\
                     */\n\
                    function second(args, bwarg) {}\n";
        with_program_state(&[("a.js", text)], &options, |state| {
            state.check_source_file(0);
            let mut diagnostics = state
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 1016)
                .map(|diagnostic| {
                    (
                        diagnostic.start.expect("TS1016 start"),
                        diagnostic.length.expect("TS1016 length"),
                        diagnostic.message_text().to_owned(),
                    )
                })
                .collect::<Vec<_>>();
            diagnostics.sort_by_key(|diagnostic| diagnostic.0);
            let expected = [
                ("function first(a, b, c)", "c"),
                ("function second(args, bwarg)", "bwarg"),
            ]
            .map(|(signature, name)| {
                let signature_start = text.find(signature).expect("host signature");
                let relative_name = signature.rfind(name).expect("parameter name");
                (
                    (signature_start + relative_name) as u32,
                    name.len() as u32,
                    "A required parameter cannot follow an optional parameter.".to_owned(),
                )
            });
            assert_eq!(diagnostics, expected);
        });
    }

    #[test]
    fn jsdoc_optional_parameters_preserve_adjacent_negative_faces() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "/** @param {number} a\n\
                     * @param {number} [b] */\n\
                    function ordered(a, b) {}\n\
                    /** @param {number} [a]\n\
                     * @param {number} b */\n\
                    function initialized(a, b = 0) {}\n\
                    /** @param {object} opts\n\
                     * @param {number} [opts.value]\n\
                     * @param {number} tail */\n\
                    function property(opts, tail) {}\n\
                    /** prose `@param {number} [a]` */\n\
                    function prose(a, b) {}\n";
        let diagnostics = checked_file_diags_with("a.js", text, &options);
        assert!(
            diagnostics.iter().all(|diagnostic| diagnostic.0 != 1016),
            "ordered/initialized/property/prose faces must not produce TS1016: {diagnostics:?}"
        );

        let ts_diagnostics = checked_file_diags_with(
            "a.ts",
            "/** @param {number} [a] */\nfunction typed(a: number, b: number) {}\n",
            &options,
        );
        assert!(
            ts_diagnostics.iter().all(|diagnostic| diagnostic.0 != 1016),
            "JSDoc optionality is a JavaScript-only effective token: {ts_diagnostics:?}"
        );
    }

    // ---- M7 8.1v JSDoc template missing-name grammar ----

    #[test]
    fn jsdoc_template_constraint_requires_a_parameter_name() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "/** @template {T} */\n\
                    function inline() {}\n\
                    /**\n\
                     * @template {NoLongerAllowed}\n\
                     * @template U\n\
                     */\n\
                    function multiline() {}\n";
        with_program_state(&[("a.js", text)], &options, |state| {
            let mut diagnostics = state
                .binder
                .source(0)
                .js_doc_diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 1069)
                .map(|diagnostic| {
                    (
                        diagnostic.start.expect("TS1069 start"),
                        diagnostic.length.expect("TS1069 length"),
                        diagnostic.message_text().to_owned(),
                    )
                })
                .collect::<Vec<_>>();
            diagnostics.sort_by_key(|diagnostic| diagnostic.0);
            let inline_start = text.find("{T}").expect("inline constraint") + "{T}".len();
            let next_tag_start =
                text.find("\n* @template U").expect("next template tag") + "\n".len();
            let expected = [inline_start, next_tag_start].map(|start| {
                (
                    start as u32,
                    1,
                    "Unexpected token. A type parameter name was expected without curly braces."
                        .to_owned(),
                )
            });
            assert_eq!(diagnostics, expected);
        });
    }

    #[test]
    fn jsdoc_template_missing_name_preserves_valid_and_non_tag_faces() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "/** @template {{ value: number }} T,U */\n\
                    function constrained() {}\n\
                    /** @template {number} [T=number] */\n\
                    function defaulted() {}\n\
                    /** @template T */\n\
                    function plain() {}\n\
                    /** prose `@template {T}` */\n\
                    function prose() {}\n";
        let diagnostics = jsdoc_parse_diag_rows("a.js", text, &options);
        assert!(
            diagnostics.iter().all(|diagnostic| diagnostic.0 != 1069),
            "valid/non-tag template faces must not produce TS1069: {diagnostics:?}"
        );

        let ts_diagnostics = jsdoc_parse_diag_rows(
            "a.ts",
            "/** @template {T} */\nfunction typed() {}\n",
            &options,
        );
        assert!(
            ts_diagnostics.iter().all(|diagnostic| diagnostic.0 != 1069),
            "JSDoc parser diagnostics are JavaScript-only: {ts_diagnostics:?}"
        );
    }

    // ---- M7 8.1w JSDoc identifier-name recovery grammar ----

    #[test]
    fn jsdoc_identifier_name_recovery_reports_missing_and_invalid_names() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "/** @augments */\n\
                    class Augments {}\n\
                    /** @implements */\n\
                    class Implements {}\n\
                    /**\n\
                     * @property {string} #id\n\
                     * @param *\n\
                     * @param {number}\n\
                     * * y\n\
                     * @param {number} * z\n\
                     */\n\
                    function invalid(x, y, z) {}\n";
        with_program_state(&[("a.js", text)], &options, |state| {
            let mut diagnostics = state
                .binder
                .source(0)
                .js_doc_diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 1003)
                .map(|diagnostic| {
                    (
                        diagnostic.start.expect("TS1003 start"),
                        diagnostic.length.expect("TS1003 length"),
                        diagnostic.message_text().to_owned(),
                    )
                })
                .collect::<Vec<_>>();
            diagnostics.sort_by_key(|diagnostic| diagnostic.0);
            let expected_starts = [
                text.find("@augments").expect("augments tag") + "@augments".len(),
                text.find("@implements").expect("implements tag") + "@implements".len(),
                text.find("@param *").expect("inline star parameter") + "@param ".len(),
                text.find("\n* * y").expect("wrapped star parameter") + "\n* ".len(),
                text.find("@param {number} * z")
                    .expect("typed star parameter")
                    + "@param {number} ".len(),
            ];
            assert_eq!(
                diagnostics,
                expected_starts
                    .map(|start| (start as u32, 0, "Identifier expected.".to_owned()))
                    .to_vec()
            );
        });
    }

    #[test]
    fn jsdoc_identifier_name_recovery_preserves_valid_wrapping_and_non_tags() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "/** @augments {Base} */\n\
                    class Augments {}\n\
                    /** @implements Base */\n\
                    class Implements {}\n\
                    /**\n\
                     * @property {string} id\n\
                     * @param\n\
                     * {number} x\n\
                     * @param {number}\n\
                     * y\n\
                     * @param {number} z\n\
                     * argument z\n\
                     */\n\
                    function valid(x, y, z) {}\n\
                    /** prose `@param *` */\n\
                    function prose(value) {}\n\
                    /** @parameter * */\n\
                    function boundary(value) {}\n";
        let diagnostics = jsdoc_parse_diag_rows("a.js", text, &options);
        assert!(
            diagnostics.iter().all(|diagnostic| diagnostic.0 != 1003),
            "valid/non-tag JSDoc faces must not produce TS1003: {diagnostics:?}"
        );

        let ts_diagnostics = jsdoc_parse_diag_rows(
            "a.ts",
            "/** @implements */\nclass Typed {}\n/** @param * */\nfunction f(x: number) {}\n",
            &options,
        );
        assert!(
            ts_diagnostics.iter().all(|diagnostic| diagnostic.0 != 1003),
            "JSDoc parser diagnostics are JavaScript-only: {ts_diagnostics:?}"
        );
    }

    // ---- M7 8.1x JSDoc satisfies required-brace grammar ----

    #[test]
    fn jsdoc_satisfies_type_expression_requires_braces() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "/**\n * @satisfies T1\n */\nconst first = {};\n\
                    const second = /** @satisfies T2 */ ({});\n";
        with_program_state(&[("a.js", text)], &options, |state| {
            let mut diagnostics = state
                .binder
                .source(0)
                .js_doc_diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 1005)
                .map(|diagnostic| {
                    (
                        diagnostic.start.expect("TS1005 start"),
                        diagnostic.length.expect("TS1005 length"),
                        diagnostic.message_text().to_owned(),
                    )
                })
                .collect::<Vec<_>>();
            diagnostics.sort_by_key(|diagnostic| diagnostic.0);
            let comment_closes = text
                .match_indices("*/")
                .map(|(start, _)| start)
                .collect::<Vec<_>>();
            let first_type =
                text.find("@satisfies T1").expect("multiline satisfies tag") + "@satisfies ".len();
            let second_type =
                text.find("@satisfies T2").expect("inline satisfies tag") + "@satisfies ".len();
            let expected = [
                (first_type, "T1".len(), "'{' expected."),
                (comment_closes[0], 0, "'}' expected."),
                (second_type, "T2".len(), "'{' expected."),
                (comment_closes[1], 0, "'}' expected."),
            ];
            assert_eq!(
                diagnostics,
                expected
                    .map(|(start, length, message)| {
                        (start as u32, length as u32, message.to_owned())
                    })
                    .to_vec()
            );
        });
    }

    #[test]
    fn jsdoc_satisfies_braces_preserve_valid_and_non_tag_faces() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "/** @satisfies {T1} */\n\
                    const valid = {};\n\
                    /** prose `@satisfies T1` */\n\
                    const prose = {};\n\
                    /** @satisfiesElse T1 */\n\
                    const boundary = {};\n";
        let diagnostics = jsdoc_parse_diag_rows("a.js", text, &options);
        assert!(
            diagnostics.iter().all(|diagnostic| diagnostic.0 != 1005),
            "valid/non-tag satisfies faces must not produce TS1005: {diagnostics:?}"
        );

        let ts_diagnostics = jsdoc_parse_diag_rows(
            "a.ts",
            "/** @satisfies T1 */\nconst typed = {};\n",
            &options,
        );
        assert!(
            ts_diagnostics.iter().all(|diagnostic| diagnostic.0 != 1005),
            "JSDoc parser diagnostics are JavaScript-only: {ts_diagnostics:?}"
        );
    }

    // ---- M7 8.1y JSDoc import-clause `from` grammar ----

    #[test]
    fn jsdoc_default_import_clause_requires_from_keyword() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "/** @import defer * as ns from \"./types\" */\n\
                    /**\n * @import foo\n */\n\
                    /** @import x = require(\"types\") */\n";
        with_program_state(&[("a.js", text)], &options, |state| {
            let mut diagnostics = state
                .binder
                .source(0)
                .js_doc_diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 1005)
                .map(|diagnostic| {
                    (
                        diagnostic.start.expect("TS1005 start"),
                        diagnostic.length.expect("TS1005 length"),
                        diagnostic.message_text().to_owned(),
                    )
                })
                .collect::<Vec<_>>();
            diagnostics.sort_by_key(|diagnostic| diagnostic.0);
            let defer_star =
                text.find("@import defer *").expect("defer import") + "@import defer ".len();
            let foo_tag = text.find("@import foo").expect("missing-from import");
            let foo_close = foo_tag + text[foo_tag..].find("*/").expect("foo comment close");
            let import_equals =
                text.find("@import x =").expect("import-equals spelling") + "@import x ".len();
            let expected = [(defer_star, 1), (foo_close, 0), (import_equals, 1)];
            assert_eq!(
                diagnostics,
                expected
                    .map(|(start, length)| {
                        (start as u32, length, "'from' expected.".to_owned())
                    })
                    .to_vec()
            );
        });
    }

    #[test]
    fn jsdoc_import_from_preserves_valid_and_non_tag_faces() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "/** @import Foo from \"./foo\" */\n\
                    /** @import * as ns from \"./foo\" */\n\
                    /** @import { Bar } from \"./foo\" */\n\
                    /** @import Foo, { Bar } from \"./foo\" */\n\
                    /** prose `@import foo` */\n\
                    /** @imports foo */\n";
        let diagnostics = jsdoc_parse_diag_rows("a.js", text, &options);
        assert!(
            diagnostics.iter().all(|diagnostic| diagnostic.0 != 1005),
            "valid/non-tag import faces must not produce TS1005: {diagnostics:?}"
        );

        let ts_diagnostics = jsdoc_parse_diag_rows("a.ts", "/** @import foo */\n", &options);
        assert!(
            ts_diagnostics.iter().all(|diagnostic| diagnostic.0 != 1005),
            "JSDoc parser diagnostics are JavaScript-only: {ts_diagnostics:?}"
        );
    }

    // ---- M7 8.1aa JSDoc import module-specifier expression grammar ----

    #[test]
    fn jsdoc_import_module_specifier_requires_an_expression() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = concat!(
            "/**\n",
            " * @import\n",
            " */\n",
            "/**\n",
            " * @import foo\n",
            " */\n",
            "/**\n",
            " * @import foo from\n",
            " */\n",
        );
        with_program_state(&[("a.js", text)], &options, |state| {
            let mut diagnostics = state
                .binder
                .source(0)
                .js_doc_diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 1109)
                .map(|diagnostic| {
                    (
                        diagnostic.start.expect("TS1109 start"),
                        diagnostic.length.expect("TS1109 length"),
                        diagnostic.message_text().to_owned(),
                    )
                })
                .collect::<Vec<_>>();
            diagnostics.sort_by_key(|diagnostic| diagnostic.0);
            let bare = text.find("@import\n").expect("bare import tag") + "@import".len();
            let default =
                text.find("@import foo\n").expect("default import tag") + "@import foo".len();
            let from = text
                .find("@import foo from\n")
                .expect("missing module specifier")
                + "@import foo from".len();
            let expected = [(bare, 1), (default, 0), (from, 0)];
            assert_eq!(
                diagnostics,
                expected
                    .map(|(start, length)| {
                        (start as u32, length, "Expression expected.".to_owned())
                    })
                    .to_vec()
            );
        });
    }

    #[test]
    fn jsdoc_import_module_specifier_preserves_valid_and_non_tag_faces() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = concat!(
            "/** @import \"./side-effect\" */\n",
            "/** @import Foo from \"./foo\" */\n",
            "/** @import * as ns from \"./foo\" */\n",
            "/** @import { Bar } from \"./foo\" */\n",
            "/** prose `@import` */\n",
            "/** @imports */\n",
            "/* @import */\n",
            "const text = '/** @import */';\n",
        );
        let diagnostics = jsdoc_parse_diag_rows("a.js", text, &options)
            .into_iter()
            .filter(|diagnostic| diagnostic.0 == 1109)
            .collect::<Vec<_>>();
        assert_eq!(
            diagnostics,
            [(
                1109,
                text.find("\"./side-effect\"").unwrap() as u32,
                1,
                "Expression expected.".to_owned(),
            )]
        );

        let ts_diagnostics = jsdoc_parse_diag_rows("a.ts", "/**\n * @import\n */\n", &options);
        assert!(
            ts_diagnostics.iter().all(|diagnostic| diagnostic.0 != 1109),
            "JSDoc parser diagnostics are JavaScript-only: {ts_diagnostics:?}"
        );
    }

    // ---- M7 8.1ab JSDoc type-reference recovery grammar ----

    #[test]
    fn jsdoc_type_reference_recovery_reports_exact_tokens() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = concat!(
            "/**\n",
            " * @template {string | number} [T=]\n",
            " * @typedef {[T]} EmptyDefault\n",
            " */\n",
            "/**\n",
            "   @typedef {{\n",
            "     foo:\n",
            "     *,\n",
            "     bar:\n",
            "     *\n",
            "   }} Broken\n",
            " */\n",
        );
        with_program_state(&[("a.js", text)], &options, |state| {
            let mut diagnostics = state
                .binder
                .source(0)
                .js_doc_diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 1110)
                .map(|diagnostic| {
                    (
                        diagnostic.start.expect("TS1110 start"),
                        diagnostic.length.expect("TS1110 length"),
                        diagnostic.message_text().to_owned(),
                    )
                })
                .collect::<Vec<_>>();
            diagnostics.sort_by_key(|diagnostic| diagnostic.0);
            let expected = [
                (
                    text.find("[T=]").expect("empty template default") + "[T=".len(),
                    1,
                ),
                (
                    text.find("*,").expect("standalone star before comma") + "*".len(),
                    1,
                ),
                (text.find("}} Broken").expect("closing typedef brace"), 1),
            ];
            assert_eq!(
                diagnostics,
                expected
                    .map(|(start, length)| { (start as u32, length, "Type expected.".to_owned()) })
                    .to_vec()
            );
        });
    }

    #[test]
    fn jsdoc_type_reference_recovery_preserves_valid_and_non_tag_faces() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = concat!(
            "/** @template {string | number} [T=string] */\n",
            "/** @template {string | number} [T] */\n",
            "/** @templates {string | number} [T=] */\n",
            "/** prose `@template {string | number} [T=]` */\n",
            "/**\n",
            " * @typedef {{\n",
            " *   foo:\n",
            " *   *,\n",
            " *   bar:\n",
            " *   *\n",
            " * }} ValidWithStars\n",
            " */\n",
            "/**\n",
            "   @typedef {{\n",
            "     foo:\n",
            "     string,\n",
            "     bar:\n",
            "     number\n",
            "   }} ValidWithoutStars\n",
            " */\n",
            "/* @template {number} [T=] */\n",
            "const text = '/** @template {number} [T=] */';\n",
        );
        let diagnostics = jsdoc_parse_diag_rows("a.js", text, &options);
        assert!(
            diagnostics.iter().all(|diagnostic| diagnostic.0 != 1110),
            "valid/non-tag type-reference faces must not produce TS1110: {diagnostics:?}"
        );

        let ts_diagnostics = jsdoc_parse_diag_rows(
            "a.ts",
            "/** @template {number} [T=] */\nconst value = 1;\n",
            &options,
        );
        assert!(
            ts_diagnostics.iter().all(|diagnostic| diagnostic.0 != 1110),
            "JSDoc parser diagnostics are JavaScript-only: {ts_diagnostics:?}"
        );
    }

    // ---- M7 8.1ac JSDoc expected-close-brace recovery grammar ----

    #[test]
    fn jsdoc_expected_close_brace_reports_exact_recovery_tokens() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = concat!(
            "/**\n",
            " * @param {number?[]} a\n",
            " * @param {...number?!} b\n",
            " * @param {...number?[]!} c\n",
            " * @typedef {C~A} C_B\n",
            " * @param {C~A} d\n",
            " */\n",
            "function f(a, b, c, d) {}\n",
        );
        with_program_state(&[("a.js", text)], &options, |state| {
            let mut diagnostics = state
                .binder
                .source(0)
                .js_doc_diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 1005)
                .map(|diagnostic| {
                    (
                        diagnostic.start.expect("TS1005 start"),
                        diagnostic.length.expect("TS1005 length"),
                        diagnostic.message_text().to_owned(),
                    )
                })
                .collect::<Vec<_>>();
            diagnostics.sort_by_key(|diagnostic| diagnostic.0);
            let expected = [
                (
                    text.find("{number?[]}").expect("postfix nullable array") + "{number".len(),
                    1,
                ),
                (
                    text.find("{...number?!}")
                        .expect("nullable before non-null")
                        + "{...number".len(),
                    1,
                ),
                (
                    text.find("{...number?[]!}")
                        .expect("nullable before non-null array")
                        + "{...number".len(),
                    1,
                ),
                (
                    text.find("{C~A}").expect("typedef inner namepath") + "{C".len(),
                    1,
                ),
                (
                    text.rfind("{C~A}").expect("parameter inner namepath") + "{C".len(),
                    1,
                ),
            ];
            assert_eq!(
                diagnostics,
                expected
                    .map(|(start, length)| { (start as u32, length, "'}' expected.".to_owned()) })
                    .to_vec()
            );
        });
    }

    #[test]
    fn jsdoc_expected_close_brace_preserves_valid_and_non_tag_faces() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = concat!(
            "/**\n",
            " * @param {?number[]} a\n",
            " * @param {number?} b\n",
            " * @param {!number[]} c\n",
            " * @param {number!} d\n",
            " * @param {(number[])?} e\n",
            " * @param {[number, number?]} f\n",
            " * @param {T extends U ? [] : T} g\n",
            " * @param {Foo.Bar} h\n",
            " * @typedef {C.A} C_A\n",
            " * @params {number?[]} prose\n",
            " * prose `@param {number?[]} prose`\n",
            " */\n",
            "function valid(a, b, c, d, e, f, g, h) {}\n",
            "/* @param {number?[]} ordinary */\n",
            "const text = '/** @param {number?[]} string */';\n",
        );
        let diagnostics = jsdoc_parse_diag_rows("a.js", text, &options);
        assert!(
            diagnostics.iter().all(|diagnostic| diagnostic.0 != 1005),
            "valid/non-tag close-brace faces must not produce TS1005: {diagnostics:?}"
        );

        let ts_diagnostics = jsdoc_parse_diag_rows(
            "a.ts",
            "/** @param {number?[]} value */\nfunction f(value: number) {}\n",
            &options,
        );
        assert!(
            ts_diagnostics.iter().all(|diagnostic| diagnostic.0 != 1005),
            "JSDoc parser diagnostics are JavaScript-only: {ts_diagnostics:?}"
        );
    }

    // ---- M7 8.1ad JSDoc template missing-equals recovery grammar ----

    #[test]
    fn jsdoc_template_missing_equals_reports_the_closing_bracket() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = concat!(
            "/**\n",
            " * @template {string | number} [T]\n",
            " * @typedef {[T]} MissingDefault\n",
            " */\n",
        );
        with_program_state(&[("a.js", text)], &options, |state| {
            let diagnostics = state
                .binder
                .source(0)
                .js_doc_diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 1005)
                .map(|diagnostic| {
                    (
                        diagnostic.start.expect("TS1005 start"),
                        diagnostic.length.expect("TS1005 length"),
                        diagnostic.message_text().to_owned(),
                    )
                })
                .collect::<Vec<_>>();
            let start = text.find("[T]").expect("bracketed template parameter") + "[T".len();
            assert_eq!(
                diagnostics,
                vec![(start as u32, 1, "'=' expected.".to_owned())]
            );
        });
    }

    #[test]
    fn jsdoc_template_recovery_classifies_adjacent_malformed_faces() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = concat!(
            "/** @template T */\n",
            "/** @template {number} U */\n",
            "/** @template [T=string] */\n",
            "/** @template {number} [U=number] */\n",
            "/** @template [T=] */\n",
            "/** @template [] */\n",
            "/** @template [const T] */\n",
            "/** @templates [T] */\n",
            "/** prose `@template [T]` */\n",
            "/* @template [T] */\n",
            "const text = '/** @template [T] */';\n",
        );
        let diagnostics = jsdoc_parse_diag_rows("a.js", text, &options);
        assert_eq!(
            diagnostics,
            [
                (
                    1110,
                    (text.find("[T=]").unwrap() + "[T=".len()) as u32,
                    1,
                    "Type expected.".to_owned(),
                ),
                (
                    1069,
                    (text.find("[]").unwrap() + "[".len()) as u32,
                    1,
                    "Unexpected token. A type parameter name was expected without curly braces."
                        .to_owned(),
                ),
                (
                    1005,
                    (text.find("[const T]").unwrap() + "[const T".len()) as u32,
                    1,
                    "'=' expected.".to_owned(),
                ),
            ]
        );

        let ts_diagnostics = jsdoc_parse_diag_rows("a.ts", "/** @template [T] */\n", &options);
        assert!(
            ts_diagnostics.is_empty(),
            "JSDoc parser diagnostics are JavaScript-only: {ts_diagnostics:?}"
        );
    }

    // ---- M7 8.1s JSDoc satisfies semantics ----

    #[test]
    fn jsdoc_satisfies_semantics_reports_named_primitive_and_function_targets() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "/**\n\
                     * @typedef {Object} Required\n\
                     * @property {number} value\n\
                     */\n\
                    const object = /** @satisfies {Required} */ ({});\n\
                    /** @satisfies {string} */\n\
                    const primitive = (1);\n\
                    /**\n\
                     * @satisfies {(a: string, ...args: number[]) => void}\n\
                     * @param {string} a\n\
                     * @param {string} b\n\
                     */\n\
                    const callable = (a, b) => {};\n";
        with_program_state(&[("a.js", text)], &options, |state| {
            state.check_source_file(0);
            let diagnostics = state
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 1360)
                .map(|diagnostic| {
                    (
                        diagnostic.start.expect("TS1360 start"),
                        diagnostic.length.expect("TS1360 length"),
                        diagnostic.message_text().to_owned(),
                    )
                })
                .collect::<Vec<_>>();
            let tags = text
                .match_indices("@satisfies")
                .map(|(start, _)| ((start + 1) as u32, "satisfies".len() as u32))
                .collect::<Vec<_>>();
            assert_eq!(
                diagnostics,
                [
                    (
                        tags[0].0,
                        tags[0].1,
                        "Type '{}' does not satisfy the expected type 'Required'.".to_owned(),
                    ),
                    (
                        tags[1].0,
                        tags[1].1,
                        "Type 'number' does not satisfy the expected type 'string'.".to_owned(),
                    ),
                    (
                        tags[2].0,
                        tags[2].1,
                        "Type '(a: string, b: string) => void' does not satisfy the expected type '(a: string, ...args: number[]) => void'.".to_owned(),
                    ),
                ]
            );
        });
    }

    #[test]
    fn jsdoc_satisfies_missing_property_keeps_relation_chain_and_declaration() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "/**\n\
                     * @typedef {Object} Required\n\
                     * @property {number} required\n\
                     */\n\
                    const value = /** @satisfies {Required} */ ({});\n";
        with_program_state(&[("a.js", text)], &options, |state| {
            state.check_source_file(0);
            let diagnostic = state
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code() == 1360)
                .expect("TS1360");
            fn flatten(chain: &tsrs2_diags::MessageChain, codes: &mut Vec<u32>) {
                codes.push(chain.code);
                for child in &chain.next {
                    flatten(child, codes);
                }
            }
            let mut codes = Vec::new();
            flatten(&diagnostic.message, &mut codes);
            assert_eq!(codes, [1360, 2741]);
            let related = diagnostic.related.first().expect("TS2728");
            assert_eq!(diagnostic.related.len(), 1);
            assert_eq!(related.file_name.as_deref(), Some("a.js"));
            let property_start = text.find("@property").expect("property tag");
            assert_eq!(
                (related.start, related.length),
                (
                    Some(property_start as u32),
                    Some(
                        text[property_start..text.find("*/").expect("JSDoc close")]
                            .encode_utf16()
                            .count() as u32
                    ),
                )
            );
            assert_eq!(related.message.code, 2728);
            assert_eq!(related.message.text, "'required' is declared here.");
        });
    }

    #[test]
    fn jsdoc_satisfies_callable_elaboration_and_nearest_decline_are_both_reported() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "const callable = () => 1;\n\
                    const didYouMean = /** @satisfies {number} */ (callable);\n\
                    const ordinary = /** @satisfies {string} */ (callable);\n";
        with_program_state(&[("a.js", text)], &options, |state| {
            state.check_source_file(0);
            let diagnostics = state
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 1360)
                .collect::<Vec<_>>();
            assert_eq!(diagnostics.len(), 2, "{:#?}", state.diagnostics);
            assert_eq!(
                diagnostics[0]
                    .related
                    .iter()
                    .map(|related| related.message.code)
                    .collect::<Vec<_>>(),
                [6212]
            );
            assert!(
                diagnostics[1]
                    .related
                    .iter()
                    .all(|related| related.message.code != 6212),
                "a non-matching return type must decline did-you-mean elaboration: {:#?}",
                diagnostics[1]
            );
        });
    }

    #[test]
    fn jsdoc_satisfies_semantics_preserves_contextual_object_and_non_tag_faces() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "/**\n\
                     * @typedef {Object} Required\n\
                     * @property {number} value\n\
                     */\n\
                    /** @satisfies {Required} */\n\
                    const contextualMissing = {};\n\
                    const inlineValid = /** @satisfies {Required} */ ({ value: 1 });\n\
                    const inlineExcess = /** @satisfies {Required} */ ({ value: 1, extra: 2 });\n\
                    /** prose `@satisfies {string}` */\n\
                    const prose = 1;\n\
                    /** @satisfiesElse {string} */\n\
                    const boundary = 1;\n\
                    const text = \"/** @satisfies {string} */ (1)\";\n";
        let diagnostics = checked_file_diags_with("a.js", text, &options);
        assert!(
            diagnostics.iter().all(|diagnostic| diagnostic.0 != 1360),
            "contextual/assignable/non-tag faces must not produce TS1360: {diagnostics:?}"
        );
    }

    // ---- M7 8.1r JSDoc cast type-predicate grammar ----

    #[test]
    fn jsdoc_cast_type_predicate_reports_invalid_return_type_position() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "let value;\n\
                    if (/** @type {value is string} */ (value)) {}\n";
        with_program_state(&[("a.js", text)], &options, |state| {
            state.check_source_file(0);
            let diagnostic = state
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code() == 1228)
                .expect("TS1228");
            let start = text.find("value is string").expect("type predicate text") as u32;
            assert_eq!(
                (
                    diagnostic.start,
                    diagnostic.length,
                    diagnostic.message_text(),
                ),
                (
                    Some(start),
                    Some("value is string".len() as u32),
                    "A type predicate is only allowed in return type position for functions and methods.",
                )
            );
        });
    }

    #[test]
    fn jsdoc_cast_type_predicate_preserves_other_type_faces() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "let value;\n\
                    const normal = /** @type {string} */ (value);\n\
                    const boundary = /** @type {value isomorphic} */ (value);\n\
                    const object = /** @type {{ is: string }} */ ({ is: \"\" });\n\
                    const otherTag = /** @types {value is string} */ (value);\n\
                    const text = \"/** @type {value is string} */ (value)\";\n\
                    const prose = /** prose `@type {value is string}` */ (value);\n\
                    const ordinary = /* @type {value is string} */ (value);\n";
        let diagnostics = checked_file_diags_with("a.js", text, &options);
        assert!(
            diagnostics.iter().all(|diagnostic| diagnostic.0 != 1228),
            "non-predicate/non-tag faces must not produce TS1228: {diagnostics:?}"
        );
    }

    // ---- M7 8.1f JSDoc nullable/non-nullable grammar ----

    #[test]
    fn jsdoc_nullable_and_non_nullable_types_report_typescript_suggestions() {
        let options = CompilerOptions {
            strict: Some(true),
            ..CompilerOptions::default()
        };
        assert_eq!(
            checked_diags_with(
                "var a: ?number;\n\
                 var b: number?;\n\
                 var c: !string;\n\
                 var d: string!;\n\
                 var e: ?void;\n",
                &options,
            ),
            [
                (
                    17020,
                    7,
                    7,
                    "'?' at the start of a type is not valid TypeScript syntax. Did you mean to write 'number | null | undefined'?"
                        .to_owned()
                ),
                (
                    17019,
                    23,
                    7,
                    "'?' at the end of a type is not valid TypeScript syntax. Did you mean to write 'number | undefined'?"
                        .to_owned()
                ),
                (
                    17020,
                    39,
                    7,
                    "'!' at the start of a type is not valid TypeScript syntax. Did you mean to write 'string'?"
                        .to_owned()
                ),
                (
                    17019,
                    55,
                    7,
                    "'!' at the end of a type is not valid TypeScript syntax. Did you mean to write 'string'?"
                        .to_owned()
                ),
                (
                    17020,
                    71,
                    5,
                    "'?' at the start of a type is not valid TypeScript syntax. Did you mean to write 'void'?"
                        .to_owned()
                ),
            ]
        );
    }

    // ---- M8-P12 JSDoc-only source type grammar ----

    #[test]
    fn jsdoc_only_source_types_report_8020_at_the_upstream_spans() {
        let text = "interface Array<T> {}\n\
                    var dotted: Array.<number>;\n\
                    var callable: function(this: number, string): string;\n\
                    var all: * = 1;\n\
                    var unknown: ? = undefined;\n\
                    var ordinary: Array<number>;\n";
        let diagnostics = checked_diags(text)
            .into_iter()
            .filter(|diagnostic| diagnostic.0 == 8020)
            .collect::<Vec<_>>();
        let callable = "function(this: number, string): string";
        assert_eq!(
            diagnostics,
            [
                (
                    8020,
                    text.find(".<").expect("JSDoc dot") as u32,
                    1,
                    "JSDoc types can only be used inside documentation comments.".to_owned(),
                ),
                (
                    8020,
                    text.find(callable).expect("JSDoc function type") as u32,
                    callable.len() as u32,
                    "JSDoc types can only be used inside documentation comments.".to_owned(),
                ),
                (
                    8020,
                    text.find("* =").expect("JSDoc all type") as u32,
                    1,
                    "JSDoc types can only be used inside documentation comments.".to_owned(),
                ),
                (
                    8020,
                    text.find("? =").expect("JSDoc unknown type") as u32,
                    1,
                    "JSDoc types can only be used inside documentation comments.".to_owned(),
                ),
            ]
        );
    }

    #[test]
    fn jsdoc_only_source_type_8020_is_silent_in_js_files() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let diagnostics = checked_file_diags_with(
            "a.js",
            "var dotted: Array.<number>;\n\
             var callable: function(this: number, string): string;\n\
             var all: * = 1;\n\
             var unknown: ? = undefined;\n",
            &options,
        );
        assert!(
            diagnostics.iter().all(|diagnostic| diagnostic.0 != 8020),
            "TS8020 is TypeScript-source-only: {diagnostics:?}"
        );
    }

    #[test]
    fn jsdoc_accessibility_on_private_name_uses_tag_span_and_publishes_checked_js() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "\r\nclass A {\r\n    /**\r\n     * @public\r\n     */\r\n    #a = 1;\r\n}\r\n";
        with_program_state(&[("a.js", text)], &options, |state| {
            state.check_source_file(0);
            let diagnostic = state
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code() == 18010)
                .expect("TS18010");
            assert_eq!((diagnostic.start, diagnostic.length), (Some(29), Some(14)));
        });
    }

    #[test]
    fn jsdoc_accessibility_rejects_non_attached_and_non_tag_comments() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "/** @public */\n\
                    class A {\n\
                      #unattached = 1;\n\
                      /** prose `@public` */\n\
                      #prose = 1;\n\
                      /* @private */\n\
                      #ordinary = 1;\n\
                      /** @publicized */\n\
                      #boundary = 1;\n\
                      /** @protected */\n\
                      visible = 1;\n\
                      #intervening = 1;\n\
                    }\n";
        let diagnostics = checked_file_diags_with("a.js", text, &options);
        assert!(
            diagnostics.iter().all(|diagnostic| diagnostic.0 != 18010),
            "negative attachment probes must not produce TS18010: {diagnostics:?}"
        );
    }

    #[test]
    fn jsdoc_import_tag_bare_with_reports_parser_and_checker_diagnostics() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let text = "/** @import * as f from \"./foo\" with */";
        with_program_state(&[("a.js", text)], &options, |state| {
            state.check_source_file(0);
            let diagnostic = state
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code() == 1464)
                .expect("TS1464");
            assert_eq!((diagnostic.start, diagnostic.length), (Some(32), Some(4)));
            let diagnostic = state
                .binder
                .source(0)
                .js_doc_diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code() == 1005)
                .expect("TS1005");
            assert_eq!(
                (
                    diagnostic.start,
                    diagnostic.length,
                    diagnostic.message_text()
                ),
                (Some(37), Some(0), "'{' expected.")
            );
        });
    }

    #[test]
    fn jsdoc_import_tag_rejects_valid_attributes_prose_and_source_text() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        for text in [
            "/** @import * as f from \"./foo\" with { \"resolution-mode\": \"import\" } */",
            "/** prose `@import * as f from \"./foo\" with` */",
            "/** @imported * as f from \"./foo\" with */",
            "/** @import \"./foo\" with */",
            "/* @import * as f from \"./foo\" with */",
            "const text = '/** @import * as f from \"./foo\" with */';",
        ] {
            let diagnostics = checked_file_diags_with("a.js", text, &options);
            assert!(
                diagnostics
                    .iter()
                    .all(|diagnostic| !matches!(diagnostic.0, 1005 | 1464)),
                "negative JSDoc import probe must not produce TS1005/TS1464: {text:?}: {diagnostics:?}"
            );
        }
        let diagnostics = checked_file_diags_with(
            "a.ts",
            "/** @import * as f from \"./foo\" with */",
            &CompilerOptions::default(),
        );
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| !matches!(diagnostic.0, 1005 | 1464)),
            "JSDoc import tags are checked only in JavaScript: {diagnostics:?}"
        );
    }

    // ---- M7 8.1c.2 declaration-file source grammar (oracle-pinned) ----

    #[test]
    fn declaration_file_requires_declare_or_export_on_value_declarations() {
        assert_eq!(
            checked_file_diags_with(
                "a.d.ts",
                "enum E {}\nfunction f(): void;\nclass C {}\n",
                &CompilerOptions::default(),
            ),
            [(
                1046,
                0,
                4,
                "Top-level declarations in .d.ts files must start with either a 'declare' or 'export' modifier."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn declaration_file_allows_type_declarations_and_explicit_value_modifiers() {
        assert_eq!(
            checked_file_diags_with(
                "a.d.ts",
                "interface I {}\ntype T = string;\ndeclare enum E {}\nexport class C {}\nexport default function f(): void;\n",
                &CompilerOptions::default(),
            ),
            []
        );
    }

    // ---- M7 8.1a modifier/decorator grammar (oracle-pinned) ----

    #[test]
    fn modifier_order_reports_the_oracle_span_and_message() {
        assert_eq!(
            checked_diags("abstract class C { abstract public p: string; }"),
            [(
                1029,
                28,
                6,
                "'public' modifier must precede 'abstract' modifier.".to_owned()
            )]
        );
        assert_eq!(
            checked_diags("abstract class C { public abstract p: string; }"),
            []
        );
    }

    #[test]
    fn illegal_static_block_decorator_reports_at_the_at_token() {
        let options = CompilerOptions {
            experimental_decorators: true,
            ..CompilerOptions::default()
        };
        assert_eq!(
            checked_diags_with(
                "declare function dec(...args: any[]): any; class C { @dec static {} }",
                &options,
            ),
            [(1206, 53, 1, "Decorators are not valid here.".to_owned())]
        );
        assert_eq!(
            checked_diags_with(
                "declare function dec(...args: any[]): any; class C { static {} }",
                &options,
            ),
            []
        );
    }

    #[test]
    fn modifier_error_suppresses_function_grammar_followers() {
        let diagnostics = checked_diags("public function f<>() {}");
        assert_eq!(
            diagnostics,
            [(
                1044,
                0,
                6,
                "'public' modifier cannot appear on a module or namespace element.".to_owned()
            )]
        );
        assert_eq!(
            checked_diags("function f<>() {}"),
            [(
                1098,
                10,
                2,
                "Type parameter list cannot be empty.".to_owned()
            )]
        );
    }

    #[test]
    fn decorators_split_by_export_carry_related_information() {
        with_program_state(
            &[(
                "a.ts",
                "declare function dec(value: any): any; @dec export @dec class C {}",
            )],
            &CompilerOptions::default(),
            |state| {
                state.check_source_file(0);
                let diagnostic = state
                    .diagnostics
                    .iter()
                    .find(|diagnostic| diagnostic.code() == 8038)
                    .expect("TS8038");
                assert_eq!((diagnostic.start, diagnostic.length), (Some(51), Some(4)));
                assert_eq!(diagnostic.related.len(), 1);
                let related = &diagnostic.related[0];
                assert_eq!(related.message.code, 1486);
                assert_eq!((related.start, related.length), (Some(39), Some(4)));
            },
        );
    }

    // ---- M7 8.1d.3v regular-expression validator (oracle-pinned) ----

    #[test]
    fn regex_validator_preserves_utf16_positions_and_target_gates() {
        let options = CompilerOptions {
            target: Some(ScriptTarget::ES5.bits()),
            ..CompilerOptions::default()
        };
        let rows = checked_diags_with("const r = /😀{/u;", &options);
        assert!(rows.iter().any(|row| {
            row.0 == 1508
                && row.1 == 13
                && row.2 == 1
                && row.3 == "Unexpected '{'. Did you mean to escape it with backslash?"
        }));
        assert!(rows.iter().any(|row| {
            row.0 == 1501
                && row.1 == 15
                && row.2 == 1
                && row.3
                    == "This regular expression flag is only available when targeting 'es6' or later."
        }));
    }

    #[test]
    fn regex_spelling_message_is_related_to_the_primary() {
        with_program_state(
            &[("a.ts", "const r = /\\p{General_Categor=Letter}/u;")],
            &CompilerOptions::default(),
            |state| {
                state.check_source_file(0);
                let primary = state
                    .diagnostics
                    .iter()
                    .find(|diagnostic| diagnostic.code() == 1524)
                    .expect("TS1524");
                assert_eq!((primary.start, primary.length), (Some(14), Some(15)));
                assert_eq!(primary.related.len(), 1);
                let related = &primary.related[0];
                assert_eq!(related.message.code, 1369);
                assert_eq!(related.message.text, "Did you mean 'General_Category'?");
                assert_eq!(related.file_name, None);
                assert_eq!((related.start, related.length), (Some(14), Some(15)));
            },
        );
    }

    #[test]
    fn regex_validator_is_suppressed_by_any_file_parse_diagnostic() {
        with_program_state_allow_parse_diagnostics(
            &[("a.ts", "const broken = ; const r = /a/z;")],
            &CompilerOptions::default(),
            |state| {
                assert!(!state.binder.source(0).parse_diagnostics.is_empty());
                state.check_source_file(0);
                assert!(
                    state
                        .diagnostics
                        .iter()
                        .all(|diagnostic| diagnostic.code() != 1499),
                    "the unrelated parse diagnostic suppresses regex validation"
                );
            },
        );
    }

    #[test]
    fn regex_validator_publishes_checked_javascript_diagnostics() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        with_program_state(&[("a.js", "const r = /a/z;")], &options, |state| {
            state.check_source_file(0);
            let diagnostic = state
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code() == 1499)
                .expect("TS1499");
            assert!(diagnostic.start.is_some());
            assert!(diagnostic.length.is_some());
        });
    }

    // ---- deferred containment (tsrs-native, 7.4 review rework) ----

    fn node_of_kind(state: &CheckerState, kind: tsrs2_syntax::SyntaxKind) -> tsrs2_syntax::NodeId {
        let source = state.binder.source(0);
        source
            .arena
            .node_ids()
            .find(|&id| source.arena.node(id).kind == kind)
            .unwrap_or_else(|| panic!("no {kind:?} in fixture"))
    }

    #[test]
    fn deferred_containment_skip_requires_the_containment_record() {
        with_program_state(
            &[(
                "a.ts",
                "declare function outer(f: (x: number) => void): void;\nouter(x => {});\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let arrow = node_of_kind(state, tsrs2_syntax::SyntaxKind::ArrowFunction);
                let call = node_of_kind(state, tsrs2_syntax::SyntaxKind::CallExpression);
                state
                    .partially_checked_ranges
                    .entry(0)
                    .or_default()
                    .push((0, u32::MAX));
                // A Vacant ancestor slot WITHOUT the containment record
                // is the benign mid-fixpoint clear (tsc 77505 `: cached`
                // on a loop-dirty fresh frame) — fully re-resolvable, so
                // the deferred check must run.
                assert!(
                    !state.deferred_context_call_reverted(arrow),
                    "benign Vacant must not trigger the containment skip"
                );
                state.contained_call_resolutions.insert(call);
                assert!(
                    state.deferred_context_call_reverted(arrow),
                    "containment-reverted Vacant triggers the skip"
                );
            },
        );
    }

    #[test]
    fn deferred_containment_sees_jsx_children_through_the_opening_element() {
        let options = CompilerOptions {
            jsx: Some(2),
            ..CompilerOptions::default()
        };
        with_program_state(
            &[(
                "a.tsx",
                "declare var React: any;\nconst e = <div>{() => 1}</div>;\n",
            )],
            &options,
            |state| {
                let arrow = node_of_kind(state, tsrs2_syntax::SyntaxKind::ArrowFunction);
                let opening = node_of_kind(state, tsrs2_syntax::SyntaxKind::JsxOpeningElement);
                state
                    .partially_checked_ranges
                    .entry(0)
                    .or_default()
                    .push((0, u32::MAX));
                assert!(!state.deferred_context_call_reverted(arrow));
                // The resolvedSignature slot lives on the OPENING
                // element — a SIBLING subtree of the children, which an
                // ancestor walk can only reach through the JsxElement
                // hop (the pre-review walk missed it).
                state.contained_call_resolutions.insert(opening);
                assert!(
                    state.deferred_context_call_reverted(arrow),
                    "children resolve the slot through JsxElement.opening_element"
                );
            },
        );
    }

    #[test]
    fn deferred_containment_sees_jsx_fragment_children_through_the_opening_fragment() {
        let options = CompilerOptions {
            jsx: Some(2),
            ..CompilerOptions::default()
        };
        with_program_state(
            &[(
                "a.tsx",
                "declare var React: any;\nconst e = <>{() => 1}</>;\n",
            )],
            &options,
            |state| {
                let arrow = node_of_kind(state, tsrs2_syntax::SyntaxKind::ArrowFunction);
                let opening = node_of_kind(state, tsrs2_syntax::SyntaxKind::JsxOpeningFragment);
                state
                    .partially_checked_ranges
                    .entry(0)
                    .or_default()
                    .push((0, u32::MAX));
                assert!(!state.deferred_context_call_reverted(arrow));
                // JsxOpeningFragment is a LEAF — the pre-review walk
                // listed it directly and could never match; the
                // JsxFragment hop is the reachable route.
                state.contained_call_resolutions.insert(opening);
                assert!(
                    state.deferred_context_call_reverted(arrow),
                    "fragment children resolve the slot through JsxFragment.opening_fragment"
                );
            },
        );
    }

    // ---- 2636 / 2637 (checkTypeParameterDeferred) — oracle-pinned ----

    #[test]
    fn interface_out_annotation_on_contravariant_use_reports_2636() {
        let diags = checked_diags("interface Foo<out T> { f: (x: T) => void }\n");
        assert_eq!(
            diags,
            [(
                2636,
                14,
                5,
                "Type 'Foo<sub-T>' is not assignable to type 'Foo<super-T>' as implied by \
                 variance annotation."
                    .to_owned()
            )]
        );
    }

    // ---- tuple renderer (phase-9 9.3a) — every head oracle-probed
    // (scratchpad probe-93a: noLib strict, vendored 6.0.3) ----

    #[test]
    fn tuple_display_labeled_members_render() {
        assert_eq!(
            checked_diags("declare const p: [a: number, b: string];\nconst q: [number] = p;\n"),
            [(
                2322,
                47,
                1,
                "Type '[a: number, b: string]' is not assignable to type '[number]'.".to_owned()
            )]
        );
    }

    #[test]
    fn tuple_display_optional_element_parenthesizes_the_union() {
        // The stored optional element is `string | undefined` (strict,
        // eOPT off) — OptionalTypeNode's postfix parenthesizer wraps
        // it: `[(string | undefined)?]`.
        assert_eq!(
            checked_diags("declare const o: [string?];\nconst n: [number] = o;\n"),
            [(
                2322,
                34,
                1,
                "Type '[(string | undefined)?]' is not assignable to type '[number]'.".to_owned()
            )]
        );
    }

    #[test]
    fn tuple_display_labeled_optional_member_is_unparenthesized() {
        // NamedTupleMember types never parenthesize (factory
        // 22247-22256 applies no rule): `a?: number | undefined`.
        assert_eq!(
            checked_diags("declare const p2: [a?: number];\nconst q2: [string] = p2;\n"),
            [(
                2322,
                38,
                2,
                "Type '[a?: number | undefined]' is not assignable to type '[string]'.".to_owned()
            )]
        );
    }

    #[test]
    fn tuple_display_rest_and_variadic_elements_render() {
        assert_eq!(
            checked_diags("declare const r: [number, ...string[]];\nconst n: [boolean] = r;\n"),
            [(
                2322,
                46,
                1,
                "Type '[number, ...string[]]' is not assignable to type '[boolean]'.".to_owned()
            )]
        );
        // Rest-element unions parenthesize through the ArrayTypeNode
        // wrap: `...(string | boolean)[]`.
        assert_eq!(
            checked_diags(
                "declare const r: [number, ...(string | boolean)[]];\nconst n: [number] = r;\n"
            ),
            [(
                2322,
                58,
                1,
                "Type '[number, ...(string | boolean)[]]' is not assignable to type '[number]'."
                    .to_owned()
            )]
        );
        // A generic variadic element renders bare: `...T`.
        assert_eq!(
            checked_diags(
                "function f2<T extends unknown[]>(...args: [string, ...T]) { const x: [number] = args; }\n"
            ),
            [(
                2322,
                66,
                1,
                "Type '[string, ...T]' is not assignable to type '[number]'.".to_owned()
            )]
        );
    }

    #[test]
    fn return_satisfies_operand_elaborates_the_element() {
        // PR #55 review P1: tsc passes the EFFECTIVE check node into
        // checkTypeAssignableToAndOptionallyElaborate (84585-84587) —
        // satisfies strips off, the array literal elaborates, and the
        // element row REPLACES the outer return head.
        assert_eq!(
            checked_diags("function f(): [string] {\n  return ([1] satisfies [number]);\n}\n"),
            [(
                2322,
                36,
                1,
                "Type 'number' is not assignable to type 'string'.".to_owned()
            )]
        );
    }

    #[test]
    fn enum_member_displays_render_qualified() {
        // PR #55 review P1: enum-member literal types print `E.A`
        // (typeToTypeNodeHelper's EnumLike arm, 51367-51399), never
        // their base literal value.
        assert_eq!(
            checked_diags("enum E { A, B }\ndeclare const x: [E.A];\nconst y: [E.B] = x;\n"),
            [(
                2322,
                46,
                1,
                "Type '[E.A]' is not assignable to type '[E.B]'.".to_owned()
            )]
        );
        assert_eq!(
            checked_diags("const enum C { X, Y }\ndeclare const x: [C.X];\nconst y: [C.Y] = x;\n"),
            [(
                2322,
                52,
                1,
                "Type '[C.X]' is not assignable to type '[C.Y]'.".to_owned()
            )]
        );
        // The 51371 single-member collapse: the member type IS the
        // declared type, so the bare enum name prints.
        assert_eq!(
            checked_diags("enum S { Only }\ndeclare const x: [S.Only];\nconst y: [string] = x;\n"),
            [(
                2322,
                49,
                1,
                "Type '[S]' is not assignable to type '[string]'.".to_owned()
            )]
        );
        // The EnumLiteral-stamped declared union prints the enum name
        // BEFORE the union walk.
        assert_eq!(
            checked_diags("enum E { A, B }\ndeclare const x: [E];\nconst y: [string] = x;\n"),
            [(
                2322,
                44,
                1,
                "Type '[E]' is not assignable to type '[string]'.".to_owned()
            )]
        );
        // Mixed unions keep interned order (string interns first).
        assert_eq!(
            checked_diags(
                "enum E { A, B }\ndeclare const x: [E.A | string];\nconst y: [boolean] = x;\n"
            ),
            [(
                2322,
                55,
                1,
                "Type '[string | E.A]' is not assignable to type '[boolean]'.".to_owned()
            )]
        );
        // A BARE enum-literal source generalizes to its base for the
        // head (reportRelationError's literal-source generalization
        // composes with the arm): 'E', not 'E.A'.
        assert_eq!(
            checked_diags("enum E { A, B }\ndeclare const x: E.A;\nconst y: [string] = x;\n"),
            [(
                2322,
                44,
                1,
                "Type 'E' is not assignable to type '[string]'.".to_owned()
            )]
        );
    }

    #[test]
    fn relation_report_normalizes_fresh_enum_member_sources() {
        // isRelatedTo normalizes a fresh literal before handing the
        // failed pair to reportErrorResults. For a single-member enum,
        // that regular member IS the declared enum and prints bare.
        assert_eq!(
            checked_diags("enum S { Only }\ndeclare let u: undefined;\nu = S.Only;\n"),
            [(
                2322,
                42,
                1,
                "Type 'S' is not assignable to type 'undefined'.".to_owned()
            )]
        );
        // Non-firing sibling: a member of a multi-member enum remains
        // qualified because its regular twin is not the enum union.
        assert_eq!(
            checked_diags("enum E { A, B }\ndeclare let u: undefined;\nu = E.A;\n"),
            [(
                2322,
                42,
                1,
                "Type 'E.A' is not assignable to type 'undefined'.".to_owned()
            )]
        );
    }

    #[test]
    fn tuple_display_empty_and_readonly_render() {
        assert_eq!(
            checked_diags("declare const e: [];\nconst n2: [number] = e;\n"),
            [(
                2322,
                27,
                2,
                "Type '[]' is not assignable to type '[number]'.".to_owned()
            )]
        );
        // The readonly TypeOperator wrap rides the 4104 face
        // (tryElaborateArrayLikeErrors' readonly report).
        assert_eq!(
            checked_diags(
                "declare const r: readonly [string, number];\nlet w: [string, number] = r as any;\nw = r;\n"
            ),
            [(
                4104,
                80,
                1,
                "The type 'readonly [string, number]' is 'readonly' and cannot be assigned to \
                 the mutable type '[string, number]'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn relation_report_elaborates_read_normalized_readonly_source() {
        assert_eq!(
            checked_chain_codes(
                "function f<T extends readonly [unknown]>(source: T, target: [...T]) {\n\
                     target = source;\n\
                 }\n"
            ),
            [[2322, 4104]]
        );
    }

    // ---- 9.3b anonymous-object display pins (oracle-probed,
    // scratchpad probe-93b-pins-final: noLib + strict + noImplicitAny
    // matching the unit env) ----

    #[test]
    fn anonymous_object_display_basic_members_render() {
        assert_eq!(
            checked_diags("declare let a: { x: string; y: number };\na = 1;\n"),
            [(
                2322,
                41,
                1,
                "Type 'number' is not assignable to type '{ x: string; y: number; }'.".to_owned()
            )]
        );
    }

    #[test]
    fn type_display_truncation_state_is_sticky_across_alias_arguments() {
        let short = "type Defaultize<T, D> = T & D;\n\
                     declare let target: Defaultize<{ \
                     property0: number; property1: number; property2: number; \
                     property3: number; property4: number; property5: number; \
                     }, { tail: number }>;\n\
                     target = 1;\n";
        let long = "type Defaultize<T, D> = T & D;\n\
                    declare let target: Defaultize<{ \
                    property0: number; property1: number; property2: number; \
                    property3: number; property4: number; property5: number; \
                    property6: number; property7: number; property8: number; \
                    property9: number; \
                    }, { tail: number }>;\n\
                    target = 1;\n";
        let message = |text| {
            checked_diags(text)
                .into_iter()
                .find(|row| row.0 == 2322)
                .expect("assignment diagnostic")
                .3
        };
        assert_eq!(
            message(short),
            "Type 'number' is not assignable to type \
             'Defaultize<{ property0: number; property1: number; property2: number; \
             property3: number; property4: number; property5: number; }, \
             { tail: number; }>'."
        );
        assert_eq!(
            message(long),
            "Type 'number' is not assignable to type \
             'Defaultize<{ property0: number; property1: number; property2: number; \
             property3: number; property4: number; property5: number; property6: number; \
             property7: number; property8: number; property9: number; }, { ...; }>'."
        );
        let options = CompilerOptions {
            no_error_truncation: Some(true),
            ..CompilerOptions::default()
        };
        assert_eq!(
            checked_diags_with(long, &options)
                .into_iter()
                .find(|row| row.0 == 2322)
                .expect("assignment diagnostic")
                .3,
            "Type 'number' is not assignable to type \
             'Defaultize<{ property0: number; property1: number; property2: number; \
             property3: number; property4: number; property5: number; property6: number; \
             property7: number; property8: number; property9: number; }, \
             { tail: number; }>'."
        );
    }

    #[test]
    fn anonymous_object_display_optional_readonly_member() {
        // The optional member's declared type keeps its undefined tail
        // (strict, eOPT off): `readonly y?: number | undefined`.
        assert_eq!(
            checked_diags("declare let b: { readonly y?: number; z: string };\nb = 1;\n"),
            [(
                2322,
                51,
                1,
                "Type 'number' is not assignable to type \
                 '{ readonly y?: number | undefined; z: string; }'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn anonymous_object_display_property_name_faces() {
        // Quoted names keep their declared quote style, identifier-able
        // and numeric names print bare, non-canonical numeric strings
        // stay quoted ("1e2").
        assert_eq!(
            checked_diags(
                "declare let c: { \"a b\": string; 'c d': number; 1: boolean; \"1e2\": string };\nc = 1;\n"
            ),
            [(
                2322,
                76,
                1,
                "Type 'number' is not assignable to type \
                 '{ \"a b\": string; 'c d': number; 1: boolean; \"1e2\": string; }'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn anonymous_object_display_index_signatures_precede_properties() {
        assert_eq!(
            checked_diags(
                "declare let d: { p: boolean; [idx: number]: unknown; [k: string]: unknown };\nd = 1;\n"
            ),
            [(
                2322,
                77,
                1,
                "Type 'number' is not assignable to type \
                 '{ [idx: number]: unknown; [k: string]: unknown; p: boolean; }'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn concrete_mapped_source_displays_as_a_resolved_index_signature() {
        let text = "function f<K extends string>(a: { [P in K]: number }, b: { [P in string]: number }) { a = b; }\n";
        assert_eq!(
            checked_diags(text),
            [(
                2322,
                text.find("a = b").expect("assignment") as u32,
                1,
                "Type '{ [x: string]: number; }' is not assignable to type \
                 '{ [P in K]: number; }'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn error_containing_concrete_mapped_type_keeps_its_declaration_face() {
        with_program_state(
            &[("a.ts", "declare let value: { [P in string]: number };\n")],
            &CompilerOptions::default(),
            |state| {
                let mapped_node = node_of_kind(state, tsrs2_syntax::SyntaxKind::MappedType);
                let mapped_type = state
                    .get_type_from_type_node(mapped_node)
                    .expect("mapped type");
                assert!(!state
                    .is_generic_mapped_type_state(mapped_type)
                    .expect("genericity"));
                state
                    .links
                    .set_mapped_contains_error(state.speculation_depth, mapped_type);
                assert_eq!(
                    state
                        .type_to_string_slice(mapped_type)
                        .expect("mapped display"),
                    "{ [P in string]: number; }"
                );
            },
        );
    }

    #[test]
    fn anonymous_object_display_nested_literal_and_union() {
        assert_eq!(
            checked_diags("declare let e: { a: { b: string | undefined } };\ne = 1;\n"),
            [(
                2322,
                49,
                1,
                "Type 'number' is not assignable to type '{ a: { b: string | undefined; }; }'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn anonymous_object_display_same_type_accessor_collapses_to_property() {
        // addPropertyToElementList's accessor fall-through: same
        // read/write type, non-class parent -> the plain property row.
        assert_eq!(
            checked_diags("declare let f: { get p(): string; set p(v: string) };\nf = 1;\n"),
            [(
                2322,
                54,
                1,
                "Type 'number' is not assignable to type '{ p: string; }'.".to_owned()
            )]
        );
    }

    #[test]
    fn anonymous_object_display_method_member_renders() {
        // 9.3b2 signature rung: the method face renders
        // (oracle-probed byte-exact).
        assert_eq!(
            checked_diags("declare let g: { m(): void };\ng = 1;\n"),
            [(
                2322,
                30,
                1,
                "Type 'number' is not assignable to type '{ m(): void; }'.".to_owned()
            )]
        );
    }

    #[test]
    fn checked_js_empty_object_literal_renders_the_empty_face() {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            no_implicit_any: Some(true),
            strict_null_checks: Some(true),
            ..CompilerOptions::default()
        };
        with_program_state(
            &[("a.js", "function f(a = null) { a = {}; }\n")],
            &options,
            |state| {
                state.check_source_file(0);
                assert_eq!(
                    diag_rows(state),
                    [(
                        2322,
                        23,
                        1,
                        "Type '{}' is not assignable to type 'null'.".to_owned()
                    )]
                );
            },
        );
    }

    #[test]
    fn jsdoc_intended_object_type_rewrites_only_with_implicit_any_off() {
        let files = [
            (
                "lib.d.ts",
                "interface Object {}\ninterface Array<T> { length: number; [n: number]: T }\n",
            ),
            (
                "a.js",
                "/** @param {Array.<Object>} values */\n\
                 const f = function(values) {};\n\
                 /** @type {string} */\n\
                 let s = f;\n",
            ),
        ];
        let rows = |no_implicit_any| {
            program_diags_with(
                &files,
                &CompilerOptions {
                    allow_js: true,
                    check_js: Some(true),
                    strict: Some(false),
                    no_implicit_any: Some(no_implicit_any),
                    ..CompilerOptions::default()
                },
                "/",
            )
        };

        assert_eq!(
            rows(false),
            [(
                "a.js".to_owned(),
                2322,
                95,
                1,
                "Type '(values: Array<any>) => void' is not assignable to type 'string'."
                    .to_owned()
            )]
        );
        assert_eq!(
            rows(true),
            [(
                "a.js".to_owned(),
                2322,
                95,
                1,
                "Type '(values: Array<Object>) => void' is not assignable to type 'string'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn checked_js_async_arrow_argument_renders_promise_signature() {
        // asyncArrowFunction_allowJs.ts's virtual file, byte-for-byte:
        // the failed callback relation must display the ordinary
        // checked-JS arrow structurally on createAnonymousTypeNode's
        // non-isJSConstructor path.
        let text = concat!(
            "\r\n",
            "// Error (good)\r\n",
            "/** @type {function(): string} */\r\n",
            "const a = () => 0\r\n",
            "\r\n",
            "// Error (good)\r\n",
            "/** @type {function(): string} */\r\n",
            "const b = async () => 0\r\n",
            "\r\n",
            "// No error (bad)\r\n",
            "/** @type {function(): string} */\r\n",
            "const c = async () => {\r\n",
            "\treturn 0\r\n",
            "}\r\n",
            "\r\n",
            "// Error (good)\r\n",
            "/** @type {function(): string} */\r\n",
            "const d = async () => {\r\n",
            "\treturn \"\"\r\n",
            "}\r\n",
            "\r\n",
            "/** @type {function(function(): string): void} */\r\n",
            "const f = (p) => {}\r\n",
            "\r\n",
            "// Error (good)\r\n",
            "f(async () => {\r\n",
            "\treturn 0\r\n",
            "})",
        );
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            no_emit: Some(true),
            target: Some(ScriptTarget::ES2017.bits()),
            ..CompilerOptions::default()
        };
        with_program_state(
            &[
                ("globals.d.ts", "interface Promise<T> {}\n"),
                ("file.js", text),
            ],
            &options,
            |state| {
                state.check_source_file(1);
                let rows = diag_rows(state)
                    .into_iter()
                    .filter(|row| row.0 == 2345)
                    .collect::<Vec<_>>();
                assert_eq!(
                    rows,
                    [(
                        2345,
                        436,
                        13,
                        "Argument of type '() => Promise<number>' is not assignable to parameter \
                         of type '() => string'."
                            .to_owned(),
                    )]
                );
            },
        );
    }

    #[test]
    fn checked_js_function_type_tag_relations_render_function_signatures() {
        // checkJsdocTypeTag6.ts's virtual file, byte-for-byte. These
        // are a function expression plus all three `more` declaration
        // forms; none is a JS constructor, so all four source types
        // take the structural signature face.
        let text = concat!(
            "\n",
            "/** @type {number} */\n",
            "function f() {\n",
            "    return 1\n",
            "}\n",
            "\n",
            "/** @type {{ prop: string }} */\n",
            "var g = function (prop) {\n",
            "}\n",
            "\n",
            "/** @type {(a: number) => number} */\n",
            "function add1(a, b) { return a + b; }\n",
            "\n",
            "/** @type {(a: number, b: number) => number} */\n",
            "function add2(a, b) { return a + b; }\n",
            "\n",
            "// TODO: Should be an error since signature doesn't match.\n",
            "/** @type {(a: number, b: number, c: number) => number} */\n",
            "function add3(a, b) { return a + b; }\n",
            "\n",
            "// Confirm initializers are compatible.\n",
            "// They can't have more parameters than the type/context.\n",
            "\n",
            "/** @type {() => void} */\n",
            "function funcWithMoreParameters(more) {} // error\n",
            "\n",
            "/** @type {() => void} */\n",
            "const variableWithMoreParameters = function (more) {}; // error\n",
            "\n",
            "/** @type {() => void} */\n",
            "const arrowWithMoreParameters = (more) => {}; // error\n",
            "\n",
            "({\n",
            "  /** @type {() => void} */\n",
            "  methodWithMoreParameters(more) {}, // error\n",
            "});\n",
        );
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            no_emit: Some(true),
            strict: Some(false),
            target: Some(ScriptTarget::ES2015.bits()),
            ..CompilerOptions::default()
        };
        let rows = checked_file_diags_with("test.js", text, &options)
            .into_iter()
            .filter(|row| row.0 == 2322)
            .collect::<Vec<_>>();
        assert_eq!(
            rows,
            [
                (
                    2322,
                    90,
                    1,
                    "Type '(prop: any) => void' is not assignable to type '{ prop: string; }'."
                        .to_owned(),
                ),
                (
                    2322,
                    643,
                    26,
                    "Type '(more: any) => void' is not assignable to type '() => void'.".to_owned(),
                ),
                (
                    2322,
                    734,
                    23,
                    "Type '(more: any) => void' is not assignable to type '() => void'.".to_owned(),
                ),
                (
                    2322,
                    817,
                    24,
                    "Type '(more: any) => void' is not assignable to type '() => void'.".to_owned(),
                ),
            ]
        );
    }

    #[test]
    fn checked_js_constructor_keeps_the_symbol_value_face() {
        // Nearest non-firing sibling: @class makes the function an
        // actual isJSConstructor. It must not fall through to `() =>
        // void`; createAnonymousTypeNode renders symbolToTypeNode
        // under Value meaning.
        let text = "/** @class */\nfunction C() {}\nlet target = \"\";\ntarget = C;\n";
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let rows = checked_file_diags_with("constructor.js", text, &options)
            .into_iter()
            .filter(|row| row.0 == 2322)
            .collect::<Vec<_>>();
        assert_eq!(
            rows,
            [(
                2322,
                text.rfind("target").expect("failing assignment") as u32,
                "target".len() as u32,
                "Type 'typeof C' is not assignable to type 'string'.".to_owned(),
            )]
        );
    }

    // ---- 9.3b2 signature-rung display pins (all byte-exact against
    // strict-mode oracle probes; scratchpad probe-93b2-pins) ----

    #[test]
    fn signature_display_optional_parameter_structural() {
        // declare-let sources render structurally: the optional
        // parameter's symbol type carries `| undefined`.
        assert_eq!(
            checked_diags("declare let f: (x?: number) => void;\nlet t1: string = f;\n"),
            [(
                2322,
                41,
                2,
                "Type '(x?: number | undefined) => void' is not assignable to type 'string'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn signature_display_optional_parameter_annotation_reuse() {
        // The fn-expression twin arms the annotation-reuse channel
        // (getTypeNamesForErrorDisplay's context-sensitive enclosing):
        // the annotation `number` prints without `| undefined`.
        assert_eq!(
            checked_diags("let g = (x?: number) => {};\nlet t2: string = g;\n"),
            [(
                2322,
                32,
                2,
                "Type '(x?: number) => void' is not assignable to type 'string'.".to_owned()
            )]
        );
    }

    #[test]
    fn signature_display_initializer_parameters_use_minimum_arity() {
        let options = CompilerOptions {
            strict: Some(false),
            ..CompilerOptions::default()
        };
        let text = "var v = <T>() => 1;\nvar v = <T>(a = 1, b = 2) => 1;\n";
        assert_eq!(
            checked_diags_with(text, &options),
            [(
                2403,
                text.rfind('v').expect("second declaration") as u32,
                1,
                "Subsequent variable declarations must have the same type.  Variable 'v' must be of type '<T>() => number', but here has type '<T>(a?: number, b?: number) => number'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn signature_display_required_parameters_remain_required() {
        let options = CompilerOptions {
            strict: Some(false),
            ..CompilerOptions::default()
        };
        let text = "var v = <T>() => 1;\nvar v = <T>(a: number, b: number) => 1;\n";
        assert_eq!(
            checked_diags_with(text, &options),
            [(
                2403,
                text.rfind('v').expect("second declaration") as u32,
                1,
                "Subsequent variable declarations must have the same type.  Variable 'v' must be of type '<T>() => number', but here has type '<T>(a: number, b: number) => number'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn signature_display_generic_constraint_and_default() {
        assert_eq!(
            checked_diags(
                "declare let f: <T extends string = \"a\">(x: T) => T;\nlet t3: string = f;\n"
            ),
            [(
                2322,
                69,
                1,
                "Type '<T extends string = \"a\">(x: T) => T' is not assignable to type 'string'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn signature_display_abstract_construct_shorthand() {
        assert_eq!(
            checked_diags(
                "interface D { d: number }\ndeclare let f: abstract new () => D;\nlet t4: string = f;\n"
            ),
            [(
                2322,
                67,
                2,
                "Type 'abstract new () => D' is not assignable to type 'string'.".to_owned()
            )]
        );
    }

    #[test]
    fn signature_display_member_order_call_index_property() {
        // createTypeNodesFromResolvedType order: call signatures,
        // construct signatures, index signatures, properties.
        assert_eq!(
            checked_diags(
                "declare let o: { (x: string): void; [k: string]: number; p: 3 };\nlet t5: string = o;\n"
            ),
            [(
                2322,
                69,
                2,
                "Type '{ (x: string): void; [k: string]: number; p: 3; }' is not assignable to type 'string'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn signature_display_diverging_accessor_faces() {
        assert_eq!(
            checked_diags(
                "declare let o: { get p(): number, set p(v: string) };\nlet t6: string = o;\n"
            ),
            [(
                2322,
                58,
                2,
                "Type '{ get p(): number; set p(v: string); }' is not assignable to type 'string'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn signature_display_overloaded_optional_method_members() {
        assert_eq!(
            checked_diags(
                "declare let o: { m?(): void; m?(x: 1): void; p: 2 };\nlet t7: string = o;\n"
            ),
            [(
                2322,
                57,
                2,
                "Type '{ m?(): void; m?(x: 1): void; p: 2; }' is not assignable to type 'string'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn signature_display_tuple_rest_expansion() {
        // getExpandedParameters: optional tuple members expand with
        // `?` and the strict `| undefined` element type.
        assert_eq!(
            checked_diags(
                "declare let f: (...args: [number, string?]) => void;\nlet t8: string = f;\n"
            ),
            [(
                2322,
                57,
                2,
                "Type '(args_0: number, args_1?: string | undefined) => void' is not assignable to type 'string'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn signature_display_labeled_tuple_rest_expansion() {
        assert_eq!(
            checked_diags(
                "declare let f: (...args: [a: number, b: string]) => void;\nlet t9: string = f;\n"
            ),
            [(
                2322,
                62,
                2,
                "Type '(a: number, b: string) => void' is not assignable to type 'string'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn signature_display_middle_rest_keeps_declared_list() {
        // 52519-52523: a mid-list REST-flagged expanded face falls
        // back to the declared parameter list.
        assert_eq!(
            checked_diags(
                "declare let f: (...args: [number, ...string[], boolean]) => void;\nlet t23: string = f;\n"
            ),
            [(
                2322,
                70,
                3,
                "Type '(...args: [number, ...string[], boolean]) => void' is not assignable to type 'string'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn signature_display_binding_pattern_with_annotation_reuse() {
        // Pattern name + reused parenthesized annotation compose.
        assert_eq!(
            checked_diags("let g = ({ a }: ({ a: (number) })) => {};\nlet t10: string = g;\n"),
            [(
                2322,
                46,
                3,
                "Type '({ a }: ({ a: (number); })) => void' is not assignable to type 'string'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn signature_display_asserts_predicate_return() {
        assert_eq!(
            checked_diags(
                "declare let f: (x: unknown) => asserts x is string;\nlet t11: string = f;\n"
            ),
            [(
                2322,
                56,
                3,
                "Type '(x: unknown) => asserts x is string' is not assignable to type 'string'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn signature_display_union_wraps_function_type() {
        assert_eq!(
            checked_diags("declare let f: (() => void) | null;\nlet t12: string = f;\n"),
            [(
                2322,
                40,
                3,
                "Type '(() => void) | null' is not assignable to type 'string'.".to_owned()
            )]
        );
    }

    #[test]
    fn signature_display_optional_tuple_wraps_function_union() {
        assert_eq!(
            checked_diags("declare let f: [(() => void)?];\nlet t13: string = f;\n"),
            [(
                2322,
                36,
                3,
                "Type '[((() => void) | undefined)?]' is not assignable to type 'string'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn signature_display_this_parameter_unshifts() {
        assert_eq!(
            checked_diags(
                "interface W { w: number }\ndeclare let f: (this: W, x: number) => void;\nlet t14: string = f;\n"
            ),
            [(
                2322,
                75,
                3,
                "Type '(this: W, x: number) => void' is not assignable to type 'string'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn signature_display_constraint_annotation_reuse_keeps_alias() {
        // The constraint face rides the REUSABLE-node path even
        // without an enclosing declaration (52832-52834): the alias
        // spelling survives where param/return positions resolve.
        assert_eq!(
            checked_diags(
                "type AB = \"a\" | \"b\";\ndeclare let f: <T extends AB>(x: T) => T;\nlet t15: string = f;\n"
            ),
            [(
                2322,
                81,
                1,
                "Type '<T extends AB>(x: T) => T' is not assignable to type 'string'.".to_owned()
            )]
        );
    }

    #[test]
    fn signature_display_context_sensitive_source_stays_structural() {
        // A context-sensitive fn expression gets NO enclosing
        // (symbolValueDeclarationIsContextSensitive) — nothing to
        // reuse; the noImplicitAny 7006 rides along.
        assert_eq!(
            checked_diags("let g = (x) => x;\nlet t16: string = g;\n"),
            [
                (
                    7006,
                    9,
                    1,
                    "Parameter 'x' implicitly has an 'any' type.".to_owned()
                ),
                (
                    2322,
                    22,
                    3,
                    "Type '(x: any) => any' is not assignable to type 'string'.".to_owned()
                )
            ]
        );
    }

    #[test]
    fn signature_display_setter_face_param_union() {
        assert_eq!(
            checked_diags(
                "declare let o: { get p(): string; set p(v: string | number) };\nlet t22: string = o;\n"
            ),
            [(
                2322,
                67,
                3,
                "Type '{ get p(): string; set p(v: string | number); }' is not assignable to type 'string'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn signature_display_rest_tuple_expansion_beats_annotation_reuse() {
        // The expanded transient faces carry no declarations, so the
        // parenthesized rest annotation cannot reuse.
        assert_eq!(
            checked_diags("let g = (...args: ([number, string])) => {};\nlet t24: string = g;\n"),
            [(
                2322,
                49,
                3,
                "Type '(args_0: number, args_1: string) => void' is not assignable to type 'string'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn signature_display_return_annotation_reuse_keeps_parens() {
        assert_eq!(
            checked_diags(
                "let g = function (x: number): (string) { return \"s\" };\nlet t25: string = g;\n"
            ),
            [(
                2322,
                73,
                1,
                "Type '(x: number) => (string)' is not assignable to type 'string'.".to_owned()
            )]
        );
    }

    // ---- 9.3b2 fabrication-audit pins (shouldReportUnmatchedPropertyError,
    // elaborateArrowFunction, expando suppression) ----

    #[test]
    fn signature_shaped_source_keeps_the_headless_relation_row() {
        // shouldReportUnmatchedPropertyError (67043): a property-less
        // callable source against a non-callable-shaped target keeps
        // the plain head — no 2741 missing-property face.
        assert_eq!(
            checked_diags(
                "interface T { f(x: number): void }\ndeclare var t: T;\nt = (x: string) => 1;\n"
            ),
            [(
                2322,
                53,
                1,
                "Type '(x: string) => number' is not assignable to type 'T'.".to_owned()
            )]
        );
    }

    #[test]
    fn signature_shaped_source_vs_callable_target_reports_missing_property() {
        // The gate's TRUE branch: both sides callable — the missing
        // property reports.
        assert_eq!(
            checked_diags(
                "interface U { (): void; p: number }\ndeclare var src: { (): void };\ndeclare var u: U;\nu = src;\n"
            ),
            [(
                2741,
                85,
                1,
                "Property 'p' is missing in type '() => void' but required in type 'U'.".to_owned()
            )]
        );
    }

    #[test]
    fn arrow_source_elaborates_the_return_position() {
        // elaborateArrowFunction: the row lands on the body
        // expression, not the declaration name.
        assert_eq!(
            checked_diags("var aLambda: (x: string) => number = (x) => 'a str';\n"),
            [(
                2322,
                44,
                7,
                "Type 'string' is not assignable to type 'number'.".to_owned()
            )]
        );
    }

    #[test]
    fn member_arrow_elaborates_through_the_paren_comma_body() {
        // The member walk's inner recursion declines through
        // paren→comma→undefined, then the report anchors at the
        // arrow's return expression (the parenthesized body).
        assert_eq!(
            checked_diags(
                "type OT = { x: (p: number) => string };\nvar obj1: OT = { x: x => (x, undefined) };\n"
            ),
            [
                (
                    2695,
                    66,
                    1,
                    "Left side of comma operator is unused and has no side effects.".to_owned()
                ),
                (
                    2322,
                    65,
                    14,
                    "Type 'undefined' is not assignable to type 'string'.".to_owned()
                )
            ]
        );
    }

    #[test]
    fn block_body_arrow_keeps_the_declaration_head() {
        assert_eq!(
            checked_diags("var aL2: (x: string) => number = (x) => { return 'a'; };\n"),
            [(
                2322,
                4,
                3,
                "Type '(x: string) => string' is not assignable to type '(x: string) => number'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn annotated_param_arrow_keeps_the_declaration_head() {
        assert_eq!(
            checked_diags("var aL3: (x: string) => number = (x: string) => 'a';\n"),
            [(
                2322,
                4,
                3,
                "Type '(x: string) => string' is not assignable to type '(x: string) => number'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn ts_expando_function_members_resolve_normally() {
        // The assignment declaration is a real export of the function
        // symbol, so both the assignment and read use the normal member
        // path without a diagnostic-side exception.
        assert_eq!(
            checked_diags("function foo() {}\nfoo.x = 1;\nvar q0: number = foo.x;\n"),
            []
        );
    }

    #[test]
    fn class_static_assignments_still_report_2339() {
        // The control: classes are NOT expando parents — the real
        // rows keep emitting (the set-ratchet regression face).
        assert_eq!(
            checked_diags("class EC { n = 1 }\nEC.prop = 2\nvar q1 = EC.prop;\n"),
            [
                (
                    2339,
                    22,
                    4,
                    "Property 'prop' does not exist on type 'typeof EC'.".to_owned()
                ),
                (
                    2339,
                    43,
                    4,
                    "Property 'prop' does not exist on type 'typeof EC'.".to_owned()
                )
            ]
        );
    }

    // ---- 9.3b2 review-round pins (union best-match, IIFE effective
    // args, optional missing removal) ----

    #[test]
    fn expando_resolution_is_name_precise() {
        // Only the assigned member resolves; other names miss in tsc
        // too — y/q report 2339, "z" reports 7053, and the expando'd
        // declaration symbol displays `typeof foo` (oracle-probed byte
        // rows).
        assert_eq!(
            checked_diags(
                "function foo() {}\nfoo.x = 1;\nfoo.y;\nfoo[\"z\"];\nconst alias = foo;\nalias.q;\nvar ok: number = foo.x;\n"
            ),
            [
                (
                    2339,
                    33,
                    1,
                    "Property 'y' does not exist on type 'typeof foo'.".to_owned()
                ),
                (
                    7053,
                    36,
                    8,
                    "Element implicitly has an 'any' type because expression of type '\"z\"' can't be used to index type 'typeof foo'."
                        .to_owned()
                ),
                (
                    2339,
                    71,
                    1,
                    "Property 'q' does not exist on type 'typeof foo'.".to_owned()
                )
            ]
        );
    }

    #[test]
    fn expando_template_key_records_like_string_literal() {
        // Round 2: getElementOrPropertyAccessName (15134) is
        // string-literal-LIKE — a `x` no-substitution template key
        // records the member name exactly as "x" does, so the
        // .x / [`x`] / ["x"] reads resolve while .y keeps its row
        // (oracle-probed byte rows).
        assert_eq!(
            checked_diags(
                "function foo() {}\nfoo[`x`] = 1;\nfoo.x;\nfoo[`x`];\nfoo[\"x\"];\nfoo.y;\n"
            ),
            [(
                2339,
                63,
                1,
                "Property 'y' does not exist on type 'typeof foo'.".to_owned()
            )]
        );
    }

    #[test]
    fn union_target_member_elaborates_through_best_match() {
        // getBestMatchIndexedAccessTypeOrUndefined's union leg: the
        // member row lands on `m` (the head suppresses), method and
        // plain flavors alike.
        assert_eq!(
            checked_diags("let o: { m: () => string } | { x: number } = { m() { return 1 } };\n"),
            [(
                2322,
                47,
                1,
                "Type '() => number' is not assignable to type '() => string'.".to_owned()
            )]
        );
        assert_eq!(
            checked_diags("let o2: { m: string } | { x: number } = { m: 1 };\n"),
            [(
                2322,
                42,
                1,
                "Type 'number' is not assignable to type 'string'.".to_owned()
            )]
        );
    }

    #[test]
    fn union_target_object_members_keep_the_union_head() {
        // The 65185 substitution needs a NULLABLE-shaped union — an
        // object-member union keeps the full union face (declared
        // source; the fresh-literal twin rides a pre-existing
        // discriminated-union verdict FN outside this slice).
        assert_eq!(
            checked_diags(
                "declare let src3: { kind: \"a\"; v: number };\nlet o3b: { kind: \"a\"; v: string } | { kind: \"b\"; v: number } = src3;\n"
            ),
            [(
                2322,
                48,
                3,
                "Type '{ kind: \"a\"; v: number; }' is not assignable to type '{ kind: \"a\"; v: string; } | { kind: \"b\"; v: number; }'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn global_object_head_selection_distinguishes_members_from_signatures() {
        assert_eq!(
            checked_diags(
                "interface Object { toString(): string }\n\
                 interface I { toString(): number }\n\
                 interface Callable { (): void }\n\
                 declare let o: Object;\n\
                 declare let i: I;\n\
                 declare let c: Callable;\n\
                 i = o;\n\
                 c = o;\n"
            ),
            [
                (
                    2696,
                    173,
                    1,
                    "The 'Object' type is assignable to very few other types. Did you mean to use the 'any' type instead?"
                        .to_owned()
                ),
                (
                    2322,
                    180,
                    1,
                    "Type 'Object' is not assignable to type 'Callable'.".to_owned()
                )
            ]
        );
        assert_eq!(
            checked_chain_codes(
                "interface Object { toString(): string }\n\
                 interface I { toString(): number }\n\
                 interface Missing { x: number }\n\
                 interface Callable { (): void }\n\
                 declare let o: Object;\n\
                 declare let i: I;\n\
                 declare let m: Missing;\n\
                 declare let c: Callable;\n\
                 i = o;\n\
                 m = o;\n\
                 c = o;\n"
            ),
            [
                vec![2696, 2201, 2322],
                vec![2696, 2741],
                vec![2322, 2696, 2658],
            ]
        );
    }

    #[test]
    fn type_variable_constraint_retry_preserves_relation_failure_frames() {
        assert_eq!(
            checked_chain_codes(
                "function f<T extends \"a\" | \"b\">(x: T) {\n\
                     let y: `${T}` = x;\n\
                 }\n"
            ),
            [vec![2322, 2322, 2322]]
        );
    }

    #[test]
    fn iife_optional_probe_counts_effective_arguments() {
        // isOptionalParameter's IIFE arm reads
        // getEffectiveCallArguments — the spread tuple counts 2, so
        // `b` is NOT optional.
        assert_eq!(
            checked_diags(
                "(function f(a, b) {\n    let s: string = f;\n})(...[1, \"\"] as const);\n"
            ),
            [(
                2322,
                28,
                1,
                "Type '(a: 1, b: \"\") => void' is not assignable to type 'string'.".to_owned()
            )]
        );
    }

    #[test]
    fn optional_target_member_reports_without_the_missing_type() {
        // The elaborateElementwise report tail strips the missing
        // type on optional targets: '() => string', not
        // '(() => string) | undefined'; shorthand rides the same
        // tail.
        assert_eq!(
            checked_diags("let o4: { m?: () => string } = { m() { return 1 } };\n"),
            [(
                2322,
                33,
                1,
                "Type '() => number' is not assignable to type '() => string'.".to_owned()
            )]
        );
        assert_eq!(
            checked_diags("declare let p: number;\nlet o6: { p?: string } = { p };\n"),
            [(
                2322,
                50,
                1,
                "Type 'number' is not assignable to type 'string'.".to_owned()
            )]
        );
    }

    // ---- 9.3b2 member-elaboration pins (method/accessor yields) ----

    #[test]
    fn method_member_elaborates_at_the_name() {
        assert_eq!(
            checked_diags("let o1: { m(): string } = { m() { return 1 } };\n"),
            [(
                2322,
                28,
                1,
                "Type '() => number' is not assignable to type '() => string'.".to_owned()
            )]
        );
    }

    #[test]
    fn accessor_pair_double_yields_one_row_per_name() {
        // generateObjectLiteralElements yields the getter AND the
        // setter — two rows, both over the shared member's read type.
        assert_eq!(
            checked_diags(
                "let o2: { p: string } = { get p() { return 1 }, set p(v: number) {} };\n"
            ),
            [
                (
                    2322,
                    30,
                    1,
                    "Type 'number' is not assignable to type 'string'.".to_owned()
                ),
                (
                    2322,
                    52,
                    1,
                    "Type 'number' is not assignable to type 'string'.".to_owned()
                )
            ]
        );
    }

    #[test]
    fn computed_method_member_keeps_the_plain_2322() {
        // Method yields carry no errorMessage — the 2418
        // computed-property swap is PropertyAssignment-only.
        assert_eq!(
            checked_diags("const k = \"m\";\nlet o3: { m(): string } = { [k]() { return 1 } };\n"),
            [(
                2322,
                43,
                3,
                "Type '() => number' is not assignable to type '() => string'.".to_owned()
            )]
        );
    }

    #[test]
    fn accessor_members_elaborate_against_index_targets() {
        assert_eq!(
            checked_diags(
                "let o4: { [k: string]: number } = { get p() { return \"s\" }, set p(v: string) {} };\n"
            ),
            [
                (
                    2322,
                    40,
                    1,
                    "Type 'string' is not assignable to type 'number'.".to_owned()
                ),
                (
                    2322,
                    64,
                    1,
                    "Type 'string' is not assignable to type 'number'.".to_owned()
                )
            ]
        );
    }

    #[test]
    fn method_member_elaborates_against_index_target() {
        assert_eq!(
            checked_diags("let o5: { [k: string]: number } = { m() { return \"s\" } };\n"),
            [(
                2322,
                36,
                1,
                "Type '() => string' is not assignable to type 'number'.".to_owned()
            )]
        );
    }

    #[test]
    fn class_static_side_displays_typeof_face() {
        assert_eq!(
            checked_diags("class A3 {}\nvar v3: number = A3;\n"),
            [(
                2322,
                16,
                2,
                "Type 'typeof A3' is not assignable to type 'number'.".to_owned()
            )]
        );
    }

    #[test]
    fn class_expression_type_queries_use_written_or_anonymous_names() {
        let messages = |text: &str, code: u32| {
            checked_diags(text)
                .into_iter()
                .filter(|row| row.0 == code)
                .map(|row| row.3)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            messages(
                "function foo<T>(x = class { prop: T }): T { return undefined; }\n\
                 foo(class { static prop = \"hello\" }).length;\n"
                ,
                2345,
            ),
            [
                "Argument of type 'typeof (Anonymous class)' is not assignable to parameter of type 'typeof (Anonymous class)'.".to_owned(),
            ]
        );
        assert_eq!(
            messages(
                "var ExpandoExpr3 = class { n = 10001; };\n\
                 ExpandoExpr3.prop = 3;\n",
                2339,
            ),
            ["Property 'prop' does not exist on type 'typeof ExpandoExpr3'.".to_owned(),]
        );
    }

    #[test]
    fn enum_object_displays_typeof_face() {
        assert_eq!(
            checked_diags("enum E3 { X }\nvar v4: number = E3;\n"),
            [(
                2322,
                18,
                2,
                "Type 'typeof E3' is not assignable to type 'number'.".to_owned()
            )]
        );
    }

    #[test]
    fn outer_generic_reference_qualifies_changed_arguments() {
        let source = "interface Array<T> { length: number; [n: number]: T }\n\
        function mixin<T extends { new (...args: any[]): {} }>(superclass: T) {\n\
            return class extends superclass { get name() { return \"\"; } };\n\
        }\n\
        class BaseClass { set name(v: string) {} }\n\
        class MyClass extends mixin(BaseClass) { get name() { return \"\"; } }\n";
        let diagnostics = checked_diags(source);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(
            (
                diagnostics[0].0,
                diagnostics[0].2,
                diagnostics[0].3.as_str()
            ),
            (
                2611,
                4,
                "'name' is defined as a property in class \
                 'mixin<typeof BaseClass>.(Anonymous class) & BaseClass', but is overridden here \
                 in 'MyClass' as an accessor."
            )
        );
        assert_eq!(
            diagnostics[0].1,
            source.rfind("name").expect("derived accessor name") as u32
        );
    }

    #[test]
    fn outer_generic_reference_omits_unchanged_qualification() {
        assert_eq!(
            checked_diags(
                "function make<P>() { return class { value!: P; method() { this.missing; } }; }\n"
            ),
            [(
                2339,
                63,
                7,
                "Property 'missing' does not exist on type '(Anonymous class)'.".to_owned()
            )]
        );
    }

    // ---- 9.3b relation-reporting pins (excess property, did-you-mean,
    // elaboration extensions) ----

    #[test]
    fn excess_property_reports_parent_skipped_2353() {
        assert_eq!(
            checked_diags("declare let a2: { x: number };\na2 = { x: 1, y: 2 };\n"),
            [(
                2353,
                44,
                1,
                "Object literal may only specify known properties, and 'y' does not exist in \
                 type '{ x: number; }'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn excess_property_with_spelling_suggestion_reports_2561() {
        assert_eq!(
            checked_diags("declare let b2: { hello: number };\nb2 = { hallo: 1 };\n"),
            [(
                2561,
                42,
                5,
                "Object literal may only specify known properties, but 'hallo' does not exist \
                 in type '{ hello: number; }'. Did you mean to write 'hello'?"
                    .to_owned()
            )]
        );
    }

    #[test]
    fn aliased_union_type_variables_keep_the_origin_shield() {
        // oracle (vendored 6.0.3, strict, noLib, 2026-07-23): clean.
        // The origin's instantiable constituents hide inside the named
        // union member `N<T, U>`; the port's cross-product verdict is
        // still unfaithful there, so the shield must cover the nested
        // shape — the direct-member probe fabricated a 2322 (9.3b5
        // review r1).
        assert_eq!(
            checked_diags(
                "type A = 1 | 2;\ntype B = 2 | 3;\ntype N<T, U> = (T & U) | 4;\n\nfunction f<T \
                 extends A, U extends B>(\n  ab: T & U\n): N<T, U> & (A | B) {\n  return ab;\n}\n"
            ),
            []
        );
    }

    #[test]
    fn unique_symbol_missing_property_prints_qualified_computed_faces() {
        // oracle (vendored 6.0.3, strict, noLib, 2026-07-23):
        // Property '[B.sym]' is missing in type '{ [A.sym]: number; }'
        // but required in type '{ [B.sym]: number; }'. @4:5 len 1,
        // related 2728 '[B.sym]' is declared here. The namespace-
        // nested symbols qualify through the property-declaration
        // enclosing (addPropertyToElementList 52265-52267) on the
        // PLAIN pass — no FQ retry involved; the propName rides
        // WriteComputedProps' name-node reprint.
        assert_eq!(
            checked_diags(
                "declare namespace A { const sym: unique symbol }\ndeclare namespace B { const \
                 sym: unique symbol }\ndeclare const a: { [A.sym]: number };\nlet b: { [B.sym]: \
                 number } = a;\n"
            ),
            [(
                2741,
                140,
                1,
                "Property '[B.sym]' is missing in type '{ [A.sym]: number; }' but required in \
                 type '{ [B.sym]: number; }'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn unique_symbol_member_uses_the_value_with_the_matching_declared_type() {
        // getContainersOfSymbol's firstVariableMatch: the unique
        // symbol member belongs to the TYPE-only SymbolConstructor,
        // but its Value expression qualifies through the in-scope
        // `Symbol` value whose type is exactly SymbolConstructor.
        let messages = checked_diags(
            "interface SymbolConstructor { readonly iterator: unique symbol; }\n\
             declare var Symbol: SymbolConstructor;\n\
             declare var source: { [Symbol.iterator]?(): string };\n\
             let target: number = source;\n",
        )
        .into_iter()
        .filter(|row| row.0 == 2322)
        .map(|row| row.3)
        .collect::<Vec<_>>();
        assert_eq!(
            messages,
            ["Type '{ [Symbol.iterator]?(): string; }' is not assignable to type 'number'."]
        );
    }

    #[test]
    fn quoted_missing_property_preserves_its_written_name() {
        let messages =
            checked_diags("declare let source: {};\nlet target: { '1.0': string } = source;\n")
                .into_iter()
                .filter(|row| row.0 == 2741)
                .map(|row| row.3)
                .collect::<Vec<_>>();

        assert_eq!(
            messages,
            [
                "Property ''1.0'' is missing in type '{}' but required in type '{ '1.0': string; }'."
            ]
        );
    }

    #[test]
    fn top_level_unique_symbol_member_face_stays_bare() {
        // oracle (vendored 6.0.3, strict, noLib, 2026-07-23):
        // Property '[s]' is missing in type '{}' but required in type
        // '{ [s]: number; }'. A global-script symbol is accessible
        // bare from the property declaration, so no qualifier prints.
        assert_eq!(
            checked_diags(
                "declare const s: unique symbol;\ndeclare const a4: {};\nlet b4: { [s]: number } \
                 = a4;\n"
            ),
            [(
                2741,
                58,
                2,
                "Property '[s]' is missing in type '{}' but required in type '{ [s]: number; }'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn umd_global_alias_is_excluded_inside_external_modules() {
        // oracle probe A (vendored 6.0.3, strict, driver.mjs
        // 2026-07-24): ONE 2741 @a.ts:34 len 1 — Property '[U.s]' is
        // missing in type '{}' but required in type
        // '{ [s]: number; }'. — the WriteComputedProps head keeps the
        // written '[U.s]'; the target member face drops the UMD
        // global-alias route (trySymbolTable 50341, enclosing is the
        // external-module property declaration) AND its module parent
        // (52996-52998, yieldModuleSymbol falsy on the
        // symbolToExpression path), leaving the bare '[s]'. related
        // 2728 @a.ts:61 len 5 '[U.s]' is declared here. The raw
        // checker stream also contains 2686 at the `U` reference
        // @a.ts:62 len 1; the public program layer consumes it through
        // @ts-ignore. Suggestions such as 6133 stay outside this sink.
        with_program_state(
            &[
                (
                    "umd.d.ts",
                    "export as namespace U;\nexport const s: unique symbol;\n",
                ),
                (
                    "a.ts",
                    "export {};\ndeclare let a: {};\nlet b: {\n    // @ts-ignore\n    [U.s]: \
                     number\n} = a;\n",
                ),
            ],
            &CompilerOptions::default(),
            |state| {
                state.check_source_file(1);
                assert_eq!(
                    diag_rows(state),
                    [
                        (
                            2686,
                            62,
                            1,
                            "'U' refers to a UMD global, but the current file is a module. \
                             Consider adding an import instead."
                                .to_owned()
                        ),
                        (
                            2741,
                            34,
                            1,
                            "Property '[U.s]' is missing in type '{}' but required in type '{ \
                             [s]: number; }'."
                                .to_owned()
                        )
                    ]
                );
            },
        );
    }

    #[test]
    fn self_import_export_value_local_wins_over_the_alias_scan() {
        // oracle probe C (vendored 6.0.3, strict, driver.mjs
        // 2026-07-24): 2741 @c.ts:91 len 1 — Property '[s]' is missing
        // in type '{}' but required in type '{ [s]: number; }'. — NOT
        // '[Self.s]': the exportSymbol arm (50348-50357) fires on the
        // "s" EXPORT_VALUE local BEFORE the later "Self" entry's alias
        // leg inside tsc's single per-entry forEachEntry pass. related
        // 2728 @c.ts:96 len 3 '[s]' is declared here; the 6133
        // suggestions stay outside the sink.
        with_program_state(
            &[(
                "c.ts",
                "export declare const s: unique symbol;\nimport * as Self from \
                 \"./c\";\ndeclare let a: {};\nlet b: { [s]: number } = a;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                state.check_source_file(0);
                assert_eq!(
                    diag_rows(state),
                    [(
                        2741,
                        91,
                        1,
                        "Property '[s]' is missing in type '{}' but required in type '{ [s]: \
                         number; }'."
                            .to_owned()
                    )]
                );
            },
        );
    }

    #[test]
    fn script_global_member_face_ignores_module_local_shadowing() {
        // oracle probe D (vendored 6.0.3, strict, driver.mjs
        // 2026-07-24): 2741 @a.ts:66 len 1 — Property '[s]' is missing
        // in type '{}' but required in type '{ [s]: number; }'.
        // related 2728 at the SCRIPT declaration (global.d.ts:49 len
        // 3). The member face re-encloses at the property declaration
        // in the script file, where the globals direct hit precedes
        // both the alias scan and the globals-tail globalThis probe
        // (50359) — the module-local shadowing `s` never enters. Pins
        // the globals-tail omission's re-justification
        // (try_symbol_table_slice header).
        with_program_state(
            &[
                (
                    "global.d.ts",
                    "declare const s: unique symbol;\ndeclare let g: { [s]: number };\n",
                ),
                (
                    "a.ts",
                    "export {};\ndeclare const s: unique symbol;\ndeclare let a: {};\nlet b: \
                     typeof g = a;\n",
                ),
            ],
            &CompilerOptions::default(),
            |state| {
                state.check_source_file(1);
                assert_eq!(
                    diag_rows(state),
                    [(
                        2741,
                        66,
                        1,
                        "Property '[s]' is missing in type '{}' but required in type '{ [s]: \
                         number; }'."
                            .to_owned()
                    )]
                );
            },
        );
    }

    #[test]
    fn alias_typed_computed_member_splits_prop_name_and_face() {
        // oracle (vendored 6.0.3, strict, noLib, 2026-07-23):
        // Property '[k]' is missing in type '{}' but required in type
        // '{ [B.sym]: number; }'. The propName re-prints the written
        // name node (`[k]`); the member face renders the NAMETYPE
        // symbol's chain (`[B.sym]`).
        assert_eq!(
            checked_diags(
                "declare namespace B { const sym: unique symbol }\ndeclare const k: typeof \
                 B.sym;\ndeclare const a2: {};\nlet b2: { [k]: number } = a2;\n"
            ),
            [(
                2741,
                106,
                2,
                "Property '[k]' is missing in type '{}' but required in type '{ [B.sym]: number; \
                 }'."
                .to_owned()
            )]
        );
    }

    #[test]
    fn early_bound_computed_string_name_reprints_the_bracket_face() {
        // oracle (vendored 6.0.3, strict, noLib, 2026-07-23):
        // Property '["ab"]' is missing in type '{}' but required in
        // type '{ ab: number; }'. The single-quoted, space-padded
        // source name normalizes through the printer: double quotes,
        // no padding — while the TYPE face keeps the identifier form.
        assert_eq!(
            checked_diags("declare const a5: {};\nlet b5: { [ 'ab' ]: number } = a5;\n"),
            [(
                2741,
                26,
                2,
                "Property '[\"ab\"]' is missing in type '{}' but required in type '{ ab: number; \
                 }'."
                .to_owned()
            )]
        );
    }

    #[test]
    fn late_bound_multi_missing_list_prints_source_verbatim() {
        // oracle (vendored 6.0.3, strict, noLib, 2026-07-23) — the
        // multi-property 2739 rides plain symbolToString: the
        // late-bound name prints its declaration SOURCE text verbatim
        // (`[ B . sym ]`, spaces kept) while the TYPE face qualifies
        // through the property enclosing (`[B.sym]`) and sorts the
        // late-bound member after the early ones.
        assert_eq!(
            checked_diags(
                "declare namespace B { const sym: unique symbol }\ndeclare const a8: {};\nlet \
                 b8: { [ B . sym ]: number; other: string } = a8;\n"
            ),
            [(
                2739,
                75,
                2,
                "Type '{}' is missing the following properties from type '{ other: string; \
                 [B.sym]: number; }': other, [ B . sym ]"
                    .to_owned()
            )]
        );
    }

    #[test]
    fn multi_missing_source_uses_the_structural_relation_face() {
        let text = "interface Number {}\n\
                    interface Obj { hello: string; world: number }\n\
                    interface NumberTo<T> { [x: number]: T }\n\
                    type NumberToNumber = NumberTo<number>;\n\
                    declare const n: NumberToNumber;\n\
                    const a: Obj = n;\n\
                    type Brand<T> = number & { __brand: T };\n\
                    declare const b: Brand<{ view: number; styleMedia: string }>;\n\
                    const c: Obj = b;\n";
        let rows: Vec<_> = checked_diags(text)
            .into_iter()
            .filter(|row| row.0 == 2739)
            .collect();
        assert_eq!(
            rows.iter().map(|row| row.3.as_str()).collect::<Vec<_>>(),
            [
                "Type 'NumberTo<number>' is missing the following properties from type 'Obj': hello, world",
                "Type 'Number & { __brand: { view: number; styleMedia: string; }; }' is missing the following properties from type 'Obj': hello, world",
            ]
        );
    }

    #[test]
    fn did_you_mean_new_reports_at_the_member_value() {
        // elaborateDidYouMeanToCallOrConstruct re-anchors the member
        // relation at the VALUE (`A2`, not the property name) and the
        // missing-property override renders the class-static typeof
        // face.
        assert_eq!(
            checked_diags(
                "class A2 { foo(): string { return '' } }\nvar c2: { [x: string]: A2 } = { a: A2 };\n"
            ),
            [(
                2741,
                76,
                2,
                "Property 'foo' is missing in type 'typeof A2' but required in type 'A2'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn shorthand_member_supports_missing_property_head() {
        // The shorthand walk feeds the literal's members; the head is
        // the parent-skipped missing-'b' face at the declaration.
        assert_eq!(
            checked_diags("var id: number = 1;\nvar person: { b: string; id: number } = { id };\n"),
            [(
                2741,
                24,
                6,
                "Property 'b' is missing in type '{ id: number; }' but required in type \
                 '{ b: string; id: number; }'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn shorthand_member_row_replaces_the_return_head() {
        // generateObjectLiteralElements yields shorthand members with
        // no inner expression — the member row anchors at the NAME.
        assert_eq!(
            checked_diags(
                "var name2: string = 'x';\nfunction foo(): { name2: number } { return { name2 }; }\n"
            ),
            [(
                2322,
                70,
                5,
                "Type 'string' is not assignable to type 'number'.".to_owned()
            )]
        );
    }

    #[test]
    fn index_signature_target_elaborates_member_rows() {
        // elaborateElementwise's targetPropType is an indexed access:
        // a property miss falls through to the applicable index
        // signature's value type.
        assert_eq!(
            checked_diags("var d2: { [x: number]: string } = { 1: 1 };\n"),
            [(
                2322,
                36,
                1,
                "Type 'number' is not assignable to type 'string'.".to_owned()
            )]
        );
    }

    #[test]
    fn constructor_return_elaborates_and_reports_2409() {
        let rows =
            checked_diags("class F { x: string = ''; constructor() { return { x: 1 }; } }\n");
        assert_eq!(
            rows,
            [
                (
                    2322,
                    51,
                    1,
                    "Type 'number' is not assignable to type 'string'.".to_owned()
                ),
                (
                    2409,
                    42,
                    6,
                    "Return type of constructor signature must be assignable to the instance \
                     type of the class."
                        .to_owned()
                )
            ]
        );
    }

    #[test]
    fn merged_declaration_initializer_elaborates_member_rows() {
        assert_eq!(
            checked_diags(
                "var p: { x: number; y: number };\nvar p: { x: number; y: number } = { x: 0, y: '' };\n"
            ),
            [(
                2322,
                75,
                1,
                "Type 'string' is not assignable to type 'number'.".to_owned()
            )]
        );
    }

    #[test]
    fn non_primitive_source_walks_as_the_empty_object_face() {
        // structuredTypeRelatedTo apparent-izes `object` in place —
        // the missing-property face renders '{}'.
        assert_eq!(
            checked_diags("var y2 = { foo: 'bar' };\ndeclare var o: object;\ny2 = o;\n"),
            [(
                2741,
                48,
                2,
                "Property 'foo' is missing in type '{}' but required in type \
                 '{ foo: string; }'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn template_literal_index_key_admits_matching_property_names() {
        // isKnownProperty probes applicability through the faithful
        // isApplicableIndexType — `sfoo` fits `[k: \`s${string}\`]`,
        // so the literal is clean (the flag-shortcut fabricated an
        // excess verdict here).
        assert_eq!(
            checked_diags(
                "type F2 = { [k: `s${string}`]: (x: string) => void };\ndeclare let f3: F2;\nf3 = { sfoo: (x) => {} };\n"
            ),
            []
        );
    }

    #[test]
    fn case_clause_excess_property_reports_2353() {
        // The comparable relation runs the same excess arm — the 2678
        // head never lands.
        assert_eq!(
            checked_diags(
                "class C3 { id: number = 1 }\nswitch (new C3()) {\n    case { id: 12, name3: '' }:\n}\n"
            ),
            [(
                2353,
                67,
                5,
                "Object literal may only specify known properties, and 'name3' does not exist \
                 in type 'C3'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn non_finite_numeric_keys_resolve_by_canonical_name() {
        // Members declared with numeric keys that stringify to
        // non-finite/huge canonical names ("Infinity",
        // "9.671406556917009e+24") resolve through both the string
        // and numeric element-access faces (binaryIntegerLiteral's
        // clean rows — the 7053 face fabricated here while the object
        // display curtained the report).
        assert_eq!(
            checked_diags("var o = { 1e999: true };\no[\"Infinity\"];\n"),
            []
        );
        assert_eq!(
            checked_diags(
                "var o2 = { 9.671406556917009e+24: true };\no2[9.671406556917009e+24];\no2[\"9.671406556917009e+24\"];\n"
            ),
            []
        );
        assert_eq!(checked_diags("var o3 = { 1e999: true };\no3[1e999];\n"), []);
    }

    #[test]
    fn interface_in_annotation_on_covariant_use_reports_2636() {
        let diags = checked_diags("interface Foo<in T> { f: () => T }\n");
        assert_eq!(
            diags,
            [(
                2636,
                14,
                4,
                "Type 'Foo<super-T>' is not assignable to type 'Foo<sub-T>' as implied by \
                 variance annotation."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn correct_variance_annotations_are_silent() {
        assert_eq!(checked_diags("interface Foo<out T> { f: () => T }\n"), []);
        // in out together: tsc skips the marker probe (modifiers must
        // be exactly In or exactly Out).
        assert_eq!(
            checked_diags("interface Foo<in out T> { f: (x: T) => void }\n"),
            []
        );
    }

    #[test]
    fn alias_out_annotation_reports_2636_with_alias_display() {
        let diags = checked_diags("type F<out T> = (x: T) => void;\n");
        assert_eq!(
            diags,
            [(
                2636,
                7,
                5,
                "Type 'F<sub-T>' is not assignable to type 'F<super-T>' as implied by \
                 variance annotation."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn alias_annotation_on_non_object_rhs_reports_2637() {
        let diags = checked_diags("type F<in T> = T[];\ninterface Array<T> { length: number }\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2637, 7, 4));
    }

    #[test]
    fn class_property_out_annotation_reports_2636() {
        // Oracle pair: 2564 (checkPropertyInitialization's
        // no-constructor face, live since 5.8c) + the variance 2636.
        let diags = checked_diags("class C<out T> { f: (x: T) => void; }\n");
        assert_eq!(
            diags,
            [
                (
                    2564,
                    17,
                    1,
                    "Property 'f' has no initializer and is not definitely assigned in the \
                     constructor."
                        .to_owned()
                ),
                (
                    2636,
                    8,
                    5,
                    "Type 'C<sub-T>' is not assignable to type 'C<super-T>' as implied by \
                     variance annotation."
                        .to_owned()
                )
            ]
        );
    }

    #[test]
    fn class_method_parameters_stay_bivariant_and_silent() {
        assert_eq!(checked_diags("class C<out T> { f(x: T): void {} }\n"), []);
    }

    #[test]
    fn multi_parameter_marker_display_names_other_parameters() {
        let diags = checked_diags("interface P<A, out B> { f: (x: B) => A }\n");
        assert_eq!(
            diags,
            [(
                2636,
                15,
                5,
                "Type 'P<A, sub-B>' is not assignable to type 'P<A, super-B>' as implied \
                 by variance annotation."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn block_nested_interfaces_are_checked_via_check_block() {
        let diags = checked_diags("{ interface J<out T> { g: (x: T) => void } }\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2636, 14, 5));
    }

    // ---- checkTypeParameters family — oracle-pinned ----

    #[test]
    fn self_referential_default_reports_2744_not_2716() {
        let diags = checked_diags("interface I<T = T> { x: T }\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2744, 16, 1));
    }

    #[test]
    fn forward_referencing_default_reports_2744() {
        let diags = checked_diags("interface I<T = U, U = string> { x: T }\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2744, 16, 1));
    }

    #[test]
    fn required_parameter_after_optional_reports_2706() {
        let diags = checked_diags("interface I<T = string, U> { x: T }\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2706, 24, 1));
    }

    #[test]
    fn cross_generic_default_cycle_reports_2716() {
        let diags = checked_diags("interface P<T = Q> { x: T }\ninterface Q<U = P> { y: U }\n");
        assert_eq!(
            diags,
            [(
                2716,
                44,
                1,
                "Type parameter 'U' has a circular default.".to_owned()
            )]
        );
    }

    #[test]
    fn default_not_satisfying_constraint_reports_2344() {
        let diags = checked_diags("interface I<T extends string = number> { x: T }\n");
        assert_eq!(
            diags,
            [(
                2344,
                31,
                6,
                "Type 'number' does not satisfy the constraint 'string'.".to_owned()
            )]
        );
    }

    #[test]
    fn circular_constraint_reports_2313_through_the_driver() {
        let diags = checked_diags("interface I<T extends T> { x: T }\n");
        assert_eq!(
            diags,
            [(
                2313,
                22,
                1,
                "Type parameter 'T' has a circular constraint.".to_owned()
            )]
        );
    }

    #[test]
    fn reserved_names_report_2368_2457_2427() {
        let diags = checked_diags("interface I<undefined> { x: number }\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2368, 12, 9));

        let diags = checked_diags("type undefined = string;\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2457, 5, 9));

        let diags = checked_diags("interface any { x: number }\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2427, 10, 3));
    }

    #[test]
    fn intrinsic_keyword_validity_reports_2795() {
        let diags = checked_diags("type Foo<T> = intrinsic;\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2795, 14, 9));

        assert_eq!(
            checked_diags("type Uppercase<S extends string> = intrinsic;\n"),
            []
        );
    }

    #[test]
    fn libless_missing_lib_names_report_the_2583_family() {
        // With lib loading landed (conformance programs always carry
        // their lib set), the 5.4-era lib_globals gate is retired: a
        // LIBLESS program reports missing default-lib names exactly
        // like tsc under noLib (oracle-pinned), with the suggested-lib
        // argument from the static feature table.
        let diags = checked_diags("interface I<T extends Map> { x: T }\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2583, 22, 3));
        assert!(diags[0].3.ends_with("'es2015' or later."), "{}", diags[0].3);
        let diags = checked_diags("interface I<T extends console> { x: T }\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2584, 22, 7));
    }

    #[test]
    fn unresolved_names_in_constraints_and_defaults_flow_2304() {
        let diags = checked_diags("interface I<T extends Missing> { x: T }\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2304, 22, 7));

        let diags = checked_diags("interface I<T = Missing> { x: T }\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2304, 16, 7));
    }

    // ---- checkTypeArgumentConstraints — oracle-pinned ----

    #[test]
    fn explicit_type_arguments_check_their_constraints() {
        let diags = checked_diags("interface I<T extends string> { x: T }\ntype X = I<number>;\n");
        assert_eq!(
            diags,
            [(
                2344,
                50,
                6,
                "Type 'number' does not satisfy the constraint 'string'.".to_owned()
            )]
        );
        assert_eq!(
            checked_diags("interface I<T extends string> { x: T }\ntype X = I<\"a\">;\n"),
            []
        );
        // Defaults fill through fillMissingTypeArguments before the
        // constraint instantiates.
        assert_eq!(
            checked_diags(
                "interface I<T extends string, U extends T = T> { x: T }\ntype X = I<\"a\">;\n"
            ),
            []
        );
        // `result = result && checkTypeAssignableTo(...)` is
        // observable: after the first failing constraint, tsc 6.0.3
        // does not publish a second 2344 for the same reference.
        assert_eq!(
            checked_diags(
                "interface Pair<T extends string, U extends number> { t: T; u: U }\n\
                 type Bad = Pair<boolean, boolean>;\n"
            ),
            [(
                2344,
                82,
                7,
                "Type 'boolean' does not satisfy the constraint 'string'.".to_owned()
            )]
        );
    }

    #[test]
    fn alias_type_arguments_check_their_constraints() {
        let diags = checked_diags(
            "type A<T extends number> = T[];\ninterface Array<T> { length: number }\ntype X = A<string>;\n",
        );
        assert_eq!(
            diags,
            [(
                2344,
                81,
                6,
                "Type 'string' does not satisfy the constraint 'number'.".to_owned()
            )]
        );
    }

    // ---- driver bookkeeping ----

    #[test]
    fn rechecking_a_type_checked_file_is_idempotent() {
        with_program_state(
            &[("a.ts", "interface Foo<out T> { f: (x: T) => void }\n")],
            &CompilerOptions::default(),
            |state| {
                state.check_source_file(0);
                let first = diag_rows(state);
                assert_eq!(first.len(), 1);
                state.check_source_file(0);
                assert_eq!(diag_rows(state), first, "TypeChecked gate must hold");
                assert!(
                    state.deferred_nodes.is_empty(),
                    "deferred set drains and clears"
                );
            },
        )
    }

    // ---- 9.3b3 symbol/value/module head pins (all rows oracle-
    // probed byte-exact against vendored 6.0.3, noLib + strict;
    // multi-file pins use the unit env's extension-less quoted module
    // names — the corpus harness roots names at "/", so goldens show
    // `import("/b")` where these pins show `import("b")`, the same
    // binder naming rule over a different fileName input) ----

    /// Program-driving helper for the multi-file pins: (file, code,
    /// start, length, message) rows in checker sink order.
    fn program_diags(files: &[(&str, &str)]) -> Vec<(String, u32, u32, u32, String)> {
        program_diags_with(files, &CompilerOptions::default(), "/")
    }

    /// The options/cwd-carrying twin: `cwd` mirrors the harness
    /// ProgramJson `cwd` the driver threads through
    /// check_program_with_libs_at.
    fn program_diags_with(
        files: &[(&str, &str)],
        options: &CompilerOptions,
        cwd: &str,
    ) -> Vec<(String, u32, u32, u32, String)> {
        with_program_state(files, options, |state| {
            state.host_current_directory = cwd.to_owned();
            for index in 0..state.binder.files().count() {
                state.check_source_file(index);
            }
            state
                .diagnostics
                .iter()
                .filter(|diag| {
                    diag.file_name.is_some()
                        && diag.category() == tsrs2_diags::DiagnosticCategory::Error
                })
                .map(|diag| {
                    (
                        diag.file_name.clone().unwrap(),
                        diag.code(),
                        diag.start.unwrap_or(u32::MAX),
                        diag.length.unwrap_or(u32::MAX),
                        diag.message_text().to_owned(),
                    )
                })
                .collect()
        })
    }

    #[test]
    fn namespace_value_faces_print_typeof_unqualified() {
        // lookupSymbolChainWorker 52950-52952: no enclosingDeclaration
        // -> chain=[symbol] -> the NESTED namespace face prints
        // `typeof Inner`, NOT `typeof Outer.Inner`.
        assert_eq!(
            checked_diags(
                "namespace Outer {\n    export namespace Inner {\n        export const x = 1;\n    }\n}\nOuter.NoSuch;\nOuter.Inner.NoSuch;\nlet n: number = Outer.Inner;\n"
            ),
            [
                (
                    2339,
                    89,
                    6,
                    "Property 'NoSuch' does not exist on type 'typeof Outer'.".to_owned()
                ),
                (
                    2339,
                    109,
                    6,
                    "Property 'NoSuch' does not exist on type 'typeof Inner'.".to_owned()
                ),
                (
                    2322,
                    121,
                    1,
                    "Type 'typeof Inner' is not assignable to type 'number'.".to_owned()
                )
            ]
        );
    }

    #[test]
    fn merged_interface_namespace_value_prints_typeof() {
        // The upstream named-object arm's VALUE_MODULE disjunct: the
        // merged value side prints `typeof X` (createAnonymousTypeNode
        // 51779) while the TYPE position keeps the plain `X` face.
        assert_eq!(
            checked_diags(
                "interface X { i: number }\nnamespace X { export const a = 1 }\nlet n: number = X;\nlet t: X = { i: 1, extra: 2 };\n"
            ),
            [
                (
                    2322,
                    65,
                    1,
                    "Type 'typeof X' is not assignable to type 'number'.".to_owned()
                ),
                (
                    2353,
                    99,
                    5,
                    "Object literal may only specify known properties, and 'extra' does not exist in type 'X'.".to_owned()
                )
            ]
        );
    }

    #[test]
    fn merged_class_and_enum_namespace_values_keep_typeof() {
        // Upstream-arm regression control: class+ns / enum+ns merges
        // keep the class-static/enum typeof split.
        assert_eq!(
            checked_diags(
                "class C {}\nnamespace C { export const a = 1 }\nenum E { A }\nnamespace E { export const b = 1 }\nlet n: number = C;\nlet m: number = E;\n"
            ),
            [
                (
                    2322,
                    98,
                    1,
                    "Type 'typeof C' is not assignable to type 'number'.".to_owned()
                ),
                (
                    2322,
                    117,
                    1,
                    "Type 'typeof E' is not assignable to type 'number'.".to_owned()
                )
            ]
        );
    }

    #[test]
    fn global_this_value_prints_typeof_global_this() {
        assert_eq!(
            checked_diags("let n: number = globalThis;\n"),
            [(
                2322,
                4,
                1,
                "Type 'typeof globalThis' is not assignable to type 'number'.".to_owned()
            )]
        );
    }

    #[test]
    fn function_namespace_merge_value_prints_typeof() {
        // The VALUE_MODULE arm runs before the FUNCTION admission at
        // the anonymous gate (tsc's 51779 disjunct order): the merged
        // fn+ns value prints `typeof f`, not a structural signature.
        assert_eq!(
            checked_diags(
                "function f() { return 1 }\nnamespace f { export const q = 1 }\nlet n: number = f;\n"
            ),
            [(
                2322,
                77,
                1,
                "Type 'typeof f' is not assignable to type 'number'.".to_owned()
            )]
        );
    }

    #[test]
    fn ambient_module_value_prints_import_face() {
        // hasNonGlobalAugmentationExternalModuleSymbol admits the
        // string-literal ModuleDeclaration; the specifier is the
        // unquoted symbol name (getSpecifierForModuleSymbol 53077).
        assert_eq!(
            program_diags(&[
                (
                    "g.d.ts",
                    "declare module \"amb\" {\n    export const v: number;\n}\n"
                ),
                ("a.ts", "import * as A from \"amb\";\nA.nope;\n"),
            ]),
            [(
                "a.ts".to_owned(),
                2339,
                28,
                4,
                "Property 'nope' does not exist on type 'typeof import(\"amb\")'.".to_owned()
            )]
        );
    }

    #[test]
    fn source_file_module_value_prints_import_face() {
        // The specifier is the binder's quoted module name minus
        // quotes — extension-free because
        // bindSourceFileAsExternalModule strips it at naming time —
        // rendered through the host's absolute normalized form (the
        // oracle host roots every fileName, so tsc binds and prints
        // `import("/b")` for this fixture; oracle-probed).
        assert_eq!(
            program_diags(&[
                ("b.ts", "export const bee = 1;\n"),
                ("a.ts", "import * as b from \"./b\";\nb.nope;\n"),
            ]),
            [(
                "a.ts".to_owned(),
                2339,
                28,
                4,
                "Property 'nope' does not exist on type 'typeof import(\"/b\")'.".to_owned()
            )]
        );
    }

    #[test]
    fn empty_ambient_module_specifier_falls_back_to_file_name() {
        // getSpecifierForModuleSymbol's fileName fallback (53080):
        // `declare module ""` binds `""`, which fails
        // ambientModuleSymbolRegex, so the specifier reads
        // getNonAugmentationDeclaration's rooted file name, extension
        // intact (oracle-probed: `typeof import("/g.d.ts")`).
        assert_eq!(
            program_diags(&[
                (
                    "g.d.ts",
                    "declare module \"\" { export const x: number; }\n"
                ),
                ("main.ts", "import * as ns from \"\";\nns.y;\n"),
            ]),
            [(
                "main.ts".to_owned(),
                2339,
                27,
                1,
                "Property 'y' does not exist on type 'typeof import(\"/g.d.ts\")'.".to_owned()
            )]
        );
    }

    #[test]
    fn fully_qualified_namespace_under_module_prints_import_qualifier() {
        // UseFullyQualifiedType roots the symbol chain at the external
        // module (getSymbolChain's container walk), so the 53117 gate
        // fires on chain[0] and the namespace rides as the
        // ImportTypeNode's qualifier — NOT the quoted-name entity face
        // (oracle-probed: `typeof import("/b").N` vs
        // `typeof import("/a").N`).
        assert_eq!(
            program_diags(&[
                ("a.ts", "export namespace N { export const x = 1; }\n"),
                ("b.ts", "export namespace N { export const x = \"s\"; }\n"),
                (
                    "c.ts",
                    "import { N as NA } from \"./a\";\nimport { N as NB } from \"./b\";\nlet v: typeof NA;\nv = NB;\n"
                ),
            ]),
            [(
                "c.ts".to_owned(),
                2322,
                80,
                1,
                "Type 'typeof import(\"/b\").N' is not assignable to type 'typeof import(\"/a\").N'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn fully_qualified_interface_under_module_prints_import_type_qualifier() {
        // The Type-meaning twin of the namespace Value face:
        // symbolToTypeNode roots the chain at the external module and
        // emits an ImportTypeNode without `typeof`.
        assert_eq!(
            program_diags(&[
                (
                    "a.ts",
                    "export namespace dom { export namespace JSX { export interface Element { a: string } } }\n"
                ),
                (
                    "b.ts",
                    "export namespace dom { export namespace JSX { export interface Element { b: string } } }\n"
                ),
                (
                    "c.ts",
                    "import { dom as A } from \"./a\";\nimport { dom as B } from \"./b\";\ndeclare let source: B.JSX.Element;\nlet target: A.JSX.Element = source;\n"
                ),
            ]),
            [(
                "c.ts".to_owned(),
                2741,
                103,
                6,
                "Property 'a' is missing in type 'import(\"/b\").dom.JSX.Element' but required in \
                 type 'import(\"/a\").dom.JSX.Element'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn fully_qualified_nested_namespace_joins_import_qualifier() {
        // The below-root links join as the qualifier spine
        // (createAccessFromSymbolChain with stopper 1; oracle-probed:
        // `typeof import("/b").A.B`).
        assert_eq!(
            program_diags(&[
                (
                    "a.ts",
                    "export namespace A { export namespace B { export const x = 1; } }\n"
                ),
                (
                    "b.ts",
                    "export namespace A { export namespace B { export const x = \"s\"; } }\n"
                ),
                (
                    "c.ts",
                    "import { A as XA } from \"./a\";\nimport { A as XB } from \"./b\";\nlet v: typeof XA.B;\nv = XB.B;\n"
                ),
            ]),
            [(
                "c.ts".to_owned(),
                2322,
                82,
                1,
                "Type 'typeof import(\"/b\").A.B' is not assignable to type 'typeof import(\"/a\").A.B'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn fully_qualified_alias_reexport_names_the_export_entry() {
        // getContainersOfSymbol's candidates leg (49994-50001): a
        // parentless namespace re-exported via `export { N as M }`
        // roots at the module (getAliasForSymbolInContainer admits the
        // container), and createAccessFromSymbolChain names the link
        // from the export-table entry (oracle-probed:
        // `typeof import("/b").M`, not `typeof N`).
        assert_eq!(
            program_diags(&[
                ("a.ts", "namespace N { export const x = 1; }\nexport { N as M };\n"),
                (
                    "b.ts",
                    "namespace N { export const x = \"s\"; }\nexport { N as M };\n"
                ),
                (
                    "c.ts",
                    "import { M as MA } from \"./a\";\nimport { M as MB } from \"./b\";\nlet v: typeof MA;\nv = MB;\n"
                ),
            ]),
            [(
                "c.ts".to_owned(),
                2322,
                80,
                1,
                "Type 'typeof import(\"/b\").M' is not assignable to type 'typeof import(\"/a\").M'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn export_table_order_names_the_qualifier() {
        // createAccessFromSymbolChain (53210-53217): the FIRST
        // resolved-export entry that same-references the link names it
        // — regardless of the symbol's own name or the import path
        // (oracle-probed both orders).
        let face = |first: &str, second: &str, import_name: &str, expected: &str| {
            let a = format!("namespace N {{ export const x = 1; }}\n{first}\n{second}\n");
            let b = format!("namespace N {{ export const x = \"s\"; }}\n{first}\n{second}\n");
            let c = format!(
                "import {{ {import_name} as NA }} from \"./a\";\nimport {{ {import_name} as NB }} from \"./b\";\nlet v: typeof NA;\nv = NB;\n"
            );
            assert_eq!(
                program_diags(&[("a.ts", &a), ("b.ts", &b), ("c.ts", &c)]),
                [(
                    "c.ts".to_owned(),
                    2322,
                    80,
                    1,
                    format!(
                        "Type 'typeof import(\"/b\").{expected}' is not assignable to type 'typeof import(\"/a\").{expected}'."
                    )
                )]
            );
        };
        face("export { N as M };", "export { N };", "N", "M");
        face("export { N };", "export { N as M };", "M", "N");
    }

    #[test]
    fn fully_qualified_nested_namespace_under_alias_reexport() {
        // The chain recursion applies the export-table naming at every
        // below-root link: the aliased root child renders `M`, the
        // parent-fast-path child renders `P` (oracle-probed:
        // `typeof import("/b").M.P`).
        assert_eq!(
            program_diags(&[
                (
                    "a.ts",
                    "namespace N { export namespace P { export const x = 1; } }\nexport { N as M };\n"
                ),
                (
                    "b.ts",
                    "namespace N { export namespace P { export const x = \"s\"; } }\nexport { N as M };\n"
                ),
                (
                    "c.ts",
                    "import { M as MA } from \"./a\";\nimport { M as MB } from \"./b\";\nlet v: typeof MA.P;\nv = MB.P;\n"
                ),
            ]),
            [(
                "c.ts".to_owned(),
                2322,
                82,
                1,
                "Type 'typeof import(\"/b\").M.P' is not assignable to type 'typeof import(\"/a\").M.P'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn default_exported_namespace_names_the_default_entry() {
        // The below-root naming scan skips only export= and late-bound
        // keys — `default` is an admissible qualifier name
        // (oracle-probed: `typeof import("/b").default`).
        assert_eq!(
            program_diags(&[
                ("a.ts", "namespace N { export const x = 1; }\nexport default N;\n"),
                (
                    "b.ts",
                    "namespace N { export const x = \"s\"; }\nexport default N;\n"
                ),
                (
                    "c.ts",
                    "import MA from \"./a\";\nimport MB from \"./b\";\nlet v: typeof MA;\nv = MB;\n"
                ),
            ]),
            [(
                "c.ts".to_owned(),
                2322,
                62,
                1,
                "Type 'typeof import(\"/b\").default' is not assignable to type 'typeof import(\"/a\").default'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn named_default_class_uses_its_written_declaration_name() {
        assert_eq!(
            program_diags(&[
                ("a.ts", "export default class Foo { p = 1 }\n"),
                ("b.ts", "import D from \"./a\"; let s: string = new D();\n"),
            ]),
            [(
                "b.ts".to_owned(),
                2322,
                25,
                1,
                "Type 'Foo' is not assignable to type 'string'.".to_owned()
            )]
        );
    }

    #[test]
    fn anonymous_default_class_keeps_the_default_symbol_name() {
        assert_eq!(
            program_diags(&[
                ("a.ts", "export default class { p = 1 }\n"),
                ("b.ts", "import D from \"./a\"; let s: string = new D();\n"),
            ]),
            [(
                "b.ts".to_owned(),
                2322,
                25,
                1,
                "Type 'default' is not assignable to type 'string'.".to_owned()
            )]
        );
    }

    #[test]
    fn export_equals_namespace_member_renders_import_qualifier() {
        // getWithAlternativeContainers' additionalContainers (50024):
        // the file whose export= IS the member's parent container
        // roots the chain; the export-table naming scan skips the
        // export= entry and falls to the symbol name (oracle-probed
        // under @module: commonjs: `typeof import("/b").Q`).
        let options = CompilerOptions {
            module: Some(1),
            ..CompilerOptions::default()
        };
        assert_eq!(
            program_diags_with(
                &[
                    (
                        "a.ts",
                        "namespace P { export namespace Q { export const x = 1; } }\nexport = P;\n"
                    ),
                    (
                        "b.ts",
                        "namespace P { export namespace Q { export const x = \"s\"; } }\nexport = P;\n"
                    ),
                    (
                        "c.ts",
                        "import PA = require(\"./a\");\nimport PB = require(\"./b\");\nlet v: typeof PA.Q;\nv = PB.Q;\n"
                    ),
                ],
                &options,
                "/"
            ),
            [(
                "c.ts".to_owned(),
                2322,
                76,
                1,
                "Type 'typeof import(\"/b\").Q' is not assignable to type 'typeof import(\"/a\").Q'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn ambient_export_equals_member_prints_bare_import_face() {
        // getSymbolChain's export= short-circuit (52978-52981): the
        // ambient module (candidates ModuleBlock arm, 49999-50001)
        // whose export= target IS the symbol renders as the bare
        // parent chain — a length-1 import face (oracle-probed under
        // @module: commonjs: `typeof import("amba")`).
        let options = CompilerOptions {
            module: Some(1),
            ..CompilerOptions::default()
        };
        assert_eq!(
            program_diags_with(
                &[
                    (
                        "g.d.ts",
                        "declare module \"amba\" { namespace Q { const x: number; } export = Q; }\ndeclare module \"ambb\" { namespace Q { const x: string; } export = Q; }\n"
                    ),
                    (
                        "a.ts",
                        "import A = require(\"amba\");\nimport B = require(\"ambb\");\nlet v: typeof A;\nv = B;\n"
                    ),
                ],
                &options,
                "/"
            ),
            [(
                "a.ts".to_owned(),
                2322,
                73,
                1,
                "Type 'typeof import(\"ambb\")' is not assignable to type 'typeof import(\"amba\")'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn script_alias_chain_prints_alias_qualified_face() {
        // getAccessibleSymbolChain's globals alias scan with the
        // candidate-table recursion (50328-50411): a script-file
        // `import M = A` reaches the nested namespace as [M, B], and
        // the alias parent's EMPTY unresolved export table falls the
        // link name back to getNameOfSymbolAsWritten (oracle-probed:
        // `typeof M.B` vs `typeof import("/m").A.B`).
        assert_eq!(
            program_diags(&[
                (
                    "s.ts",
                    "namespace A { export namespace B { export const x = 1; } }\nimport M = A;\n"
                ),
                (
                    "m.ts",
                    "namespace A { export namespace B { export const x = \"s\"; } }\nexport { A };\n"
                ),
                (
                    "c.ts",
                    "import { A as XA } from \"./m\";\nlet v: typeof XA.B;\nv = A.B;\n"
                ),
            ]),
            [(
                "c.ts".to_owned(),
                2322,
                51,
                1,
                "Type 'typeof M.B' is not assignable to type 'typeof import(\"/m\").A.B'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn script_global_direct_hit_beats_the_alias_scan() {
        // trySymbolTable's direct hit (50321-50327) precedes the alias
        // scan: the global namespace renders its bare name while the
        // module side names the export entry (oracle-probed:
        // `typeof N` vs `typeof import("/m").O`).
        assert_eq!(
            program_diags(&[
                (
                    "s.ts",
                    "namespace N { export const x = 1; }\nimport M = N;\n"
                ),
                (
                    "m.ts",
                    "namespace N { export const x = \"s\"; }\nexport { N as O };\n"
                ),
                (
                    "c.ts",
                    "import { O } from \"./m\";\nlet v: typeof O;\nv = N;\n"
                ),
            ]),
            [(
                "c.ts".to_owned(),
                2322,
                42,
                1,
                "Type 'typeof N' is not assignable to type 'typeof import(\"/m\").O'.".to_owned()
            )]
        );
    }

    #[test]
    fn same_name_unexported_namespaces_take_the_2719_face() {
        // A namespace that is neither exported nor re-exported has no
        // qualifying container (the candidates filter, 50014), so both
        // faces stay `typeof N` after the fully-qualified re-render —
        // reportRelationError swaps the generic head to 2719
        // (65097-65098; oracle-probed).
        assert_eq!(
            program_diags(&[
                (
                    "a.ts",
                    "namespace N { export const x = 1; }\nexport const val = N;\n"
                ),
                (
                    "b.ts",
                    "namespace N { export const x = \"s\"; }\nexport const val = N;\n"
                ),
                (
                    "c.ts",
                    "import { val as va } from \"./a\";\nimport { val as vb } from \"./b\";\nlet v: typeof va;\nv = vb;\n"
                ),
            ]),
            [(
                "c.ts".to_owned(),
                2719,
                84,
                1,
                "Type 'typeof N' is not assignable to type 'typeof N'. Two different types with this name exist, but they are unrelated."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn type_parameter_name_collision_takes_the_2719_face() {
        // Type parameters never chain (lookupSymbolChainWorker 52946:
        // isTypeParameter forces [symbol]), so shadowed same-name
        // parameters stay `T` under the re-render and the head swaps
        // to 2719 (oracle-probed).
        assert_eq!(
            checked_diags(
                "function f<T>(a: T) {\n    return function g<T>(b: T): T {\n        return a;\n    };\n}\n"
            ),
            [(
                2719,
                66,
                6,
                "Type 'T' is not assignable to type 'T'. Two different types with this name exist, but they are unrelated."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn inherited_signature_type_parameter_collision_keeps_the_2719_chain() {
        fn flatten(
            chain: &tsrs2_diags::MessageChain,
            codes: &mut Vec<u32>,
            texts: &mut Vec<String>,
        ) {
            codes.push(chain.code);
            texts.push(chain.text.clone());
            for child in &chain.next {
                flatten(child, codes, texts);
            }
        }

        // The source `T` belongs to I while the target `T` belongs to
        // A's generic call signature. UseFullyQualifiedType must not
        // turn the former into `I.T`: tsc's type-parameter short
        // circuit keeps both names bare, selects 2719, then appends
        // the target-parameter constraint reason (5082).
        let (codes, texts) = with_program_state(
            &[(
                "a.ts",
                "interface A { a: <T>(x: T) => void; }\n\
                 interface I<T> extends A { a: (x: T) => void; }\n",
            )],
            &CompilerOptions::default(),
            |state| {
                state.check_source_file(0);
                let diagnostic = state
                    .diagnostics
                    .iter()
                    .find(|diagnostic| diagnostic.code() == 2430)
                    .expect("inheritance relation reports 2430");
                let mut codes = Vec::new();
                let mut texts = Vec::new();
                flatten(&diagnostic.message, &mut codes, &mut texts);
                (codes, texts)
            },
        );
        assert_eq!(codes, [2430, 2326, 2322, 2328, 2719, 5082]);
        assert_eq!(
            texts,
            [
                "Interface 'I<T>' incorrectly extends interface 'A'.",
                "Types of property 'a' are incompatible.",
                "Type '(x: T) => void' is not assignable to type '<T>(x: T) => void'.",
                "Types of parameters 'x' and 'x' are incompatible.",
                "Type 'T' is not assignable to type 'T'. Two different types with this name exist, but they are unrelated.",
                "'T' could be instantiated with an arbitrary type which could be unrelated to 'T'.",
            ]
        );
    }

    #[test]
    fn source_file_specifier_roots_at_the_program_cwd() {
        // The oracle host absolutizes every fileName against the
        // ProgramJson cwd (program-host.mjs absoluteProgramFileName),
        // so the extension-free source-file specifier renders
        // cwd-rooted (oracle-probed under @currentDirectory: /src:
        // `typeof import("/src/b")`).
        assert_eq!(
            program_diags_with(
                &[
                    ("b.ts", "export const bee = 1;\n"),
                    ("a.ts", "import * as b from \"./b\";\nb.nope;\n"),
                ],
                &CompilerOptions::default(),
                "/src"
            ),
            [(
                "a.ts".to_owned(),
                2339,
                28,
                4,
                "Property 'nope' does not exist on type 'typeof import(\"/src/b\")'.".to_owned()
            )]
        );
    }

    #[test]
    fn fully_qualified_specifier_roots_at_the_program_cwd() {
        // The cwd rooting rides the chain faces too (oracle-probed:
        // `typeof import("/src/b").N` vs `typeof import("/src/a").N`).
        assert_eq!(
            program_diags_with(
                &[
                    ("a.ts", "export namespace N { export const x = 1; }\n"),
                    ("b.ts", "export namespace N { export const x = \"s\"; }\n"),
                    (
                        "c.ts",
                        "import { N as NA } from \"./a\";\nimport { N as NB } from \"./b\";\nlet v: typeof NA;\nv = NB;\n"
                    ),
                ],
                &CompilerOptions::default(),
                "/src"
            ),
            [(
                "c.ts".to_owned(),
                2322,
                80,
                1,
                "Type 'typeof import(\"/src/b\").N' is not assignable to type 'typeof import(\"/src/a\").N'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn module_specifier_needing_escapes_stays_behind_the_curtain() {
        // tsc prints `typeof import("a\"b")` (the printer's
        // escapeString over the synthesized specifier literal); escape
        // rewriting stays behind the curtain
        // (string_literal_name_slice posture), so the 2339 is
        // suppressed rather than misprinted as `import("a"b")`.
        assert_eq!(
            program_diags(&[
                (
                    "d.d.ts",
                    "declare module \"a\\\"b\" { export const x: number; }\n"
                ),
                ("main.ts", "import * as m from \"a\\\"b\";\nm.y;\n"),
            ]),
            []
        );
    }

    #[test]
    fn module_export_alias_over_merged_local_is_a_known_value_property() {
        // The NEW_FP family this slice fixed at source: `export { A }`
        // over a local that merges a type-only import alias with a
        // const is a VALUE property of the module face — both
        // isKnownProperty (via getPropertyOfObjectType) and
        // getNamedMembers gate through the alias-FOLLOWING
        // symbolIsValue (50092-50094), so the object literal below
        // reports NO 2353 (tsc emits only a 6133 unused-suggestion
        // here; that band's absence is a pre-existing suggestion-side
        // FN, not part of this pin).
        assert_eq!(
            program_diags(&[
                ("z.ts", "interface A {}\nexport type { A };\n"),
                (
                    "a.ts",
                    "import { A } from './z';\nconst A = 0;\nexport { A };\nexport class B {};\n"
                ),
                (
                    "b.ts",
                    "import * as types from './a';\nlet t: typeof types = {\n  A: undefined as any,\n  B: undefined as any,\n};\n"
                ),
            ]),
            []
        );
        // The properties view itself carries the alias export.
        with_program_state(
            &[
                ("z.ts", "interface A {}\nexport type { A };\n"),
                (
                    "a.ts",
                    "import { A } from './z';\nconst A = 0;\nexport { A };\nexport class B {};\n",
                ),
            ],
            &CompilerOptions::default(),
            |state| {
                let root = state.binder.source(1).root;
                let module_symbol = state.binder.node_symbol(root).expect("module symbol");
                let module_type = state
                    .get_type_of_symbol(module_symbol)
                    .expect("module type");
                let names: Vec<String> = state
                    .get_properties_of_object_type_owned(module_type)
                    .expect("properties")
                    .into_iter()
                    .map(|p| state.symbol_display_name(p))
                    .collect();
                assert_eq!(names, ["A", "B"]);
            },
        );
    }

    #[test]
    fn expando_namespace_cross_file_merge_keeps_name_precision() {
        // The amalgamated-duplicates merge clones per-file symbols
        // into fresh program symbols; the stage-3.4c expando-record
        // consults follow the merge sources, so assigned members
        // (p1) suppress, namespace exports (p2) resolve, and an
        // unassigned name still reports with the merged `typeof EM`
        // face. The cross-file fn+ns merge itself is tsc error 2433.
        assert_eq!(
            program_diags(&[
                (
                    "expando.ts",
                    "function EM(n: number) { return n }\nEM.p1 = 111;\nvar r1 = EM.p1;\nvar r2 = EM.p2;\nEM.zzz;\n"
                ),
                ("ns.ts", "namespace EM { export var p2 = 222 }\n"),
            ]),
            [
                (
                    "expando.ts".to_owned(),
                    2339,
                    84,
                    3,
                    "Property 'zzz' does not exist on type 'typeof EM'.".to_owned()
                ),
                (
                    "ns.ts".to_owned(),
                    2433,
                    10,
                    2,
                    "A namespace declaration cannot be in a different file from a class or function with which it is merged.".to_owned()
                )
            ]
        );
    }

    // ---- 9.3b4 type-operator display pins (all rows oracle-probed
    // byte-exact against vendored 6.0.3, noLib; strict unless noted;
    // target-position annotations because source-position operator
    // types generalize to their constraints in reportRelationError) ----

    #[test]
    fn keyof_faces_render_the_type_operator_arm() {
        // f2: keyof (keyof T) resolves through the apparent type
        // (never under noLib) — nesting is display-covered via the
        // g4 indexed-access object below. f3: keyof (T & U)
        // distributes into a union whose TypeOperator members join
        // bare. f4: the nullable-candidate substitution (65185)
        // reports against the stripped `keyof T`. f5: TypeOperator
        // joins an intersection bare.
        assert_eq!(
            checked_diags(
                "\nfunction f1<T>(x: number) { const y: keyof T = x; }\nfunction f2<T>(x: number) { const y: keyof keyof T = x; }\nfunction f3<T, U>(x: number) { const y: keyof (T & U) = x; }\nfunction f4<T>(x: number) { const y: keyof T | null = x; }\nfunction f5<T, U>(x: number) { const y: keyof T & U = x; }\n"
            ),
            [
                (
                    2322,
                    35,
                    1,
                    "Type 'number' is not assignable to type 'keyof T'.".to_owned()
                ),
                (
                    2322,
                    87,
                    1,
                    "Type 'number' is not assignable to type 'never'.".to_owned()
                ),
                (
                    2322,
                    148,
                    1,
                    "Type 'number' is not assignable to type 'keyof T | keyof U'.".to_owned()
                ),
                (
                    2322,
                    206,
                    1,
                    "Type 'number' is not assignable to type 'keyof T'.".to_owned()
                ),
                (
                    2322,
                    268,
                    1,
                    "Type 'number' is not assignable to type 'keyof T & U'.".to_owned()
                ),
            ]
        );
    }

    // ---- 9.3b5 display special tail (all oracle-probed byte-exact;
    // probe-f/probe-b batches in the session scratchpad) ----

    #[test]
    fn operator_error_retries_identical_names_fully_qualified_and_keeps_them() {
        // getTypeNamesForErrorDisplay 50751-50754: equal renders retry
        // through getTypeNameForErrorDisplay and the retried texts are
        // used EVEN IF STILL EQUAL — same-type operands print
        // `'symbol' and 'symbol'`; tsc has no third fallback.
        assert_eq!(
            checked_diags("declare const s: symbol;\nvar r = s + s;\n"),
            [(
                2365,
                33,
                5,
                "Operator '+' cannot be applied to types 'symbol' and 'symbol'.".to_owned()
            )]
        );
    }

    #[test]
    fn class_extends_heritage_flows_2454_and_reports_2507_empty_face() {
        // The extends expression of a CLASS is expression context
        // (isExpressionWithTypeArgumentsInClassExtendsClause) — its
        // identifier flow-stamps, so the unassigned `x` reports 2454;
        // the 2507 face renders the canonical emptyTypeLiteralType as
        // `{}` and the errorType continuation replaces the old
        // curtain unwind.
        assert_eq!(
            checked_diags("var x: {};\nclass C6 extends x { }\n"),
            [
                (
                    2454,
                    28,
                    1,
                    "Variable 'x' is used before being assigned.".to_owned()
                ),
                (
                    2507,
                    28,
                    1,
                    "Type '{}' is not a constructor function type.".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn extends_interface_reports_2689_before_the_reprobe_gate() {
        // checkAndReportErrorForExtendingInterface is SECOND in the
        // 48114 resolveName failure chain — ahead of the port's
        // all-meanings re-probe gate, which used to swallow the report
        // because I resolves under the Interface meaning.
        assert_eq!(
            checked_diags("interface I {\n    foo: string;\n}\nclass C extends I { }\n"),
            [(
                2689,
                49,
                1,
                "Cannot extend an interface 'I'. Did you mean 'implements'?".to_owned()
            )]
        );
    }

    #[test]
    fn type_parameter_base_reports_2507_with_did_you_mean_related() {
        // 57172-57183: a TypeParameter base constructor adds the 2735
        // related info anchored at declarations[0], with the
        // constraint's construct return (unknownType fallback).
        with_program_state(
            &[(
                "a.ts",
                "function f<T>(ctor: T) { class C extends ctor { } return C; }\n",
            )],
            &CompilerOptions::default(),
            |state| {
                state.check_source_file(0);
                let row = state
                    .diagnostics
                    .iter()
                    .find(|diag| diag.code() == 2507)
                    .expect("2507 emitted");
                assert_eq!(
                    row.message_text(),
                    "Type 'T' is not a constructor function type."
                );
                assert_eq!(row.start, Some(41));
                assert_eq!(row.related.len(), 1);
                assert_eq!(
                    row.related[0].message.text,
                    "Did you mean for 'T' to be constrained to type 'new (...args: any[]) => unknown'?"
                );
                assert_eq!(row.related[0].start, Some(11));
            },
        );
    }

    #[test]
    fn invalid_base_constructor_return_reports_2509_and_continues() {
        // 57277-57286: the 2509 head renders through the display slice
        // and resolution continues with the emptyArray sentinel.
        assert_eq!(
            checked_diags("declare const x: new () => number;\nclass C extends x { }\n"),
            [(
                2509,
                51,
                1,
                "Base constructor return type 'number' is not an object type or intersection of object types with statically known members."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn origin_intersection_of_unions_renders_the_syntactic_face() {
        // 51542-51544: the denormalized union substitutes its ORIGIN
        // wholesale — `(A | B) & (C | D)` prints the syntactic shape
        // with union members parenthesized by the intersection rule.
        // (2454 lands first in sink order: checkIdentifier runs before
        // the assignment relation.)
        assert_eq!(
            checked_diags(
                "interface A { a: string }\ninterface B { b: string }\ninterface C { c: string }\ninterface D { d: string }\nvar y: (A | B) & (C | D);\nvar x: A & B;\ny = x;\n"
            ),
            [
                (
                    2454,
                    148,
                    1,
                    "Variable 'x' is used before being assigned.".to_owned()
                ),
                (
                    2322,
                    144,
                    1,
                    "Type 'A & B' is not assignable to type '(A | B) & (C | D)'.".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn origin_with_instantiable_members_stays_curtained() {
        // The narrowed verdict shield: `T & U ⊆ (A | B) & T & U` holds
        // in tsc through a normalized-intersection path the port lacks
        // (T & U ⊆ 2 passes standalone but fails inside the
        // intersection-target walk), so instantiable-membered origins
        // keep the curtain — the wrong verdict must not report.
        assert_eq!(
            checked_diags(
                "type A = 1 | 2;\ntype B = 2 | 3;\nfunction f2<T extends A, U extends B>(ab: T & U): (A | B) & T & U { return ab; }\n"
            ),
            []
        );
    }

    #[test]
    fn all_consumed_object_rest_renders_the_empty_face() {
        // getRestType results are BORN resolved
        // (make_resolved_anonymous_type) — an all-consumed rest is a
        // REAL `{}` and the 2741 single-missing face renders it.
        assert_eq!(
            checked_diags(
                "declare const s: { a: number };\nconst { a, ...r } = s;\nconst q: { b: string } = r;\n"
            ),
            [(
                2741,
                61,
                1,
                "Property 'b' is missing in type '{}' but required in type '{ b: string; }'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn unique_symbol_relation_faces_take_the_fq_typeof_chain() {
        // reportRelationError's GENERALIZED render is
        // getTypeNameForErrorDisplay (UseFullyQualifiedType) and
        // getBaseTypeOfLiteralType passes unique symbols through
        // unchanged — the namespace chain qualifies.
        assert_eq!(
            checked_diags(
                "declare namespace NS { const tp: unique symbol; }\nvar z: object = NS.tp;\n"
            ),
            [(
                2322,
                54,
                1,
                "Type 'typeof NS.tp' is not assignable to type 'object'.".to_owned()
            )]
        );
    }

    #[test]
    fn unique_symbol_plain_face_is_the_operator_keyword() {
        // typeToString's DEFAULT flags include AllowUniqueESSymbolType
        // (50717) — with generalization skipped (singleton-capable
        // target) the plain render is the `unique symbol` operator.
        assert_eq!(
            checked_diags("declare const local: unique symbol;\nvar z: \"a\" | \"b\" = local;\n"),
            [(
                2322,
                40,
                1,
                "Type 'unique symbol' is not assignable to type '\"a\" | \"b\"'.".to_owned()
            )]
        );
    }

    #[test]
    fn string_literal_faces_spell_escapes_but_not_non_ascii() {
        // 51401-51403: NoAsciiEscaping — escapeString('"') only.
        assert_eq!(
            checked_diags("var x: \"AB\\r\\nC\" = \"AB\\nC\";\n"),
            [(
                2322,
                4,
                1,
                "Type '\"AB\\nC\"' is not assignable to type '\"AB\\r\\nC\"'.".to_owned()
            )]
        );
    }

    #[test]
    fn unique_symbol_member_name_renders_the_computed_face() {
        // 53427-53429: nameType UniqueESSymbol →
        // createComputedPropertyName(symbolToExpression(symbol, Value))
        // — the [symbol]-chain face `[sym]`.
        assert_eq!(
            checked_diags(
                "declare const sym: unique symbol;\nconst o = { [sym]: 0 };\nconst t: { [key: symbol]: string } = o;\n"
            ),
            [(
                2322,
                64,
                1,
                "Type '{ [sym]: number; }' is not assignable to type '{ [key: symbol]: string; }'."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn instantiation_expression_type_renders_structurally() {
        // 51755-51770: the error path falls through the
        // InstantiationExpressionType arm to the ordinary structural
        // walk (the TypeQuery reuse leg needs an enclosing-armed
        // context and the placeholder is the recursion guard).
        assert_eq!(
            checked_diags(
                "declare const f: { (): number; g<U>(): U; };\nconst h = f<number>;\n"
            ),
            [(
                2635,
                57,
                6,
                "Type '{ (): number; g<U>(): U; }' has no signatures for which the type argument list is applicable."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn json_declaration_twin_precedes_the_json_resolution() {
        // A present <stem>.d.json.ts twin wins the TYPES probe. The
        // false option reports getResolutionDiagnostic's 6263 without
        // loading the declaration; true loads its string default.
        // Without the twin the JSON literal shape resolves and relates.
        let base_options = CompilerOptions {
            resolve_json_module: Some(true),
            // ModuleKind.CommonJS
            module: Some(1),
            ..CompilerOptions::default()
        };
        let run =
            |files: &[(&str, &str)], options: &CompilerOptions| -> Vec<(u32, u32, u32, String)> {
                let names: Vec<String> = files.iter().map(|(name, _)| (*name).to_owned()).collect();
                with_program_state(files, options, |state| {
                    // The unit harness has no ProgramJson host.
                    state.host_file_paths = names.iter().cloned().collect();
                    state.check_source_file(0);
                    diag_rows(state)
                })
            };
        let with_twin = [
            (
                "/main.ts",
                "import data from \"./data.json\";\nlet x: string = data;\n",
            ),
            ("/data.json", "{}"),
            (
                "/data.d.json.ts",
                "declare var val: string;\nexport default val;\n",
            ),
        ];
        assert_eq!(
            run(
                &with_twin,
                &CompilerOptions {
                    allow_arbitrary_extensions: Some(false),
                    ..base_options.clone()
                },
            ),
            [(
                6263,
                17,
                13,
                "Module './data.json' was resolved to '/data.d.json.ts', but '--allowArbitraryExtensions' is not set.".to_owned(),
            )]
        );
        assert_eq!(
            run(
                &with_twin,
                &CompilerOptions {
                    allow_arbitrary_extensions: Some(true),
                    ..base_options.clone()
                },
            ),
            []
        );
        let without_twin = run(
            &[
                (
                    "/main.ts",
                    "import data from \"./data.json\";\nlet x: string = data;\n",
                ),
                ("/data.json", "{}"),
            ],
            &base_options,
        );
        assert_eq!(
            without_twin,
            [(
                2322,
                36,
                1,
                "Type '{}' is not assignable to type 'string'.".to_owned()
            )]
        );
    }

    #[test]
    fn indexed_access_faces_parenthesize_the_object_side_only() {
        // g2: chained accesses join bare (the kind is listed in no
        // parenthesizer rule); g3/g4: union and TypeOperator OBJECT
        // sides wrap (parenthesizeNonArrayTypeOfPostfixType); g5: a
        // literal index over a template resolves through the apparent
        // type (2339 on `{}` under noLib); g7: the INDEX side joins
        // bare.
        assert_eq!(
            checked_diags(
                "\nfunction g1<T, K extends keyof T>(x: number) { const y: T[K] = x; }\nfunction g2<T, K extends keyof T, K2 extends keyof T[K]>(x: number) { const y: T[K][K2] = x; }\nfunction g3<T, U, K extends keyof (T | U)>(x: number) { const y: (T | U)[K] = x; }\nfunction g4<T, K extends keyof keyof T>(x: number) { const y: (keyof T)[K] = x; }\nfunction g5<T extends string>(x: number) { const y: `a${T}`[\"x\"] = x; }\nfunction g6<T, K extends keyof T>(x: number) { const y: T[K] | null = x; }\nfunction g7<T, K extends keyof T>(x: number) { const y: T[keyof T] = x; }\n"
            ),
            [
                (
                    2322,
                    54,
                    1,
                    "Type 'number' is not assignable to type 'T[K]'.".to_owned()
                ),
                (
                    2322,
                    145,
                    1,
                    "Type 'number' is not assignable to type 'T[K][K2]'.".to_owned()
                ),
                (
                    2322,
                    226,
                    1,
                    "Type 'number' is not assignable to type '(T | U)[K]'.".to_owned()
                ),
                (
                    2322,
                    306,
                    1,
                    "Type 'number' is not assignable to type '(keyof T)[K]'.".to_owned()
                ),
                (
                    2339,
                    389,
                    3,
                    "Property 'x' does not exist on type '{}'.".to_owned()
                ),
                (
                    2322,
                    454,
                    1,
                    "Type 'number' is not assignable to type 'T[K]'.".to_owned()
                ),
                (
                    2322,
                    529,
                    1,
                    "Type 'number' is not assignable to type 'T[keyof T]'.".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn template_literal_faces_render_head_spans_and_tail() {
        // h3: a union span distributes at construction — the display
        // renders the resulting union of templates, members bare;
        // h4: nullable-candidate substitution strips to the bare
        // template; h5: adjacent spans share an empty middle text.
        assert_eq!(
            checked_diags(
                "\nfunction h1<T extends string>(x: number) { const y: `a${T}b` = x; }\nfunction h2<T extends string>(x: number) { const y: `${T}` = x; }\nfunction h3<T extends string, U extends string>(x: number) { const y: `a${T | U}b` = x; }\nfunction h4<T extends string>(x: number) { const y: `a${T}` | null = x; }\nfunction h5<T extends string, U extends string>(x: number) { const y: `${T}${U}` = x; }\n"
            ),
            [
                (
                    2322,
                    50,
                    1,
                    "Type 'number' is not assignable to type '`a${T}b`'.".to_owned()
                ),
                (
                    2322,
                    118,
                    1,
                    "Type 'number' is not assignable to type '`${T}`'.".to_owned()
                ),
                (
                    2322,
                    202,
                    1,
                    "Type 'number' is not assignable to type '`a${T}b` | `a${U}b`'.".to_owned()
                ),
                (
                    2322,
                    274,
                    1,
                    "Type 'number' is not assignable to type '`a${T}`'.".to_owned()
                ),
                (
                    2322,
                    366,
                    1,
                    "Type 'number' is not assignable to type '`${T}${U}`'.".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn template_literal_texts_reescape_like_the_printer() {
        // Cooked texts re-escape through template_text_raw: CRLF is
        // the map's pair entry, a null before a digit prints `\x00`
        // (getReplacement's lookahead), unmapped controls and
        // non-ASCII take `\uXXXX` (astral = two surrogate escapes),
        // and `$`/`{` are identity when not forming `${`.
        assert_eq!(
            checked_diags(
                "\nfunction e1<T extends string>(x: number) { const y: `a\\r\\nb${T}` = x; }\nfunction e2<T extends string>(x: number) { const y: `a\\u0000b${T}` = x; }\nfunction e3<T extends string>(x: number) { const y: `a\\u00001${T}` = x; }\nfunction e4<T extends string>(x: number) { const y: `a\\u0001b${T}` = x; }\nfunction e5<T extends string>(x: number) { const y: `あ${T}` = x; }\nfunction e6<T extends string>(x: number) { const y: `😀${T}` = x; }\nfunction e7<T extends string>(x: number) { const y: `a\\rb${T}` = x; }\nfunction e8<T extends string>(x: number) { const y: `a$b{c${T}` = x; }\n"
            ),
            [
                (
                    2322,
                    50,
                    1,
                    "Type 'number' is not assignable to type '`a\\r\\nb${T}`'.".to_owned()
                ),
                (
                    2322,
                    122,
                    1,
                    "Type 'number' is not assignable to type '`a\\0b${T}`'.".to_owned()
                ),
                (
                    2322,
                    196,
                    1,
                    "Type 'number' is not assignable to type '`a\\x001${T}`'.".to_owned()
                ),
                (
                    2322,
                    270,
                    1,
                    "Type 'number' is not assignable to type '`a\\u0001b${T}`'.".to_owned()
                ),
                (
                    2322,
                    344,
                    1,
                    "Type 'number' is not assignable to type '`\\u3042${T}`'.".to_owned()
                ),
                (
                    2322,
                    411,
                    1,
                    "Type 'number' is not assignable to type '`\\uD83D\\uDE00${T}`'.".to_owned()
                ),
                (
                    2322,
                    479,
                    1,
                    "Type 'number' is not assignable to type '`a\\rb${T}`'.".to_owned()
                ),
                (
                    2322,
                    549,
                    1,
                    "Type 'number' is not assignable to type '`a$b{c${T}`'.".to_owned()
                ),
            ]
        );
        assert_eq!(
            checked_diags(
                "function s<T extends string>(x: number) { const y: `\\uD800${T}` = x; }"
            ),
            [(
                2322,
                48,
                1,
                "Type 'number' is not assignable to type '`\\uD800${T}`'.".to_owned()
            )]
        );
    }

    #[test]
    fn string_mapping_faces_render_the_intrinsic_reference() {
        // Local intrinsic aliases stand in for the lib set (same
        // symbol-name route). m4: keyof over a string mapping
        // resolves through the apparent type (never under noLib);
        // m5: a mapping nests bare inside a template span.
        assert_eq!(
            checked_diags(
                "\ntype Uppercase<S extends string> = intrinsic;\ntype Lowercase<S extends string> = intrinsic;\ntype Capitalize<S extends string> = intrinsic;\n\nfunction m1<T extends string>(x: number) { const y: Uppercase<T> = x; }\nfunction m2<T extends string>(x: number) { const y: Lowercase<Uppercase<T>> = x; }\nfunction m3<T extends string>(x: number) { const y: Uppercase<T> | null = x; }\nfunction m4<T extends string>(x: number) { const y: keyof Uppercase<T> = x; }\nfunction m5<T extends string>(x: number) { const y: `a${Uppercase<T>}b` = x; }\n"
            ),
            [
                (
                    2322,
                    190,
                    1,
                    "Type 'number' is not assignable to type 'Uppercase<T>'.".to_owned()
                ),
                (
                    2322,
                    262,
                    1,
                    "Type 'number' is not assignable to type 'Lowercase<Uppercase<T>>'.".to_owned()
                ),
                (
                    2322,
                    345,
                    1,
                    "Type 'number' is not assignable to type 'Uppercase<T>'.".to_owned()
                ),
                (
                    2322,
                    424,
                    1,
                    "Type 'number' is not assignable to type 'never'.".to_owned()
                ),
                (
                    2322,
                    502,
                    1,
                    "Type 'number' is not assignable to type '`a${Uppercase<T>}b`'.".to_owned()
                ),
            ]
        );
        assert_eq!(
            checked_diags(
                "type Uppercase<S extends string> = intrinsic;\nfunction s<T extends string>(x: number) { const y: Uppercase<`\\uD800a${T}`> = x; }"
            ),
            [(
                2322,
                94,
                1,
                "Type 'number' is not assignable to type '`\\uD800A${Uppercase<T>}`'.".to_owned()
            )]
        );
    }

    #[test]
    fn any_intrinsics_hide_internal_names_in_type_display() {
        with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
            let error = state.tables.intrinsics.error;
            let unresolved = state.tables.intrinsics.unresolved;
            let any = state.tables.intrinsics.any;
            let intrinsic_marker = state.tables.intrinsics.intrinsic_marker;
            let unknown = state.tables.intrinsics.unknown;

            assert_eq!(state.type_to_string_slice(error).unwrap(), "any");
            assert_eq!(state.type_to_string_slice(unresolved).unwrap(), "any");
            assert_eq!(state.type_to_string_slice(any).unwrap(), "any");
            assert_eq!(
                state.type_to_string_slice(intrinsic_marker).unwrap(),
                "intrinsic"
            );
            assert_eq!(state.type_to_string_slice(unknown).unwrap(), "unknown");
        });
    }

    #[test]
    fn operator_faces_in_array_positions_follow_the_postfix_rule() {
        // Local Array/ReadonlyArray interfaces supply the display
        // sugar targets. TypeOperator elements wrap ((keyof T)[],
        // and again under the readonly face); indexed-access,
        // template, and reference elements join bare.
        assert_eq!(
            checked_diags(
                "\ninterface Array<T> { length: number; }\ninterface ReadonlyArray<T> { length: number; }\n\ntype Uppercase<S extends string> = intrinsic;\ntype Lowercase<S extends string> = intrinsic;\ntype Capitalize<S extends string> = intrinsic;\n\nfunction a1<T>(x: number) { const y: (keyof T)[] = x; }\nfunction a2<T, K extends keyof T>(x: number) { const y: T[K][] = x; }\nfunction a3<T extends string>(x: number) { const y: `a${T}`[] = x; }\nfunction a4<T extends string>(x: number) { const y: Uppercase<T>[] = x; }\nfunction a5<T>(x: number) { const y: readonly (keyof T)[] = x; }\n"
            ),
            [
                (
                    2322,
                    262,
                    1,
                    "Type 'number' is not assignable to type '(keyof T)[]'.".to_owned()
                ),
                (
                    2322,
                    337,
                    1,
                    "Type 'number' is not assignable to type 'T[K][]'.".to_owned()
                ),
                (
                    2322,
                    403,
                    1,
                    "Type 'number' is not assignable to type '`a${T}`[]'.".to_owned()
                ),
                (
                    2322,
                    472,
                    1,
                    "Type 'number' is not assignable to type 'Uppercase<T>[]'.".to_owned()
                ),
                (
                    2322,
                    531,
                    1,
                    "Type 'number' is not assignable to type 'readonly (keyof T)[]'.".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn iterable_protocol_faces_elide_only_trailing_default_arguments() {
        assert_eq!(
            checked_diags(
                "\
interface Iterable<T, TReturn = any, TNext = any> {}
interface IterableIterator<T, TReturn = any, TNext = any> {}
interface AsyncIterable<T, TReturn = any, TNext = any> {}
interface AsyncIterableIterator<T, TReturn = any, TNext = any> {}
interface Generator<T, TReturn = any, TNext = any> {}
interface Other<T, U = any> {}
declare let a: Iterable<string, any, any>;
declare let b: IterableIterator<string, void, any>;
declare let c: AsyncIterable<string, any, any>;
declare let d: AsyncIterableIterator<string, void, any>;
declare let e: Generator<string, any, any>;
declare let f: Other<string, any>;
const aa: number = a;
const bb: number = b;
const cc: number = c;
const dd: number = d;
const ee: number = e;
const ff: number = f;
"
            ),
            [
                (
                    2322,
                    608,
                    2,
                    "Type 'Iterable<string>' is not assignable to type 'number'.".to_owned()
                ),
                (
                    2322,
                    630,
                    2,
                    "Type 'IterableIterator<string, void>' is not assignable to type 'number'."
                        .to_owned()
                ),
                (
                    2322,
                    652,
                    2,
                    "Type 'AsyncIterable<string>' is not assignable to type 'number'.".to_owned()
                ),
                (
                    2322,
                    674,
                    2,
                    "Type 'AsyncIterableIterator<string, void>' is not assignable to type 'number'."
                        .to_owned()
                ),
                (
                    2322,
                    696,
                    2,
                    "Type 'Generator<string, any, any>' is not assignable to type 'number'."
                        .to_owned()
                ),
                (
                    2322,
                    718,
                    2,
                    "Type 'Other<string, any>' is not assignable to type 'number'.".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn global_array_reference_with_appended_this_keeps_array_type_sugar() {
        assert_eq!(
            checked_diags(
                "\
interface Array<T> { length: number; }
type T3 = number[];
interface I3 extends T3 { length: string }
"
            ),
            [(
                2430,
                69,
                2,
                "Interface 'I3' incorrectly extends interface 'number[]'.".to_owned()
            )]
        );
    }

    #[test]
    fn operator_faces_in_optional_tuple_positions_split_by_kind() {
        // strict:false keeps optional elements bare (no `| undefined`
        // widening), exposing parenthesizeTypeOfOptionalType per
        // kind: TypeOperator wraps, indexed-access and template
        // faces join bare.
        let options = CompilerOptions {
            strict: Some(false),
            ..CompilerOptions::default()
        };
        let diags = with_program_state(
            &[(
                "a.ts",
                "\nfunction o1<T>(x: number) { const y: [(keyof T)?] = x; }\nfunction o2<T, K extends keyof T>(x: number) { const y: [T[K]?] = x; }\nfunction o3<T extends string>(x: number) { const y: [`a${T}`?] = x; }\n",
            )],
            &options,
            |state| {
                state.check_source_file(0);
                diag_rows(state)
            },
        );
        assert_eq!(
            diags,
            [
                (
                    2322,
                    35,
                    1,
                    "Type 'number' is not assignable to type '[(keyof T)?]'.".to_owned()
                ),
                (
                    2322,
                    111,
                    1,
                    "Type 'number' is not assignable to type '[T[K]?]'.".to_owned()
                ),
                (
                    2322,
                    178,
                    1,
                    "Type 'number' is not assignable to type '[`a${T}`?]'.".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn mapped_name_type_and_readonly_index_write_are_checked() {
        let rows = checked_diags(
            "type Bad<T extends string> = { [K in T as {}]: T };\n\
             function write<T, K extends keyof T>(\n\
               target: { readonly [P in keyof T]: T[P] }, key: K, value: T[K]\n\
             ) { target[key] = value; }\n",
        );
        let codes: Vec<u32> = rows.iter().map(|row| row.0).collect();
        assert!(codes.contains(&2322), "{rows:?}");
        assert!(codes.contains(&2542), "{rows:?}");
    }

    #[test]
    fn template_number_pattern_admits_the_tonumber_coercion_forms() {
        // Audit pin (oracle-probed byte-exact): `${number}` placeholder
        // validity rides the FULL JS ToNumber — radix forms 0b/0o/0x
        // and exponent forms admit; "other" and the JS-rejected "inf"
        // spelling refuse. The M4-era local coercion slice dropped
        // 0b/0o, and the 9.3b4 template display unmasked the stale
        // verdicts as templateLiteralTypesPatterns 2345 fabrications
        // (the reporting Err had contained them).
        assert_eq!(
            checked_diags(
                "declare function numbers(x: `${number}`): void;\nnumbers(\"1\");\nnumbers(\"-1\");\nnumbers(\"0\");\nnumbers(\"0b1\");\nnumbers(\"0o1\");\nnumbers(\"0x1\");\nnumbers(\"1e21\");\nnumbers(\"other\");\nnumbers(\"inf\");\nnumbers(\"0x100000000000000000000000000000000\");\nnumbers(\"0b111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111\");\nnumbers(\"0o77777777777777777777777777777777777777777777777777\");\n",
            ),
            [
                (
                    2345,
                    164,
                    7,
                    "Argument of type '\"other\"' is not assignable to parameter of type '`${number}`'.".to_owned()
                ),
                (
                    2345,
                    182,
                    5,
                    "Argument of type '\"inf\"' is not assignable to parameter of type '`${number}`'.".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn template_text_escape_tables_cover_the_map() {
        // Spec twins for cooked texts a .ts fixture cannot spell
        // directly (the scanner normalizes raw CR/CRLF to LF and the
        // source-expressible escapes ride the probe pins above):
        // the vendored tables at _tsc.js:16275-16295 — mapped
        // entries, the CRLF pair, LF identity, the null lookahead
        // against a non-digit, and per-unit surrogate escapes.
        assert_eq!(super::template_text_raw("a\r\nb"), "a\\r\\nb");
        assert_eq!(super::template_text_raw("a\rb"), "a\\rb");
        assert_eq!(super::template_text_raw("a\nb"), "a\nb");
        assert_eq!(
            super::template_text_raw("a\tb\u{8}\u{B}\u{C}"),
            "a\\tb\\b\\v\\f"
        );
        assert_eq!(super::template_text_raw("a\0b"), "a\\0b");
        assert_eq!(super::template_text_raw("a\u{0}1"), "a\\x001");
        assert_eq!(super::template_text_raw("a\0あ"), "a\\0\\u3042");
        assert_eq!(
            super::template_text_raw("\u{2028}\u{2029}\u{85}"),
            "\\u2028\\u2029\\u0085"
        );
        assert_eq!(super::template_text_raw("\u{1}\u{1F}"), "\\u0001\\u001F");
        assert_eq!(super::template_text_raw("\u{7F}"), "\u{7F}");
        assert_eq!(super::template_text_raw("😀"), "\\uD83D\\uDE00");
        assert_eq!(super::template_text_raw("a`b\\c"), "a\\`b\\\\c");
        assert_eq!(super::template_text_raw("${x}$y{z"), "\\${x}$y{z");
        assert_eq!(super::template_text_raw("$${"), "$\\${");
        assert_eq!(
            super::template_text_utf16_raw(&[0xD800, b'a' as u16, 0xDC00]),
            "\\uD800a\\uDC00"
        );
    }
}

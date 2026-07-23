//! The check driver (M4 5.4): checkSourceFileWorker's two-phase pass —
//! eager statements IN SOURCE ORDER, then the deferred-node drain —
//! plus the first live statement-position checks (type parameter
//! lists and the 2636 variance-annotation probe).
//!
//! Dispatch discipline: checkSourceElementWorker's switch is ported
//! with the FULL kind list; arms whose workers belong to a later M4
//! stage are explicit no-op escapes named after their tsc worker and
//! owner stage (grep `source_element_stub`). An Unsupported unwind
//! abandons the CURRENT element's remaining checks only (an honest
//! FN) — the driver continues with the next element, so one
//! out-of-slice construct never silences a whole file.
//!
//! Grammar checks: checkGrammarStatementInAmbientContext is LIVE from
//! 5.5a (checkExpressionStatement's head; the EmptyStatement/Debugger
//! and checkBlock routes share it); checkGrammarSourceFile and
//! checkGrammarModifiers remain M7-stub hooks (slots exist, emit
//! nothing).
//!
//! The unreachable-code slice (checkSourceElementUnreachable 86763 +
//! the withinUnreachableCode save/restore) is elided whole: its
//! default-options output is suggestion-band (addErrorOrSuggestion
//! isError only under allowUnreachableCode === false, an option absent
//! from CompilerOptions), and its flow arm needs M5's
//! isReachableFlowNode — it lands with M5.

use tsrs2_binder::{node_util, SymbolId};
use tsrs2_diags::{gen as diagnostics, DiagnosticMessage};
use tsrs2_syntax::{for_each_child, NodeData, NodeId, SyntaxKind};
use tsrs2_types::{
    ElementFlags, ModifierFlags, NodeCheckFlags, ObjectFlags, TypeData, TypeFacts, TypeFlags,
    TypeId,
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
    /// - registerForUnusedIdentifiersCheck + the addLazyDiagnostic
    ///   unused-identifiers block: noUnusedLocals/noUnusedParameters
    ///   are absent from CompilerOptions, so unusedIsError is
    ///   constant-false — the band is inert until M7 models the
    ///   options. checkPotentialUncheckedRenamedBindingElementsInTypes
    ///   shares that addLazyDiagnostic block but is NOT option-gated —
    ///   live from 5.8a (eager identity keeps tsc's order: it runs
    ///   after the deferred drain, before checkExternalModuleExports).
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
        // 87028-87040 addLazyDiagnostic block (eager identity): the
        // unused-identifiers drain is M7-inert; the renamed-binding
        // drain runs for non-declaration files, un-option-gated.
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
    /// The represented skipTypeCheckingWorker arm: skipLibCheck omits
    /// semantic checking for declaration files. Bind and
    /// initialization-time diagnostics are filtered at the program
    /// assembly layer so the complete per-file bind/check stream is
    /// suppressed while syntax diagnostics remain visible.
    ///
    /// KNOWN-GAP since M4 (m4-review B31): the other worker arms —
    /// @ts-nocheck, checkJs-off JS files, noCheck — are missing, so
    /// those files are CHECKED and their rows dropped at the output
    /// stage, where tsc never checks them at all. Two exposures: any
    /// file-less diagnostic such a check produces becomes an FP once
    /// the B30 global-merge regime lands (the two land together —
    /// m7-tail-steps.md 8.5), and checking files tsc skips writes
    /// shared caches in an order tsc never runs (an M6-era
    /// order-sensitivity risk).
    fn skip_type_checking(&self, root: NodeId) -> bool {
        self.options.skip_lib_check == Some(true)
            && self.binder.source_of_node(root).is_declaration_file
    }

    /// checkGrammarSourceFile (90323) — M7-stub grammar hook (ambient
    /// top-level declare-modifier grammar).
    fn check_grammar_source_file(&mut self, _root: NodeId) {}

    /// checkGrammarModifiers (89010) — M7-stub grammar hook; the
    /// false return feeds callers' && chains (checkVariableStatement's
    /// grammar ladder sits in tsc's slots). Callers whose FOLLOWERS
    /// tsc suppresses behind a true verdict consult
    /// check_grammar_modifiers_would_report instead.
    pub(crate) fn check_grammar_modifiers(&mut self, _node: NodeId) -> bool {
        false
    }

    /// tsrs-native: checkGrammarModifiers (89010-89325) — the
    /// WOULD-REPORT boolean skeleton: tsc's exact verdict with every
    /// diagnostic elided.
    /// The reported modifier rows themselves stay M7 FNs; this twin
    /// only keeps follower grammar checks in tsc's `||` slots
    /// (heritage-clause walk, type-parameter/parameter lists). Elided
    /// faces, each impossible or options-dead in the conformance
    /// domain: the JSDoc in/out host hop (TS band), and export's
    /// verbatimModuleSyntax-CJS arm (needs the emit-format surface;
    /// a miss there under-suppresses only when the same node ALSO
    /// carries a follower grammar error).
    pub(crate) fn check_grammar_modifiers_would_report(&mut self, node: NodeId) -> bool {
        let source = self.binder.source_of_node(node);
        let node_kind = self.kind_of(node);
        let parent = self.parent_of(node);
        let parent_kind = parent.map(|parent| self.kind_of(parent));
        let modifiers: Vec<NodeId> = self.nodes_of(node_util::modifiers_of(source, node));
        // reportObviousDecoratorErrors (89384): kinds that can never
        // be decorated report on the first decorator.
        let can_have_illegal_decorators = matches!(
            node_kind,
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
        if can_have_illegal_decorators
            && modifiers
                .iter()
                .any(|&modifier| self.kind_of(modifier) == SyntaxKind::Decorator)
        {
            return true;
        }
        // reportObviousModifierErrors (89326): no modifier list →
        // false without the walk; an obviously-illegal first modifier
        // reports; otherwise fall through.
        if modifiers.is_empty() {
            // `!node.modifiers` and the empty array behave alike: the
            // walk below has nothing to do and every tail check needs
            // a flag.
            return false;
        }
        let modifier_kinds: Vec<SyntaxKind> = modifiers
            .iter()
            .map(|&modifier| self.kind_of(modifier))
            .collect();
        // findFirstModifierExcept (89164): the FIRST plain modifier
        // decides — a leading allowed modifier defers to the walk even
        // when an illegal one follows.
        let first_plain_modifier = |except: Option<SyntaxKind>| -> bool {
            modifier_kinds
                .iter()
                .copied()
                .find(|&kind| kind != SyntaxKind::Decorator)
                .is_some_and(|kind| Some(kind) != except)
        };
        match node_kind {
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
            | SyntaxKind::TypeParameter => {}
            SyntaxKind::ClassStaticBlockDeclaration
            | SyntaxKind::PropertyAssignment
            | SyntaxKind::ShorthandPropertyAssignment
            | SyntaxKind::NamespaceExportDeclaration
            | SyntaxKind::MissingDeclaration => {
                if first_plain_modifier(None) {
                    return true;
                }
            }
            _ => {
                if !matches!(
                    parent_kind,
                    Some(SyntaxKind::ModuleBlock) | Some(SyntaxKind::SourceFile)
                ) {
                    let illegal = match node_kind {
                        SyntaxKind::FunctionDeclaration => {
                            first_plain_modifier(Some(SyntaxKind::AsyncKeyword))
                        }
                        SyntaxKind::ClassDeclaration | SyntaxKind::ConstructorType => {
                            first_plain_modifier(Some(SyntaxKind::AbstractKeyword))
                        }
                        SyntaxKind::ClassExpression
                        | SyntaxKind::InterfaceDeclaration
                        | SyntaxKind::TypeAliasDeclaration => first_plain_modifier(None),
                        SyntaxKind::VariableStatement => {
                            let using = match self.data_of(node) {
                                NodeData::VariableStatement(data) => {
                                    data.declaration_list.is_some_and(|list| {
                                        self.node_flags(list) & tsrs2_types::NodeFlags::USING.bits()
                                            != 0
                                    })
                                }
                                _ => false,
                            };
                            if using {
                                first_plain_modifier(Some(SyntaxKind::AwaitKeyword))
                            } else {
                                first_plain_modifier(None)
                            }
                        }
                        SyntaxKind::EnumDeclaration => {
                            first_plain_modifier(Some(SyntaxKind::ConstKeyword))
                        }
                        // Debug.assertNever domain — parse-recovery
                        // shapes answer "no obvious error" and take
                        // the walk.
                        _ => false,
                    };
                    if illegal {
                        return true;
                    }
                }
            }
        }
        // isParameter(node) && parameterIsThisKeyword(node) (89016).
        if node_kind == SyntaxKind::Parameter {
            let is_this = match self.data_of(node) {
                NodeData::Parameter(data) => data
                    .name
                    .is_some_and(|name| self.identifier_text_of(name) == Some("this")),
                _ => false,
            };
            if is_this {
                return true;
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
        let name_is_private_identifier = self
            .name_of_node(node)
            .is_some_and(|name| self.kind_of(name) == SyntaxKind::PrivateIdentifier);
        let parent_is_ambient = parent.is_some_and(|parent| {
            self.node_flags(parent) & tsrs2_types::NodeFlags::AMBIENT.bits() != 0
        });
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
                    // Both message flavors (overload 1249 / 1206)
                    // report.
                    return true;
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
                                return true;
                            }
                        }
                    }
                }
                if flags.bits()
                    & !(ModifierFlags::EXPORT_DEFAULT.bits() | ModifierFlags::DECORATOR.bits())
                    != 0
                {
                    return true;
                }
                if has_leading_decorators && flags.intersects(ModifierFlags::MODIFIER) {
                    // Decorators before AND after export: reports only
                    // when the file has no parse diagnostics.
                    return !self.has_parse_diagnostics(node);
                }
                flags |= ModifierFlags::DECORATOR;
                if !flags.intersects(ModifierFlags::MODIFIER) {
                    has_leading_decorators = true;
                } else if flags.intersects(ModifierFlags::EXPORT) {
                    saw_export_before_decorators = true;
                }
                continue;
            }
            if modifier_kind != SyntaxKind::ReadonlyKeyword {
                if matches!(
                    node_kind,
                    SyntaxKind::PropertySignature | SyntaxKind::MethodSignature
                ) {
                    return true;
                }
                if node_kind == SyntaxKind::IndexSignature
                    && (modifier_kind != SyntaxKind::StaticKeyword || !parent_is_class_like)
                {
                    return true;
                }
            }
            if !matches!(
                modifier_kind,
                SyntaxKind::InKeyword | SyntaxKind::OutKeyword | SyntaxKind::ConstKeyword
            ) && node_kind == SyntaxKind::TypeParameter
            {
                return true;
            }
            match modifier_kind {
                SyntaxKind::ConstKeyword => {
                    if !matches!(
                        node_kind,
                        SyntaxKind::EnumDeclaration | SyntaxKind::TypeParameter
                    ) {
                        return true;
                    }
                    if node_kind == SyntaxKind::TypeParameter
                        && !matches!(
                            parent_kind,
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
                        return true;
                    }
                }
                SyntaxKind::OverrideKeyword => {
                    if flags.intersects(
                        ModifierFlags::OVERRIDE
                            | ModifierFlags::AMBIENT
                            | ModifierFlags::READONLY
                            | ModifierFlags::ACCESSOR
                            | ModifierFlags::ASYNC,
                    ) {
                        return true;
                    }
                    flags |= ModifierFlags::OVERRIDE;
                }
                SyntaxKind::PublicKeyword
                | SyntaxKind::ProtectedKeyword
                | SyntaxKind::PrivateKeyword => {
                    if flags.intersects(
                        ModifierFlags::ACCESSIBILITY_MODIFIER
                            | ModifierFlags::OVERRIDE
                            | ModifierFlags::STATIC
                            | ModifierFlags::ACCESSOR
                            | ModifierFlags::READONLY
                            | ModifierFlags::ASYNC
                            | ModifierFlags::ABSTRACT,
                    ) {
                        return true;
                    }
                    if matches!(
                        parent_kind,
                        Some(SyntaxKind::ModuleBlock) | Some(SyntaxKind::SourceFile)
                    ) {
                        return true;
                    }
                    if name_is_private_identifier {
                        return true;
                    }
                    flags |= match modifier_kind {
                        SyntaxKind::PublicKeyword => ModifierFlags::PUBLIC,
                        SyntaxKind::ProtectedKeyword => ModifierFlags::PROTECTED,
                        _ => ModifierFlags::PRIVATE,
                    };
                }
                SyntaxKind::StaticKeyword => {
                    if flags.intersects(
                        ModifierFlags::STATIC
                            | ModifierFlags::READONLY
                            | ModifierFlags::ASYNC
                            | ModifierFlags::ACCESSOR
                            | ModifierFlags::ABSTRACT
                            | ModifierFlags::OVERRIDE,
                    ) {
                        return true;
                    }
                    if matches!(
                        parent_kind,
                        Some(SyntaxKind::ModuleBlock) | Some(SyntaxKind::SourceFile)
                    ) || node_kind == SyntaxKind::Parameter
                    {
                        return true;
                    }
                    flags |= ModifierFlags::STATIC;
                }
                SyntaxKind::AccessorKeyword => {
                    if flags.intersects(
                        ModifierFlags::ACCESSOR | ModifierFlags::READONLY | ModifierFlags::AMBIENT,
                    ) || node_kind != SyntaxKind::PropertyDeclaration
                    {
                        return true;
                    }
                    flags |= ModifierFlags::ACCESSOR;
                }
                SyntaxKind::ReadonlyKeyword => {
                    if flags.intersects(ModifierFlags::READONLY | ModifierFlags::ACCESSOR) {
                        return true;
                    }
                    if !matches!(
                        node_kind,
                        SyntaxKind::PropertyDeclaration
                            | SyntaxKind::PropertySignature
                            | SyntaxKind::IndexSignature
                            | SyntaxKind::Parameter
                    ) {
                        return true;
                    }
                    flags |= ModifierFlags::READONLY;
                }
                SyntaxKind::ExportKeyword => {
                    // The verbatimModuleSyntax CommonJS arm is elided
                    // (emit-format surface); see the fn header.
                    if flags.intersects(
                        ModifierFlags::EXPORT
                            | ModifierFlags::AMBIENT
                            | ModifierFlags::ABSTRACT
                            | ModifierFlags::ASYNC,
                    ) {
                        return true;
                    }
                    if parent_is_class_like
                        || node_kind == SyntaxKind::Parameter
                        || block_scope_kind == using_kinds.0
                        || block_scope_kind == using_kinds.1
                    {
                        return true;
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
                            return true;
                        }
                    }
                    if block_scope_kind == using_kinds.0 || block_scope_kind == using_kinds.1 {
                        return true;
                    }
                    if !flags.intersects(ModifierFlags::EXPORT) {
                        return true;
                    }
                    if saw_export_before_decorators {
                        return true;
                    }
                    flags |= ModifierFlags::DEFAULT;
                }
                SyntaxKind::DeclareKeyword => {
                    if flags.intersects(
                        ModifierFlags::AMBIENT
                            | ModifierFlags::ASYNC
                            | ModifierFlags::OVERRIDE
                            | ModifierFlags::ACCESSOR,
                    ) {
                        return true;
                    }
                    if parent_is_class_like && node_kind != SyntaxKind::PropertyDeclaration {
                        return true;
                    }
                    if node_kind == SyntaxKind::Parameter
                        || block_scope_kind == using_kinds.0
                        || block_scope_kind == using_kinds.1
                    {
                        return true;
                    }
                    if parent_is_ambient && parent_kind == Some(SyntaxKind::ModuleBlock) {
                        return true;
                    }
                    if parent_is_class_like && name_is_private_identifier {
                        return true;
                    }
                    flags |= ModifierFlags::AMBIENT;
                }
                SyntaxKind::AbstractKeyword => {
                    if flags.intersects(ModifierFlags::ABSTRACT) {
                        return true;
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
                            return true;
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
                            return true;
                        }
                        if flags.intersects(
                            ModifierFlags::STATIC
                                | ModifierFlags::PRIVATE
                                | ModifierFlags::ASYNC
                                | ModifierFlags::OVERRIDE
                                | ModifierFlags::ACCESSOR,
                        ) {
                            return true;
                        }
                    }
                    if name_is_private_identifier {
                        return true;
                    }
                    flags |= ModifierFlags::ABSTRACT;
                }
                SyntaxKind::AsyncKeyword => {
                    if flags.intersects(ModifierFlags::ASYNC | ModifierFlags::AMBIENT)
                        || parent_is_ambient
                    {
                        return true;
                    }
                    if node_kind == SyntaxKind::Parameter {
                        return true;
                    }
                    if flags.intersects(ModifierFlags::ABSTRACT) {
                        return true;
                    }
                    flags |= ModifierFlags::ASYNC;
                }
                SyntaxKind::InKeyword | SyntaxKind::OutKeyword => {
                    let in_out_flag = if modifier_kind == SyntaxKind::InKeyword {
                        ModifierFlags::IN
                    } else {
                        ModifierFlags::OUT
                    };
                    // `node.kind !== TypeParameter || parent && !(...)`:
                    // a parentless type parameter does NOT report.
                    if node_kind != SyntaxKind::TypeParameter
                        || parent_kind.is_some_and(|kind| {
                            !matches!(
                                kind,
                                SyntaxKind::InterfaceDeclaration
                                    | SyntaxKind::ClassDeclaration
                                    | SyntaxKind::ClassExpression
                                    | SyntaxKind::TypeAliasDeclaration
                            )
                        })
                    {
                        return true;
                    }
                    if flags.intersects(in_out_flag) {
                        return true;
                    }
                    if in_out_flag == ModifierFlags::IN && flags.intersects(ModifierFlags::OUT) {
                        return true;
                    }
                    flags |= in_out_flag;
                }
                _ => {}
            }
        }
        if node_kind == SyntaxKind::Constructor {
            return flags.intersects(
                ModifierFlags::STATIC | ModifierFlags::OVERRIDE | ModifierFlags::ASYNC,
            );
        }
        if matches!(
            node_kind,
            SyntaxKind::ImportDeclaration | SyntaxKind::ImportEqualsDeclaration
        ) && flags.intersects(ModifierFlags::AMBIENT)
        {
            return true;
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
            if name_is_pattern || has_dot_dot_dot {
                return true;
            }
        }
        if flags.intersects(ModifierFlags::ASYNC) {
            // checkGrammarAsyncModifier (89391): async is legal only
            // on these four kinds.
            return !matches!(
                node_kind,
                SyntaxKind::MethodDeclaration
                    | SyntaxKind::FunctionDeclaration
                    | SyntaxKind::FunctionExpression
                    | SyntaxKind::ArrowFunction
            );
        }
        false
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
    ///
    /// The aggregation walk widens the report range over ADJACENT
    /// unreachable statements of the same canHaveStatements parent
    /// (marking each reported) so ONE 7027 covers the run. Only the
    /// error face lands (addErrorOrSuggestion isError ⇔
    /// allowUnreachableCode === false); the default-options suggestion
    /// face stays unmodeled with the suggestion channel, but the
    /// reported-set/return-value bookkeeping runs identically so
    /// withinUnreachableCode suppression matches tsc under every
    /// option value.
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
        if self.options.allow_unreachable_code == Some(false) {
            // getTokenPosOfNode = skipTrivia from the node's pos.
            let start = tsrs2_syntax::skip_trivia(
                &self.binder.source_of_node(start_node).text,
                self.pos_of(start_node) as usize,
            );
            let end = self.end_of(end_node) as usize;
            self.error_at_byte_range(
                start_node,
                start,
                end,
                &diagnostics::Unreachable_code_detected,
            );
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
    /// Head elisions: the PartiallyTypeChecked gate (nodesToCheck path
    /// unported), the canHaveJSDoc comment/tag walk and every JSDoc*
    /// kind arm (JS/JSDoc checking is the M2 3.4c residual), and the
    /// cancellationToken arms. The unreachable-code gate (86582) is
    /// LIVE since 6.6b. Kind arms are in tsc switch order; stubs name
    /// their tsc worker and owner stage.
    fn check_source_element_worker(&mut self, node: NodeId) -> CheckResult2<()> {
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

    /// One no-op escape per not-yet-landed checkSourceElementWorker
    /// arm (and per value-level SILENT stub — a divergence that keeps
    /// producing a result instead of unwinding). The worker name +
    /// owner stage make each disposition greppable and visible to the
    /// `xtask escapes` audit; the site emits nothing (FN) until its
    /// stage ports the worker.
    pub(crate) fn source_element_stub(&self, _worker: &str, _owner: &str) -> CheckResult2<()> {
        Ok(())
    }

    /// tsc-port: checkBlock @6.0.3
    /// tsc-hash: ea6aec550a59633f1e11e780af1c7be7f4c89f5b46519add41fcaa41c4c823ad
    /// tsc-span: _tsc.js:83214-83228
    ///
    /// The isFunctionOrModuleBlock flowAnalysisDisabled save/restore is
    /// M5 flow state (no field yet), and registerForUnusedIdentifiersCheck
    /// is inert until M7 (worker note) — both branches reduce to the
    /// statement loop.
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
                    let text = self
                        .identifier_text_of(name.expect("bound type parameters have names"))
                        .unwrap_or_default()
                        .to_owned();
                    self.error_at(name, &diagnostics::Duplicate_identifier_0, &[&text]);
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
        // walk (duplicate-extends family) — the would-report skeleton
        // supplies tsc's verdict (the modifier row stays the M7 FN).
        if !self.check_grammar_modifiers_would_report(node) {
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
    /// (grammarErrorAtPos 1237-family) are M7-stub grammar sites. This
    /// arm is what makes checkSourceElement(default/constraint) FORCE
    /// references BEFORE hasNonCircularTypeParameterDefault reads the
    /// default slot — the 2716-lands-on-the-second-parameter ordering
    /// depends on it (oracle-pinned). Heritage
    /// ExpressionWithTypeArguments routes here since 5.8c (§6/§7).
    pub(crate) fn check_type_reference_node(&mut self, node: NodeId) -> CheckResult2<()> {
        let type_arguments = match self.data_of(node) {
            NodeData::TypeReference(data) => data.type_arguments,
            NodeData::ExpressionWithTypeArguments(data) => data.type_arguments,
            _ => unreachable!("kind/data agree"),
        };
        for argument in self.nodes_of(type_arguments) {
            self.check_source_element(Some(argument));
        }
        self.check_type_reference_or_import(node, type_arguments.is_some())
    }

    /// tsc-port: checkTypeReferenceOrImport @6.0.3
    /// tsc-hash: 0530fc32ad383a5bd0d271dcce464434ccc750513aa2907e511fc65df2ee907c
    /// tsc-span: _tsc.js:81771-81793
    ///
    /// The deprecation-suggestion tail is suggestion-band (unmodeled).
    /// Unresolved names unwind Unsupported out of getTypeFromTypeNode
    /// (annotate.rs) instead of flowing errorType, so the isErrorType
    /// guard is the Ok path here; the 2304 was already emitted by the
    /// resolver.
    fn check_type_reference_or_import(
        &mut self,
        node: NodeId,
        has_type_arguments: bool,
    ) -> CheckResult2<()> {
        let ty = self.get_type_from_type_node(node)?;
        if ty != self.tables.intrinsics.error && has_type_arguments {
            // Conditional-scope escape: inside a ConditionalType, the
            // check type's parameters carry the extends-clause
            // constraint context (M8 conditional machinery) — the
            // slice's constraint check over the RAW parameters
            // fabricates 2344 (conditionalTypes2 pins the true-branch
            // reference staying silent).
            if self.has_conditional_type_ancestor(node) {
                return self.source_element_stub(
                    "checkTypeArgumentConstraints under a ConditionalType",
                    "M8",
                );
            }
            // addLazyDiagnostic runs inline (eager identity).
            if let Some(type_parameters) =
                self.get_type_parameters_for_type_reference_or_import(node)?
            {
                self.check_type_argument_constraints(node, &type_parameters)?;
            }
        }
        Ok(())
    }

    /// The conditional-scope probe feeding the M8 constraint escapes.
    fn has_conditional_type_ancestor(&self, node: NodeId) -> bool {
        let mut current = self.parent_of(node);
        while let Some(candidate) = current {
            if self.kind_of(candidate) == SyntaxKind::ConditionalType {
                return true;
            }
            current = self.parent_of(candidate);
        }
        false
    }

    /// tsc-port: getTypeParametersForTypeReferenceOrImport @6.0.3
    /// (covers getTypeParametersForTypeAndSymbol in the same span)
    /// tsc-hash: cb54b2481679e0a7eb4e9530f2d7710e9bf374f4323522fcf273ab2d8d9aab8f
    /// tsc-span: _tsc.js:81703-81718
    fn get_type_parameters_for_type_reference_or_import(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<Option<Vec<TypeId>>> {
        let ty = self.get_type_from_type_node(node)?;
        if ty == self.tables.intrinsics.error {
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
        // Conditional-scope escape (same M8 class as the constraint
        // check above): `T extends K ? Obj[T] : ...` narrows T in the
        // true branch — the raw-parameter index check fabricates 2536
        // (stringMappingReduction / unknownControlFlow pins).
        if self.has_conditional_type_ancestor(node) {
            return self
                .source_element_stub("checkIndexedAccessIndexType under a ConditionalType", "M8");
        }
        let resolved = self.get_type_from_indexed_access_type_node(node)?;
        self.check_indexed_access_index_type(resolved, node)?;
        Ok(())
    }

    /// tsc-port: checkMappedType @6.0.3
    /// tsc-hash: 12a5060787f6d1849d7f77ba2d3beb1f786fb8263e2fcd929c49d5c9673375e4
    /// tsc-span: _tsc.js:81924-81940
    ///
    /// getTypeFromMappedTypeNode is the annotate.rs M8-stub: when the
    /// resolver escapes, the nameType/constraint rows escape with it —
    /// the grammar, recursion and reportImplicitAny rows above still
    /// fire (§11 containment note).
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
        let _ = ty;
        Err(Unsupported::new(
            "checkMappedType nameType/constraint rows (getNameTypeFromMappedType — mapped types M8)",
        ))
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
    /// forEachChild recursion ONLY — the M8-stub stays on the annotate
    /// side, no self-force here.
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
    ///
    /// The JSDoc host arms are JS-only (elided).
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
                    let is_readonly = tsrs2_binder::node_util::has_syntactic_modifier(
                        source,
                        parent,
                        tsrs2_types::ModifierFlags::READONLY,
                    );
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

    /// tsc-port: checkTypeAssignableTo @6.0.3 (5.4 slice)
    /// tsc-hash: c54f432c89f2f52677994a63f73b2d9e30dadfe890712c62749b4aab33e7f833
    /// tsc-span: _tsc.js:63931-63933
    ///
    /// checkTypeRelatedTo's failure path (64842+) builds an
    /// elaboration CHAIN whose tail renders through the full
    /// nodeBuilder; this slice emits the HEAD message only —
    /// code/span/args tsc-identical, the chain tail honestly elided
    /// (T2 band). reportRelationError's argument shaping (65064-65135)
    /// is ported: getTypeNamesForErrorDisplay equality fallback
    /// escapes (UseFullyQualifiedType re-render is nodeBuilder work),
    /// literal-source generalization is live, and the
    /// TypeParameter-target elaboration arm — WITH its ForCheck marker
    /// guard (65075) — only shapes the elided chain, so it reduces to
    /// the guard itself. A failure whose types the display slice
    /// cannot render unwinds Unsupported: no diagnostic rather than an
    /// unfaithful one.
    pub(crate) fn check_type_assignable_to(
        &mut self,
        source: TypeId,
        target: TypeId,
        error_node: Option<NodeId>,
        head_message: &'static DiagnosticMessage,
    ) -> CheckResult2<bool> {
        let related = self.is_type_assignable_to(source, target)?;
        if !related {
            if let Some(error_node) = error_node {
                // The 65185 nullable-candidate substitution runs at
                // the failing level's entry — every report arm below
                // (excess/common-property heads, unmatched-property
                // faces, the rendered pair) sees the substituted
                // target, exactly like tsc's in-engine reporting.
                let target = self.nullable_stripped_report_target(source, target)?;
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
                let generic_head = std::ptr::eq(
                    head_message,
                    &diagnostics::Type_0_is_not_assignable_to_type_1,
                );
                // A failed relation whose SOURCE is the global
                // Object type selects between a 2696 head (when an
                // override-flavored deep incompatibility suppressed
                // the generic head: missing props, method-return
                // 2201-family) and a 2322 head with 2696 in the
                // chain TAIL (signature mismatches). The selection
                // needs the overrideNextErrorInfo tracking (T2 error
                // machinery) — escape rather than guess (corpus:
                // parserAutomaticSemicolonInsertion1 wants 2322,
                // objectTypeHidingMembersOfObjectAssignmentCompat2
                // wants 2696).
                if generic_head
                    && self.tables.flags_of(source).intersects(TypeFlags::OBJECT)
                    && self.tables.type_of(source).symbol.is_some()
                    && source == self.global_object_type()?
                {
                    return Err(Unsupported::new(
                        "Object-source relation head selection \
                         (overrideNextErrorInfo tracking, T2)",
                    ));
                }
                if generic_head
                    && self.report_unmatched_property_head(source, target, error_node)?
                {
                    return Ok(related);
                }
                // reportErrorResults 65258 + the `!headMessage &&
                // maybeSuppress` roll-back (65284): under the GENERIC
                // head a readonly→mutable array/tuple failure reports
                // 4104 and the head never lands. The suppression keys
                // on errorInfo CHANGING — a false verdict with no
                // report (tuple source vs non-array target) still
                // takes the head. Explicit heads keep their code (tsc
                // chains 4104 into the elided tail).
                if generic_head
                    && self.tables.flags_of(source).intersects(TypeFlags::OBJECT)
                    && self.tables.flags_of(target).intersects(TypeFlags::OBJECT)
                {
                    let reported_before = self.diagnostics.len();
                    if !self.try_elaborate_array_like_errors(source, target, true, error_node)?
                        && self.diagnostics.len() > reported_before
                    {
                        return Ok(related);
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
        };
        Ok(matches!(
            checker.excess_properties_worker(source, target, Some(error_node))?,
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

    /// tsc-port: tryElaborateArrayLikeErrors @6.0.3
    /// tsc-hash: 4d8d191f532ffe704ad74834cc079e0c2f02d50f2a1159f8bde055450d13c086
    /// tsc-span: _tsc.js:65123-65143
    ///
    /// Both faces of tsc's use: with `report_errors` the
    /// readonly-source→mutable-target failure EMITS 4104 (the
    /// reportErrorResults call, 65258 — and under the generic head
    /// tsc's `!headMessage && maybeSuppress` arm rolls the head back,
    /// so 4104 stands alone); without it the boolean gates the
    /// missing-properties head (reportUnmatchedProperty's `else if`,
    /// 66750).
    fn try_elaborate_array_like_errors(
        &mut self,
        source: TypeId,
        target: TypeId,
        report_errors: bool,
        error_node: NodeId,
    ) -> CheckResult2<bool> {
        let report_readonly_mismatch =
            |state: &mut Self, source: TypeId, target: TypeId| -> CheckResult2<()> {
                let source_text = state.type_to_string_slice(source)?;
                let target_text = state.type_to_string_slice(target)?;
                state.error_at(
                Some(error_node),
                &diagnostics::The_type_0_is_readonly_and_cannot_be_assigned_to_the_mutable_type_1,
                &[&source_text, &target_text],
            );
                Ok(())
            };
        if self.tables.is_tuple_type(source) {
            let tuple_readonly = {
                let tuple_target = self.tables.reference_target(source);
                match &self.tables.type_of(tuple_target).data {
                    tsrs2_types::TypeData::TupleTarget(data) => data.readonly,
                    _ => false,
                }
            };
            if tuple_readonly && self.is_mutable_array_or_tuple(target)? {
                if report_errors {
                    report_readonly_mismatch(self, source, target)?;
                }
                return Ok(false);
            }
            return Ok(self.is_array_type(target)? || self.tables.is_tuple_type(target));
        }
        if self.is_readonly_array_type(source)? && self.is_mutable_array_or_tuple(target)? {
            if report_errors {
                report_readonly_mismatch(self, source, target)?;
            }
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
            && !self.try_elaborate_array_like_errors(source, target, false, error_node)?
        {
            return Ok(false);
        }
        // 66735: the single-property face renders through
        // getTypeNamesForErrorDisplay — the context-sensitive
        // enclosing pass plus the equal→fully-qualified retry; the
        // multi-property 2739/2740 faces use plain typeToString
        // (66752-66757, no enclosing).
        let (source_text, target_text) = if unmatched.len() == 1 {
            let source_text = self.type_to_string_slice_with_error_enclosing(source)?;
            let target_text = self.type_to_string_slice_with_error_enclosing(target)?;
            if source_text == target_text {
                (
                    self.get_type_name_for_error_display(source)?,
                    self.get_type_name_for_error_display(target)?,
                )
            } else {
                (source_text, target_text)
            }
        } else {
            (
                self.type_to_string_slice(source)?,
                self.type_to_string_slice(target)?,
            )
        };
        if unmatched.len() == 1 {
            let prop = unmatched[0];
            let prop_name = self.missing_property_display_name(unmatched[0]);
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
        let names: Vec<String> = unmatched
            .iter()
            .map(|&prop| self.missing_property_display_name(prop))
            .collect();
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
    /// identifiers print their declaration text (`#x`), everything
    /// else unescapes like symbolToString.
    fn missing_property_display_name(&self, prop: SymbolId) -> String {
        let escaped = &self.binder.symbol(prop).escaped_name;
        if escaped.starts_with('#') {
            return escaped.clone();
        }
        if let Some(stripped) = escaped.strip_prefix("__#") {
            let _ = stripped;
            if let Some(declaration) = self.binder.symbol(prop).value_declaration {
                let source = self.binder.source_of_node(declaration);
                if let Some(name) =
                    tsrs2_binder::node_util::get_name_of_declaration(source, declaration)
                {
                    return tsrs2_binder::node_util::declaration_name_to_string(source, Some(name));
                }
            }
        }
        tsrs2_binder::unescape_leading_underscores(escaped).to_owned()
    }

    /// tsc-port: checkTypeComparableTo @6.0.3 (5.4-slice shape)
    /// tsc-hash: e58eb977753b557ce9b0c944ca7602c6b1b4981cd57f5ce5d3181ab726e31d4d
    /// tsc-span: _tsc.js:63937-63939
    ///
    /// The comparable twin of check_type_assignable_to above: HEAD
    /// message only, reportRelationError argument shaping (literal
    /// generalization + identical-name escape), chain tail elided.
    pub(crate) fn check_type_comparable_to(
        &mut self,
        source: TypeId,
        target: TypeId,
        error_node: Option<NodeId>,
        head_message: &'static DiagnosticMessage,
    ) -> CheckResult2<bool> {
        let related = self.is_type_comparable_to(source, target)?;
        if !related {
            if let Some(error_node) = error_node {
                // 65185 nullable-candidate substitution (see the
                // assignable twin).
                let target = self.nullable_stripped_report_target(source, target)?;
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
    pub(crate) fn type_to_string_slice(&mut self, ty: TypeId) -> CheckResult2<String> {
        self.type_to_string_slice_ex(ty, /*fully_qualified*/ false)
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
        self.type_to_string_slice_ex(ty, /*fully_qualified*/ true)
    }

    fn type_to_string_slice_ex(
        &mut self,
        ty: TypeId,
        fully_qualified: bool,
    ) -> CheckResult2<String> {
        Ok(self.type_to_string_slice_node(ty, fully_qualified)?.0)
    }

    /// The kind-carrying face of the slice renderer: the nodeBuilder
    /// emits factory TypeNodes and the factory's parenthesizer rules
    /// branch on the CHILD node's kind at every join, so the string
    /// slice returns the would-be node kind beside the text and the
    /// joins apply the same rules (`SliceTypeNodeKind`).
    fn type_to_string_slice_node(
        &mut self,
        ty: TypeId,
        fully_qualified: bool,
    ) -> CheckResult2<(String, SliceTypeNodeKind)> {
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
                    Some(symbol) => self.symbol_display_name(symbol),
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
                    let name = if fully_qualified {
                        self.get_fully_qualified_name(symbol)
                    } else {
                        self.symbol_display_name(symbol)
                    };
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
                let parent_name = if fully_qualified {
                    self.get_fully_qualified_name(parent)
                } else {
                    self.symbol_display_name(parent)
                };
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
            return Ok((
                if fully_qualified {
                    self.get_fully_qualified_name(symbol)
                } else {
                    self.symbol_display_name(symbol)
                },
                SliceTypeNodeKind::Reference,
            ));
        }
        match &self.tables.type_of(ty).data {
            TypeData::Intrinsic { name, .. } => {
                Ok(((*name).to_owned(), SliceTypeNodeKind::Keyword))
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
                    Ok((
                        format!("\"{}\"", string_literal_type_display_text(text)),
                        SliceTypeNodeKind::Literal,
                    ))
                }
                tsrs2_types::LiteralValue::Number(value) => Ok((
                    tsrs2_types::js_number_to_string(*value),
                    SliceTypeNodeKind::Literal,
                )),
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
                    self.symbol_value_face_slice(symbol, true)
                } else {
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
    ) -> CheckResult2<(String, SliceTypeNodeKind)> {
        let type_of = self.tables.type_of(ty);
        if let (Some(alias_symbol), alias_arguments) =
            (type_of.alias_symbol, type_of.alias_type_arguments.clone())
        {
            let name = if fully_qualified {
                self.get_fully_qualified_name(alias_symbol)
            } else {
                self.symbol_display_name(alias_symbol)
            };
            return match alias_arguments {
                Some(arguments) if !arguments.is_empty() => {
                    // Type-argument lists never parenthesize in the
                    // slice (parenthesizeOrdinalTypeArgument wraps only
                    // a LEADING function/constructor head, 20607-20612
                    // — not a producible child).
                    let mut rendered = Vec::new();
                    for argument in arguments.iter() {
                        rendered.push(self.type_to_string_slice_ex(*argument, fully_qualified)?);
                    }
                    Ok((
                        format!("{name}<{}>", rendered.join(", ")),
                        SliceTypeNodeKind::Reference,
                    ))
                }
                _ => Ok((name, SliceTypeNodeKind::Reference)),
            };
        }
        let flags = self.tables.flags_of(ty);
        // typeToTypeNodeHelper's keyword arm precedes the union walk:
        // the interned `true | false` pair carries TypeFlags::BOOLEAN
        // (getUnionType's boolean-pair stamp — tables mirror it) and
        // prints as the keyword, never as its members.
        if flags.intersects(TypeFlags::BOOLEAN) && flags.intersects(TypeFlags::UNION) {
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
                    // NARROWED verdict shield: origins with
                    // INSTANTIABLE members are the cross-product
                    // relation band where the port's verdict is NOT
                    // yet faithful — `T & U ⊆ (A | B) & T & U` holds
                    // in tsc through a normalized-intersection path
                    // the port lacks (each constituent relates
                    // individually here, and `T & U ⊆ 2` passes
                    // standalone but fails inside the intersection-
                    // target walk; FP-gate catch #8). Rendering those
                    // origins would report the wrong verdicts, so the
                    // shield stays EXACTLY for them until the relation
                    // producer lands (9.9x/M8 owner); concrete-typed
                    // origins (the interface cross products) render.
                    if types.iter().any(|&member| {
                        self.tables
                            .flags_of(member)
                            .intersects(TypeFlags::INSTANTIABLE)
                    }) {
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
            let mut rendered = Vec::new();
            for member in types {
                let (text, kind) = self.type_to_string_slice_node(member, fully_qualified)?;
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
                let mut rendered = Vec::with_capacity(arity);
                for (i, &argument) in arguments.iter().take(arity).enumerate() {
                    let flags = element_flags[i];
                    let (text, kind) = self.type_to_string_slice_node(argument, fully_qualified)?;
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
            let name = if fully_qualified {
                self.get_fully_qualified_name(symbol)
            } else {
                self.symbol_display_name(symbol)
            };
            let arguments = self.get_type_arguments(ty)?;
            // typeReferenceToTypeNode's array sugar: references to the
            // global Array/ReadonlyArray print as element sugar (the
            // sugar probe reads the PLAIN name — lib globals are
            // parentless, so the qualified head matches too).
            if arguments.len() == 1 && (name == "Array" || name == "ReadonlyArray") {
                let (element, kind) =
                    self.type_to_string_slice_node(arguments[0], fully_qualified)?;
                // 51945-51947: ArrayTypeNode (postfix-parenthesized
                // element) + the readonly TypeOperator for
                // ReadonlyArray (an array operand never parenthesizes,
                // 20570-20576).
                let array = array_type_node_text(element, kind);
                return Ok(if name == "Array" {
                    (array, SliceTypeNodeKind::Array)
                } else {
                    (format!("readonly {array}"), SliceTypeNodeKind::TypeOperator)
                });
            }
            let local_parameter_count = match &self.tables.type_of(target).data {
                TypeData::GenericType {
                    type_parameters,
                    outer_type_parameter_count,
                    ..
                } => {
                    if *outer_type_parameter_count > 0 {
                        // Outer parameters render as enclosing-declaration
                        // qualification in the nodeBuilder — out of slice.
                        return Err(Unsupported::new(
                            "reference display with outer type parameters (nodeBuilder, T2 M8)",
                        ));
                    }
                    type_parameters.len() - outer_type_parameter_count
                }
                _ => unreachable!("reference targets are GenericType or symbol-less TupleTarget"),
            };
            let mut rendered = Vec::new();
            for argument in arguments.iter().take(local_parameter_count) {
                rendered.push(self.type_to_string_slice_ex(*argument, fully_qualified)?);
            }
            return Ok((
                if rendered.is_empty() {
                    name
                } else {
                    format!("{name}<{}>", rendered.join(", "))
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
            let name = if fully_qualified {
                self.get_fully_qualified_name(symbol)
            } else {
                self.symbol_display_name(symbol)
            };
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
            return Ok((
                format!("{object}[{index_text}]"),
                SliceTypeNodeKind::IndexedAccess,
            ));
        }
        Err(Unsupported::new(
            "typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)",
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
    /// instantiation-expression TypeQuery reuse, JS constructors,
    /// class/enum/value-module symbol heads, typeof-function
    /// (shouldWriteTypeOfFunctionSymbol) — renders a symbol reference
    /// or `typeof X` face instead and stays behind the curtain for
    /// later 9.3b rungs; the visitedTypes revisit faces
    /// (getTypeAliasForTypeLiteral / `...` elision) likewise.
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
            // isJSConstructor head (51764) rides the checkJs band.
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
            if symbol_flags
                .intersects(tsrs2_types::SymbolFlags::FUNCTION | tsrs2_types::SymbolFlags::METHOD)
                && self
                    .binder
                    .symbol(symbol)
                    .value_declaration
                    .is_some_and(|declaration| self.is_in_js_file(declaration))
            {
                return Err(Unsupported::new(
                    "typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)",
                ));
            }
            if symbol_flags.intersects(tsrs2_types::SymbolFlags::FUNCTION)
                && self.symbol_has_expando_assignment_merged(symbol)
            {
                // tsc's expando binding gives the DECLARATION symbol a
                // namespace face (bindPotentiallyMissingNamespaces),
                // so the ValueModule disjunct (51779) prints it under
                // the Value meaning — `typeof foo` (oracle-probed).
                // The fn-EXPRESSION flavor flags the VARIABLE symbol
                // instead, so its type renders structurally MINUS the
                // unbound members (tsc prints them: recorded
                // stage-3.4c T2 residue; the row keys are unaffected).
                let name = if fully_qualified {
                    self.get_fully_qualified_name(symbol)
                } else {
                    self.symbol_display_name(symbol)
                };
                return Ok((format!("typeof {name}"), SliceTypeNodeKind::TypeQuery));
            }
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

    /// tsc-port: symbolToTypeNode @6.0.3 (error-path Value slice)
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
    /// length-1 chain keeps the bare import face; other roots render
    /// the entity face over the same chain. The import face's
    /// node16/nodenext resolution-mode attributes (53125-53150)
    /// and /node_modules/ specifier swap (53151-53174) read
    /// impliedNodeFormat, which the port does not model: the swap can
    /// only fire on node_modules fixtures (host-adjudicated band) and
    /// the attributes only change message text under node16 matrices —
    /// recorded T2 residue, row keys unaffected.
    fn symbol_value_face_slice(
        &mut self,
        symbol: SymbolId,
        fully_qualified: bool,
    ) -> CheckResult2<(String, SliceTypeNodeKind)> {
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
            // ImportTypeNode with isTypeOf (meaning === Value).
            let specifier = self.specifier_for_module_symbol_slice(symbol)?;
            let literal = string_literal_name_slice(&specifier, false)?;
            return Ok((
                format!("typeof import({literal})"),
                SliceTypeNodeKind::ImportType,
            ));
        }
        if fully_qualified {
            let chain = self
                .symbol_chain_slice(symbol, tsrs2_types::SymbolFlags::VALUE, true)?
                .expect("getSymbolChain with endOfChain always yields (52991-52999)");
            let root = chain[0];
            if self.symbol_has_external_module_declaration(root) {
                let specifier = self.specifier_for_module_symbol_slice(root)?;
                let literal = string_literal_name_slice(&specifier, false)?;
                // 53175-53185: the export= short-circuit (52978-52981)
                // leaves a length-1 chain — the bare ImportTypeNode.
                if chain.len() == 1 {
                    return Ok((
                        format!("typeof import({literal})"),
                        SliceTypeNodeKind::ImportType,
                    ));
                }
                let mut qualifier = Vec::with_capacity(chain.len() - 1);
                for index in 1..chain.len() {
                    qualifier
                        .push(self.qualifier_symbol_name_slice(chain[index - 1], chain[index])?);
                }
                let qualifier = qualifier.join(".");
                return Ok((
                    format!("typeof import({literal}).{qualifier}"),
                    SliceTypeNodeKind::ImportType,
                ));
            }
            // 53186-53197: the entity face over the chain —
            // getNameOfSymbolAsWritten at the root (the slice's
            // symbol_display_name posture), the export-table naming
            // below it, isTypeOf wrapping the TypeQuery.
            let mut parts = Vec::with_capacity(chain.len());
            parts.push(self.symbol_display_name(root));
            for index in 1..chain.len() {
                parts.push(self.qualifier_symbol_name_slice(chain[index - 1], chain[index])?);
            }
            return Ok((
                format!("typeof {}", parts.join(".")),
                SliceTypeNodeKind::TypeQuery,
            ));
        }
        // 53186-53197 with the [symbol] chain: the bare-name face.
        Ok((
            format!("typeof {}", self.symbol_display_name(symbol)),
            SliceTypeNodeKind::TypeQuery,
        ))
    }

    /// tsc-port: getSymbolChain @6.0.3 (error-path slice)
    /// tsc-hash: 8ccb0f4b99b34c677210c369edfdf15d1f0cc32eed7f57b6b153783b4808d291
    /// tsc-span: _tsc.js:52958-53016
    ///
    /// lookupSymbolChainWorker's chain builder with no
    /// enclosingDeclaration. yieldModuleSymbol is TRUE on the
    /// symbolToTypeNode path (DoNotIncludeSymbolChain unset), so the
    /// module-parent suppression (52996-52998) never fires; the
    /// TypeLiteral/ObjectLiteral parent guard (52991-52995) is kept
    /// verbatim though the module/namespace parents this face walks
    /// cannot carry those flags. getQualifiedLeftMeaning (50291) fixes
    /// Value → Value, so the top-level Value meaning rides the whole
    /// recursion.
    fn symbol_chain_slice(
        &mut self,
        symbol: SymbolId,
        meaning: tsrs2_types::SymbolFlags,
        end_of_chain: bool,
    ) -> CheckResult2<Option<Vec<SymbolId>>> {
        let mut accessible = self.accessible_symbol_chain_slice(symbol, meaning)?;
        let needs_walk = match &accessible {
            None => true,
            Some(chain) => {
                let link_meaning = if chain.len() == 1 {
                    meaning
                } else {
                    Self::qualified_left_meaning(meaning)
                };
                self.needs_qualification_slice(chain[0], link_meaning)?
            }
        };
        if needs_walk {
            let walk_from = accessible.as_ref().map_or(symbol, |chain| chain[0]);
            let parents = self.containers_of_symbol_slice(walk_from)?;
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
    /// No enclosingDeclaration: forEachSymbolTableInScope's location
    /// walk is empty and the single consulted table is `globals`
    /// (50283-50289). The isPropertyOrMethodDeclarationSymbol guard
    /// (50295) cannot match this face's module/namespace declaration
    /// lists, and the accessibleChainCache is a recomputation-only
    /// economy the slice skips.
    fn accessible_symbol_chain_slice(
        &mut self,
        symbol: SymbolId,
        meaning: tsrs2_types::SymbolFlags,
    ) -> CheckResult2<Option<Vec<SymbolId>>> {
        let globals = self.globals.clone();
        let mut visited = Vec::new();
        self.accessible_chain_from_table_slice(
            &globals,
            None,
            symbol,
            meaning,
            /*ignore_qualification*/ false,
            /*is_local_name_lookup*/ true,
            &mut visited,
        )
    }

    /// getAccessibleSymbolChainFromSymbolTable (50313-50319): the
    /// visited guard is table-object identity in tsc — keyed by the
    /// owning symbol here (each symbol resolves one exports view;
    /// `globals` is the None key).
    #[allow(clippy::too_many_arguments)]
    fn accessible_chain_from_table_slice(
        &mut self,
        table: &tsrs2_binder::SymbolTable,
        table_key: Option<SymbolId>,
        symbol: SymbolId,
        meaning: tsrs2_types::SymbolFlags,
        ignore_qualification: bool,
        is_local_name_lookup: bool,
        visited: &mut Vec<Option<SymbolId>>,
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
        );
        visited.pop();
        result
    }

    /// trySymbolTable (50331-50360): the direct hit, then the alias
    /// scan in table order. The exportSymbol arm (50348-50357) needs a
    /// LOCALS table, which the no-enclosing walk never consults; the
    /// globals-tail globalThis probe (50359) can only yield the
    /// globalThis face, which the named-object arm renders upstream —
    /// both skipped.
    fn try_symbol_table_slice(
        &mut self,
        table: &tsrs2_binder::SymbolTable,
        symbol: SymbolId,
        meaning: tsrs2_types::SymbolFlags,
        ignore_qualification: bool,
        is_local_name_lookup: bool,
        visited: &mut Vec<Option<SymbolId>>,
    ) -> CheckResult2<Option<Vec<SymbolId>>> {
        let escaped = self.binder.symbol(symbol).escaped_name.clone();
        let direct = table.get(&escaped).copied();
        if self.symbol_chain_is_accessible_slice(
            symbol,
            direct,
            None,
            meaning,
            ignore_qualification,
        )? {
            return Ok(Some(vec![symbol]));
        }
        for (name, &entry) in table.iter() {
            if !self
                .binder
                .symbol(entry)
                .flags
                .intersects(tsrs2_types::SymbolFlags::ALIAS)
            {
                continue;
            }
            if name == tsrs2_types::InternalSymbolName::EXPORT_EQUALS
                || name == tsrs2_types::InternalSymbolName::DEFAULT
            {
                continue;
            }
            // The isUMDExportSymbol leg (50341) needs an
            // enclosingDeclaration and useOnlyExternalAliasing is
            // false on the error path (52959) — both filters are off.
            if is_local_name_lookup
                && self.symbol_has_declaration_of_kind(entry, SyntaxKind::NamespaceExport)
            {
                // isNamespaceReexportDeclaration (50341): `export * as
                // ns from` — the only grammatical NamespaceExport.
                continue;
            }
            if !ignore_qualification
                && self.symbol_has_declaration_of_kind(entry, SyntaxKind::ExportSpecifier)
            {
                continue;
            }
            let resolved = self.resolve_alias(entry)?;
            if let Some(chain) = self.candidate_list_for_symbol_slice(
                entry,
                resolved,
                symbol,
                meaning,
                ignore_qualification,
                visited,
            )? {
                return Ok(Some(chain));
            }
        }
        Ok(None)
    }

    /// getCandidateListForSymbol (50361-50374): the alias itself, or
    /// the alias prepended to a chain found in its target's export
    /// table (qualification ignored inside).
    fn candidate_list_for_symbol_slice(
        &mut self,
        entry: SymbolId,
        resolved: SymbolId,
        symbol: SymbolId,
        meaning: tsrs2_types::SymbolFlags,
        ignore_qualification: bool,
        visited: &mut Vec<Option<SymbolId>>,
    ) -> CheckResult2<Option<Vec<SymbolId>>> {
        if self.symbol_chain_is_accessible_slice(
            symbol,
            Some(entry),
            Some(resolved),
            meaning,
            ignore_qualification,
        )? {
            return Ok(Some(vec![entry]));
        }
        let candidate_table = self.get_exports_of_symbol(resolved)?;
        let inner = self.accessible_chain_from_table_slice(
            &candidate_table,
            Some(resolved),
            symbol,
            meaning,
            /*ignore_qualification*/ true,
            /*is_local_name_lookup*/ false,
            visited,
        )?;
        if let Some(inner) = inner {
            if self.can_qualify_symbol_slice(entry, Self::qualified_left_meaning(meaning))? {
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
        self.can_qualify_symbol_slice(merged_entry, meaning)
    }

    /// canQualifySymbol (50321-50324): no qualification needed, or the
    /// parent chain is itself accessible.
    fn can_qualify_symbol_slice(
        &mut self,
        entry: SymbolId,
        meaning: tsrs2_types::SymbolFlags,
    ) -> CheckResult2<bool> {
        if !self.needs_qualification_slice(entry, meaning)? {
            return Ok(true);
        }
        let Some(parent) = self.get_parent_of_symbol(entry) else {
            return Ok(false);
        };
        Ok(self
            .accessible_symbol_chain_slice(parent, Self::qualified_left_meaning(meaning))?
            .is_some())
    }

    /// tsc-port: needsQualification @6.0.3 (error-path slice)
    /// tsc-hash: 1bde4c0406bef43d2e90c293732295ec0503439a0784611675ed408d4cd0141d
    /// tsc-span: _tsc.js:50376-50396
    ///
    /// No enclosingDeclaration ⇒ the walk visits only `globals`. Every
    /// slice-reachable call passes a symbol that IS the globals entry
    /// under its own name (a direct or alias accessibility hit), so
    /// the shadowed-slot tail is defensive fidelity: aliases resolve
    /// (export-specifier declared ones excepted, 50384-50390) and the
    /// meaning test decides. getSymbolFlags' transitive-alias union collapses to
    /// the resolved symbol's flags — resolveAlias resolves chains to
    /// their non-alias tail.
    fn needs_qualification_slice(
        &mut self,
        symbol: SymbolId,
        meaning: tsrs2_types::SymbolFlags,
    ) -> CheckResult2<bool> {
        let escaped = self.binder.symbol(symbol).escaped_name.clone();
        let Some(&entry) = self.globals.get(&escaped) else {
            return Ok(false);
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
        Ok(flags.intersects(meaning))
    }

    /// tsc-port: getContainersOfSymbol @6.0.3 (error-path slice)
    /// tsc-hash: 22e0144d0040f4fb713cbdddd579457d28490db454397361da682542484911d7
    /// tsc-span: _tsc.js:49989-50051
    ///
    /// No enclosingDeclaration ⇒ reexportContainers
    /// (getAlternativeContainingModules) stay empty. This face's
    /// symbols are VALUE_MODULE-flagged (symbol_value_face routing):
    /// the TypeParameter guard is inert, the class-expression-
    /// assignment candidates arm (50003-50009) cannot match a
    /// module/function declaration list, and the
    /// getVariableDeclarationOfObjectLiteral / firstVariableMatch
    /// probes (50038-50046) both need a NON-namespace container, which
    /// module/namespace parents never are.
    fn containers_of_symbol_slice(&mut self, symbol: SymbolId) -> CheckResult2<Vec<SymbolId>> {
        if let Some(container) = self.get_parent_of_symbol(symbol) {
            return self.with_alternative_containers_slice(container, Some(container));
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
            let expanded = self.with_alternative_containers_slice(container, None)?;
            let mut expanded = expanded.into_iter();
            if let Some(first) = expanded.next() {
                best.push(first);
            }
            alternatives.extend(expanded);
        }
        best.extend(alternatives);
        Ok(best)
    }

    /// getWithAlternativeContainers (50023-50047), no enclosing:
    /// additionalContainers (files whose export= IS the symbol's
    /// PARENT container — the closure reads the outer `container`,
    /// 50048-50050) ahead of the container itself; reexportContainers,
    /// the accessible-container early return, firstVariableMatch and
    /// objectLiteralContainer are all inert on this path.
    fn with_alternative_containers_slice(
        &mut self,
        container: SymbolId,
        parent_container: Option<SymbolId>,
    ) -> CheckResult2<Vec<SymbolId>> {
        let mut result = Vec::new();
        if let Some(parent_container) = parent_container {
            let declarations = self.binder.symbol(container).declarations.clone();
            for declaration in declarations {
                if let Some(file_symbol) = self
                    .file_symbol_if_export_equals_container_slice(declaration, parent_container)?
                {
                    result.push(file_symbol);
                }
            }
        }
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

    /// tsc-port: createTypeNodeFromObjectType @6.0.3
    /// tsc-hash: 1190da69649fad92283f6058fe227821ed7a562223b62b2e4193b555f06359bd
    /// tsc-span: _tsc.js:51894-51938
    ///
    /// The mapped-type head (isGenericMappedType/containsError) cannot
    /// be reached — no Mapped TypeData is minted before 9.5/M8. The
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
        // Captured BEFORE the lazy walk: a members link already
        // Resolved here means the type was BORN resolved
        // (make_resolved_anonymous_type / the widening clone — its
        // producer computed the complete member set through live
        // machinery), the trust signal for the empty-face admission
        // below.
        let born_resolved = self.links.ty(ty).resolved_members.resolved().is_some();
        let members = self.resolve_structured_type_members(ty)?;
        let resolved = self.members_of(members);
        let properties = resolved.properties.clone();
        let call_signatures = resolved.call_signatures.clone();
        let construct_signatures = resolved.construct_signatures.clone();
        let index_infos = resolved.index_infos.clone();
        if properties.is_empty() && index_infos.is_empty() {
            if call_signatures.is_empty() && construct_signatures.is_empty() {
                // The member-less TypeLiteral (51900-51906). The
                // symbol guard is load-bearing FP shielding, not
                // display fidelity: symbol-CARRYING shapes that
                // resolve empty today MAY do so because their member
                // machinery is unported (module-namespace M7, JSON
                // imports M7, checkJs object literals M8 —
                // corpus-probed at 7.5: dropping the guard unmasked 5
                // fabricated 2339/2322 rows in exactly those bands).
                // 9.3b5 narrowing, two REAL-empty admits:
                // (1) the canonical emptyTypeLiteralType singleton —
                // every empty source `{}` annotation resolves to it
                // (getTypeFromTypeNode's members-empty collapse) and
                // its checker-created symbol carries
                // Transient|TypeLiteral in tsc too;
                // (2) BORN-resolved types from a non-JS declaration —
                // make_resolved_anonymous_type producers (object
                // literals, spread/rest results, import attributes)
                // and the widening clone computed the complete member
                // set through live machinery, so resolving empty IS
                // tsc's `{}` (the all-consumed object rest pins).
                // JS-file declarations stay curtained: their members
                // can live in unbound JSDoc/expando machinery
                // (fixSignatureCaching band), the 7.5-probe
                // fabrication flavor. Lazily-resolved symbol-carrying
                // empties (module-namespace faces, JSON module
                // objects, instantiated literals) keep the guard.
                let symbol = self.tables.type_of(ty).symbol;
                let js_declared = symbol.is_some_and(|symbol| {
                    self.binder
                        .symbol(symbol)
                        .value_declaration
                        .is_some_and(|declaration| self.is_in_js_file(declaration))
                });
                if symbol.is_none()
                    || ty == self.empty_type_literal_type
                    || (born_resolved && !js_declared)
                {
                    return Ok(("{}".to_owned(), SliceTypeNodeKind::TypeLiteral));
                }
                return Err(Unsupported::new(
                    "typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)",
                ));
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
        // createTypeNodesFromResolvedType (52137-52240): call
        // signatures, then construct signatures (the 52157 abstract
        // `continue` is unreachable while the re-derivation above
        // curtains every abstract-bearing shape), then index
        // signatures, then properties. The checkTruncationLength
        // probes are approximateLength gates the slice does not
        // model: an over-long literal prints whole where tsc elides
        // `... N more ...` — text-only divergence (T2 tail), the row
        // keys are position+code and unaffected.
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
        for &property in &properties {
            self.property_signature_slice(property, fully_qualified, &mut rendered)?;
        }
        if rendered.is_empty() {
            // 52238: every property skipped -> undefined members ->
            // the member-less literal face.
            return Ok(("{}".to_owned(), SliceTypeNodeKind::TypeLiteral));
        }
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
                    let name = self.property_name_slice(property)?;
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
                let name = self.property_name_slice(property)?;
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
        let name = self.property_name_slice(property)?;
        let type_text = self.type_to_string_slice_ex(property_type, fully_qualified)?;
        let readonly = if self.is_readonly_symbol(property) {
            "readonly "
        } else {
            ""
        };
        let question = if optional { "?" } else { "" };
        rendered.push(format!("{readonly}{name}{question}: {type_text}"));
        Ok(())
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
    /// walk regardless), the JSDoc thisTag arm (52805-52821, [JSDOC]
    /// no-parse) and the JSDocSignature overload-comment tail
    /// (52605-52620). options.modifiers is empty at every slice call
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
    /// declaration lookup (the JSDocParameterTag arm is [JSDOC]
    /// no-parse), the type read, and the rest/optional bits
    /// (isRestParameter on the declaration OR the RestParameter check
    /// flag; isOptionalParameter OR the OptionalParameter check flag).
    fn declared_parameter_face_slice(
        &mut self,
        parameter: SymbolId,
    ) -> CheckResult2<SliceParameterFace> {
        let declaration = self
            .binder
            .symbol(parameter)
            .declarations
            .iter()
            .copied()
            .find(|&declaration| matches!(self.data_of(declaration), NodeData::Parameter(_)));
        let ty = self.get_type_of_symbol(parameter)?;
        let check_flags = self.links.symbol(parameter).check_flags;
        let rest = declaration.is_some_and(|declaration| {
            matches!(self.data_of(declaration), NodeData::Parameter(data)
                if data.dot_dot_dot_token.is_some())
        }) || check_flags.intersects(tsrs2_types::CheckFlags::REST_PARAMETER);
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
            let annotation = match self.data_of(declaration) {
                NodeData::Parameter(data) => data.r#type,
                _ => None,
            };
            let question = matches!(self.data_of(declaration), NodeData::Parameter(data)
                if data.question_token.is_some());
            if let Some(annotation) = annotation {
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
        let name_text =
            match &face.name {
                Some(name) => name.clone(),
                None => {
                    let name_node =
                        face.declaration
                            .and_then(|declaration| match self.data_of(declaration) {
                                NodeData::Parameter(data) => data.name,
                                _ => None,
                            });
                    match name_node {
                        Some(name) => match self.data_of(name) {
                            NodeData::Identifier(data) => {
                                tsrs2_binder::unescape_leading_underscores(&data.escaped_text)
                                    .to_owned()
                            }
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
    /// hasEffectiveQuestionToken's JSDoc arms are no-parse; the
    /// initializer arm reads getMinArgumentCount under
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
        if question {
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
    /// folded in; the JSDocParameterTag arms are no-parse. The
    /// parameter-property arms consult the syntactic modifier mask
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
        if annotation_type == self.tables.intrinsics.error {
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
    /// Hash-private names (getClonedHashPrivateName) and unique-symbol
    /// computed names (symbolToExpression) stay out of slice. tsc
    /// classifies computed/element-access names through
    /// checkExpression's StringLike; the slice reads the late-bound
    /// nameType's flags instead — identical for the literal-typed keys
    /// late binding produces, and the display walk cannot re-enter
    /// checkExpression (recorded deviation).
    fn property_name_slice(&mut self, property: SymbolId) -> CheckResult2<String> {
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
                // 53427-53429: createComputedPropertyName(
                // symbolToExpression(nameType.symbol, Value)). The
                // error path's chain is [symbol] (52946 without an
                // enclosing), so the face is the bare declaration
                // name — oracle: `{ [sym]: number; }`.
                let name_symbol = self
                    .tables
                    .type_of(name_type)
                    .symbol
                    .expect("unique symbols carry their declaration symbol");
                return Ok(format!("[{}]", self.symbol_display_name(name_symbol)));
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

/// The would-be TypeNode kind of a slice rendering. The factory's
/// parenthesizer rules (_tsc.js 20540-20617) branch on the child
/// node's KIND at each join; the string renderer carries the kind
/// beside the text so the joins below apply the same rules. Only
/// kinds the slice can produce are listed — the parenthesizer arms
/// for the rest (conditional/infer heads) land with their shapes.
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
/// signature rung produces the first two; conditional heads stay
/// unproducible (M8).
fn union_constituent_needs_parens(kind: SliceTypeNodeKind) -> bool {
    matches!(
        kind,
        SliceTypeNodeKind::Union
            | SliceTypeNodeKind::Intersection
            | SliceTypeNodeKind::FunctionType
            | SliceTypeNodeKind::ConstructorType
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
    use tsrs2_types::CompilerOptions;

    use crate::state::test_support::with_program_state;
    use crate::state::CheckerState;

    /// Drive the check driver over a single-file program and return
    /// the checker sink as (code, start, length, head message) rows.
    fn checked_diags(text: &str) -> Vec<(u32, u32, u32, String)> {
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
            state.check_source_file(0);
            diag_rows(state)
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
            .filter(|diag| diag.file_name.is_some())
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
    fn ts_expando_function_members_suppress_instead_of_fabricating() {
        // tsc binds `foo.x = 1` onto the function symbol even in .ts
        // files; until stage 3.4c binds them, lookups on flagged
        // parents suppress (errorType) rather than emit 2339/7053.
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

    // ---- 9.3b2 review-round pins (expando name-precision, union
    // best-match, IIFE effective args, optional missing removal) ----

    #[test]
    fn expando_suppression_is_name_precise() {
        // Only the ASSIGNED member suppresses; other names miss in
        // tsc too — y/q report 2339, "z" reports 7053, and the
        // expando'd declaration symbol displays `typeof foo`
        // (oracle-probed byte rows).
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
        // .x / [`x`] / ["x"] reads suppress while .y keeps its row
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
                .filter(|diag| diag.file_name.is_some())
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
    fn json_declaration_twin_suppresses_the_json_resolution() {
        // A present <stem>.d.json.ts twin makes the
        // resolveJsonModule-vs-arbitrary-extensions winner undecidable
        // — the import routes to the Suppressed channel (no 2307, no
        // fabricated 2322); without the twin the JSON literal shape
        // resolves and relates.
        let options = CompilerOptions {
            resolve_json_module: Some(true),
            // ModuleKind.CommonJS
            module: Some(1),
            ..CompilerOptions::default()
        };
        let run = |files: &[(&str, &str)]| -> Vec<(u32, u32, u32, String)> {
            let names: Vec<String> = files.iter().map(|(name, _)| (*name).to_owned()).collect();
            with_program_state(files, &options, |state| {
                // The unit harness has no ProgramJson host — seed the
                // resolver's host paths (the Suppressed channel keys
                // on them) from the program list.
                state.host_file_paths = names.iter().cloned().collect();
                state.check_source_file(0);
                diag_rows(state)
            })
        };
        let with_twin = run(&[
            (
                "/main.ts",
                "import data from \"./data.json\";\nlet x: string = data;\n",
            ),
            ("/data.json", "{}"),
            (
                "/data.d.json.ts",
                "declare var val: string;\nexport default val;\n",
            ),
        ]);
        assert_eq!(with_twin, []);
        let without_twin = run(&[
            (
                "/main.ts",
                "import data from \"./data.json\";\nlet x: string = data;\n",
            ),
            ("/data.json", "{}"),
        ]);
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

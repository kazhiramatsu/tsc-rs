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
use tsrs2_types::{ModifierFlags, NodeCheckFlags, ObjectFlags, TypeData, TypeFlags, TypeId};

use crate::state::{CheckResult2, CheckerState, SignatureKind, Unsupported};

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
        // test is BOTH signals: the node sits inside an already-
        // contained range AND some call-like ancestor's
        // resolvedSignature is Vacant — a call that was ATTEMPTED
        // (it visited this argument) but whose sentinel the
        // containment reverted. Range-inclusion alone is too broad
        // (the first cut regressed 164 accepted identities whose
        // containment was unrelated to their context — the set-ratchet
        // caught it live); a Resolved slot (success or failure-face
        // stash) feeds contextual reads exactly like tsc, so those
        // still check.
        // Scope: FUNCTION kinds only — the fabrication class is
        // contextless PARAMETER typing (7006/7044/18046). Other
        // deferred kinds (assertions, calls) carry their own operands
        // and still check — the first kind-blind cut regressed a
        // deferred assertion's 2352 (subtypingWithCallSignatures3).
        let is_function_kind = matches!(
            self.kind_of(node),
            SyntaxKind::FunctionExpression
                | SyntaxKind::ArrowFunction
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::MethodSignature
        );
        let file_index = self.binder.file_index_of_node(node);
        let (pos, end) = {
            let raw = self.binder.source_of_node(node).arena.node(node);
            (raw.pos, raw.end)
        };
        let inside_contained = is_function_kind
            && self
                .partially_checked_ranges
                .get(&file_index)
                .is_some_and(|ranges| {
                    ranges
                        .iter()
                        .any(|&(range_pos, range_end)| range_pos <= pos && end <= range_end)
                });
        if inside_contained {
            let mut context_call_reverted = false;
            let mut current = node;
            while let Some(parent) = self.parent_of(current) {
                if matches!(
                    self.kind_of(parent),
                    SyntaxKind::CallExpression
                        | SyntaxKind::NewExpression
                        | SyntaxKind::TaggedTemplateExpression
                        | SyntaxKind::Decorator
                        | SyntaxKind::JsxOpeningElement
                        | SyntaxKind::JsxSelfClosingElement
                        | SyntaxKind::JsxOpeningFragment
                ) && matches!(
                    self.links.node(parent).resolved_signature,
                    crate::links::LinkSlot::Vacant
                ) {
                    context_call_reverted = true;
                    break;
                }
                current = parent;
            }
            if context_call_reverted {
                self.mark_partially_checked_node(
                    node,
                    "deferred check under a contained call resolution (context unavailable)",
                );
                return;
            }
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
                // Contextual tuple-from-intersection: literal callers
                // elaborate element mismatches before reaching this
                // generic relation. If no element row was reportable,
                // the remaining verdict depends on contextual tuple
                // arity (M6), so keep the head contained.
                if self
                    .tables
                    .object_flags_of(source)
                    .intersects(tsrs2_types::ObjectFlags::ARRAY_LITERAL)
                    && self
                        .tables
                        .flags_of(target)
                        .intersects(TypeFlags::INTERSECTION)
                {
                    let members = match &self.tables.type_of(target).data {
                        tsrs2_types::TypeData::Intersection { types } => types.to_vec(),
                        _ => Vec::new(),
                    };
                    let mut tuple_member = None;
                    let mut only_contextual_arity_is_unresolved = true;
                    for member in members {
                        if self.is_array_type(member)? || self.tables.is_tuple_type(member) {
                            tuple_member.get_or_insert(member);
                            continue;
                        }
                        // The contextual source can already carry
                        // target members, so a second relation check
                        // here would bless unrelated requirements such
                        // as `{ p: string }`. Only the literal-length
                        // shape is part of the known arity gap.
                        if !self.is_tuple_arity_only_constraint(member)? {
                            only_contextual_arity_is_unresolved = false;
                            break;
                        }
                    }
                    if tuple_member.is_some() {
                        if only_contextual_arity_is_unresolved {
                            return Err(Unsupported::new(
                                "array-literal relation against a tuple-bearing intersection \
                                 after element elaboration (contextual tuple arity, M6)",
                            ));
                        }
                        // The relation tail is T2, but the outer head
                        // is determinate. Render the contextual tuple
                        // (the actual array-literal source in tsc),
                        // then avoid the missing-property override,
                        // which is inapplicable to an intersection
                        // target and otherwise escapes before 2322.
                        let (source_text, target_text) = self
                            .tuple_intersection_relation_display_from_syntax(error_node)
                            .ok_or_else(|| {
                                Unsupported::new(
                                    "tuple-bearing intersection relation display without a \
                                     directly annotated tuple (nodeBuilder work, M6)",
                                )
                            })?;
                        self.error_at(
                            Some(error_node),
                            head_message,
                            &[&source_text, &target_text],
                        );
                        return Ok(related);
                    }
                }
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
                let mut source_text = self.type_to_string_slice(source)?;
                let mut target_text = self.type_to_string_slice(target)?;
                if source_text == target_text {
                    // getTypeNamesForErrorDisplay (50748-50756): equal
                    // renders re-render fully qualified.
                    source_text = self.get_type_name_for_error_display(source)?;
                    target_text = self.get_type_name_for_error_display(target)?;
                }
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

    /// Narrow display bridge for the determinate 2322 head above.
    /// The full tuple renderer is M6; a directly written annotation
    /// already contains both strings needed by this relation head.
    fn tuple_intersection_relation_display_from_syntax(
        &self,
        error_node: NodeId,
    ) -> Option<(String, String)> {
        let mut cursor = Some(error_node);
        for _ in 0..6 {
            let current = cursor?;
            if let Some(annotation) = self.type_annotation_of(current) {
                if let NodeData::IntersectionType(data) = self.data_of(annotation) {
                    let tuple_node = self.nodes_of(data.types).into_iter().find(|&member| {
                        matches!(
                            self.kind_of(member),
                            SyntaxKind::TupleType | SyntaxKind::ArrayType
                        )
                    })?;
                    return Some((
                        self.text_of_node(tuple_node).ok()?,
                        self.text_of_node(annotation).ok()?,
                    ));
                }
            }
            cursor = self.parent_of(current);
        }
        None
    }

    /// The remaining M6 hole is specifically a contextual tuple's
    /// literal `length` relation. It must not hide unrelated required
    /// members on another intersection constituent.
    fn is_tuple_arity_only_constraint(&mut self, ty: TypeId) -> CheckResult2<bool> {
        if !self.tables.flags_of(ty).intersects(TypeFlags::OBJECT) {
            return Ok(false);
        }
        let properties = self.get_properties_of_type(ty)?;
        if properties.len() != 1
            || self.binder.symbol(properties[0]).escaped_name.as_str() != "length"
        {
            return Ok(false);
        }
        Ok(self
            .get_signatures_of_type(ty, SignatureKind::Call)?
            .is_empty()
            && self
                .get_signatures_of_type(ty, SignatureKind::Construct)?
                .is_empty()
            && self.get_index_infos_of_type(ty)?.is_empty())
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
        let source_text = self.type_to_string_slice(source)?;
        let target_text = self.type_to_string_slice(target)?;
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
        let source_text = self.type_to_string_slice(source)?;
        let target_text = self.type_to_string_slice(target)?;
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
                let mut source_text = self.type_to_string_slice(source)?;
                let mut target_text = self.type_to_string_slice(target)?;
                if source_text == target_text {
                    // getTypeNamesForErrorDisplay (50748-50756): equal
                    // renders re-render fully qualified.
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
            return Ok(match name {
                Some(name) => format!("{prefix}{name}"),
                None => "?".to_owned(),
            });
        }
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::TYPE_PARAMETER) {
            return match self.tables.type_of(ty).symbol {
                Some(symbol) => Ok(self.symbol_display_name(symbol)),
                None => Ok("?".to_owned()),
            };
        }
        // Named object types (interface/class/enum declared shapes)
        // print their symbol name — the nodeBuilder's symbol reference
        // without qualification (lib types like Date flow into 2344
        // args; anonymous __type shapes stay out of slice).
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
                    return Ok(if fully_qualified {
                        self.get_fully_qualified_name(symbol)
                    } else {
                        self.symbol_display_name(symbol)
                    });
                }
            }
        }
        match &self.tables.type_of(ty).data {
            TypeData::Intrinsic { name, .. } => Ok((*name).to_owned()),
            TypeData::Literal { value } => match value {
                tsrs2_types::LiteralValue::String(text)
                    if text.chars().all(|c| {
                        c.is_ascii() && !c.is_ascii_control() && c != '"' && c != '\\'
                    }) =>
                {
                    Ok(format!("\"{text}\""))
                }
                tsrs2_types::LiteralValue::Number(value) => {
                    Ok(tsrs2_types::js_number_to_string(*value))
                }
                _ => Err(Unsupported::new(
                    "literal display beyond plain strings/numbers (nodeBuilder, T2/M8)",
                )),
            },
            _ => self.type_to_string_slice_structured(ty, fully_qualified),
        }
    }

    fn type_to_string_slice_structured(
        &mut self,
        ty: TypeId,
        fully_qualified: bool,
    ) -> CheckResult2<String> {
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
                    let mut rendered = Vec::new();
                    for argument in arguments.iter() {
                        rendered.push(self.type_to_string_slice_ex(*argument, fully_qualified)?);
                    }
                    Ok(format!("{name}<{}>", rendered.join(", ")))
                }
                _ => Ok(name),
            };
        }
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::UNION | TypeFlags::INTERSECTION) {
            let (types, origin) = match &self.tables.type_of(ty).data {
                TypeData::Union { types, origin } => (types.to_vec(), *origin),
                TypeData::Intersection { types } => (types.to_vec(), None),
                _ => unreachable!("union/intersection flag implies composite data"),
            };
            if let Some(origin) = origin {
                // typeToString prints the ORIGIN of a denormalized
                // union (nodeBuilder typeToTypeNodeHelper origin read):
                // keyof origins render as `keyof T`. Non-keyof origins
                // (syntactic `A | B` denormalizations) stay behind the
                // curtain — the M5/M6 verdict bands lean on this
                // suppression as their FP shield (5.9d re-measure:
                // 2403/2322/2536 rows over narrowable unions fabricate
                // without flow narrowing).
                let origin_flags = self.tables.flags_of(origin);
                if origin_flags.intersects(TypeFlags::INDEX) {
                    let inner = match self.tables.type_of(origin).data {
                        TypeData::Index { ty: inner, .. } => inner,
                        _ => unreachable!("INDEX flag implies Index data"),
                    };
                    let inner = self.type_to_string_slice_ex(inner, fully_qualified)?;
                    return Ok(format!("keyof {inner}"));
                }
                return Err(Unsupported::new(
                    "origin-union display beyond keyof origins (M5/M6 verdict shield; nodeBuilder tail, M8)",
                ));
            }
            let separator = if flags.intersects(TypeFlags::UNION) {
                " | "
            } else {
                " & "
            };
            let mut rendered = Vec::new();
            for member in types {
                rendered.push(self.type_to_string_slice_ex(member, fully_qualified)?);
            }
            return Ok(rendered.join(separator));
        }
        if self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::REFERENCE)
        {
            let target = self.tables.reference_target(ty);
            let Some(symbol) = self.tables.type_of(target).symbol else {
                return Err(Unsupported::new(
                    "symbol-less reference display (tuple renderer, M6)",
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
                let element = self.type_to_string_slice_ex(arguments[0], fully_qualified)?;
                return Ok(if name == "Array" {
                    format!("{element}[]")
                } else {
                    format!("readonly {element}[]")
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
            return Ok(if rendered.is_empty() {
                name
            } else {
                format!("{name}<{}>", rendered.join(", "))
            });
        }
        Err(Unsupported::new(
            "typeToString beyond the 5.4 display slice (nodeBuilder, T2/M8)",
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
}

#![allow(dead_code, unused_imports)]

mod context;
mod tracker;

pub(crate) use crate::syntactic_type_node_builder::SyntacticTypeNodeBuilder;
pub(crate) use context::{
    add_symbol_type_to_context, can_possibly_expand_type, check_truncation_length,
    check_truncation_length_if_expanding, no_inference_fallback_is_set, restore_flags,
    restore_no_inference_fallback, restore_symbol_type_to_context, save_no_inference_fallback,
    save_restore_flags, should_expand_type, with_context, FlagsRestore, NodeBuilderContext,
    SymbolTypeRestore, TrackedSymbol, DEFAULT_MAXIMUM_TRUNCATION_LENGTH,
    NO_TRUNCATION_MAXIMUM_TRUNCATION_LENGTH,
};
pub(crate) use tracker::NodeBuilderTracker;

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

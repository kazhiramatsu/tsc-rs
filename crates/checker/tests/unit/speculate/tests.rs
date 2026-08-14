use tsc_binder::flow::FlowId;
use tsc_diagnostics::{gen as diagnostics, RelatedInfo};
use tsc_types::{
    CompilerOptions, NodeCheckFlags, SymbolFlags, TypeData, TypeFlags, TypeSystemPropertyName,
};

use super::SpeculationOutcome;
use crate::flow::FlowType;
use crate::links::LinkSlot;
use crate::state::test_support::with_program_state;
use crate::state::{
    CheckAbort, CheckerState, InProgressMappedType, ResolvedMembers, SignatureKind,
};

fn with_state<R>(run: impl FnOnce(&mut CheckerState) -> R) -> R {
    with_program_state(
        &[("a.ts", "declare var a: string;\n")],
        &CompilerOptions::default(),
        run,
    )
}

/// Push something onto every checkpoint-covered piece of state a
/// unit test can reach without heavier machinery, then return the
/// values needed to assert restoration.
fn mutate_everything(state: &mut CheckerState) {
    let root = state.binder.source(0).root;
    let string = state.tables.intrinsics.string;
    // A: transient stacks.
    state.resolution_results.push(true);
    state
        .resolution_property_names
        .push(TypeSystemPropertyName::TYPE);
    state.resolution_start += 7;
    state.contextual_type_nodes.push(root);
    state.contextual_types.push(Some(string));
    state.contextual_is_cache.push(false);
    state.contextual_binding_patterns.push(root);
    state.inference_context_nodes.push(root);
    state.inference_contexts.push(None);
    state.awaited_type_stack.push(string);
    state
        .active_type_mappers_caches
        .push(std::collections::HashMap::new());
    state.mapped_types_in_progress.push(InProgressMappedType {
        node: root,
        ty: string,
    });
    state.flow_loop_start += 3;
    state.shared_flow.push((FlowId(0), FlowType::Type(string)));
    state
        .reduce_label_overrides
        .insert((0, FlowId(1)), vec![FlowId(2)]);
    state.exhaustive_switch_computing.insert(root);
    // B: counters / flags.
    state.instantiation_depth += 5;
    state.inline_level += 2;
    state.in_variance_computation = true;
    state.variance_type_parameter = Some(string);
    state.suggestion_count += 4;
    state.is_inference_partially_blocked = true;
    // D: diagnostics sinks.
    let diagnostic = state.create_error(None, &diagnostics::Cannot_find_name_0, &["x"]);
    state.visible_global_diagnostics.push(diagnostic.clone());
    state.push_error_diagnostic(diagnostic);
    state.mark_partially_checked_node(root, "7.0t rollback test");
    state.elaborated_satisfies_expressions.insert(root);
    state.potential_this_collisions.push(root);
    state.potential_new_target_collisions.push(root);
    state.potential_weak_map_set_collisions.push(root);
    state.potential_reflect_collisions.push(root);
    state
        .potential_unused_renamed_binding_elements_in_types
        .push(root);
}

/// The observable projection of every checkpoint-covered field —
/// captured before begin and compared after rollback (the state
/// constructor itself emits lib-less global diagnostics, so
/// absolute zeros are wrong; restoration-to-baseline is the
/// contract).
#[derive(Debug, PartialEq)]
struct Observed {
    speculation_depth: u32,
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
    active_type_mappers_caches: usize,
    mapped_types_in_progress: usize,
    flow_loop_start: u32,
    shared_flow: usize,
    reduce_label_overrides: usize,
    exhaustive_switch_computing: usize,
    instantiation_depth: u32,
    inline_level: u32,
    in_variance_computation: bool,
    variance_type_parameter: Option<tsc_types::TypeId>,
    suggestion_count: u32,
    is_inference_partially_blocked: bool,
    diagnostics: usize,
    visible_global_diagnostics: usize,
    partial_check_records: usize,
    partially_checked_files: usize,
    elaborated_satisfies_expressions: usize,
    potential_this_collisions: usize,
    potential_new_target_collisions: usize,
    potential_weak_map_set_collisions: usize,
    potential_reflect_collisions: usize,
    potential_unused_renamed_binding_elements_in_types: usize,
}

fn observe(state: &CheckerState) -> Observed {
    Observed {
        speculation_depth: state.speculation_depth,
        resolution_results: state.resolution_results.len(),
        resolution_property_names: state.resolution_property_names.len(),
        resolution_start: state.resolution_start,
        contextual_type_nodes: state.contextual_type_nodes.len(),
        contextual_types: state.contextual_types.len(),
        contextual_is_cache: state.contextual_is_cache.len(),
        contextual_binding_patterns: state.contextual_binding_patterns.len(),
        inference_context_nodes: state.inference_context_nodes.len(),
        inference_contexts: state.inference_contexts.len(),
        awaited_type_stack: state.awaited_type_stack.len(),
        active_type_mappers_caches: state.active_type_mappers_caches.len(),
        mapped_types_in_progress: state.mapped_types_in_progress.len(),
        flow_loop_start: state.flow_loop_start,
        shared_flow: state.shared_flow.len(),
        reduce_label_overrides: state.reduce_label_overrides.len(),
        exhaustive_switch_computing: state.exhaustive_switch_computing.len(),
        instantiation_depth: state.instantiation_depth,
        inline_level: state.inline_level,
        in_variance_computation: state.in_variance_computation,
        variance_type_parameter: state.variance_type_parameter,
        suggestion_count: state.suggestion_count,
        is_inference_partially_blocked: state.is_inference_partially_blocked,
        diagnostics: state.diagnostics.len(),
        visible_global_diagnostics: state.visible_global_diagnostics.len(),
        partial_check_records: state.partial_check_records.len(),
        partially_checked_files: state.partially_checked_ranges.len(),
        elaborated_satisfies_expressions: state.elaborated_satisfies_expressions.len(),
        potential_this_collisions: state.potential_this_collisions.len(),
        potential_new_target_collisions: state.potential_new_target_collisions.len(),
        potential_weak_map_set_collisions: state.potential_weak_map_set_collisions.len(),
        potential_reflect_collisions: state.potential_reflect_collisions.len(),
        potential_unused_renamed_binding_elements_in_types: state
            .potential_unused_renamed_binding_elements_in_types
            .len(),
    }
}

#[test]
fn rollback_restores_stacks_counters_and_sinks() {
    with_state(|state| {
        let before = observe(state);
        let checkpoint = state.begin_speculation();
        assert_eq!(state.speculation_depth, 1);
        mutate_everything(state);
        state.rollback_speculation(checkpoint);
        assert_eq!(observe(state), before);
    });
}

#[test]
fn rollback_keeps_instantiation_count_and_deferred_nodes() {
    with_state(|state| {
        let root = state.binder.source(0).root;
        let checkpoint = state.begin_speculation();
        // instantiation_count is monotone per element in tsc —
        // reset only at the three check entry points, never on a
        // failed candidate.
        state.instantiation_count += 11;
        // tsc checkNodeDeferred (86899) registers unconditionally;
        // nodes deferred under a failed candidate are still
        // checked.
        state.deferred_nodes.entry(root).or_default().insert(root);
        state.rollback_speculation(checkpoint);
        assert_eq!(state.instantiation_count, 11);
        assert!(state.deferred_nodes.contains_key(&root));
    });
}

#[test]
fn commit_keeps_sinks_and_budget_consumption() {
    with_state(|state| {
        let root = state.binder.source(0).root;
        let before = observe(state);
        let checkpoint = state.begin_speculation();
        let diagnostic = state.create_error(None, &diagnostics::Cannot_find_name_0, &["x"]);
        state.push_error_diagnostic(diagnostic);
        state.mark_partially_checked_node(root, "7.0t commit test");
        state.suggestion_count += 1;
        state.commit_speculation(checkpoint);
        assert_eq!(state.speculation_depth, 0);
        assert_eq!(state.diagnostics.len(), before.diagnostics + 1);
        assert_eq!(
            state.partial_check_records.len(),
            before.partial_check_records + 1
        );
        assert_eq!(state.suggestion_count, before.suggestion_count + 1);
    });
}

#[test]
fn link_protocols_are_temporary_on_commit_and_rollback() {
    with_state(|state| {
        let committed_node = state.binder.source(0).root;
        let rolled_back_node = state
            .binder
            .source(0)
            .arena
            .node_ids()
            .nth(1)
            .expect("fixture has a declaration");
        let committed_type = state
            .tables
            .create_type(TypeFlags::OBJECT, TypeData::Object);
        let rolled_back_type = state
            .tables
            .create_type(TypeFlags::OBJECT, TypeData::Object);
        let committed_symbol = state
            .binder
            .create_symbol(SymbolFlags::VARIABLE, "committed".to_owned());
        let rolled_back_symbol = state
            .binder
            .create_symbol(SymbolFlags::VARIABLE, "rolledBack".to_owned());
        let committed_members = state.alloc_members(ResolvedMembers::default());
        let rolled_back_members = state.alloc_members(ResolvedMembers::default());
        let exercise = |state: &mut CheckerState, node, symbol, ty, members| {
            state.links.set_node_resolved_signature_call_protocol(
                state.speculation_depth,
                node,
                LinkSlot::Resolving,
            );
            state.links.set_node_resolved_signature_call_protocol(
                state.speculation_depth,
                node,
                LinkSlot::Resolved(state.any_signature),
            );
            state.links.set_node_resolved_type(
                state.speculation_depth,
                node,
                LinkSlot::Resolved(state.tables.intrinsics.string),
            );
            state.links.set_symbol_declared_type(
                state.speculation_depth,
                symbol,
                LinkSlot::Resolved(state.tables.intrinsics.string),
            );
            state.links.set_symbol_unique_es_symbol_type(
                state.speculation_depth,
                symbol,
                state.tables.intrinsics.es_symbol,
            );
            state
                .links
                .set_type_members(state.speculation_depth, ty, LinkSlot::Resolved(members));
        };

        let committed = state.begin_speculation();
        exercise(
            state,
            committed_node,
            committed_symbol,
            committed_type,
            committed_members,
        );
        state.commit_speculation(committed);
        assert!(matches!(
            state.links.node(committed_node).resolved_signature,
            LinkSlot::Vacant
        ));
        assert!(matches!(
            state.links.node(committed_node).resolved_type,
            LinkSlot::Vacant
        ));
        assert!(matches!(
            state.links.symbol(committed_symbol).declared_type,
            LinkSlot::Vacant
        ));
        assert!(state
            .links
            .symbol(committed_symbol)
            .unique_es_symbol_type
            .is_none());
        assert!(matches!(
            state.links.ty(committed_type).resolved_members,
            LinkSlot::Vacant
        ));

        let rolled_back = state.begin_speculation();
        exercise(
            state,
            rolled_back_node,
            rolled_back_symbol,
            rolled_back_type,
            rolled_back_members,
        );
        state.rollback_speculation(rolled_back);
        assert!(matches!(
            state.links.node(rolled_back_node).resolved_signature,
            LinkSlot::Vacant
        ));
        assert!(matches!(
            state.links.node(rolled_back_node).resolved_type,
            LinkSlot::Vacant
        ));
        assert!(matches!(
            state.links.symbol(rolled_back_symbol).declared_type,
            LinkSlot::Vacant
        ));
        assert!(state
            .links
            .symbol(rolled_back_symbol)
            .unique_es_symbol_type
            .is_none());
        assert!(matches!(
            state.links.ty(rolled_back_type).resolved_members,
            LinkSlot::Vacant
        ));
        assert_eq!(state.links.speculative_resolved_signature_mark(), 0);
        assert_eq!(state.links.speculative_resolved_type_mark(), 0);
        assert_eq!(state.links.speculative_symbol_declared_type_mark(), 0);
        assert_eq!(state.links.speculative_unique_es_symbol_type_mark(), 0);
        assert_eq!(state.links.speculative_type_members_mark(), 0);
    });
}

#[test]
fn declaration_signatures_commit_and_nested_rollback_restores() {
    with_program_state(
        &[(
            "a.ts",
            "declare function f(): string;\ndeclare function g(): number;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let declaration = |state: &CheckerState, name: &str| {
                let symbol = state
                    .resolve_file_scope_name(name, SymbolFlags::FUNCTION)
                    .expect("function resolves");
                state.binder.symbol(symbol).declarations[0]
            };
            let f = declaration(state, "f");
            let g = declaration(state, "g");

            let committed = state.begin_speculation();
            let f_signature = state
                .get_signature_from_declaration(f)
                .expect("signature resolves");
            state.commit_speculation(committed);
            assert!(matches!(
                state.links.node(f).resolved_signature,
                LinkSlot::Resolved(signature) if signature == f_signature
            ));

            let outer = state.begin_speculation();
            let inner = state.begin_speculation();
            state
                .get_signature_from_declaration(g)
                .expect("nested signature resolves");
            state.commit_speculation(inner);
            assert!(matches!(
                state.links.node(g).resolved_signature,
                LinkSlot::Resolved(_)
            ));
            state.rollback_speculation(outer);
            assert!(matches!(
                state.links.node(g).resolved_signature,
                LinkSlot::Vacant
            ));
            assert_eq!(state.links.speculative_declaration_signature_mark(), 0);
        },
    );
}

#[test]
fn selected_context_state_commits_and_nested_rollback_restores() {
    with_state(|state| {
        let mut nodes = state.binder.source(0).arena.node_ids();
        let committed_node = nodes.next().expect("fixture root");
        let nested_node = nodes.next().expect("fixture declaration");
        let committed_symbol = state
            .binder
            .create_symbol(SymbolFlags::FUNCTION_SCOPED_VARIABLE, "x".to_owned());
        let nested_symbol = state
            .binder
            .create_symbol(SymbolFlags::FUNCTION_SCOPED_VARIABLE, "y".to_owned());

        let selected = state.begin_speculation();
        state.links.or_node_check_flags(
            state.speculation_depth,
            committed_node,
            NodeCheckFlags::CONTEXT_CHECKED,
        );
        state.links.set_symbol_type_contextual(
            state.speculation_depth,
            committed_symbol,
            LinkSlot::Resolved(state.tables.intrinsics.string),
        );
        state.commit_speculation(selected);

        assert!(state
            .links
            .node(committed_node)
            .check_flags
            .intersects(NodeCheckFlags::CONTEXT_CHECKED));
        assert_eq!(
            state
                .links
                .symbol(committed_symbol)
                .type_of_symbol
                .resolved(),
            Some(state.tables.intrinsics.string)
        );
        assert_eq!(state.links.speculative_context_checked_mark(), 0);
        assert_eq!(state.links.speculative_symbol_type_mark(), 0);

        let outer = state.begin_speculation();
        let inner = state.begin_speculation();
        state.links.or_node_check_flags(
            state.speculation_depth,
            nested_node,
            NodeCheckFlags::CONTEXT_CHECKED,
        );
        state.links.set_symbol_type_contextual(
            state.speculation_depth,
            nested_symbol,
            LinkSlot::Resolved(state.tables.intrinsics.number),
        );
        state.commit_speculation(inner);
        assert!(state
            .links
            .node(nested_node)
            .check_flags
            .intersects(NodeCheckFlags::CONTEXT_CHECKED));
        assert_eq!(
            state.links.symbol(nested_symbol).type_of_symbol.resolved(),
            Some(state.tables.intrinsics.number)
        );

        state.rollback_speculation(outer);
        assert!(!state
            .links
            .node(nested_node)
            .check_flags
            .intersects(NodeCheckFlags::CONTEXT_CHECKED));
        assert!(matches!(
            state.links.symbol(nested_symbol).type_of_symbol,
            LinkSlot::Vacant
        ));
        assert_eq!(state.links.speculative_context_checked_mark(), 0);
        assert_eq!(state.links.speculative_symbol_type_mark(), 0);
    });
}

#[test]
fn rejected_candidate_retains_completed_context_state_and_its_diagnostics() {
    with_state(|state| {
        let node = state.binder.source(0).root;
        let contextual_symbol = state.binder.create_symbol(
            SymbolFlags::FUNCTION_SCOPED_VARIABLE,
            "contextual".to_owned(),
        );
        let temporary_symbol = state.binder.create_symbol(
            SymbolFlags::FUNCTION_SCOPED_VARIABLE,
            "temporary".to_owned(),
        );
        let diagnostics_before = state.diagnostics.len();

        let result = state
            .speculate(|state| {
                state.links.or_node_check_flags(
                    state.speculation_depth,
                    node,
                    NodeCheckFlags::CONTEXT_CHECKED,
                );
                state.links.set_symbol_type_contextual(
                    state.speculation_depth,
                    contextual_symbol,
                    LinkSlot::Resolved(state.tables.intrinsics.string),
                );
                state.links.set_symbol_type(
                    state.speculation_depth,
                    temporary_symbol,
                    LinkSlot::Resolved(state.tables.intrinsics.number),
                );
                let contextual_diagnostics_start = state.diagnostics.len();
                let contextual_diagnostic =
                    state.create_error(None, &diagnostics::Cannot_find_name_0, &["contextual"]);
                state.push_error_diagnostic(contextual_diagnostic);
                state.record_completed_contextual_diagnostics_since(
                    contextual_diagnostics_start,
                    state.visible_global_diagnostics.len(),
                );
                let diagnostic =
                    state.create_error(None, &diagnostics::Cannot_find_name_0, &["trial"]);
                state.push_error_diagnostic(diagnostic);
                Ok(SpeculationOutcome::Reject(7))
            })
            .expect("candidate rejection completes");

        assert_eq!(result, 7);
        assert!(state
            .links
            .node(node)
            .check_flags
            .intersects(NodeCheckFlags::CONTEXT_CHECKED));
        assert_eq!(
            state
                .links
                .symbol(contextual_symbol)
                .type_of_symbol
                .resolved(),
            Some(state.tables.intrinsics.string)
        );
        assert!(matches!(
            state.links.symbol(temporary_symbol).type_of_symbol,
            LinkSlot::Vacant
        ));
        assert_eq!(state.diagnostics.len(), diagnostics_before + 1);
        assert_eq!(
            state.diagnostics.last().unwrap().message_text(),
            "Cannot find name 'contextual'."
        );
        assert_eq!(state.links.speculative_context_checked_mark(), 0);
        assert_eq!(state.links.speculative_symbol_type_mark(), 0);
        assert!(state.completed_contextual_diagnostics.is_empty());
        assert!(state
            .completed_contextual_visible_global_diagnostics
            .is_empty());
        assert_eq!(state.speculation_depth, 0);
    });
}

#[test]
fn speculate_outcomes_commit_and_rollback() {
    with_state(|state| {
        let baseline = state.diagnostics.len();
        let committed = state.speculate(|state| {
            let diagnostic = state.create_error(None, &diagnostics::Cannot_find_name_0, &["kept"]);
            state.push_error_diagnostic(diagnostic);
            Ok(SpeculationOutcome::Commit(1))
        });
        assert_eq!(committed, Ok(1));
        assert_eq!(state.diagnostics.len(), baseline + 1);

        let rolled_back = state.speculate(|state| {
            let diagnostic =
                state.create_error(None, &diagnostics::Cannot_find_name_0, &["dropped"]);
            state.push_error_diagnostic(diagnostic);
            Ok(SpeculationOutcome::Rollback(2))
        });
        assert_eq!(rolled_back, Ok(2));
        assert_eq!(state.diagnostics.len(), baseline + 1);
        assert_eq!(state.speculation_depth, 0);
    });
}

#[test]
fn signature_return_seals_commit_and_nested_rollback_restores() {
    with_program_state(
        &[("a.ts", "declare function f(): string;\n")],
        &CompilerOptions::default(),
        |state| {
            let symbol = state
                .resolve_file_scope_name("f", SymbolFlags::FUNCTION)
                .expect("f resolves");
            let ty = state.get_type_of_symbol(symbol).expect("f types");
            let signature = state
                .get_signatures_of_type(ty, SignatureKind::Call)
                .expect("f signatures")[0];
            assert!(matches!(
                state.signature_of(signature).resolved_return_type,
                LinkSlot::Vacant
            ));

            let rolled_back = state.begin_speculation();
            state.seal_signature_return_type(signature, state.tables.intrinsics.string);
            state.rollback_speculation(rolled_back);
            assert!(matches!(
                state.signature_of(signature).resolved_return_type,
                LinkSlot::Vacant
            ));

            let outer = state.begin_speculation();
            let inner = state.begin_speculation();
            state.seal_signature_return_type(signature, state.tables.intrinsics.string);
            state.commit_speculation(inner);
            assert!(matches!(
                state.signature_of(signature).resolved_return_type,
                LinkSlot::Resolved(_)
            ));
            state.rollback_speculation(outer);
            assert!(matches!(
                state.signature_of(signature).resolved_return_type,
                LinkSlot::Vacant
            ));

            let committed = state.begin_speculation();
            state.seal_signature_return_type(signature, state.tables.intrinsics.string);
            state.commit_speculation(committed);
            assert!(matches!(
                state.signature_of(signature).resolved_return_type,
                LinkSlot::Resolved(ty) if ty == state.tables.intrinsics.string
            ));
        },
    );
}

/// The boundary ordering rule: by the time the caller sees the
/// Err, the rollback has already happened — outer Err-revert twins
/// run at the entry depth.
#[test]
fn speculate_rolls_back_before_err_reaches_caller() {
    with_state(|state| {
        // Not an oracle-crash containment event: the test-only
        // variant exercises transaction ordering without adding a
        // production abort kind.
        let boundary_probe = || CheckAbort::BoundaryProbe;
        let before = observe(state);
        let result: Result<(), _> = state.speculate(|state| {
            assert_eq!(state.speculation_depth, 1);
            mutate_everything(state);
            Err(boundary_probe())
        });
        assert_eq!(result, Err(boundary_probe()));
        assert_eq!(observe(state), before);
    });
}

#[test]
fn nested_speculation_resolves_lifo() {
    with_state(|state| {
        let string = state.tables.intrinsics.string;
        let outer = state.begin_speculation();
        state.awaited_type_stack.push(string);
        let inner = state.begin_speculation();
        assert_eq!(state.speculation_depth, 2);
        state.awaited_type_stack.push(string);
        state.rollback_speculation(inner);
        assert_eq!(state.speculation_depth, 1);
        assert_eq!(state.awaited_type_stack.len(), 1);
        state.rollback_speculation(outer);
        assert_eq!(state.speculation_depth, 0);
        assert_eq!(state.awaited_type_stack.len(), 0);
    });
}

#[test]
fn tsc_eager_nested_reporting_rows_survive_inner_rollback_while_ordinary_rows_do_not() {
    with_state(|state| {
        let diagnostics_before = state.diagnostics.len();
        let visible_before = state.visible_global_diagnostics.len();
        let outer = state.begin_speculation();
        let outer_capture = state.begin_tsc_eager_iteration_diagnostic_capture();
        let inner = state.begin_speculation();

        let ordinary_before =
            state.create_error(None, &diagnostics::Cannot_find_name_0, &["ordinary before"]);
        state.push_error_diagnostic(ordinary_before.clone());

        let nested_capture = state.begin_tsc_eager_iteration_diagnostic_capture();
        let eager = state.create_error(
            None,
            &diagnostics::Cannot_find_name_0,
            &["nested reporting"],
        );
        state.push_error_diagnostic(eager.clone());
        state.visible_global_diagnostics.push(eager.clone());
        state.end_tsc_eager_iteration_diagnostic_capture(nested_capture);

        let ordinary_after =
            state.create_error(None, &diagnostics::Cannot_find_name_0, &["ordinary after"]);
        state.push_error_diagnostic(ordinary_after.clone());
        assert_eq!(state.tsc_eager_diagnostics, std::slice::from_ref(&eager));
        assert_eq!(
            state.tsc_eager_visible_global_diagnostics,
            std::slice::from_ref(&eager)
        );

        state.rollback_speculation(inner);
        assert!(!state.diagnostics.contains(&ordinary_before));
        assert!(!state.diagnostics.contains(&ordinary_after));
        assert!(state.diagnostics.contains(&eager));
        assert!(state.visible_global_diagnostics.contains(&eager));
        assert_eq!(state.tsc_eager_iteration_capture_depth, 1);

        state.end_tsc_eager_iteration_diagnostic_capture(outer_capture);
        state.rollback_speculation(outer);
        assert_eq!(state.diagnostics.len(), diagnostics_before + 1);
        assert_eq!(state.visible_global_diagnostics.len(), visible_before + 1);
        assert_eq!(
            state
                .diagnostics
                .iter()
                .filter(|diagnostic| **diagnostic == eager)
                .count(),
            1
        );
        assert!(state.tsc_eager_diagnostics.is_empty());
        assert!(state.tsc_eager_visible_global_diagnostics.is_empty());
        assert_eq!(state.tsc_eager_iteration_capture_depth, 0);
    });
}

#[test]
fn tsc_eager_nested_speculation_commit_is_balanced_inside_reporting_capture() {
    with_state(|state| {
        let diagnostics_before = state.diagnostics.len();
        let outer = state.begin_speculation();
        let outer_capture = state.begin_tsc_eager_iteration_diagnostic_capture();
        let inner = state.begin_speculation();
        let nested_capture = state.begin_tsc_eager_iteration_diagnostic_capture();
        let eager = state.create_error(None, &diagnostics::Cannot_find_name_0, &["nested commit"]);
        state.push_error_diagnostic(eager.clone());
        state.end_tsc_eager_iteration_diagnostic_capture(nested_capture);

        state.commit_speculation(inner);
        assert_eq!(state.speculation_depth, 1);
        assert_eq!(state.tsc_eager_iteration_capture_depth, 1);
        state.end_tsc_eager_iteration_diagnostic_capture(outer_capture);
        state.rollback_speculation(outer);

        assert_eq!(state.diagnostics.len(), diagnostics_before + 1);
        assert_eq!(
            state
                .diagnostics
                .iter()
                .filter(|diagnostic| **diagnostic == eager)
                .count(),
            1
        );
        assert!(state.tsc_eager_diagnostics.is_empty());
        assert!(state.tsc_eager_visible_global_diagnostics.is_empty());
        assert_eq!(state.tsc_eager_iteration_capture_depth, 0);
    });
}

#[test]
fn tsc_eager_iteration_diagnostic_survives_nested_and_outer_rollback_in_both_sinks() {
    with_state(|state| {
        let diagnostics_before = state.diagnostics.len();
        let visible_before = state.visible_global_diagnostics.len();
        let outer = state.begin_speculation();
        let inner = state.begin_speculation();

        let ordinary =
            state.create_error(None, &diagnostics::Cannot_find_name_0, &["ordinary trial"]);
        state.push_error_diagnostic(ordinary.clone());

        let outer_capture = state.begin_tsc_eager_iteration_diagnostic_capture();
        let nested_capture = state.begin_tsc_eager_iteration_diagnostic_capture();
        let eager =
            state.create_error(None, &diagnostics::Cannot_find_name_0, &["eager iteration"]);
        let root_index = state.push_error_diagnostic(eager);
        state.end_tsc_eager_iteration_diagnostic_capture(nested_capture);
        assert_eq!(state.tsc_eager_diagnostics.len(), 1);
        assert!(state.tsc_eager_diagnostics[0].related.is_empty());

        let related = state.create_error(None, &diagnostics::Did_you_forget_to_use_await, &[]);
        state.diagnostics[root_index].related.push(RelatedInfo {
            file_name: related.file_name,
            start: related.start,
            length: related.length,
            message: related.message,
        });
        let eager = state.diagnostics[root_index].clone();
        state.visible_global_diagnostics.push(eager.clone());
        state.end_tsc_eager_iteration_diagnostic_capture(outer_capture);

        assert_eq!(state.tsc_eager_diagnostics, std::slice::from_ref(&eager));
        assert_eq!(
            state.tsc_eager_visible_global_diagnostics,
            std::slice::from_ref(&eager)
        );
        assert_eq!(state.tsc_eager_diagnostics[0].related.len(), 1);

        state.rollback_speculation(inner);
        assert!(!state.diagnostics.contains(&ordinary));
        assert!(state.diagnostics.contains(&eager));
        assert!(state.visible_global_diagnostics.contains(&eager));
        assert_eq!(state.tsc_eager_diagnostics, std::slice::from_ref(&eager));

        state.rollback_speculation(outer);
        assert_eq!(state.diagnostics.len(), diagnostics_before + 1);
        assert_eq!(state.visible_global_diagnostics.len(), visible_before + 1);
        assert!(state.diagnostics.contains(&eager));
        assert!(state.visible_global_diagnostics.contains(&eager));
        assert!(state.tsc_eager_diagnostics.is_empty());
        assert!(state.tsc_eager_visible_global_diagnostics.is_empty());
        assert_eq!(state.tsc_eager_iteration_capture_depth, 0);
    });
}

#[test]
fn nested_commit_promotes_tsc_eager_iteration_diagnostic_to_outer_rollback() {
    with_state(|state| {
        let diagnostics_before = state.diagnostics.len();
        let outer = state.begin_speculation();
        let inner = state.begin_speculation();
        let capture = state.begin_tsc_eager_iteration_diagnostic_capture();
        let eager = state.create_error(None, &diagnostics::Cannot_find_name_0, &["nested commit"]);
        state.push_error_diagnostic(eager.clone());
        state.end_tsc_eager_iteration_diagnostic_capture(capture);

        state.commit_speculation(inner);
        assert_eq!(state.speculation_depth, 1);
        assert_eq!(state.tsc_eager_diagnostics, std::slice::from_ref(&eager));
        state.rollback_speculation(outer);

        assert_eq!(state.diagnostics.len(), diagnostics_before + 1);
        assert!(state.diagnostics.contains(&eager));
        assert!(state.tsc_eager_diagnostics.is_empty());
        assert!(state.tsc_eager_visible_global_diagnostics.is_empty());
    });
}

#[test]
fn outermost_commit_keeps_tsc_eager_rows_and_clears_journals() {
    with_state(|state| {
        let diagnostics_before = state.diagnostics.len();
        let visible_before = state.visible_global_diagnostics.len();
        let checkpoint = state.begin_speculation();
        let capture = state.begin_tsc_eager_iteration_diagnostic_capture();
        let eager = state.create_error(None, &diagnostics::Cannot_find_name_0, &["outer commit"]);
        state.push_error_diagnostic(eager.clone());
        state.visible_global_diagnostics.push(eager.clone());
        state.end_tsc_eager_iteration_diagnostic_capture(capture);
        assert_eq!(state.tsc_eager_diagnostics.len(), 1);
        assert_eq!(state.tsc_eager_visible_global_diagnostics.len(), 1);

        state.commit_speculation(checkpoint);

        assert_eq!(state.diagnostics.len(), diagnostics_before + 1);
        assert_eq!(state.visible_global_diagnostics.len(), visible_before + 1);
        assert!(state.diagnostics.contains(&eager));
        assert!(state.visible_global_diagnostics.contains(&eager));
        assert!(state.tsc_eager_diagnostics.is_empty());
        assert!(state.tsc_eager_visible_global_diagnostics.is_empty());
        assert_eq!(state.tsc_eager_iteration_capture_depth, 0);
    });
}

#[test]
#[should_panic(expected = "LIFO")]
fn out_of_order_resolution_panics() {
    with_state(|state| {
        let outer = state.begin_speculation();
        let inner = state.begin_speculation();
        state.rollback_speculation(outer);
        // Unreachable; silence the must-resolve drop guard on the
        // inner checkpoint if the panic above ever regresses.
        state.rollback_speculation(inner);
    });
}

#[test]
#[should_panic(expected = "dropped without commit_speculation")]
fn unresolved_checkpoint_drop_panics() {
    with_state(|state| {
        let checkpoint = state.begin_speculation();
        drop(checkpoint);
    });
}

/// The B35 convention: revert twins restore state and are legal at
/// any depth (before 7.0t this panicked via assert_writable).
#[test]
fn revert_twin_is_legal_under_speculation() {
    with_state(|state| {
        let root = state.binder.source(0).root;
        state.links.set_node_enum_values_computed(0, root);
        state.speculation_depth = 1;
        state.links.revert_node_enum_values_computed(root);
        state.speculation_depth = 0;
        assert!(!state.links.node(root).enum_values_computed);
    });
}

#[test]
fn ranges_rollback_truncates_files_and_removes_new_ones() {
    with_program_state(
        &[
            ("a.ts", "declare var a: string;\n"),
            ("b.ts", "declare var b: string;\n"),
        ],
        &CompilerOptions::default(),
        |state| {
            let file_a = state.binder.source(0).root;
            let file_b = state.binder.source(1).root;
            state.mark_partially_checked_node(file_a, "pre-existing");
            let records_before = state.partial_check_records.len();
            let checkpoint = state.begin_speculation();
            // Same file: appends to the existing range vector.
            let statement_a = state.binder.source(0).arena.node_ids().nth(1);
            if let Some(node) = statement_a {
                state.mark_partially_checked_node(node, "speculative same-file");
            }
            // New file: inserts a fresh map key.
            state.mark_partially_checked_node(file_b, "speculative new-file");
            state.rollback_speculation(checkpoint);
            assert_eq!(state.partially_checked_ranges.len(), 1);
            assert_eq!(state.partially_checked_ranges[&0].len(), 1);
            assert_eq!(state.partial_check_records.len(), records_before);
        },
    );
}

/// Cold structural signature answers may be computed during a
/// trial, but the permanent raw-signature cache stays untouched.
#[test]
fn erased_signature_cache_is_bypassed_under_speculation() {
    with_program_state(
        &[("a.ts", "declare function f<T>(x: T): T;\n")],
        &CompilerOptions::default(),
        |state| {
            let symbol = state
                .resolve_file_scope_name("f", SymbolFlags::FUNCTION)
                .expect("f resolves");
            let ty = state.get_type_of_symbol(symbol).expect("f types");
            let signature = state
                .get_signatures_of_type(ty, SignatureKind::Call)
                .expect("f has call signatures")[0];
            let erased = state
                .speculate(|state| {
                    let erased = state.get_erased_signature(signature)?;
                    assert!(state
                        .signature_of(signature)
                        .erased_signature_cache
                        .is_none());
                    Ok(SpeculationOutcome::Rollback(erased))
                })
                .expect("trial computes without publishing");
            assert_ne!(erased, signature);
            assert!(state
                .signature_of(signature)
                .erased_signature_cache
                .is_none());
        },
    );
}

/// The canonical twin: prerequisite structural caches may be
/// warm, but the cold canonical slot is still not published.
#[test]
fn canonical_signature_cache_is_bypassed_under_speculation() {
    with_program_state(
        &[("a.ts", "declare function f<T>(x: T): T;\n")],
        &CompilerOptions::default(),
        |state| {
            let symbol = state
                .resolve_file_scope_name("f", SymbolFlags::FUNCTION)
                .expect("f resolves");
            let ty = state.get_type_of_symbol(symbol).expect("f types");
            let signature = state
                .get_signatures_of_type(ty, SignatureKind::Call)
                .expect("f has call signatures")[0];
            let own_parameter = state
                .signature_of(signature)
                .type_parameters
                .clone()
                .expect("f is generic")[0];
            state
                .get_signature_instantiation(
                    signature,
                    Some(&[own_parameter]),
                    /*is_javascript*/ false,
                    /*inferred_type_parameters*/ None,
                )
                .expect("identity instantiation warms at depth 0");
            let canonical = state
                .speculate(|state| {
                    let canonical = state.get_canonical_signature(signature)?;
                    assert!(state
                        .signature_of(signature)
                        .canonical_signature_cache
                        .is_none());
                    Ok(SpeculationOutcome::Rollback(canonical))
                })
                .expect("trial computes without publishing");
            assert_ne!(canonical, signature);
            assert!(state
                .signature_of(signature)
                .canonical_signature_cache
                .is_none());
        },
    );
}

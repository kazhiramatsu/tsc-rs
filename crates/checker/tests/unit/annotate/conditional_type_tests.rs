use tsc_types::{CompilerOptions, TypeData, TypeFlags};

use crate::relpin::find_probe_annotation;
use crate::state::test_support::with_program_state;

#[test]
fn annotation_context_absence_preserves_other_node_flags_and_repeated_types() {
    with_program_state(
        &[("a.ts", "function f<T>() { let v: [T, string]; }")],
        &CompilerOptions::default(),
        |state| {
            let node = find_probe_annotation(state.binder.source(0), "v").unwrap();
            state.links.or_node_check_flags(
                state.speculation_depth,
                node,
                tsc_types::NodeCheckFlags::CONTEXT_CHECKED,
            );
            let ty = state.get_type_from_type_node(node).unwrap();
            let flags = state.links.node(node).check_flags;
            assert!(state.no_flow_context_nodes.contains(node));
            assert!(flags.intersects(tsc_types::NodeCheckFlags::CONTEXT_CHECKED));
            assert_eq!(state.get_type_from_type_node(node).unwrap(), ty);
            assert_eq!(state.links.node(node).check_flags, flags);
            // The fact also holds for a different semantic type at this
            // location; it does not memoize the type returned by the worker.
            let number = state.tables.intrinsics.number;
            assert_eq!(
                state
                    .get_conditional_flow_type_of_type(number, node)
                    .unwrap(),
                number
            );
        },
    );
}

#[test]
fn negative_annotation_syntax_survives_rollback_without_creating_a_hot_link() {
    with_program_state(
        &[("a.ts", "let v: string;")],
        &CompilerOptions::default(),
        |state| {
            let node = find_probe_annotation(state.binder.source(0), "v").unwrap();
            let links = state.links.memory_usage();
            let checkpoint = state.begin_speculation();
            assert_eq!(
                state.get_type_from_type_node(node).unwrap(),
                state.tables.intrinsics.string
            );
            state.rollback_speculation(checkpoint);
            assert!(state.no_flow_context_nodes.contains(node));
            assert_eq!(state.links.memory_usage(), links);
            assert_eq!(
                state.get_type_from_type_node(node).unwrap(),
                state.tables.intrinsics.string
            );
            assert_eq!(state.links.memory_usage(), links);
        },
    );
}

#[test]
fn empty_conditional_constraints_do_not_hide_later_type_parameter_narrowing() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T>() { let v: T extends string ? T : number; }",
        )],
        &CompilerOptions::default(),
        |state| {
            let node = find_probe_annotation(state.binder.source(0), "v").unwrap();
            let branch = state
                .record_of(node)
                .as_conditional_type()
                .unwrap()
                .true_type
                .unwrap();
            let number = state.tables.intrinsics.number;
            assert_eq!(
                state
                    .get_conditional_flow_type_of_type(number, branch)
                    .unwrap(),
                number
            );
            assert!(!state.no_flow_context_nodes.contains(branch));
            let narrowed = state.get_type_from_type_node(branch).unwrap();
            assert!(state
                .tables
                .flags_of(narrowed)
                .intersects(TypeFlags::SUBSTITUTION));
            assert_eq!(state.get_type_from_type_node(branch).unwrap(), narrowed);
        },
    );
}

#[test]
fn mapped_context_is_retained_after_a_non_parameter_type_request() {
    with_program_state(
        &[("a.ts", "function f<T>() { let v: { [K in keyof T]: K }; }")],
        &CompilerOptions::default(),
        |state| {
            let node = find_probe_annotation(state.binder.source(0), "v").unwrap();
            let value = state
                .record_of(node)
                .as_mapped_type()
                .unwrap()
                .r#type
                .unwrap();
            let number = state.tables.intrinsics.number;
            assert_eq!(
                state
                    .get_conditional_flow_type_of_type(number, value)
                    .unwrap(),
                number
            );
            assert!(!state.no_flow_context_nodes.contains(value));
            let ty = state.get_type_from_type_node(value).unwrap();
            assert!(state
                .tables
                .flags_of(ty)
                .intersects(TypeFlags::TYPE_PARAMETER));
            assert_eq!(state.get_type_from_type_node(value).unwrap(), ty);
        },
    );
}

#[test]
fn finished_annotation_storage_releases_pages_without_caching_the_type() {
    with_program_state(
        &[("a.ts", "let v: string;")],
        &CompilerOptions::default(),
        |state| {
            let node = find_probe_annotation(state.binder.source(0), "v").unwrap();
            let before = state.get_type_from_type_node(node).unwrap();
            assert!(state.no_flow_context_nodes.contains(node));
            assert!(state.no_flow_context_nodes.memory_usage().1 >= 4096);
            state.no_flow_context_nodes.release_finished_buffers();
            assert_eq!(state.no_flow_context_nodes.memory_usage(), (0, 0));
            assert!(!state.no_flow_context_nodes.contains(node));
            assert_eq!(state.get_type_from_type_node(node).unwrap(), before);
            assert!(state.no_flow_context_nodes.contains(node));
        },
    );
}

#[test]
fn conditional_and_substitution_models_are_constructible_and_renderable() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T>() { let v: T extends string ? T : number; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let annotation =
                find_probe_annotation(state.binder.source(0), "v").expect("v annotation");
            let conditional = state
                .get_type_from_type_node(annotation)
                .expect("conditional model");
            assert!(state
                .tables
                .flags_of(conditional)
                .intersects(TypeFlags::CONDITIONAL));
            let TypeData::Conditional(data) = state.tables.type_of(conditional).data else {
                panic!("conditional flags require the semantic payload");
            };
            let data = *state.tables.conditional_data(data);
            let root = state.tables.conditional_root(data.root);
            assert!(root.is_distributive);
            assert_eq!(root.node, annotation.index());
            assert_eq!(
                root.outer_type_parameters
                    .as_ref()
                    .expect("function parameter is captured")
                    .len(),
                1
            );

            let true_type = state
                .get_true_type_from_conditional_type(conditional)
                .expect("true arm");
            let TypeData::Substitution(substitution) = state.tables.type_of(true_type).data else {
                panic!("true-arm narrowing creates a substitution");
            };
            assert_eq!(substitution.base_type, data.check_type);
            assert_eq!(substitution.constraint, state.tables.intrinsics.string);
            assert_eq!(
                state
                    .get_normalized_type(true_type, /*writing*/ true)
                    .expect("writing normalization"),
                substitution.base_type
            );
            assert_eq!(
                state
                    .type_to_string(conditional)
                    .expect("every constructible conditional renders"),
                "T extends string ? T : number"
            );
            assert_eq!(
                state
                    .get_type_from_type_node(annotation)
                    .expect("node cache requery"),
                conditional
            );
        },
    );
}

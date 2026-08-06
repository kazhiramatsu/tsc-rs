use tsc_types::{CompilerOptions, TypeData, TypeFlags};

use crate::relpin::find_probe_annotation;
use crate::state::test_support::with_program_state;

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
            let TypeData::Conditional(data) = state.tables.type_of(conditional).data.clone() else {
                panic!("conditional flags require the semantic payload");
            };
            let root = state.tables.conditional_root(data.root);
            assert!(root.is_distributive);
            assert_eq!(root.node, annotation.0);
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
            let TypeData::Substitution(substitution) = state.tables.type_of(true_type).data.clone()
            else {
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
                    .type_to_string_slice(conditional)
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

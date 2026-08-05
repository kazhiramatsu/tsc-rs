use tsc_types::CompilerOptions;

use crate::relpin::find_probe_annotation;
use crate::state::test_support::with_program_state;

#[test]
fn function_type_annotations_construct_generic_signatures() {
    with_program_state(
        &[("a.ts", "declare var v: <T extends string>(x: T) => T;\n")],
        &CompilerOptions::default(),
        |state| {
            let annotation =
                find_probe_annotation(state.binder.source(0), "v").expect("var with annotation");
            let signature = state
                .get_signature_from_declaration(annotation)
                .expect("generic signature");
            let type_parameters = state
                .signature_of(signature)
                .type_parameters
                .clone()
                .expect("typeParameters");
            assert_eq!(type_parameters.len(), 1);
            let constraint = state
                .get_constraint_from_type_parameter(type_parameters[0])
                .expect("constraint");
            assert_eq!(constraint, Some(state.tables.intrinsics.string));
            // Erasure maps the parameter and return to any.
            let erased = state.get_erased_signature(signature).expect("erased");
            let erased_return = state
                .get_return_type_of_signature(erased)
                .expect("erased return");
            assert_eq!(erased_return, state.tables.intrinsics.any);
        },
    );
}

#[test]
fn generic_signature_relations_resolve_live() {
    // LIVE since M6 7.5 (the stub era asserted the
    // instantiateSignatureInContextOf escape here): the generic
    // source instantiates in the context of the canonical target
    // and alpha-equivalent generics relate (oracle-probed
    // b8_generic_to_generic, scratchpad probe75.mjs).
    with_program_state(
        &[(
            "a.ts",
            "declare var v: <T>(x: T) => T;\ndeclare var w: <U>(x: U) => U;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let v = find_probe_annotation(state.binder.source(0), "v").expect("v");
            let w = find_probe_annotation(state.binder.source(0), "w").expect("w");
            let source = state.get_type_from_type_node(v).expect("v type");
            let target = state.get_type_from_type_node(w).expect("w type");
            let related = state
                .is_type_assignable_to(source, target)
                .expect("generic signature relations resolve live (M6 7.5)");
            assert!(related);
        },
    );
}

use tsc_types::{CompilerOptions, SymbolFlags};

use super::test_support::with_program_state;

#[test]
fn resolved_conditional_leaves_stack_balanced_and_slot_requeryable() {
    // Infer-type annotations are resolved by the conditional
    // evaluator. The resolution stack remains balanced and the
    // cached SECOND query returns the same semantic type.
    with_program_state(
        &[(
            "a.ts",
            "declare var v: string extends infer U ? U : never;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let symbol = state
                .resolve_file_scope_name("v", SymbolFlags::VALUE)
                .expect("v resolves");
            let first = state
                .get_type_of_symbol(symbol)
                .expect("infer conditional resolves");
            assert_eq!(first, state.tables.intrinsics.string);
            assert_eq!(state.resolution_targets.len(), 0);
            let second = state
                .get_type_of_symbol(symbol)
                .expect("cached infer conditional resolves");
            assert_eq!(first, second);
            assert_eq!(state.resolution_targets.len(), 0);
        },
    );
}

#[test]
fn signature_return_type_seal_is_first_write_wins() {
    // tsc 59839 `signature.resolvedReturnType ??= type` (m4-review
    // A4): the second (outer-frame) fill loses and receives the
    // first frame's value back.
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let signature = state.clone_signature(state.unknown_signature);
        state.signature_mut(signature).resolved_return_type = crate::links::LinkSlot::Vacant;
        let string = state.tables.intrinsics.string;
        let number = state.tables.intrinsics.number;
        assert_eq!(state.seal_signature_return_type(signature, string), string);
        assert_eq!(state.seal_signature_return_type(signature, number), string);
    });
}

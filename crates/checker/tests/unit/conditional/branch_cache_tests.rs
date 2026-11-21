//! The conditional-branch cache answers only for the type a slot holds: two
//! conditional types that share a slot each read their own branches.

use tsc_types::{CompilerOptions, TypeId};

use super::{ConditionalBranchSlot, CONDITIONAL_BRANCH_SLOTS};
use crate::relpin::find_probe_annotation;
use crate::state::test_support::with_program_state;
use crate::state::CheckerState;

fn annotation(state: &mut CheckerState, name: &str) -> TypeId {
    let node = find_probe_annotation(state.binder.source(0), name).expect("annotated variable");
    state
        .get_type_from_type_node(node)
        .expect("annotation resolves")
}

#[test]
fn two_conditional_types_sharing_a_slot_read_their_own_branches() {
    let source = "function f<T, U>() {\n\
        let a: T extends string ? number : boolean;\n\
        let b: U extends number ? string : bigint;\n\
    }\n";
    with_program_state(&[("a.ts", source)], &CompilerOptions::default(), |state| {
        let a = annotation(state, "a");
        let b = annotation(state, "b");
        let a_branches = (
            state
                .get_true_type_from_conditional_type(a)
                .expect("a's true branch"),
            state
                .get_false_type_from_conditional_type(a)
                .expect("a's false branch"),
        );
        let b_branches = (
            state
                .get_true_type_from_conditional_type(b)
                .expect("b's true branch"),
            state
                .get_false_type_from_conditional_type(b)
                .expect("b's false branch"),
        );
        assert_ne!(a_branches, b_branches);
        // b's entry in a's slot, as if b had evicted a there.
        let slot = a.index() as usize & (CONDITIONAL_BRANCH_SLOTS - 1);
        state.conditional_branches[slot] = ConditionalBranchSlot {
            ty: Some(b),
            true_type: Some(b_branches.0),
            false_type: Some(b_branches.1),
        };
        assert_eq!(
            state
                .get_true_type_from_conditional_type(a)
                .expect("a again"),
            a_branches.0
        );
        assert_eq!(
            state
                .get_false_type_from_conditional_type(a)
                .expect("a again"),
            a_branches.1
        );
        // a took its slot back; b still reads its own branches.
        assert_eq!(state.conditional_branches[slot].ty, Some(a));
        assert_eq!(
            state
                .get_true_type_from_conditional_type(b)
                .expect("b again"),
            b_branches.0
        );
        assert_eq!(
            state
                .get_false_type_from_conditional_type(b)
                .expect("b again"),
            b_branches.1
        );
    });
}

#[test]
fn conditional_branch_reads_use_their_own_mappers_and_count_only_cold_calls() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T, V>() { let v: V; let x: T extends { p: infer U } ? V : V; }",
        )],
        &CompilerOptions::default(),
        |state| {
            let ty = annotation(state, "x");
            let tsc_types::TypeData::Conditional(id) = state.tables.type_of(ty).data else {
                panic!("deferred conditional")
            };
            let data = *state.tables.conditional_data(id);
            let mut root = state.tables.conditional_root(data.root).clone();
            let t = annotation(state, "v");
            // Populate each optional list even when the source has no named alias.
            assert!(!root.infer_type_parameters.is_empty());
            root.outer_type_parameters = Some(Box::new([t]));
            root.alias_symbol = state.tables.type_of(t).symbol;
            root.alias_type_arguments = Some(Box::new([t]));
            let root = state.tables.create_conditional_root(root);
            let string = state.tables.intrinsics.string;
            let number = state.tables.intrinsics.number;
            let mapper = state.make_unary_type_mapper(t, string);
            let combined = state.make_unary_type_mapper(t, number);
            let ty = state.tables.create_conditional_type(
                tsc_types::ConditionalTypeData {
                    root,
                    mapper: Some(mapper),
                    combined_mapper: Some(combined),
                    ..data
                },
                None,
                None,
            );
            let before = state.total_instantiation_count;
            for _ in 0..2 {
                assert_eq!(
                    state.get_true_type_from_conditional_type(ty).unwrap(),
                    string
                );
                assert_eq!(
                    state.get_false_type_from_conditional_type(ty).unwrap(),
                    string
                );
                assert_eq!(
                    state
                        .get_inferred_true_type_from_conditional_type(ty)
                        .unwrap(),
                    number
                );
                assert_eq!(state.total_instantiation_count - before, 3);
            }
        },
    );
}

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

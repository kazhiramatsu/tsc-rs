//! checkTypeRelatedTo's identity verdicts for a conditional type paired with
//! another kind, answered before the relation frame, against the full frame:
//! the same verdict, no identity-cache entry, the same types created.

use tsc_types::{CompilerOptions, TypeId};

use super::{ExpandingFlags, RecursionFlags, RelationChecker, RelationErrorState};
use crate::relate::RelationKind;
use crate::relpin::find_probe_annotation;
use crate::state::test_support::with_program_state;
use crate::state::CheckerState;

const SOURCE: &str = "function f<T extends string, U>() {\n\
    let simplifiesToT: T extends unknown ? T : never;\n\
    let simplifiesToNever: T extends unknown ? never : T;\n\
    let deferred: U extends string ? number : boolean;\n\
    let t: T;\n\
    let u: U;\n\
    let s: string;\n\
    let n: never;\n\
}\n";

fn resolve(state: &mut CheckerState, name: &str) -> TypeId {
    if name == "silentNever" {
        return state.tables.intrinsics.silent_never;
    }
    let node = find_probe_annotation(state.binder.source(0), name).expect("annotated variable");
    state
        .get_type_from_type_node(node)
        .expect("annotation resolves")
}

/// checkTypeRelatedTo's frame without the identity verdicts in front of it.
fn full_frame(state: &mut CheckerState, source: TypeId, target: TypeId) -> bool {
    let relation = RelationKind::Identity;
    let relation_count = (16_000_000 - state.relations.cache(relation).len() as i64) >> 3;
    let scratch = std::mem::take(&mut state.relation_scratch);
    let mut checker = RelationChecker {
        st: state,
        relation,
        maybe_keys: scratch.maybe_keys,
        maybe_keys_set: scratch.maybe_keys_set,
        source_stack: scratch.source_stack,
        target_stack: scratch.target_stack,
        maybe_count: 0,
        source_depth: 0,
        target_depth: 0,
        expanding_flags: ExpandingFlags::NONE,
        overflow: false,
        relation_count,
        error_state: RelationErrorState::default(),
    };
    let result = checker
        .is_related_to(
            source,
            target,
            RecursionFlags::BOTH,
            false,
            super::IntersectionState::NONE,
        )
        .expect("full identity check");
    assert!(!checker.overflow, "no overflow on these pairs");
    state.relation_scratch = checker.into_scratch();
    super::is_true(result)
}

#[derive(Debug, PartialEq)]
struct Outcome {
    related: bool,
    identity_entries: usize,
    types: usize,
}

fn outcome(source: &str, target: &str, through_frame: bool) -> Outcome {
    with_program_state(&[("a.ts", SOURCE)], &CompilerOptions::default(), |state| {
        let source = resolve(state, source);
        let target = resolve(state, target);
        let identity_entries = state.relations.cache(RelationKind::Identity).len();
        let types = state.tables.len();
        let related = if through_frame {
            full_frame(state, source, target)
        } else {
            state
                .check_type_related_to(source, target, RelationKind::Identity)
                .expect("identity check")
        };
        Outcome {
            related,
            identity_entries: state.relations.cache(RelationKind::Identity).len()
                - identity_entries,
            types: state.tables.len() - types,
        }
    })
}

fn assert_same_as_frame(source: &str, target: &str, related: bool, answered_in_front: bool) {
    for (a, b) in [(source, target), (target, source)] {
        let fast = outcome(a, b, false);
        assert_eq!(fast, outcome(a, b, true), "{a} against {b}");
        assert_eq!(fast.related, related, "{a} against {b}");
        if answered_in_front {
            assert_eq!(
                fast.identity_entries, 0,
                "{a} against {b}: an identity verdict writes no cache entry"
            );
        }
    }
}

#[test]
fn a_conditional_that_normalizes_to_the_other_side_is_identical() {
    // `T extends unknown ? never : T` simplifies to never
    // (getSimplifiedConditionalType: T's restrictive instantiation is
    // assignable to unknown), the other side's type.
    assert_same_as_frame("simplifiesToNever", "n", true, true);
}

#[test]
fn a_conditional_and_another_kind_with_different_flags_are_not_identical() {
    assert_same_as_frame("deferred", "s", false, true);
}

#[test]
fn a_conditional_that_normalizes_to_never_matches_another_never_singleton() {
    // A different never type with the same singleton flags.
    assert_same_as_frame("simplifiesToNever", "silentNever", true, true);
}

#[test]
fn a_pair_whose_normalized_flags_agree_falls_through_to_the_full_check() {
    // `T extends unknown ? T : never` normalizes to the distributed copy of
    // T, a type parameter like U: equal flags and no singleton, so the full
    // check decides (and may write its cache entry, as the frame does).
    assert_same_as_frame("simplifiesToT", "u", false, false);
}

#[test]
fn kept_recursion_entries_match_fresh_walks_at_every_depth() {
    use super::RelationStackEntry;
    with_program_state(
        &[("a.ts", "interface Box<T> { next: Box<Box<T>> }\ndeclare let a: Box<string>;\ndeclare let b: Box<number>;\ndeclare let c: Box<boolean> & { tag: 1 };\n")],
        &CompilerOptions::default(),
        |state| {
            let a = resolve(state, "a");
            let b = resolve(state, "b");
            let c = resolve(state, "c");
            let types = [a, b, a, c, b, c];
            let entries: Vec<_> = types.iter().copied().map(RelationStackEntry::new).collect();
            assert!(!state.is_deeply_nested_on_stack(a, &entries, 1, 3).unwrap());
            assert!(entries.iter().all(|entry| entry.identity.get().is_none()));
            for max_depth in [1, 2, 3, 10] {
                for depth in 1..=types.len() {
                    for ty in types {
                        let expected = state.is_deeply_nested_type(ty, &types, depth, max_depth).unwrap();
                        assert_eq!(state.is_deeply_nested_on_stack(ty, &entries, depth, max_depth).unwrap(), expected,
                            "type {ty:?}, depth {depth}, threshold {max_depth}");
                    }
                }
            }
        },
    );
}

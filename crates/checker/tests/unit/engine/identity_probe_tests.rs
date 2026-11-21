//! checkTypeRelatedTo's identity verdicts for a conditional type paired with
//! another kind, answered before the relation frame, against the full frame:
//! the same verdict, no identity-cache entry, the same types created.

use tsc_types::{CompilerOptions, TypeId};

use super::{RecursionFlags, RelationChecker};
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

fn identity_checker<'s, 'a>(state: &'s mut CheckerState<'a>) -> RelationChecker<'s, 'a> {
    let relation = RelationKind::Identity;
    let relation_count = (16_000_000 - state.relations.cache(relation).len() as i64) >> 3;
    RelationChecker::new(state, relation, relation_count)
}

/// checkTypeRelatedTo's frame without the identity verdicts in front of it.
fn full_frame(state: &mut CheckerState, source: TypeId, target: TypeId) -> bool {
    let mut checker = identity_checker(state);
    let result = checker
        .is_related_to(
            source,
            target,
            RecursionFlags::BOTH,
            false,
            super::IntersectionState::NONE,
        )
        .expect("full identity check");
    assert!(!checker.work.overflow, "no overflow on these pairs");
    checker.recycle();
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

#[test]
fn normalization_gate_matches_the_full_loop_for_all_normalizing_kinds() {
    fn observe(worker: bool) -> (Vec<TypeId>, usize, u64, u64, Vec<u32>) {
        let source = r#"
            interface Box<T> { value: T }
            function f<T extends string, V extends unknown[], O, K extends keyof O>() {
                var primitive: string;
                var literal: 'x';
                var parameter: T;
                var reference: Box<T>;
                var union: Box<T> | string;
                var reducible: ({ tag: 0 } & { tag: 1 }) | number;
                var intersection: Box<T> & { tag: 1 };
                var conditional: T extends unknown ? T : never;
                var indexed: O[K];
                var tuple: [T, ...V];
            }
        "#;
        with_program_state(&[("a.ts", source)], &CompilerOptions::default(), |state| {
            let mut inputs = Vec::new();
            for name in [
                "primitive",
                "literal",
                "parameter",
                "reference",
                "union",
                "intersection",
                "conditional",
                "indexed",
                "tuple",
            ] {
                inputs.push(resolve(state, name));
            }
            assert!(state
                .tables
                .flags_of(inputs[6])
                .intersects(tsc_types::TypeFlags::CONDITIONAL));
            assert!(state
                .tables
                .flags_of(inputs[7])
                .intersects(tsc_types::TypeFlags::INDEXED_ACCESS));
            let tuple = *inputs.last().unwrap();
            assert!(state.is_generic_tuple_type(tuple));
            assert!(state
                .tables
                .object_flags_of(tuple)
                .intersects(tsc_types::ObjectFlags::REFERENCE));
            let plain_union = state.tables.type_of(inputs[4]);
            assert_eq!(plain_union.flags, tsc_types::TypeFlags::UNION);
            assert!(!plain_union.object_flags.intersects(
                tsc_types::ObjectFlags::REFERENCE | tsc_types::ObjectFlags::CONTAINS_INTERSECTIONS
            ));
            let reducible = resolve(state, "reducible");
            assert!(state
                .tables
                .object_flags_of(reducible)
                .intersects(tsc_types::ObjectFlags::CONTAINS_INTERSECTIONS));
            inputs.push(reducible);
            let fresh = state.tables.get_fresh_type_of_literal_type(inputs[1]);
            assert!(state.tables.is_fresh_literal_type(fresh));
            inputs.push(fresh);
            let substitution = state
                .tables
                .get_or_create_substitution_type(inputs[2], state.tables.intrinsics.string);
            inputs.push(substitution);
            let mut normalized = Vec::new();
            for ty in inputs {
                for writing in [false, true] {
                    let result = if worker {
                        state.get_normalized_type_worker(ty, writing)
                    } else {
                        state.get_normalized_type(ty, writing)
                    };
                    normalized.push(result.unwrap());
                }
            }
            (
                normalized,
                state.tables.len(),
                state.instantiation_count,
                state.total_instantiation_count,
                state.diagnostics.iter().map(|d| d.code()).collect(),
            )
        })
    }
    assert_eq!(observe(false), observe(true));
}

#[test]
fn active_keys_follow_the_live_prefix_across_unwinds_and_frame_loans() {
    use super::{IntersectionState, RelationComparisonResult, RelationKey};

    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let mut checker = identity_checker(state);
        let keys: Vec<_> = (0..40)
            .map(|i| RelationKey::Generic(format!("*{i}").into()))
            .collect();
        for (i, key) in keys.iter().enumerate() {
            checker.push_active_key(key);
            assert_eq!(checker.work.maybe_keys_set.is_empty(), i < 32);
            assert!(keys[..=i].iter().all(|key| checker.has_active_key(key)));
        }
        let frame = checker.loan_frame(IntersectionState::NONE);
        let outer = std::ptr::from_ref(&*frame.work);
        let nested = RelationChecker::new(checker.st, RelationKind::Assignable, 17);
        assert_ne!(outer, std::ptr::from_ref(&*nested.work));
        assert_eq!(nested.work.relation_count, 17);
        assert!(nested.work.maybe_keys.is_empty());
        nested.recycle();
        checker.restore_frame(frame);
        let budget = checker.work.relation_count;
        for depth in [34, 32, 2] {
            checker.reset_maybe_stack(depth, false, RelationComparisonResult::NONE);
            assert!(keys[..depth].iter().all(|key| checker.has_active_key(key)));
            assert!(keys[depth..].iter().all(|key| !checker.has_active_key(key)));
            assert_eq!(checker.work.maybe_keys_set.is_empty(), depth <= 32);
        }
        assert_eq!(checker.work.relation_count, budget);
        for key in &keys[5..36] {
            checker.push_active_key(key);
        }
        assert!(checker.has_active_key(&keys[35]));
        assert!(!checker.has_active_key(&keys[2]));
        checker.reset_maybe_stack(0, true, RelationComparisonResult::NONE);
        assert_eq!(checker.work.relation_count, budget - 33);
        assert!(checker.work.maybe_keys_set.is_empty());
        for (i, key) in keys.iter().enumerate() {
            let verdict = checker.st.relations.cache(checker.relation).get(key);
            assert_eq!(verdict.is_some(), i < 2 || (5..36).contains(&i));
            assert!(!checker.has_active_key(key));
        }
        checker.recycle();
        let string = state.tables.intrinsics.string;
        let number = state.tables.intrinsics.number;
        let (_, diagnostic) = state
            .check_relation_with_error_output_at_worker(
                string,
                number,
                RelationKind::Assignable,
                None,
                None,
                None,
            )
            .unwrap();
        assert!(diagnostic.is_some());
        assert!(state
            .check_type_related_to(string, string, RelationKind::Assignable)
            .unwrap());
        let again = identity_checker(state);
        assert_eq!(outer, std::ptr::from_ref(&*again.work));
        assert!(again.work.maybe_keys.is_empty());
        assert!(again.work.source_stack.is_empty() && again.work.target_stack.is_empty());
        assert!(again.error_state.error_chain.is_none());
    });
}

#[test]
fn active_keys_follow_the_loan_through_a_forward_constraint_clamp() {
    use super::{IntersectionState, RelationFrameLoan, RelationKey};
    use crate::inference::CompareTypesFn;
    use tsc_types::{InferenceFlags, SymbolFlags};

    let text = "declare function callback<T extends U, U extends string>(x: T, y: U): T;";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        let symbol = state
            .resolve_file_scope_name("callback", SymbolFlags::FUNCTION)
            .unwrap();
        let ty = state.get_type_of_symbol(symbol).unwrap();
        let signature = state
            .get_signatures_of_type(ty, crate::state::SignatureKind::Call)
            .unwrap()[0];
        let parameters = state
            .signature_of(signature)
            .cloned_type_parameters()
            .unwrap();
        let context = state.create_inference_context(
            &parameters,
            Some(signature),
            InferenceFlags::NONE,
            Some(CompareTypesFn::RelationFrame),
        );
        let first = state.inference_context(context).inferences[0];
        let forward = state.inference_context(context).inferences[1];
        let number = state.tables.intrinsics.number;
        let string = state.tables.intrinsics.string;
        state.inference_info_mut(first).candidates = Some(vec![number].into());
        state.inference_info_mut(forward).candidates = Some(vec![number].into());

        let mut checker = RelationChecker::new(state, RelationKind::Assignable, 29);
        let active = RelationKey::Generic("*active".into());
        checker.push_active_key(&active);
        let frame = checker.loan_frame(IntersectionState::NONE);
        checker.st.relation_frame_loan = RelationFrameLoan::Available(frame);
        assert_eq!(checker.st.get_inferred_type(context, 0).unwrap(), string);
        assert_eq!(
            checker.st.inference_info(forward).inferred_type,
            Some(string)
        );
        let RelationFrameLoan::Available(frame) =
            std::mem::replace(&mut checker.st.relation_frame_loan, RelationFrameLoan::None)
        else {
            panic!("clamp did not return the loan")
        };
        checker.restore_frame(frame);
        assert_eq!(checker.work.relation_count, 29);
        assert_eq!(checker.work.maybe_count, 1);
        assert!(checker.has_active_key(&active));
        assert!(checker.error_state.error_chain.is_none());
        checker.recycle();
    });
}

#[test]
fn short_membership_keeps_the_ordered_result_for_present_ids() {
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        state.stable_type_ordering = true;
        let a = state.tables.get_string_literal_type("a");
        let b = state.tables.get_string_literal_type("b");
        let c = state.tables.get_string_literal_type("c");
        let fresh = state.tables.get_fresh_type_of_literal_type(a);
        let checker = identity_checker(state);
        assert!(!checker.contains_type_ordered(&[c, b, a], c));
        for slice in [&[][..], &[a, b, c], &[c, b, a], &[a; 8], &[a; 9]] {
            let ctx = crate::type_order::order_ctx!(checker.st);
            for id in [a, b, c, fresh] {
                let expected =
                    tsc_types::type_order::search_type(&checker.st.tables, ctx.order(), slice, id);
                assert_eq!(checker.contains_type_ordered(slice, id), expected.is_ok());
            }
        }
    });
}

#[test]
fn unknown_like_union_publishes_after_its_first_scan() {
    use tsc_types::{ObjectFlags, UnionReduction};

    let options = CompilerOptions {
        strict_null_checks: Some(true),
        ..Default::default()
    };
    with_program_state(
        &[(
            "a.ts",
            "declare const key: unique symbol; let obj: { [key]: number };",
        )],
        &options,
        |state| {
            let undefined = state.tables.intrinsics.undefined;
            let null = state.tables.intrinsics.null;
            let computed = ObjectFlags::IS_UNKNOWN_LIKE_UNION_COMPUTED;
            let value = ObjectFlags::IS_UNKNOWN_LIKE_UNION;
            assert!(!state.is_unknown_like_union_type(undefined).unwrap());
            let object = resolve(state, "obj");
            for (part, expected) in [(state.empty_object_type, true), (object, false)] {
                let union = state
                    .get_union_type_ex(&[undefined, null, part], UnionReduction::None)
                    .unwrap();
                state.tables.strict_null_checks = false;
                assert!(!state.is_unknown_like_union_type(union).unwrap());
                assert!(!state.tables.object_flags_of(union).intersects(computed));
                state.tables.strict_null_checks = true;
                let first = state.flow_frame_begin();
                assert_eq!(state.is_unknown_like_union_type(union).unwrap(), expected);
                let first_taint = state.flow_frame_end(first).0;
                if part == object {
                    assert_eq!(first_taint.transient, 0);
                }
                let flags = state.tables.object_flags_of(union);
                assert!(flags.intersects(computed));
                assert_eq!(flags.intersects(value), expected);
                let counts = (
                    state.tables.len(),
                    state.binder.transient_symbol_count(),
                    state.total_instantiation_count,
                );
                let repeat = state.flow_frame_begin();
                assert_eq!(state.is_unknown_like_union_type(union).unwrap(), expected);
                assert_ne!(state.flow_frame_end(repeat).0.transient, 0);
                assert_eq!(
                    counts,
                    (
                        state.tables.len(),
                        state.binder.transient_symbol_count(),
                        state.total_instantiation_count
                    )
                );
            }
        },
    );
}

#[test]
fn large_union_trace_preserves_the_threshold_and_event_fields() {
    use tsc_types::{
        tracing::{CheckerTracer, Tracing},
        ObjectFlags, UnionReduction,
    };

    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let checker = identity_checker(state);
        // The disabled path returns before even inspecting the IDs.
        let unused = TypeId::new(u32::MAX - 1);
        checker.trace_unions_or_intersections_too_large(unused, unused);
        let literals: Vec<_> = (0..1001)
            .map(|n| checker.st.tables.get_number_literal_type(n as f64))
            .collect();
        let source = checker
            .st
            .get_union_type_ex(&literals[..1000], UnionReduction::None)
            .unwrap();
        let target = checker
            .st
            .get_union_type_ex(&literals, UnionReduction::None)
            .unwrap();
        let trace = Tracing::start("trace", "", true);
        checker.st.tracer = Some(CheckerTracer::new(trace.clone(), 0));
        checker.trace_unions_or_intersections_too_large(source, target);
        assert!(checker
            .st
            .tables
            .object_flags_of(source)
            .intersects(ObjectFlags::PRIMITIVE_UNION));
        let flags =
            checker.st.tables.object_flags_of(source).bits() & !ObjectFlags::PRIMITIVE_UNION.bits();
        checker.st.tables.type_mut(source).object_flags = ObjectFlags::from_bits(flags);
        checker.trace_unions_or_intersections_too_large(source, source);
        checker.trace_unions_or_intersections_too_large(source, target);
        let events: Vec<serde_json::Value> = serde_json::from_str(&trace.finish()[0].text).unwrap();
        let events: Vec<_> = events
            .iter()
            .filter(|e| e["name"] == "traceUnionsOrIntersectionsTooLarge_DepthLimit")
            .collect();
        assert_eq!(events.len(), 1);
        let args = &events[0]["args"];
        assert_eq!(args["sourceId"], crate::tracing::trace_type_id(source));
        assert_eq!(args["sourceSize"], 1000);
        assert_eq!(args["targetId"], crate::tracing::trace_type_id(target));
        assert_eq!(args["targetSize"], 1001);
    });
}

#[test]
fn relation_counts_follow_both_profile_readers() {
    use crate::{
        lane_profile,
        line_profile::{LineProfiler, OP_RELATIONS},
    };
    fn observe(line: bool, lane: bool) -> (u64, u64, usize, usize, u64) {
        let text = "let a: { p: { x: string } }; let b: { p: { x: string } };";
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
            let a = resolve(state, "a");
            let b = resolve(state, "b");
            state.line_profile = LineProfiler::with_readers(line, lane);
            let before = state.profile_ops[OP_RELATIONS];
            let cost = lane_profile::counted(state);
            assert!(full_frame(state, a, a));
            assert_eq!(
                state.profile_ops[OP_RELATIONS] - before,
                u64::from(line || lane)
            );
            assert!(full_frame(state, a, b));
            (
                state.profile_ops[OP_RELATIONS] - before,
                lane_profile::counted(state) - cost,
                state.tables.len(),
                state.binder.transient_symbol_count(),
                state.total_instantiation_count,
            )
        })
    }
    let observed = observe(true, false);
    // The identical pair, the outer objects, their p properties and their x properties.
    assert_eq!(observed.0, 4);
    assert_eq!(observed, observe(false, true));
    assert_eq!(observed, observe(true, true));
    let idle = observe(false, false);
    assert_eq!(idle.0, 0);
    assert_eq!(observed.1 - idle.1, observed.0 * 1534);
    assert_eq!(
        (idle.2, idle.3, idle.4),
        (observed.2, observed.3, observed.4)
    );
}

#[test]
fn empty_anonymous_objects_keep_late_members_and_unresolved_taint() {
    with_program_state(
        &[(
            "a.ts",
            "declare const key: unique symbol; let obj: { [key]: number };",
        )],
        &CompilerOptions::default(),
        |state| {
            let frame = state.flow_frame_begin();
            assert!(!state
                .is_empty_anonymous_object_type(state.tables.intrinsics.string)
                .unwrap());
            assert_ne!(state.flow_frame_end(frame).0.transient, 0);
            assert!(!state
                .is_empty_anonymous_object_type(state.any_function_type)
                .unwrap());
            let obj = resolve(state, "obj");
            let frame = state.flow_frame_begin();
            assert!(!state.is_empty_anonymous_object_type(obj).unwrap());
            assert_eq!(state.flow_frame_end(frame).0.transient, 0);
            let symbol = state.binder.create_symbol(
                tsc_types::SymbolFlags::TYPE_LITERAL,
                tsc_types::EscapedName::from_identifier_escaped_text("__type"),
            );
            let synthetic = state.create_resolved_empty_anonymous_type(Some(symbol));
            let members = state.resolve_structured_type_members(synthetic).unwrap();
            state.members_mut(members).properties = vec![symbol].into();
            let frame = state.flow_frame_begin();
            assert!(state.is_empty_anonymous_object_type(synthetic).unwrap());
            assert_ne!(state.flow_frame_end(frame).0.transient, 0);
        },
    );
}

#[test]
fn literal_rejection_matches_payload_equality_for_every_relation() {
    let source = "namespace A { export enum E { X = 0 } }
        namespace B { export enum E { X = 0 } }
        let a: A.E.X; let b: B.E.X;";
    with_program_state(&[("a.ts", source)], &CompilerOptions::default(), |state| {
        let a = resolve(state, "a");
        let b = resolve(state, "b");
        let zero = state.tables.get_number_literal_type(-0.0);
        assert_eq!(zero, state.tables.get_number_literal_type(0.0));
        let nan = state.tables.get_number_literal_type(f64::NAN);
        let other_nan = state
            .tables
            .get_number_literal_type(f64::from_bits(f64::NAN.to_bits() + 1));
        let big = state
            .tables
            .get_bigint_literal_type(tsc_types::PseudoBigInt {
                negative: false,
                base10_value: "1".into(),
            });
        let high = state.tables.get_string_literal_type_from_utf16(&[0xd800]);
        let replacement = state.tables.get_string_literal_type_from_utf16(&[0xfffd]);
        let values = [a, b, zero, nan, other_nan, big, high, replacement];
        for lhs in values {
            let lhs = state.tables.type_of(lhs).regular_type().unwrap();
            let fresh = state.tables.get_fresh_type_of_literal_type(lhs);
            for relation in RelationKind::ALL {
                assert!(state.is_type_related_to(fresh, lhs, relation).unwrap());
                for rhs in values {
                    let rhs = state.tables.type_of(rhs).regular_type().unwrap();
                    if lhs == rhs {
                        continue;
                    }
                    let unequal = !state.tables.literal_type_values_equal(lhs, rhs);
                    assert_eq!(state.unequal_value_literals(lhs, rhs), unequal);
                    let related = state.is_type_related_to(fresh, rhs, relation).unwrap();
                    let expected = if unequal {
                        false
                    } else if relation == RelationKind::Identity {
                        let flags = state.tables.flags_of(lhs);
                        flags == state.tables.flags_of(rhs)
                            && flags.intersects(tsc_types::TypeFlags::SINGLETON)
                    } else {
                        (relation == RelationKind::Comparable
                            && state.is_simple_type_related_to(rhs, lhs, relation).unwrap())
                            || state.is_simple_type_related_to(lhs, rhs, relation).unwrap()
                    };
                    assert_eq!(related, expected, "{lhs:?} / {rhs:?} / {relation:?}");
                }
            }
        }
    });
}

use crate::relpin::find_probe_annotation;
use crate::state::test_support::with_program_state;
use tsc_types::{
    CompilerOptions, EscapedName, LiteralValue, ObjectFlags, PseudoBigInt, SymbolId, TypeFacts,
    TypeFlags,
};

#[test]
fn non_nullable_union_facts_match_the_member_fold_for_every_mask_bit() {
    for strict in [false, true] {
        let options = CompilerOptions {
            strict_null_checks: Some(strict),
            ..Default::default()
        };
        with_program_state(
            &[(
                "a.ts",
                "declare let a: { tag: 1 }; declare let b: () => void;",
            )],
            &options,
            |state| {
                let mut members = Vec::new();
                for name in ["a", "b"] {
                    let node = find_probe_annotation(state.binder.source(0), name).unwrap();
                    members.push(state.get_type_from_type_node(node).unwrap());
                }
                let t = &mut state.tables;
                members.extend([
                    t.intrinsics.string,
                    t.intrinsics.number,
                    t.intrinsics.bigint,
                    t.intrinsics.false_regular,
                    t.intrinsics.true_regular,
                    t.intrinsics.es_symbol,
                    t.intrinsics.non_primitive,
                ]);
                for text in ["", "nonempty"] {
                    let literal = t.get_string_literal_type(text);
                    members.push(literal);
                    members.push(t.get_fresh_type_of_literal_type(literal));
                }
                members.extend([
                    t.get_number_literal_type(0.0),
                    t.get_number_literal_type(1.0),
                ]);
                for value in ["0", "1"] {
                    members.push(t.get_bigint_literal_type(PseudoBigInt {
                        negative: false,
                        base10_value: value.to_owned(),
                    }));
                }
                members.push(t.create_computed_enum_type(SymbolId::new(0)));
                members.push(t.get_enum_literal_type(
                    LiteralValue::Number(0.0),
                    SymbolId::new(0),
                    SymbolId::new(0),
                ));
                members.push(t.get_enum_literal_type(
                    LiteralValue::String("enum".into()),
                    SymbolId::new(0),
                    SymbolId::new(0),
                ));
                members.push(t.create_unique_es_symbol_type(
                    SymbolId::new(0),
                    EscapedName::internal("unique"),
                ));
                members.sort();
                let union = t.get_union_type_from_sorted_list(
                    members.clone(),
                    ObjectFlags::NONE,
                    None,
                    None,
                    None,
                );
                assert!(t
                    .object_flags_of(union)
                    .intersects(ObjectFlags::NON_NULLABLE_UNION));
                let possible = super::CheckerState::possible_object_type_facts(strict);
                let outside_object = TypeFacts::from_bits(TypeFacts::ALL.bits() & !possible.bits());
                // The summary path must not resolve either object member.
                let objects: Vec<_> = members
                    .iter()
                    .copied()
                    .filter(|&ty| state.tables.flags_of(ty).intersects(TypeFlags::OBJECT))
                    .collect();
                let before: Vec<_> = objects
                    .iter()
                    .map(|&object| {
                        state
                            .links
                            .read_ty(object, |links| links.resolved_members.resolved())
                    })
                    .collect();
                assert!(before.iter().any(Option::is_none));
                assert_eq!(
                    state.get_type_facts(union, outside_object).unwrap(),
                    TypeFacts::NONE
                );
                for (&object, before) in objects.iter().zip(before) {
                    assert_eq!(
                        state
                            .links
                            .read_ty(object, |links| links.resolved_members.resolved()),
                        before
                    );
                }
                // Exercise the production fallback before the oracle can
                // resolve anything itself.
                let object_facts = state
                    .get_type_facts(union, TypeFacts::TYPEOF_EQ_OBJECT)
                    .unwrap();
                assert_eq!(object_facts, TypeFacts::TYPEOF_EQ_OBJECT);
                for &object in &objects {
                    assert!(state
                        .links
                        .read_ty(object, |links| links.resolved_members.resolved())
                        .is_some());
                }
                for mask in (0..27).map(|bit| TypeFacts::from_bits(1 << bit)).chain([
                    TypeFacts::ALL,
                    outside_object,
                    TypeFacts::IS_UNDEFINED_OR_NULL,
                    TypeFacts::NE_UNDEFINED_OR_NULL,
                ]) {
                    let mut expected = TypeFacts::NONE;
                    for &member in &members {
                        expected |= state.get_type_facts_worker(member, mask).unwrap();
                    }
                    expected = TypeFacts::from_bits(expected.bits() & mask.bits());
                    assert_eq!(
                        state.get_type_facts(union, mask).unwrap(),
                        expected,
                        "strict={strict}, mask={mask:?}"
                    );
                }
                // Object-fact masks must retain the member-resolution walk.
                for &object in &objects {
                    assert!(state
                        .links
                        .read_ty(object, |links| links.resolved_members.resolved())
                        .is_some());
                }
                state.resolve_structured_type_members(union).unwrap();
                assert!(state
                    .tables
                    .object_flags_of(union)
                    .intersects(ObjectFlags::NON_NULLABLE_UNION));
            },
        );
    }
}

#[test]
fn unsummarized_nullable_union_uses_the_indexed_facts_walk() {
    for strict in [false, true] {
        let options = CompilerOptions {
            strict_null_checks: Some(strict),
            ..Default::default()
        };
        with_program_state(&[("a.ts", "")], &options, |state| {
            let i = &state.tables.intrinsics;
            let mut members = vec![i.string, i.undefined, i.null, i.void, i.unknown];
            members.sort();
            let union = state.tables.get_union_type_from_sorted_list(
                members.clone(),
                ObjectFlags::NONE,
                None,
                None,
                None,
            );
            assert!(!state
                .tables
                .object_flags_of(union)
                .intersects(ObjectFlags::NON_NULLABLE_UNION));
            for mask in [
                TypeFacts::ALL,
                TypeFacts::IS_UNDEFINED_OR_NULL,
                TypeFacts::EQ_UNDEFINED_OR_NULL,
            ] {
                let mut expected = TypeFacts::NONE;
                for &member in &members {
                    expected |= state.get_type_facts_worker(member, mask).unwrap();
                }
                assert_eq!(
                    state.get_type_facts(union, mask).unwrap().bits(),
                    expected.bits() & mask.bits()
                );
            }
        });
    }
}

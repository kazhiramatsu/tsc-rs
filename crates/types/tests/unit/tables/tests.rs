use super::*;

fn tables() -> TypeTables {
    TypeTables::new(
        /*strict_null_checks*/ true,
        /*eopt*/ false,
        &IdentityDomain::new(),
    )
}

#[test]
fn a_cursor_reads_an_interned_list_and_stops_at_its_end() {
    let mut tables = tables();
    let a = tables.intrinsics.number;
    let b = tables.intrinsics.string;
    let list = tables.intern_type_list(&[a, b]);
    let mut cursor = tables.cursor(list);
    assert_eq!(cursor.len(), 2);
    assert_eq!(cursor.next(&tables), Some(a));
    assert_eq!(cursor.next(&tables), Some(b));
    assert_eq!(cursor.next(&tables), None);
}

#[test]
fn a_finished_checkers_type_range_is_not_reissued_when_its_records_are_dropped() {
    let domain = IdentityDomain::new();
    let mut first = TypeTables::new(true, false, &domain);
    let lease = first.lease.as_ref().expect("direct type room");
    let first_range = lease.range();
    let watch = lease.watch();
    let storage = first.take_finished_storage();
    assert!(first.types.is_empty());
    drop(first);
    drop(storage);

    let mut later = TypeTables::new(true, false, &domain);
    let later_range = later.lease.as_ref().expect("later type room").range();
    assert!(watch.is_held(), "finished range remains reserved");
    assert!(
        first_range.end() <= later_range.start() || later_range.end() <= first_range.start(),
        "finishing a checker cannot affect another checker's identities"
    );
    let value = later.get_string_literal_type("still live");
    assert!(later
        .type_of(value)
        .flags
        .intersects(TypeFlags::STRING_LITERAL));
}

#[test]
fn non_nullable_union_summary_uses_final_member_flags() {
    for strict in [false, true] {
        let mut t = TypeTables::new(strict, false, &IdentityDomain::new());
        let string = t.intrinsics.string;
        let number = t.intrinsics.number;
        let object = t.create_type(TypeFlags::OBJECT, TypeData::PLAIN_OBJECT);
        let literal = t.get_string_literal_type("a");
        let enum_type = t.create_computed_enum_type(SymbolId::new(10));
        for member in [
            number,
            object,
            literal,
            enum_type,
            t.intrinsics.bigint,
            t.intrinsics.es_symbol,
            t.intrinsics.non_primitive,
            t.intrinsics.false_regular,
        ] {
            let mut members = vec![string, member];
            members.sort();
            let union =
                t.get_union_type_from_sorted_list(members, ObjectFlags::NONE, None, None, None);
            assert!(t
                .object_flags_of(union)
                .intersects(ObjectFlags::NON_NULLABLE_UNION));
        }
        let intersection = t.create_origin_union_or_intersection_type(
            TypeFlags::INTERSECTION,
            vec![string, number],
        );
        let nested =
            t.create_origin_union_or_intersection_type(TypeFlags::UNION, vec![string, number]);
        let parameter = t.create_synthesized_type_parameter(None);
        let mapping = t.create_type(
            TypeFlags::STRING_MAPPING,
            TypeData::StringMapping { ty: string },
        );
        for member in [
            t.intrinsics.undefined,
            t.intrinsics.null,
            t.intrinsics.void,
            t.intrinsics.any,
            t.intrinsics.unknown,
            t.intrinsics.never,
            t.intrinsics.numeric_string,
            parameter,
            intersection,
            mapping,
            nested,
        ] {
            let mut members = vec![string, member];
            members.sort();
            let union = t.get_union_type_from_sorted_list(
                members,
                ObjectFlags::NON_NULLABLE_UNION,
                None,
                None,
                None,
            );
            assert!(
                !t.object_flags_of(union)
                    .intersects(ObjectFlags::NON_NULLABLE_UNION),
                "excluded flags: {:?}",
                t.flags_of(member)
            );
        }
        // A filtered union gets its own summary, independently of the
        // nullable source's flags and the builder's nullable-elision mode.
        let mut members = vec![string, number, t.intrinsics.undefined];
        members.sort();
        let nullable =
            t.get_union_type_from_sorted_list(members, ObjectFlags::NONE, None, None, None);
        let filtered = t.filter_type(nullable, |tables, member| {
            !tables.flags_of(member).intersects(TypeFlags::UNDEFINED)
        });
        assert!(t
            .object_flags_of(filtered)
            .intersects(ObjectFlags::NON_NULLABLE_UNION));
        assert!(!t
            .object_flags_of(nullable)
            .intersects(ObjectFlags::NON_NULLABLE_UNION));
    }
}

#[test]
fn intrinsics_are_allocated_in_tsc_order() {
    let t = tables();
    // anyType is the first allocation, like tsc typeCount order.
    assert_eq!(t.intrinsics.any, TypeId::new(0));
    assert!(t.intrinsics.unknown < t.intrinsics.undefined);
    assert!(t.intrinsics.false_regular < t.intrinsics.true_fresh);
    // strictNullChecks aliases the widening variants (47033/47050).
    assert_eq!(t.intrinsics.undefined_widening, t.intrinsics.undefined);
    assert_eq!(t.intrinsics.null_widening, t.intrinsics.null);
    // exactOptionalPropertyTypes off aliases undefinedOrMissing.
    assert_eq!(t.intrinsics.undefined_or_missing, t.intrinsics.undefined);

    let loose = TypeTables::new(false, false, &IdentityDomain::new());
    assert_ne!(
        loose.intrinsics.undefined_widening,
        loose.intrinsics.undefined
    );
    assert_ne!(loose.intrinsics.null_widening, loose.intrinsics.null);

    // 47101-47102: templateConstraintType + numericStringType sit
    // between numberOrBigInt and uniqueLiteral (skipping them
    // shifted every later id off the oracle's).
    assert!(t.intrinsics.number_or_bigint < t.intrinsics.template_constraint);
    assert!(t.intrinsics.template_constraint < t.intrinsics.numeric_string);
    assert!(t.intrinsics.numeric_string < t.intrinsics.unique_literal);
}

#[test]
fn template_constraint_and_numeric_string_shapes() {
    let mut t = tables();
    assert!(t
        .flags_of(t.intrinsics.template_constraint)
        .intersects(TypeFlags::UNION));
    match &t.type_of(t.intrinsics.numeric_string).data {
        TypeData::TemplateLiteral(data) => {
            let data = t.template_literal(*data);
            assert_eq!(data.texts.len(), 2);
            assert!(data.texts.iter().all(|text| text.is_empty()));
            assert_eq!(t.list(data.types).len(), 1);
            assert_eq!(t.list(data.types)[0], t.intrinsics.number);
        }
        other => panic!("numeric_string should be a template literal: {other:?}"),
    }
    let number = t.intrinsics.number;
    let again = t.get_template_literal_type(&[String::new(), String::new()], &[number]);
    assert_eq!(again, t.intrinsics.numeric_string);
}

#[test]
fn js_number_to_string_matches_ecmascript_number_to_string() {
    assert_eq!(js_number_to_string(0.0), "0");
    assert_eq!(js_number_to_string(-0.0), "0");
    assert_eq!(js_number_to_string(1.0), "1");
    assert_eq!(js_number_to_string(123.456), "123.456");
    assert_eq!(js_number_to_string(-1.5), "-1.5");
    // Above 2^63: `as i64` saturation fabricated 9223372036854775807.
    assert_eq!(js_number_to_string(1e19), "10000000000000000000");
    assert_eq!(js_number_to_string(1e20), "100000000000000000000");
    // The decimal/exponent thresholds (ES2023 6.1.6.1.20).
    assert_eq!(js_number_to_string(1e21), "1e+21");
    assert_eq!(js_number_to_string(2.5e21), "2.5e+21");
    assert_eq!(js_number_to_string(1e-6), "0.000001");
    assert_eq!(js_number_to_string(1e-7), "1e-7");
    assert_eq!(js_number_to_string(1.5e-7), "1.5e-7");
    assert_eq!(js_number_to_string(f64::INFINITY), "Infinity");
    assert_eq!(js_number_to_string(f64::NEG_INFINITY), "-Infinity");
    assert_eq!(js_number_to_string(f64::NAN), "NaN");
}

#[test]
fn boolean_is_a_flagged_union_of_regular_literals() {
    let t = tables();
    let boolean = t.type_of(t.intrinsics.boolean);
    assert!(boolean.flags.intersects(TypeFlags::UNION));
    assert!(boolean.flags.intersects(TypeFlags::BOOLEAN));
    let TypeData::Union { types, .. } = &boolean.data else {
        panic!("boolean must be a union");
    };
    assert_eq!(
        t.list(*types),
        [t.intrinsics.false_regular, t.intrinsics.true_regular]
    );
}

#[test]
fn literal_types_intern_by_value_and_wire_freshness() {
    let mut t = tables();
    let one_a = t.get_number_literal_type(1.0);
    let one_b = t.get_number_literal_type(1.0);
    let two = t.get_number_literal_type(2.0);
    assert_eq!(one_a, one_b);
    assert_ne!(one_a, two);

    // SameValueZero: -0 and +0 share an entry.
    assert_eq!(
        t.get_number_literal_type(-0.0),
        t.get_number_literal_type(0.0)
    );

    let fresh = t.get_fresh_type_of_literal_type(one_a);
    assert_ne!(fresh, one_a);
    assert!(t.is_fresh_literal_type(fresh));
    assert!(!t.is_fresh_literal_type(one_a));
    assert_eq!(t.get_fresh_type_of_literal_type(one_a), fresh);
    assert_eq!(t.get_fresh_type_of_literal_type(fresh), fresh);
    assert_eq!(t.get_regular_type_of_literal_type(None, fresh), one_a);

    let a = t.get_string_literal_type("a");
    assert_eq!(t.get_string_literal_type("a"), a);
    let big = t.get_bigint_literal_type(PseudoBigInt {
        negative: false,
        base10_value: "1".to_owned(),
    });
    assert_eq!(
        t.get_bigint_literal_type(PseudoBigInt {
            negative: false,
            base10_value: "1".to_owned(),
        }),
        big
    );
}

#[test]
fn unions_intern_by_sorted_member_list() {
    let mut t = tables();
    let one = t.get_number_literal_type(1.0);
    let two = t.get_number_literal_type(2.0);
    let a = t.get_union_type(None, &[one, two], UnionReduction::Literal);
    let b = t.get_union_type(None, &[two, one], UnionReduction::Literal);
    assert_eq!(a, b);
    // Flattening: (1 | 2) | 2 == 1 | 2.
    assert_eq!(
        t.get_union_type(None, &[a, two], UnionReduction::Literal),
        a
    );
    // Singletons collapse; empties are never.
    assert_eq!(t.get_union_type(None, &[one], UnionReduction::Literal), one);
    assert_eq!(
        t.get_union_type(None, &[], UnionReduction::Literal),
        t.intrinsics.never
    );
}

#[test]
fn union_literal_reduction_drops_subsumed_literals() {
    let mut t = tables();
    let one = t.get_number_literal_type(1.0);
    let a = t.get_string_literal_type("a");
    // "a" | string reduces to string; 1 | number reduces to number.
    assert_eq!(
        t.get_union_type(None, &[a, t.intrinsics.string], UnionReduction::Literal),
        t.intrinsics.string
    );
    assert_eq!(
        t.get_union_type(None, &[one, t.intrinsics.number], UnionReduction::Literal),
        t.intrinsics.number
    );
    // Fresh literal folds into its regular partner.
    let fresh = t.get_fresh_type_of_literal_type(one);
    assert_eq!(
        t.get_union_type(None, &[fresh, one], UnionReduction::Literal),
        one
    );
    // UnionReduction::None keeps the subsumed literal.
    let unreduced = t.get_union_type(None, &[a, t.intrinsics.string], UnionReduction::None);
    let TypeData::Union { types, .. } = &t.type_of(unreduced).data else {
        panic!("unreduced union stays a union");
    };
    assert_eq!(t.list(*types).len(), 2);
}

#[test]
fn union_any_unknown_absorption() {
    let mut t = tables();
    let string = t.intrinsics.string;
    assert_eq!(
        t.get_union_type(None, &[t.intrinsics.any, string], UnionReduction::Literal),
        t.intrinsics.any
    );
    assert_eq!(
        t.get_union_type(
            None,
            &[t.intrinsics.unknown, string],
            UnionReduction::Literal
        ),
        t.intrinsics.unknown
    );
    assert_eq!(
        t.get_union_type(
            None,
            &[t.intrinsics.wildcard, string],
            UnionReduction::Literal
        ),
        t.intrinsics.wildcard
    );
    assert_eq!(
        t.get_union_type(None, &[t.intrinsics.error, string], UnionReduction::Literal),
        t.intrinsics.error
    );
    // never members vanish.
    assert_eq!(
        t.get_union_type(None, &[t.intrinsics.never, string], UnionReduction::Literal),
        string
    );
}

#[test]
fn union_folds_nullable_members_without_strict_null_checks() {
    let mut loose = TypeTables::new(false, false, &IdentityDomain::new());
    let number = loose.intrinsics.number;
    let null = loose.intrinsics.null;
    // number | null collapses to number at construction (61347-61349).
    assert_eq!(
        loose.get_union_type(None, &[number, null], UnionReduction::Literal),
        number
    );
    // All-nullable sets fold to the (non-)widening singletons.
    assert_eq!(
        loose.get_union_type(None, &[null], UnionReduction::Literal),
        null
    );
    // A widening null plus a NON-widening undefined: the
    // IncludesNonWideningType bit is global, so the null branch
    // returns the non-widening nullType (61566-61568).
    let widening = loose.intrinsics.null_widening;
    assert_eq!(
        loose.get_union_type(
            None,
            &[widening, loose.intrinsics.undefined],
            UnionReduction::Literal
        ),
        loose.intrinsics.null
    );
    assert_eq!(
        loose.get_union_type(None, &[widening, widening], UnionReduction::Literal),
        widening
    );
    // Under strictNullChecks nullable members stay.
    let mut strict = tables();
    let strict_union = strict.get_union_type(
        None,
        &[strict.intrinsics.number, strict.intrinsics.null],
        UnionReduction::Literal,
    );
    assert!(strict.flags_of(strict_union).intersects(TypeFlags::UNION));
}

#[test]
fn union_dedups_missing_against_undefined() {
    // exactOptionalPropertyTypes tables: undefinedOrMissing = missing.
    let mut t = TypeTables::new(true, true, &IdentityDomain::new());
    let missing = t.intrinsics.missing;
    let undefined = t.intrinsics.undefined;
    assert_ne!(missing, undefined);
    // undefined | missing folds to undefined (61540-61544).
    assert_eq!(
        t.get_union_type(None, &[undefined, missing], UnionReduction::Literal),
        undefined
    );
}

#[test]
fn two_union_fast_path_caches_by_reduction() {
    let mut t = tables();
    let one = t.get_number_literal_type(1.0);
    let two = t.get_number_literal_type(2.0);
    let string = t.intrinsics.string;
    let union = t.get_union_type(None, &[one, two], UnionReduction::Literal);
    let first = t.get_union_type(None, &[union, string], UnionReduction::Literal);
    let second = t.get_union_type(None, &[union, string], UnionReduction::Literal);
    assert_eq!(first, second);
    let reversed = t.get_union_type(None, &[string, union], UnionReduction::Literal);
    // Same worker result; the cache key is order-normalized.
    assert_eq!(first, reversed);
}

#[test]
fn named_union_members_denormalize_into_origin() {
    let mut t = tables();
    let one = t.get_number_literal_type(1.0);
    let two = t.get_number_literal_type(2.0);
    let named = t.get_union_type(None, &[one, two], UnionReduction::Literal);
    // Synthesize an alias (M4 machinery) to make the union "named".
    t.set_alias(named, Some(crate::ty::SymbolId::new(0)), None);
    // A union containing ONLY the named union returns it unchanged.
    let string = t.intrinsics.string;
    let widened = t.get_union_type(None, &[named, string], UnionReduction::Literal);
    let TypeData::Union { types, origin, .. } = &t.type_of(widened).data else {
        panic!("union expected");
    };
    // typeSet is id-sorted: the string intrinsic precedes the
    // literal types allocated by this test.
    assert_eq!(t.list(*types), [string, one, two]);
    let origin = origin.expect("named member denormalizes into an origin");
    let TypeData::Union {
        types: origin_types,
        ..
    } = &t.type_of(origin).data
    else {
        panic!("origin is a union");
    };
    // insertType keeps id order: string (intrinsic) precedes the
    // later-allocated named union.
    assert_eq!(t.list(*origin_types), [string, named]);
}

#[test]
fn get_type_list_id_compresses_consecutive_ids() {
    let t = tables();
    assert_eq!(
        t.get_type_list_id(&[
            TypeId::new(5),
            TypeId::new(6),
            TypeId::new(7),
            TypeId::new(9)
        ]),
        "5:3,9"
    );
    assert_eq!(t.get_type_list_id(&[TypeId::new(3)]), "3");
    assert_eq!(t.get_type_list_id(&[]), "");
}

#[test]
fn tuple_targets_intern_by_flags_and_readonly() {
    let mut t = tables();
    let req = [ElementFlags::REQUIRED, ElementFlags::OPTIONAL];
    let req_flags = TupleTargetFlags::new(&req).expect("not single-rest");
    let a = t.get_tuple_target_type(None, req_flags, false, None);
    let b = t.get_tuple_target_type(None, req_flags, false, None);
    let readonly = t.get_tuple_target_type(None, req_flags, true, None);
    assert_eq!(a, b);
    assert_ne!(a, readonly);
    let Some(data) = t.tuple_target_of(a) else {
        panic!("tuple target expected");
    };
    assert_eq!(data.min_length, 1);
    assert_eq!(data.fixed_length, 2);
    assert!(!data.has_rest_element);

    let rest = [ElementFlags::REQUIRED, ElementFlags::REST];
    let with_rest = t.get_tuple_target_type(
        None,
        TupleTargetFlags::new(&rest).expect("not single-rest"),
        false,
        None,
    );
    let Some(data) = t.tuple_target_of(with_rest) else {
        panic!("tuple target expected");
    };
    assert_eq!(data.min_length, 1);
    assert_eq!(data.fixed_length, 1);
    assert!(data.has_rest_element);
}

/// A tuple target is shared by its elements' kinds (the first that applies),
/// its readonly bit and its labels when one is set, as the string key keyed
/// it.
#[test]
fn tuple_targets_share_by_kind_readonly_and_labels() {
    let mut t = tables();
    let required = [ElementFlags::REQUIRED, ElementFlags::OPTIONAL];
    let a = t.get_tuple_target_type(None, TupleTargetFlags::new(&required).unwrap(), false, None);
    let mixed = [
        ElementFlags::REQUIRED | ElementFlags::OPTIONAL,
        ElementFlags::OPTIONAL,
    ];
    assert_eq!(
        t.get_tuple_target_type(None, TupleTargetFlags::new(&mixed).unwrap(), false, None),
        a
    );
    let three = [
        ElementFlags::REQUIRED,
        ElementFlags::OPTIONAL,
        ElementFlags::OPTIONAL,
    ];
    assert_ne!(
        t.get_tuple_target_type(None, TupleTargetFlags::new(&three).unwrap(), false, None),
        a
    );
    let flags = TupleTargetFlags::new(&required).unwrap();
    assert_eq!(
        t.get_tuple_target_type(None, flags, false, Some(&[None, None])),
        a
    );
    let labeled = t.get_tuple_target_type(None, flags, false, Some(&[Some(5), None]));
    assert_ne!(labeled, a);
    assert_eq!(
        t.get_tuple_target_type(None, flags, false, Some(&[Some(5), None])),
        labeled
    );
    assert_ne!(
        t.get_tuple_target_type(None, flags, false, Some(&[Some(6), None])),
        labeled
    );
    assert_ne!(
        t.get_tuple_target_type(None, flags, true, Some(&[Some(5), None])),
        labeled
    );
}

#[test]
fn tuple_length_types_exist_before_member_queries() {
    for (flags, lengths) in [
        (vec![], vec![0]),
        (vec![ElementFlags::REQUIRED; 2], vec![2]),
        (
            vec![ElementFlags::REQUIRED, ElementFlags::OPTIONAL],
            vec![1, 2],
        ),
    ] {
        for readonly in [false, true] {
            let mut t = tables();
            let flags = TupleTargetFlags::new(&flags).expect("fixed tuple shape");
            let target = t.get_tuple_target_type(None, flags, readonly, None);
            let Some(data) = t.tuple_target_of(target) else {
                panic!("tuple target");
            };
            let length_type = data.length_type;
            assert!(length_type < target);
            let literals: Vec<_> = lengths
                .iter()
                .map(|length| t.get_number_literal_type(*length as f64))
                .collect();
            assert!(literals.iter().all(|literal| *literal < target));
            assert_eq!(
                length_type,
                t.get_union_type(None, &literals, UnionReduction::Literal)
            );
            assert_eq!(target, t.get_tuple_target_type(None, flags, readonly, None));
        }
    }
    for tail in [ElementFlags::REST, ElementFlags::VARIADIC] {
        let mut t = tables();
        let flags = [ElementFlags::REQUIRED, tail];
        let target =
            t.get_tuple_target_type(None, TupleTargetFlags::new(&flags).unwrap(), false, None);
        let Some(data) = t.tuple_target_of(target) else {
            panic!("tuple target");
        };
        assert_eq!(data.length_type, t.intrinsics.number);
    }
}

#[test]
fn tuple_target_flags_exclude_the_checker_owned_single_rest_shape() {
    assert!(TupleTargetFlags::new(&[ElementFlags::REST]).is_none());
    assert!(TupleTargetFlags::new(&[]).is_some());
    assert!(TupleTargetFlags::new(&[ElementFlags::REQUIRED, ElementFlags::REST,]).is_some());
}

#[test]
fn normalized_tuples_splice_variadic_tuples() {
    let mut t = tables();
    let number = t.intrinsics.number;
    let string = t.intrinsics.string;
    let boolean = t.intrinsics.boolean;
    // [string, boolean]
    let inner = t
        .create_tuple_type(None, &[string, boolean], None, false, None)
        .expect("inner tuple");
    // [number, ...[string, boolean]] normalizes to [number, string, boolean].
    let outer_flags = [ElementFlags::REQUIRED, ElementFlags::VARIADIC];
    let outer_target = t.get_tuple_target_type(
        None,
        TupleTargetFlags::new(&outer_flags).expect("not single-rest"),
        false,
        None,
    );
    let outer = t
        .create_normalized_tuple_type(None, outer_target, &[number, inner])
        .expect("normalized");
    let direct = t
        .create_tuple_type(None, &[number, string, boolean], None, false, None)
        .expect("direct tuple");
    assert_eq!(outer, direct);
}

#[test]
fn template_literal_types_fold_and_intern() {
    let mut t = tables();
    let string = t.intrinsics.string;
    let number = t.intrinsics.number;
    // `a${string}` interns by texts+types.
    let a1 = t.get_template_literal_type(&["a".into(), "".into()], &[string]);
    let a2 = t.get_template_literal_type(&["a".into(), "".into()], &[string]);
    assert_eq!(a1, a2);
    let ab = t.get_template_literal_type(&["a".into(), "b".into()], &[string]);
    let moved = t.get_template_literal_type(&["ab".into(), "".into()], &[string]);
    assert_ne!(ab, moved);
    let changed_type = t.get_template_literal_type(&["a".into(), "b".into()], &[number]);
    assert_ne!(ab, changed_type);
    // All-literal spans fold to a plain string literal (62071-62073).
    let one = t.get_number_literal_type(1.0);
    let folded = t.get_template_literal_type(&["a".into(), "b".into()], &[one]);
    assert_eq!(folded, t.get_string_literal_type("a1b"));
    // The all-literal fold stays in the JavaScript UTF-16 domain:
    // a lone surrogate is not interned as U+FFFD.
    let suffix = t.get_string_literal_type("x");
    let folded_surrogate = t.get_template_literal_type_from_texts(
        &[TemplateText::from_utf16(&[0xD800]), TemplateText::default()],
        &[suffix],
    );
    let folded_replacement = t.get_template_literal_type_from_texts(
        &[TemplateText::from_utf16(&[0xFFFD]), TemplateText::default()],
        &[suffix],
    );
    assert_ne!(folded_surrogate, folded_replacement);
    assert_eq!(
        t.literal_value(folded_surrogate),
        Some(LiteralValue::String(
            crate::JsString::from_code_units(&[0xD800, b'x' as u16]).as_js()
        ))
    );
    // `${string}` with empty texts collapses to string (62075-62078).
    let s = t.get_template_literal_type(&["".into(), "".into()], &[string]);
    assert_eq!(s, string);
    // `${number}` stays a pattern template.
    let n = t.get_template_literal_type(&["".into(), "".into()], &[number]);
    assert!(t.flags_of(n).intersects(TypeFlags::TEMPLATE_LITERAL));
    assert!(t.is_pattern_literal_type(n));

    // Fixed texts intern by JavaScript UTF-16 code units: an
    // unpaired surrogate stays distinct from U+FFFD, while a
    // valid pair is the same text as its scalar UTF-8 spelling.
    let surrogate = t.get_template_literal_type_from_texts(
        &[TemplateText::from_utf16(&[0xD800]), TemplateText::default()],
        &[number],
    );
    let replacement = t.get_template_literal_type_from_texts(
        &[TemplateText::from_utf16(&[0xFFFD]), TemplateText::default()],
        &[number],
    );
    assert_ne!(surrogate, replacement);
    assert_eq!(
        TemplateText::from_utf16(&[0xD83D, 0xDE00]),
        TemplateText::from_utf8("😀")
    );
}

#[test]
fn template_scratch_keeps_cached_payloads_after_growth_and_limit_errors() {
    let mut t = tables();
    let number = t.intrinsics.number;
    let string = t.intrinsics.string;
    let texts = [
        TemplateText::from_utf16(&[0xD800, b'a' as u16]),
        TemplateText::from_utf8("0123456789abcdef😀"),
        TemplateText::from_utf16(&[0xDC00]),
    ];
    let types = [number, string];
    let original = t.get_template_literal_type_from_texts(&texts, &types);
    // Each reusable segment buffer sees the longest segment before the
    // capacity check; later cache hits should only rotate those buffers.
    for _ in 0..6 {
        assert_eq!(
            t.get_template_literal_type_from_texts(&texts, &types),
            original
        );
    }
    let capacities = |tables: &TypeTables| {
        let scratch = tables.template_scratch.last().unwrap();
        let mut units = scratch
            .new_texts
            .iter()
            .map(Vec::capacity)
            .chain(std::iter::once(scratch.text.capacity()))
            .collect::<Vec<_>>();
        units.sort_unstable();
        (
            scratch.new_types.capacity(),
            scratch.new_texts.capacity(),
            units,
        )
    };
    let retained = capacities(&t);
    for _ in 0..8 {
        assert_eq!(
            t.get_template_literal_type_from_texts(&texts, &types),
            original
        );
        assert_eq!(capacities(&t), retained);
    }

    let empty = [TemplateText::default(), TemplateText::default()];
    assert_eq!(
        t.get_template_literal_type_from_texts(&empty, &[number]),
        t.intrinsics.numeric_string
    );
    assert_eq!(
        t.get_template_literal_type_from_texts(&empty, &[string]),
        string
    );
    let oversized_types = vec![number; MAX_TEMPLATE_LITERAL_TYPE_SPANS + 1];
    let oversized_texts = vec![TemplateText::default(); oversized_types.len() + 1];
    assert_eq!(
        t.try_get_template_literal_type_from_texts(&oversized_texts, &oversized_types),
        Err(TemplateLiteralTooLarge),
    );
    assert_eq!(
        t.get_template_literal_type_from_texts(&texts, &types),
        original
    );
    let TypeData::TemplateLiteral(payload) = t.type_of(original).data else {
        panic!("expected a stored template literal");
    };
    assert_eq!(&*t.template_literal(payload).texts, &texts);
    assert_eq!(t.list(t.template_literal(payload).types), types);
}

#[test]
fn borrowed_template_keys_keep_the_stored_text_hash() {
    use std::hash::{Hash, Hasher};

    let t = tables();
    let types = [t.intrinsics.number, t.intrinsics.string];
    let texts = [
        TemplateText::from_utf16(&[0xD800]),
        TemplateText::default(),
        TemplateText::from_utf8("é😀"),
    ];
    let mut stored = rustc_hash::FxHasher::default();
    types.as_slice().hash(&mut stored);
    texts.as_slice().hash(&mut stored);
    assert_eq!(
        hash_template_literal_key(&types, texts.len(), texts.iter().map(TemplateText::units)),
        stored.finish(),
    );
}

#[test]
fn mapped_types_carry_root_and_instantiation_identity() {
    let mut t = tables();
    let symbol = SymbolId::new(7);
    let root = t.create_mapped_type(42, None, None, Some(symbol));
    assert_eq!(t.flags_of(root), TypeFlags::OBJECT);
    assert_eq!(t.object_flags_of(root), ObjectFlags::MAPPED);
    assert_eq!(t.type_of(root).symbol, Some(symbol));
    let TypeData::Mapped(root_data) = &t.type_of(root).data else {
        panic!("mapped constructor must use mapped semantic payload");
    };
    assert_eq!(root_data.declaration, 42);
    assert_eq!(root_data.target, None);
    assert_eq!(root_data.mapper, None);

    let mapper = MapperId::new(3);
    let instance = t.create_mapped_type(42, Some(root), Some(mapper), Some(symbol));
    assert_eq!(
        t.object_flags_of(instance),
        ObjectFlags::INSTANTIATED_MAPPED
    );
    let TypeData::Mapped(instance_data) = &t.type_of(instance).data else {
        panic!("mapped instance must retain mapped semantic payload");
    };
    assert_eq!(instance_data.target, Some(root));
    assert_eq!(instance_data.mapper, Some(mapper));
}

#[test]
fn reverse_mapped_types_retain_inference_inputs() {
    let mut t = tables();
    let source = t.intrinsics.string;
    let mapped = t.intrinsics.number;
    let constraint = t.intrinsics.es_symbol;
    let reverse = t.create_reverse_mapped_type(source, mapped, constraint);
    assert_eq!(t.flags_of(reverse), TypeFlags::OBJECT);
    assert_eq!(
        t.object_flags_of(reverse),
        ObjectFlags::REVERSE_MAPPED | ObjectFlags::ANONYMOUS
    );
    let TypeData::ReverseMapped(data) = &t.type_of(reverse).data else {
        panic!("reverse mapped constructor must retain its inputs");
    };
    assert_eq!(data.source, source);
    assert_eq!(data.mapped_type, mapped);
    assert_eq!(data.constraint_type, constraint);
}

#[test]
fn conditional_type_model_constructibility() {
    let mut t = tables();
    let check = t.create_synthesized_type_parameter(None);
    let extends = t.intrinsics.string;
    let root = t.create_conditional_root(ConditionalRootData {
        node: 42,
        check_type: check,
        extends_type: extends,
        is_distributive: true,
        infer_type_parameters: Box::new([]),
        outer_type_parameters: Some(Box::new([check])),
        alias_symbol: Some(SymbolId::new(7)),
        alias_type_arguments: Some(Box::new([check])),
    });
    let conditional = t.create_conditional_type(
        ConditionalTypeData {
            root,
            check_type: check,
            extends_type: extends,
            mapper: None,
            combined_mapper: None,
        },
        Some(SymbolId::new(7)),
        Some(&[check]),
    );
    assert_eq!(t.flags_of(conditional), TypeFlags::CONDITIONAL);
    let TypeData::Conditional(data) = &t.type_of(conditional).data else {
        panic!("conditional constructor must retain its root");
    };
    let data = *t.conditional_data(*data);
    assert_eq!(data.root, root);
    assert_eq!(data.check_type, check);
    assert_eq!(data.extends_type, extends);
    assert!(t.conditional_root(root).is_distributive);
    assert_eq!(
        t.conditional_root(root).outer_type_parameters.as_deref(),
        Some([check].as_slice())
    );
}

#[test]
fn substitution_type_model_constructibility() {
    let mut t = tables();
    let base = t.create_synthesized_type_parameter(None);
    let constraint = t.intrinsics.string;
    let substitution = t.get_substitution_type(base, constraint);
    assert_eq!(
        substitution,
        t.get_substitution_type(base, constraint),
        "substitution pairs intern"
    );
    let data = t
        .substitution_data(substitution)
        .expect("nontrivial substitution has a payload");
    assert_eq!(data.base_type, base);
    assert_eq!(data.constraint, constraint);
    assert_eq!(
        t.get_substitution_type(base, t.intrinsics.unknown),
        base,
        "ordinary substitution creation collapses unknown constraints"
    );
    let no_infer = t.get_or_create_substitution_type(base, t.intrinsics.unknown);
    assert!(t.is_no_infer_type(no_infer));
}

#[test]
fn distinct_anonymous_object_types_never_intern() {
    let mut t = tables();
    let a = t.create_type(TypeFlags::OBJECT, TypeData::PLAIN_OBJECT);
    let b = t.create_type(TypeFlags::OBJECT, TypeData::PLAIN_OBJECT);
    assert_ne!(a, b);
}

#[test]
fn optionality_follows_strict_null_checks() {
    let mut t = tables();
    let number = t.intrinsics.number;
    let optional = t.add_optionality(None, number, /*is_property*/ true, true);
    let TypeData::Union { types, .. } = &t.type_of(optional).data else {
        panic!("optional property type must union undefined");
    };
    assert!(t.list(*types).contains(&t.intrinsics.undefined));
    assert_eq!(t.add_optionality(None, number, true, false), number);

    let mut loose = TypeTables::new(false, false, &IdentityDomain::new());
    let number = loose.intrinsics.number;
    assert_eq!(loose.add_optionality(None, number, true, true), number);
}

#[test]
fn template_literal_types_over_tsgo_limits_are_too_large() {
    // tsgo getTemplateLiteralType (checker.go:29610-29672): more than
    // 100,000 placeholders, or more than 50,000,000 UTF-8 bytes of text,
    // is too large; the checker reports TS2589 and uses the error type.
    let mut t = tables();
    let string = t.intrinsics.string;
    let try_template = |t: &mut TypeTables, texts: &[String], types: &[TypeId]| {
        let texts = texts
            .iter()
            .map(|text| TemplateText::from_utf8(text))
            .collect::<Vec<_>>();
        t.try_get_template_literal_type_from_texts(&texts, types)
    };
    let limit = MAX_TEMPLATE_LITERAL_TYPE_SPANS;
    let at_limit = try_template(
        &mut t,
        &vec![String::new(); limit + 1],
        &vec![string; limit],
    );
    assert!(at_limit.is_ok());
    let over_limit = try_template(
        &mut t,
        &vec![String::new(); limit + 2],
        &vec![string; limit + 1],
    );
    assert_eq!(over_limit, Err(TemplateLiteralTooLarge));
    assert_eq!(
        t.get_template_literal_type(&vec![String::new(); limit + 2], &vec![string; limit + 1]),
        t.intrinsics.error
    );
    // Text is measured in UTF-8 bytes, as Go counts it.
    let half = MAX_TEMPLATE_LITERAL_TYPE_LENGTH / 2;
    let fits = try_template(&mut t, &["a".repeat(half), "a".repeat(half)], &[string]);
    assert!(fits.is_ok());
    let over = try_template(&mut t, &["a".repeat(half), "a".repeat(half + 1)], &[string]);
    assert_eq!(over, Err(TemplateLiteralTooLarge));
}

/// A string literal's text is one type whichever form looks it up: a `str`,
/// a JavaScript string, UTF-16 units or a template text (a lone surrogate
/// stays distinct from U+FFFD, and a surrogate pair is its scalar).
#[test]
fn a_string_literal_text_is_one_type_from_every_entry() {
    let mut t = tables();
    let plain = t.get_string_literal_type("abc");
    assert_eq!(
        t.get_string_literal_type_from_utf16(&[0x61, 0x62, 0x63]),
        plain
    );
    assert_eq!(
        t.get_string_literal_type_from_text(&TemplateText::from_utf8("abc")),
        plain
    );
    assert_eq!(
        t.literal_value(plain),
        Some(LiteralValue::String("abc".into()))
    );
    let lone = t.get_string_literal_type_from_utf16(&[0xD800]);
    assert_eq!(
        t.get_string_literal_type(crate::JsString::from_code_units(&[0xD800]).as_js()),
        lone
    );
    assert_ne!(t.get_string_literal_type("\u{FFFD}"), lone);
    let pair = t.get_string_literal_type_from_utf16(&[0xD83D, 0xDE00]);
    assert_eq!(t.get_string_literal_type("\u{1F600}"), pair);
    assert_ne!(pair, plain);
}

#[test]
fn safe_integer_number_formatting_preserves_previous_bytes() {
    fn before(value: f64) -> String {
        if value.is_nan() {
            return "NaN".to_string();
        }
        if value == f64::INFINITY {
            return "Infinity".to_string();
        }
        if value == f64::NEG_INFINITY {
            return "-Infinity".to_string();
        }
        if value == 0.0 {
            return "0".to_string();
        }
        if value < 0.0 {
            return format!("-{}", before(-value));
        }
        // Shortest round-trip digits: "d[.ddd]e<exp>" from LowerExp.
        let exp_form = format!("{value:e}");
        let (mantissa, exp) = exp_form.split_once('e').expect("LowerExp always emits 'e'");
        let exp: i32 = exp.parse().expect("LowerExp exponent is an integer");
        let mut digits: String = mantissa.chars().filter(|c| *c != '.').collect();
        while digits.len() > 1 && digits.ends_with('0') {
            digits.pop();
        }
        // value = 0.<digits> * 10^n with k significant digits.
        let k = digits.len() as i32;
        let n = exp + 1;
        if k <= n && n <= 21 {
            format!("{digits}{}", "0".repeat((n - k) as usize))
        } else if 0 < n && n <= 21 {
            format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
        } else if -6 < n && n <= 0 {
            format!("0.{}{digits}", "0".repeat((-n) as usize))
        } else {
            let e = n - 1;
            let sign = if e >= 0 { "+" } else { "-" };
            if k == 1 {
                format!("{digits}e{sign}{}", e.abs())
            } else {
                format!("{}.{}e{sign}{}", &digits[..1], &digits[1..], e.abs())
            }
        }
    }

    let mut count = 0;
    let mut compare = |value: f64| {
        assert_eq!(js_number_to_string(value), before(value), "{value:?}");
        count += 1;
    };
    for integer in -65_536..=65_536 {
        compare(f64::from(integer));
    }
    for value in [
        0.0,
        -0.0,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        1e-7,
        1e-6,
        1e20,
        1e21,
        9_007_199_254_740_991.0,
        9_007_199_254_740_992.0,
        4_294_967_295.0,
    ] {
        compare(value);
        compare(-value);
        if value.is_finite() {
            compare(f64::from_bits(value.to_bits().wrapping_add(1)));
            compare(f64::from_bits(value.to_bits().wrapping_sub(1)));
        }
    }
    let mut bits = 0x9e3779b97f4a7c15u64;
    for _ in 0..100_000 {
        bits ^= bits << 13;
        bits ^= bits >> 7;
        bits ^= bits << 17;
        compare(f64::from_bits(bits));
        compare((bits % 9_007_199_254_740_992) as f64);
    }
    assert_eq!(count, 331_115);
}

#[test]
fn named_union_identity_membership_preserves_overlaps_and_fresh_literals() {
    use crate::SymbolId;
    let mut t = tables();
    let a = t.get_string_literal_type("a");
    let fresh = t.get_fresh_type_of_literal_type(a);
    let b = t.get_string_literal_type("b");
    let c = t.get_string_literal_type("c");
    let left = t.get_union_type(None, &[a, b], UnionReduction::None);
    let right = t.get_union_type(None, &[b, c], UnionReduction::None);
    t.set_alias(left, Some(SymbolId::new(0)), None);
    t.set_alias(right, Some(SymbolId::new(1)), None);
    assert_eq!(
        t.get_union_type(None, &[left, a], UnionReduction::None),
        left
    );
    let overlap = t.get_union_type(None, &[left, right], UnionReduction::None);
    let TypeData::Union { types, origin, .. } = t.type_of(overlap).data else {
        panic!("union");
    };
    assert_eq!(t.list(types), [a, b, c]);
    assert!(
        origin.is_none(),
        "overlapping named unions have no denormalized origin"
    );
    let with_fresh = t.get_union_type(None, &[left, fresh], UnionReduction::None);
    let TypeData::Union { types, origin, .. } = t.type_of(with_fresh).data else {
        panic!("union");
    };
    assert_eq!(t.list(types), [a, fresh, b]);
    let origin = origin.expect("fresh and regular literal IDs are distinct members");
    let TypeData::Union { types, .. } = t.type_of(origin).data else {
        panic!("origin");
    };
    assert_eq!(t.list(types), [fresh, left]);
}

#[test]
fn tuple_targets_share_element_parameters_but_preserve_this_constraints() {
    let mut t = tables();
    let two = TupleTargetFlags::new(&[ElementFlags::REQUIRED; 2]).unwrap();
    let three = TupleTargetFlags::new(&[ElementFlags::OPTIONAL; 3]).unwrap();
    let required = t.get_tuple_target_type(None, two, false, None);
    let readonly = t.get_tuple_target_type(None, two, true, None);
    let optional = t.get_tuple_target_type(None, three, false, None);
    let a = t.tuple_target_of(required).unwrap();
    let b = t.tuple_target_of(readonly).unwrap();
    let c = t.tuple_target_of(optional).unwrap();
    assert_eq!(a.type_parameters, b.type_parameters);
    assert_eq!(t.list(a.type_parameters), &t.list(c.type_parameters)[..2]);
    assert_ne!(a.this_type, b.this_type);
    assert_ne!(a.this_type, c.this_type);
    for (target, this_type) in [
        (required, a.this_type),
        (readonly, b.this_type),
        (optional, c.this_type),
    ] {
        assert!(
            matches!(t.type_of(this_type).data, TypeData::TypeParameter { is_this_type: true, constraint: Some(own), .. } if own == target)
        );
    }
}

#[test]
fn compact_literals_preserve_bits_kinds_and_fresh_indices() {
    let mut t = tables();
    let initial_count = t.len();
    let initial_numbers = t.number_literals.len();
    let initial_heap = t.memory_usage().type_heap_bytes;
    let values = [
        -0.0,
        1.25,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::from_bits(0x7ff8_0000_0000_0042),
    ];
    for (offset, value) in values.into_iter().enumerate() {
        let regular = t.get_number_literal_type(value);
        assert_eq!(regular.index() as usize, initial_count + offset * 2);
        let fresh = t.get_fresh_type_of_literal_type(regular);
        assert_eq!(fresh.index(), regular.index() + 1);
        let TypeData::Literal {
            value: regular_index,
            ..
        } = t.type_of(regular).data
        else {
            panic!("number literal data");
        };
        let TypeData::Literal {
            value: fresh_index, ..
        } = t.type_of(fresh).data
        else {
            panic!("fresh number literal data");
        };
        assert_eq!(regular_index, fresh_index);
        for id in [regular, fresh] {
            let Some(LiteralValue::Number(actual)) = t.literal_value(id) else {
                panic!("number literal value");
            };
            assert_eq!(actual.to_bits(), value.to_bits());
            let decoded = t.literal_data_of(id).unwrap();
            assert_eq!(t.literal_data_equal(decoded, decoded), !value.is_nan());
        }
        assert_eq!(t.get_number_literal_type(value), regular);
        assert_eq!(t.number_literals.len(), initial_numbers + offset + 1);
    }
    assert_eq!(
        t.memory_usage().type_heap_bytes - initial_heap,
        t.number_literals.capacity() * std::mem::size_of::<NumberBits>(),
    );
    let zero = t.get_number_literal_type(0.0);
    let Some(LiteralValue::Number(value)) = t.literal_value(zero) else {
        panic!("zero value");
    };
    assert_eq!(value.to_bits(), (-0.0f64).to_bits());

    let text = crate::JsString::from_code_units(&[0xd800, 0x61]);
    let string = t.get_string_literal_type(text.as_js());
    let bigint = t.get_bigint_literal_type(PseudoBigInt {
        negative: true,
        base10_value: "12345678901234567890".to_owned(),
    });
    for id in [string, bigint] {
        let fresh = t.get_fresh_type_of_literal_type(id);
        assert_eq!(t.literal_value(id), t.literal_value(fresh));
        assert_eq!(t.flags_of(id), t.flags_of(fresh));
    }
    assert_eq!(
        t.literal_value(string),
        Some(LiteralValue::String(text.as_js()))
    );
    let number_data = t.literal_data_of(zero).unwrap();
    let string_data = t.literal_data_of(string).unwrap();
    let bigint_data = t.literal_data_of(bigint).unwrap();
    assert!(!t.literal_data_equal(number_data, string_data));
    assert!(!t.literal_data_equal(string_data, bigint_data));
    assert!(!t.literal_data_equal(bigint_data, number_data));
    assert_eq!(t.literal_data_of(t.intrinsics.number), None);
    assert_eq!(t.literal_value(t.intrinsics.true_regular), None);
}

#[test]
fn compact_enum_literals_keep_the_first_members_bits_and_symbol() {
    let mut t = tables();
    for (enum_index, value, equal_value) in [
        (700, -0.0, 0.0),
        (701, f64::from_bits(0x7ff8_0000_0000_0042), f64::NAN),
    ] {
        let symbol = SymbolId::new(enum_index + 1);
        let enum_id = SymbolId::new(enum_index);
        let regular = t.get_enum_literal_type(LiteralValue::Number(value), enum_id, symbol);
        assert_eq!(
            t.get_enum_literal_type(
                LiteralValue::Number(equal_value),
                enum_id,
                SymbolId::new(enum_index + 2)
            ),
            regular,
        );
        let fresh = t.get_fresh_type_of_literal_type(regular);
        for id in [regular, fresh] {
            assert!(t.flags_of(id).intersects(TypeFlags::ENUM_LITERAL));
            assert_eq!(t.type_of(id).symbol, Some(symbol));
            let Some(LiteralValue::Number(actual)) = t.literal_value(id) else {
                panic!("numeric enum value");
            };
            assert_eq!(actual.to_bits(), value.to_bits());
        }
    }
}

#[test]
fn compact_conditionals_keep_distinct_instantiations_after_table_growth() {
    let mut t = tables();
    let check = t.intrinsics.string;
    let extends = t.intrinsics.number;
    let root = t.create_conditional_root(ConditionalRootData {
        node: 73,
        check_type: check,
        extends_type: extends,
        is_distributive: false,
        infer_type_parameters: Box::new([]),
        outer_type_parameters: None,
        alias_symbol: None,
        alias_type_arguments: None,
    });
    let start = t.len();
    let mut saved = Vec::new();
    for index in 0..65 {
        let data = ConditionalTypeData {
            root,
            check_type: check,
            extends_type: extends,
            mapper: Some(MapperId::new(index)),
            combined_mapper: Some(MapperId::new(index + 100)),
        };
        let id = t.create_conditional_type(data, Some(SymbolId::new(700)), Some(&[check]));
        assert_eq!(id.index() as usize, start + index as usize);
        saved.push((id, data));
    }
    for (id, expected) in saved {
        let TypeData::Conditional(data) = t.type_of(id).data else {
            panic!("conditional payload id");
        };
        assert_eq!(*t.conditional_data(data), expected);
        assert_eq!(t.alias_symbol(id), Some(SymbolId::new(700)));
        assert_eq!(t.alias_type_arguments(id), Some([check].as_slice()));
        assert_eq!(t.flags_of(id), TypeFlags::CONDITIONAL);
    }
}

#[test]
fn packed_instantiation_keys_preserve_all_shapes_and_bounds() {
    let mut t = tables();
    let value = t.intrinsics.string;
    let replacement = t.intrinsics.number;
    let mut expected = Vec::new();
    for target in [
        TypeId::new(0),
        TypeId::new((1 << 31) - 1),
        TypeId::new(1 << 31),
        TypeId::new(u32::MAX - 1),
    ] {
        // An interned list (the empty one) and an id that names no list.
        for arguments in [t.empty_type_list(), TypeListId::new(u32::MAX - 1)] {
            for single_signature in [false, true] {
                for (alias_symbol, alias_arguments) in [
                    (None, None),
                    (Some(SymbolId::new(7)), None),
                    (Some(SymbolId::new(7)), Some(TypeListId::new(3))),
                    (Some(SymbolId::new(8)), Some(TypeListId::new(3))),
                    (None, Some(TypeListId::new(3))),
                ] {
                    let key = InstantiationKey {
                        arguments,
                        alias_symbol,
                        alias_arguments,
                        single_signature,
                    };
                    let packed = InstantiationCache::packed_key(target, key);
                    assert_eq!(
                        packed.is_some(),
                        target.index() < 1 << 31
                            && alias_symbol.is_none()
                            && alias_arguments.is_none()
                    );
                    if let Some(packed) = packed {
                        assert_eq!((packed & ((1 << 31) - 1)) as u32, target.index());
                        assert_eq!(
                            ((packed >> 31) & u64::from(u32::MAX)) as u32,
                            arguments.index()
                        );
                        assert_eq!(packed >> 63, u64::from(single_signature));
                    }
                    assert_eq!(t.instantiation_get(target, key), None);
                    t.instantiation_insert(target, key, value);
                    expected.push(((target, key), value));
                }
            }
        }
    }
    assert!(!t.instantiations.plain.is_empty());
    assert!(!t.instantiations.full.is_empty());
    assert_eq!(t.instantiations.len(), expected.len());
    for &((target, key), value) in &expected {
        assert_eq!(t.instantiation_get(target, key), Some(value));
        t.instantiation_insert(target, key, replacement);
        assert_eq!(t.instantiation_get(target, key), Some(replacement));
    }
    assert_eq!(t.instantiations.len(), expected.len());
}

#[test]
fn borrowed_instantiation_probes_distinguish_collisions_and_keep_miss_order() {
    let mut t = tables();
    let target = t.intrinsics.any;
    // Fresh IDs keep these sequences distinct from built-in union lists.
    let a: [TypeId; 3] = std::array::from_fn(|_| t.create_type_parameter(false, None));
    let b = [a[1], a[0], a[2]];
    let list_a = t.intern_type_list(&a);
    let list_b = t.intern_type_list(&b);
    let mut collisions = InstantiationCache::default();
    for (list, value) in [(list_a, a[0]), (list_b, b[0])] {
        collisions.insert(target, InstantiationKey::plain(list), value, Some(7));
    }
    for args in [a, b] {
        assert_eq!(
            collisions
                .find_arguments(target, false, &args, 7)
                .unwrap()
                .value,
            args[0]
        );
    }

    assert!(collisions.find_arguments(a[2], false, &a, 7).is_none());
    assert!(collisions.find_arguments(target, true, &a, 7).is_none());

    t.instantiation_seed(target, |_| InstantiationKey::plain(list_a));
    let lists = t.list_ids.len();
    assert_eq!(t.create_type_reference(target, &a), target);
    t.instantiation_insert(target, InstantiationKey::plain(list_a), a[2]);
    assert_eq!(
        t.lookup_instantiation(target, &a, None, Some(&[]), false)
            .ok(),
        Some(a[2])
    );
    assert_eq!(t.list_ids.len(), lists);
    let count = t.types.len();
    let mut worker = || {
        let miss = t
            .lookup_instantiation(target, &b[..2], None, None, true)
            .unwrap_err();
        let key = miss.key;
        // The miss interned its arguments: the key names that list.
        assert_eq!(t.list_ids.len(), lists + 1);
        assert_eq!(t.list(key.arguments), &b[..2]);
        let other = t.intern_type_list(&a[..2]);
        assert_ne!(other, key.arguments);
        assert_eq!(t.list_ids.len(), lists + 2);
        Err::<(), _>(key)
    };
    let failed_key = worker().unwrap_err();
    assert_eq!(t.instantiation_get(target, failed_key), None);
    assert_eq!(t.types.len(), count);
    assert_eq!(t.list_ids.len(), lists + 2);

    // An ID-only insertion can precede the allocation of that list: the id
    // is the handle the next interned list (one item, sixteen bytes) takes.
    let future = InstantiationKey::plain(
        TypeListId::from_position(t.arena.next_raw(16).expect("room in the run"))
            .expect("a nonzero handle"),
    );
    t.instantiation_insert(target, future, b[2]);
    assert_eq!(t.create_type_reference(target, &b[..1]), b[2]);
    t.instantiation_insert(target, future, b[0]);
    assert_eq!(t.create_type_reference(target, &b[..1]), b[0]);

    assert!(!t.instantiations.full.contains_key(&(target, future)));
    let lists = t.list_ids.len();
    let outer = t
        .lookup_instantiation(target, &a[..1], None, None, false)
        .unwrap_err();
    assert_eq!(t.list_ids.len(), lists + 1);
    assert_eq!(t.list(outer.key.arguments), &a[..1]);
    let old_capacity = t.instantiations.plain.capacity();
    t.instantiations.reserve(old_capacity + 1);
    assert!(t.instantiations.plain.capacity() > old_capacity);
    let inner = t
        .lookup_instantiation(target, &a[..1], None, None, false)
        .unwrap_err();
    t.publish_instantiation(inner, b[1]);
    assert_eq!(t.create_type_reference(target, &a[..1]), b[1]);
    assert_eq!(t.instantiation_front_get(target, &a[..1]), Some(b[1]));
    let len = t.instantiations.len();
    t.publish_instantiation(outer, b[0]);
    assert_eq!(t.instantiations.len(), len);
    assert_eq!(t.instantiation_front_get(target, &a[..1]), None);
    assert_eq!(t.create_type_reference(target, &a[..1]), b[0]);
    // The publications interned nothing; a new sequence is a new list.
    let lists = t.list_ids.len();
    let last = t.intern_type_list(&a[2..]);
    assert_eq!(t.list_ids.len(), lists + 1);
    assert_eq!(t.list(last), &a[2..]);
}

/// getObjectTypeInstantiation's seed is asked of the map once a target: the
/// first call writes the target under its own key unless that key is taken,
/// a later call neither reads the key nor touches the map, and each target
/// has its own bit — below and past the words written so far.
#[test]
fn an_instantiation_target_is_seeded_once() {
    let mut t = tables();
    let key = |list: u32, alias: Option<u32>| InstantiationKey {
        arguments: TypeListId::new(list),
        alias_symbol: alias.map(SymbolId::new),
        alias_arguments: None,
        single_signature: false,
    };
    let (near, far, taken) = (TypeId::new(5), TypeId::new(64 * 9 + 63), TypeId::new(6));
    let other = TypeId::new(77);

    // The first instantiation of a target seeds its own key with itself.
    t.instantiation_seed(near, |_| key(3, None));
    assert_eq!(t.instantiation_get(near, key(3, None)), Some(near));
    assert_eq!(t.instantiations.len(), 1);

    // A later one does not ask for the key and leaves the map alone, even
    // where the entry has been replaced since.
    t.instantiation_insert(near, key(3, None), other);
    t.instantiation_seed(near, |_| {
        unreachable!("a seeded target's key is not read again")
    });
    assert_eq!(t.instantiation_get(near, key(3, None)), Some(other));
    assert_eq!(t.instantiations.len(), 1);

    // A key that is taken when the target is first instantiated stays.
    t.instantiation_insert(taken, key(4, Some(7)), other);
    t.instantiation_seed(taken, |_| key(4, Some(7)));
    assert_eq!(t.instantiation_get(taken, key(4, Some(7))), Some(other));
    t.instantiation_seed(taken, |_| {
        unreachable!("a seeded target's key is not read again")
    });
    assert_eq!(t.instantiations.len(), 2);

    // Each target has its own bit: the neighbour in the same word and a
    // target past the words written so far are seeded when their turn comes.
    t.instantiation_seed(far, |_| key(3, None));
    assert_eq!(t.instantiation_get(far, key(3, None)), Some(far));
    t.instantiation_seed(far, |_| {
        unreachable!("a seeded target's key is not read again")
    });
    let neighbour = TypeId::new(4);
    assert_eq!(t.instantiation_get(neighbour, key(3, None)), None);
    t.instantiation_seed(neighbour, |_| key(3, None));
    assert_eq!(
        t.instantiation_get(neighbour, key(3, None)),
        Some(neighbour)
    );
    assert_eq!(t.instantiations.len(), 4);
}

/// The front is one page for a checker of any program: 1,024 slots of
/// sixteen bytes, allocated with the first entry, whatever the reservation
/// says of the program's size. The reserved bytes the tables report count
/// its slots from the start, and every key's slot lies inside it.
#[test]
fn the_instantiation_front_is_one_page_whatever_the_reservation() {
    assert_eq!(InstantiationFront::SLOTS, 1_024);
    let front_bytes = InstantiationFront::SLOTS * std::mem::size_of::<[u32; 4]>();
    assert_eq!(front_bytes, 16 << 10);

    // A program of 400 symbols a checker, and one of four million.
    for (nodes, symbols) in [(12_800, 400), (1 << 24, 1 << 22)] {
        let mut t = tables();
        let before = t.reserved_lookup_bytes();
        assert!(before >= front_bytes);
        t.reserve_capacity_hint(nodes, symbols);
        assert!(t.reserved_lookup_bytes() >= before);
        assert!(t.instantiations.front.slots.is_empty());
        let (string, number) = (t.intrinsics.string, t.intrinsics.number);
        let target = t.intrinsics.any;
        let one = t.create_type_reference(target, &[string]);
        let two = t.create_type_reference(target, &[string, number]);
        assert_eq!(
            t.instantiations.front.slots.len(),
            InstantiationFront::SLOTS
        );
        assert_eq!(t.instantiations.front.memory_usage(), front_bytes);
        assert_eq!(t.instantiation_front_get(target, &[string]), Some(one));
        assert_eq!(
            t.instantiation_front_get(target, &[string, number]),
            Some(two)
        );
    }

    for index in 0..4_096u32 {
        let key = [index + 1, index.wrapping_mul(2_654_435_761) | 1, index % 3];
        assert!(InstantiationFront::slot(key) < InstantiationFront::SLOTS);
    }
}

/// The front of the instantiations map answers the plain key of one or two
/// arguments from the arguments themselves and only repeats the map: a
/// reference found or made through the map is answered by the front
/// afterwards, without another list or map entry; keys of no argument or of
/// three are left to the map; a slot holds one key (another key of the same
/// slot replaces it, and both are still answered by the map); and when the
/// map changes an answer the front forgets what it held.
#[test]
fn the_instantiation_front_repeats_the_map_for_one_or_two_arguments() {
    let mut t = tables();
    let (string, number, boolean) = (
        t.intrinsics.string,
        t.intrinsics.number,
        t.intrinsics.boolean,
    );
    let target = t.intrinsics.any;

    // Nothing is held before the map has answered.
    assert_eq!(t.instantiation_front_get(target, &[string]), None);
    let one = t.create_type_reference(target, &[string]);
    let two = t.create_type_reference(target, &[string, number]);
    let swapped = t.create_type_reference(target, &[number, string]);
    assert!(one != two && two != swapped && one != swapped);
    assert_eq!(t.instantiation_front_get(target, &[string]), Some(one));
    assert_eq!(
        t.instantiation_front_get(target, &[string, number]),
        Some(two)
    );
    assert_eq!(
        t.instantiation_front_get(target, &[number, string]),
        Some(swapped)
    );
    assert_eq!(t.instantiation_front_get(number, &[string]), None);

    // A reference that exists is answered without another list or entry.
    let (lists, entries) = (t.list_ids.len(), t.instantiations.len());
    assert_eq!(t.create_type_reference(target, &[string]), one);
    assert_eq!(t.create_type_reference(target, &[string, number]), two);
    assert_eq!((t.list_ids.len(), t.instantiations.len()), (lists, entries));

    // Keys the front does not take are the map's alone.
    let none = t.create_type_reference(target, &[]);
    let three = t.create_type_reference(target, &[string, number, boolean]);
    assert_eq!(t.instantiation_front_get(target, &[]), None);
    assert_eq!(
        t.instantiation_front_get(target, &[string, number, boolean]),
        None
    );
    assert_eq!(t.create_type_reference(target, &[]), none);
    assert_eq!(
        t.create_type_reference(target, &[string, number, boolean]),
        three
    );
    assert_eq!(InstantiationFront::key(target, &[]), None);
    assert_eq!(
        InstantiationFront::key(target, &[string, number, boolean]),
        None
    );

    // One key a slot: two keys of the same slot replace each other in the
    // front, and the map answers both.
    let first = InstantiationFront::key(target, &[string]).unwrap();
    let rival = (0..u32::MAX - 1)
        .map(TypeId::new)
        .find(|&candidate| {
            candidate != string
                && InstantiationFront::key(target, &[candidate]).is_some_and(|key| {
                    InstantiationFront::slot(key) == InstantiationFront::slot(first)
                })
        })
        .unwrap();
    t.instantiation_front_put(target, &[rival], two);
    assert_eq!(t.instantiation_front_get(target, &[rival]), Some(two));
    assert_eq!(t.instantiation_front_get(target, &[string]), None);
    assert_eq!(t.create_type_reference(target, &[string]), one);
    assert_eq!(t.instantiation_front_get(target, &[string]), Some(one));
    assert_eq!(t.instantiation_front_get(target, &[rival]), None);

    // The front repeats the map: a changed answer empties it.
    let list = t.intern_type_list(&[string, number]);
    assert_eq!(
        t.instantiation_front_get(target, &[string, number]),
        Some(two)
    );
    t.instantiation_insert(target, InstantiationKey::plain(list), two);
    assert_eq!(
        t.instantiation_front_get(target, &[string, number]),
        Some(two)
    );
    t.instantiation_insert(target, InstantiationKey::plain(list), three);
    assert_eq!(t.instantiation_front_get(target, &[string, number]), None);
    assert_eq!(t.instantiation_front_get(target, &[string]), None);
    assert_eq!(t.create_type_reference(target, &[string, number]), three);
    assert_eq!(
        t.instantiation_front_get(target, &[string, number]),
        Some(three)
    );
}

#[test]
fn type_list_interning_keeps_four_byte_candidates_and_exact_boundary_items() {
    assert_eq!(std::mem::size_of::<TypeListId>(), 4);
    let mut tables = tables();
    let items = [TypeId::new(0), TypeId::new(u32::MAX - 1)];
    let id = tables.intern_type_list(&items);
    assert_eq!(tables.list(id), &items);
    assert_eq!(tables.intern_type_list(&items), id);
    let reversed = [items[1], items[0]];
    assert_ne!(tables.intern_type_list(&reversed), id);
    assert_ne!(tables.intern_type_list(&items[..1]), id);
    for index in 1..1024 {
        tables.intern_type_list(&[TypeId::new(index)]);
    }
    assert_eq!(tables.list(id), &items);
    assert_eq!(tables.intern_type_list(&items), id);
    let _: &hashbrown::HashTable<TypeListId> = &tables.list_ids;
}

#[test]
fn single_string_template_spans_keep_canonical_text_and_literal_identity() {
    let mut t = tables();
    let values = [
        vec![],
        vec![b'A' as u16, 0, b'B' as u16],
        "ΟΣß𐐨".encode_utf16().collect(),
        vec![0xD800],
        vec![0xDC00],
        vec![0xD800, b'x' as u16, 0xDC00],
        vec![0xD800, 0xDC00],
    ];
    for value in values {
        let input = t.get_string_literal_type_from_utf16(&value);
        let fresh = t.get_fresh_type_of_literal_type(input);
        for (prefix, suffix) in [("on", "!"), ("", ""), ("left", "")] {
            let mut expected = prefix.encode_utf16().collect::<Vec<_>>();
            expected.extend_from_slice(&value);
            expected.extend(suffix.encode_utf16());
            let expected = t.get_string_literal_type_from_utf16(&expected);
            let texts = [
                TemplateText::from_utf8(prefix),
                TemplateText::from_utf8(suffix),
            ];
            let count = t.len();
            for ty in [input, fresh] {
                for _ in 0..3 {
                    assert_eq!(
                        t.try_get_template_literal_type_from_texts(&texts, &[ty]),
                        Ok(expected)
                    );
                    assert_eq!(t.len(), count);
                }
            }
        }
    }
}

#[test]
fn single_string_template_spans_join_surrogates_across_non_ascii_boundaries() {
    let mut t = tables();
    let low = t.get_string_literal_type_from_utf16(&[0xDC00]);
    let expected = t.get_string_literal_type_from_utf16(&[0xD800, 0xDC00]);
    let texts = [TemplateText::from_utf16(&[0xD800]), TemplateText::default()];
    assert_eq!(
        t.get_template_literal_type_from_texts(&texts, &[low]),
        expected
    );
    let input = t.get_string_literal_type("value");
    let expected = t.get_string_literal_type("évalue界");
    let texts = [TemplateText::from_utf8("é"), TemplateText::from_utf8("界")];
    assert_eq!(
        t.get_template_literal_type_from_texts(&texts, &[input]),
        expected
    );
}

#[test]
fn single_string_template_span_obeys_the_text_limit_before_interning() {
    let mut t = tables();
    let text = "x".repeat(MAX_TEMPLATE_LITERAL_TYPE_LENGTH);
    let value = t.get_string_literal_type(text.as_str());
    let empty = [TemplateText::default(), TemplateText::default()];
    assert_eq!(
        t.try_get_template_literal_type_from_texts(&empty, &[value]),
        Ok(value)
    );
    let over = [TemplateText::from_utf8("a"), TemplateText::default()];
    let count = t.len();
    assert_eq!(
        t.try_get_template_literal_type_from_texts(&over, &[value]),
        Err(TemplateLiteralTooLarge)
    );
    assert_eq!(t.len(), count);
    let small = t.get_string_literal_type("y");
    assert_eq!(
        t.get_template_literal_type_from_texts(&over, &[small]),
        t.get_string_literal_type("ay")
    );
}

#[test]
fn single_string_template_spans_return_plain_literals_for_enum_inputs() {
    let mut t = tables();
    let input = t.get_enum_literal_type(
        LiteralValue::String("value".into()),
        SymbolId::new(20),
        SymbolId::new(21),
    );
    let fresh = t.get_fresh_type_of_literal_type(input);
    let expected = t.get_string_literal_type("onvalue!");
    let texts = [TemplateText::from_utf8("on"), TemplateText::from_utf8("!")];
    let count = t.len();
    for ty in [input, fresh] {
        assert_eq!(
            t.get_template_literal_type_from_texts(&texts, &[ty]),
            expected
        );
        assert!(t.flags_of(ty).intersects(TypeFlags::ENUM_LITERAL));
        assert_eq!(t.len(), count);
    }
    assert!(!t.flags_of(expected).intersects(TypeFlags::ENUM_LITERAL));
}

#[test]
fn template_string_spans_widen_ascii_in_bulk_at_vector_boundaries() {
    let mut t = tables();
    for length in [0, 1, 7, 8, 15, 16, 31, 32, 63, 64, 65, 127] {
        for tail in ["", "\u{00e9}", "\u{1f600}"] {
            let mut input = "a".repeat(length);
            input.push('\0');
            input.push_str(tail);
            let left = t.get_string_literal_type(input.as_str());
            let right = t.get_string_literal_type("b");
            let expected = t.get_string_literal_type(format!("é{input}界b").as_str());
            let texts = [
                TemplateText::from_utf8("é"),
                TemplateText::from_utf8("界"),
                TemplateText::default(),
            ];
            assert_eq!(
                t.get_template_literal_type_from_texts(&texts, &[left, right]),
                expected
            );
        }
    }
}

#[test]
fn literal_type_value_comparison_preserves_ieee_values_kinds_and_fresh_twins() {
    let mut t = tables();
    let zero = t.get_number_literal_type(0.0);
    let negative_zero = t.get_number_literal_type(-0.0);
    let one = t.get_number_literal_type(1.0);
    let infinity = t.get_number_literal_type(f64::INFINITY);
    let nan = t.get_number_literal_type(f64::NAN);
    let other_nan = t.get_number_literal_type(f64::from_bits(0x7ff8_0000_0000_0001));
    let string = t.get_string_literal_type("1");
    let high = t.get_string_literal_type_from_utf16(&[0xD800]);
    let replacement = t.get_string_literal_type("\u{fffd}");
    let bigint = t.get_bigint_literal_type(PseudoBigInt {
        negative: false,
        base10_value: "1".into(),
    });
    let other_bigint = t.get_bigint_literal_type(PseudoBigInt {
        negative: true,
        base10_value: "1".into(),
    });
    let numeric_enum = t.get_enum_literal_type(
        LiteralValue::Number(1.0),
        SymbolId::new(20),
        SymbolId::new(21),
    );
    let string_enum = t.get_enum_literal_type(
        LiteralValue::String("1".into()),
        SymbolId::new(22),
        SymbolId::new(23),
    );
    let fresh = t.get_fresh_type_of_literal_type(one);
    let fresh_enum = t.get_fresh_type_of_literal_type(numeric_enum);
    let count = t.len();
    for (a, b, equal) in [
        (zero, negative_zero, true),
        (zero, one, false),
        (one, fresh, true),
        (infinity, infinity, true),
        (nan, nan, false),
        (nan, other_nan, false),
        (one, numeric_enum, true),
        (one, fresh_enum, true),
        (string, string_enum, true),
        (high, high, true),
        (high, replacement, false),
        (bigint, bigint, true),
        (bigint, other_bigint, false),
        (one, string, false),
        (one, bigint, false),
        (string, bigint, false),
        (one, t.intrinsics.number, false),
        (string, t.intrinsics.string, false),
        (t.intrinsics.true_regular, t.intrinsics.true_regular, false),
    ] {
        assert_eq!(t.literal_type_values_equal(a, b), equal);
        assert_eq!(t.literal_type_values_equal(b, a), equal);
        assert_eq!(t.len(), count);
    }
}

#[test]
fn a_cloned_type_parameter_keeps_its_target_and_mapper_in_its_record() {
    let mut t = tables();
    let parameter = |target| TypeData::TypeParameter {
        is_this_type: false,
        is_distributed: false,
        constraint: None,
        target,
        mapper: None,
    };
    let declared = t.create_type(TypeFlags::TYPE_PARAMETER, parameter(None));
    let clone = t.create_type(TypeFlags::TYPE_PARAMETER, parameter(Some(declared)));
    // A declared parameter and a type of another kind have neither.
    for ty in [declared, t.intrinsics.any] {
        assert_eq!(t.type_parameter_target(ty), None);
        assert_eq!(t.type_parameter_mapper(ty), None);
    }
    assert_eq!(t.type_parameter_target(clone), Some(declared));
    assert_eq!(t.type_parameter_mapper(clone), None);
    t.set_type_parameter_mapper(clone, MapperId::new(7));
    assert_eq!(t.type_parameter_mapper(clone), Some(MapperId::new(7)));
    assert_eq!(t.type_parameter_target(clone), Some(declared));
    // Both live in the 32-byte record: no table beside the type store.
    assert_eq!(std::mem::size_of::<Type>(), 32);
}

#[test]
fn an_instantiated_objects_target_and_mapper_live_in_its_record() {
    let mut tables = tables();
    let target = tables.create_type(TypeFlags::OBJECT, TypeData::PLAIN_OBJECT);
    // A plain object has neither, and the store's all-zero record is one.
    assert_eq!(tables.type_of(target).instantiated_target(), None);
    assert_eq!(tables.type_of(target).instantiated_mapper(), None);
    assert_eq!(
        TypeData::PLAIN_OBJECT,
        TypeData::Object {
            node_position: 0,
            instantiated_target: None,
            instantiated_mapper: None,
        }
    );
    // An instantiated shell is created with both.
    let shell = tables.create_type(
        TypeFlags::OBJECT,
        TypeData::Object {
            node_position: 0,
            instantiated_target: Some(target),
            instantiated_mapper: Some(MapperId::new(7)),
        },
    );
    assert_eq!(tables.type_of(shell).instantiated_target(), Some(target));
    assert_eq!(
        tables.type_of(shell).instantiated_mapper(),
        Some(MapperId::new(7))
    );
    // Stamping the instantiation expression's node leaves them as they are.
    let TypeData::Object { node_position, .. } = &mut tables.type_mut(shell).data else {
        unreachable!();
    };
    *node_position = 41;
    assert_eq!(tables.type_of(shell).deferred_node_position(), 41);
    assert_eq!(tables.type_of(shell).instantiated_target(), Some(target));
    // An isolated single-signature type gets a mapper and no target.
    let isolated = tables.create_type(TypeFlags::OBJECT, TypeData::PLAIN_OBJECT);
    tables
        .type_mut(isolated)
        .set_isolated_signature_mapper(MapperId::new(9));
    assert_eq!(tables.type_of(isolated).instantiated_target(), None);
    assert_eq!(
        tables.type_of(isolated).instantiated_mapper(),
        Some(MapperId::new(9))
    );
    // Other kinds of type have neither.
    let string = tables.intrinsics.string;
    assert_eq!(tables.type_of(string).instantiated_target(), None);
    assert_eq!(tables.type_of(string).instantiated_mapper(), None);
    // Both live in the 32-byte record: no table beside the type store.
    assert_eq!(std::mem::size_of::<Type>(), 32);
}

#[test]
#[should_panic(expected = "isolated-signature mapper written over instantiation links")]
fn an_isolated_signature_mapper_is_not_written_over_an_instantiation() {
    let mut tables = tables();
    let target = tables.create_type(TypeFlags::OBJECT, TypeData::PLAIN_OBJECT);
    let shell = tables.create_type(
        TypeFlags::OBJECT,
        TypeData::Object {
            node_position: 0,
            instantiated_target: Some(target),
            instantiated_mapper: Some(MapperId::new(7)),
        },
    );
    tables
        .type_mut(shell)
        .set_isolated_signature_mapper(MapperId::new(9));
}

#[test]
fn reference_clones_copy_the_argument_slot_and_start_without_a_node() {
    let mut tables = tables();
    let target = tables.create_type(TypeFlags::OBJECT, TypeData::PLAIN_OBJECT);
    let string = tables.intrinsics.string;
    let eager = tables.create_type_reference(target, &[string]);
    assert_eq!(tables.type_of(eager).deferred_node_position(), 0);
    let source = tables.create_deferred_reference_shell(target);
    let TypeData::Reference { node_position, .. } = &mut tables.type_mut(source).data else {
        unreachable!();
    };
    *node_position = u32::MAX;
    let unresolved = tables.clone_type_reference(source);
    assert_eq!(tables.type_of(unresolved).deferred_node_position(), 0);
    assert_eq!(tables.try_type_argument_list(unresolved), None);
    tables.set_resolved_type_arguments(source, &[string]);
    let resolved = tables.clone_type_reference(source);
    assert_eq!(tables.type_of(source).deferred_node_position(), u32::MAX);
    assert_eq!(tables.type_of(resolved).deferred_node_position(), 0);
    assert_eq!(tables.reference_target(resolved), target);
    assert_eq!(
        tables.try_type_argument_list(resolved),
        tables.try_type_argument_list(eager)
    );
}

#[test]
fn plain_union_lookup_checks_colliding_lists_and_keeps_first_creation() {
    let mut t = tables();
    let (a, b) = (
        t.get_string_literal_type("aa"),
        t.get_string_literal_type("bb"),
    );
    let members = [a, b];
    let reversed = [b, a];
    let other = t.create_origin_union_or_intersection_type(TypeFlags::UNION, reversed);
    let reversed_id = t.intern_type_list(&reversed);
    let hash = hash_type_list(&members);
    // Give a different ordered list the very same hash as the query.
    t.plain_unions.insert_unique(
        hash,
        PlainUnion {
            hash,
            members: reversed_id,
            ty: other,
        },
        |e| e.hash,
    );

    let args = [a, b, a];
    let lists = t.list_ids.len();
    let count = t.len();
    let union = t.get_union_type_from_sorted_list(
        members,
        ObjectFlags::PRIMITIVE_UNION,
        None,
        Some(&args),
        None,
    );
    assert_eq!(t.len(), count + 1);
    assert_eq!(t.members_of(union), members);
    // The call interned the members and the alias arguments.
    let interned = t.list_ids.len();
    assert!(interned >= lists + 2);
    let members_list = t.intern_type_list(&members);
    assert_eq!(t.list_ids.len(), interned);
    assert_eq!(t.list(members_list), &members[..]);
    assert_eq!(
        t.list(t.alias_type_argument_list(union).unwrap()),
        &args[..]
    );
    assert_eq!(t.alias_type_arguments(union), Some(&args[..]));

    for i in 0..128 {
        let literal = t.get_number_literal_type(f64::from(i));
        t.get_union_type_from_sorted_list([a, literal], ObjectFlags::NONE, None, None, None);
    }
    let lists = t.list_ids.len();
    let count = t.len();
    assert_eq!(
        t.get_union_type_from_sorted_list(members, ObjectFlags::NONE, None, Some(&[b, a, b]), None),
        union
    );
    assert_eq!(t.list_ids.len(), lists);
    assert_eq!(t.len(), count);
    assert!(t
        .object_flags_of(union)
        .intersects(ObjectFlags::PRIMITIVE_UNION));
    assert_eq!(t.alias_type_arguments(union), Some(&args[..]));

    let alias = Some(SymbolId::new(17));
    let aliased = t.get_union_type_from_sorted_list(members, ObjectFlags::NONE, alias, None, None);
    let with_origin =
        t.get_union_type_from_sorted_list(members, ObjectFlags::NONE, None, None, Some(aliased));
    assert_ne!(aliased, union);
    assert_ne!(with_origin, union);
    t.release_scalar_lookup_buffers();
    assert_eq!(t.plain_unions.capacity(), 0);
}

#[test]
fn one_sorted_named_union_can_reuse_members_without_a_set() {
    let mut t = tables();
    let ids: Vec<_> = (0..10)
        .map(|n| t.get_number_literal_type(f64::from(n)))
        .collect();
    let alias = Some(SymbolId::new(7));
    for size in [2, 8, 10] {
        let members = &ids[..size];
        let named =
            t.get_union_type_from_sorted_list(members, ObjectFlags::NONE, alias, None, None);
        let mut scratch = UnionScratch::default();
        let count = t.len();
        let result = t.named_union_origin(None, members, &[named], None, &mut scratch);
        assert!(matches!(result, NamedUnionOrigin::Existing(id) if id == named));
        assert_eq!(t.len(), count);
        assert!(scratch.named_members.is_empty());
        if size <= 8 {
            let mut scratch = UnionScratch::default();
            let reduced = &members[..size - 1];
            let result = t.named_union_origin(None, reduced, &[named], None, &mut scratch);
            assert!(matches!(result, NamedUnionOrigin::Existing(id) if id == named));
            assert!(scratch.named_members.is_empty());
        }
        let mut scratch = UnionScratch::default();
        let result = t.named_union_origin(None, members, &[named], alias, &mut scratch);
        let NamedUnionOrigin::Created(origin) = result else {
            panic!("expected alias origin")
        };
        assert_eq!(t.members_of(origin), &[named]);
        assert_eq!(t.len(), count + 1);
    }
}

/// Until an entry of the plain shape is written without its hash, `full`
/// holds no plain key: a plain miss is answered without it, and once such an
/// entry exists it is found (with or without a hash) and moved by its
/// hashed write.
#[test]
fn a_plain_key_written_before_its_hash_is_found_and_moved() {
    let mut t = tables();
    let target = t.intrinsics.any;
    let args: [TypeId; 2] = std::array::from_fn(|_| t.create_type_parameter(false, None));
    let key = InstantiationKey::plain(t.intern_type_list(&args));
    // A table hash as the cache makes them (its 32 bits spread).
    let hash = spread_instantiation_hash(9);
    let mut cache = InstantiationCache::default();
    assert!(!cache.full_holds_plain);
    assert_eq!(cache.get(target, key, Some(hash)), None);
    cache.insert(target, key, args[0], None);
    assert!(cache.full_holds_plain);
    assert_eq!(cache.get(target, key, Some(hash)), Some(args[0]));
    assert_eq!(cache.get(target, key, None), Some(args[0]));
    cache.insert(target, key, args[1], Some(hash));
    assert!(cache.full.is_empty());
    assert_eq!(cache.get(target, key, Some(hash)), Some(args[1]));
}

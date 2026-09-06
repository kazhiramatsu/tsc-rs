use super::*;

fn tables() -> TypeTables {
    TypeTables::new(/*strict_null_checks*/ true, /*eopt*/ false)
}

#[test]
fn intrinsics_are_allocated_in_tsc_order() {
    let t = tables();
    // anyType is the first allocation, like tsc typeCount order.
    assert_eq!(t.intrinsics.any, TypeId(0));
    assert!(t.intrinsics.unknown < t.intrinsics.undefined);
    assert!(t.intrinsics.false_regular < t.intrinsics.true_fresh);
    // strictNullChecks aliases the widening variants (47033/47050).
    assert_eq!(t.intrinsics.undefined_widening, t.intrinsics.undefined);
    assert_eq!(t.intrinsics.null_widening, t.intrinsics.null);
    // exactOptionalPropertyTypes off aliases undefinedOrMissing.
    assert_eq!(t.intrinsics.undefined_or_missing, t.intrinsics.undefined);

    let loose = TypeTables::new(false, false);
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
        TypeData::TemplateLiteral { texts, types } => {
            assert_eq!(texts.len(), 2);
            assert!(texts.iter().all(|text| text.is_empty()));
            assert_eq!(types.len(), 1);
            assert_eq!(types[0], t.intrinsics.number);
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
        types.as_ref(),
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
    assert_eq!(t.get_regular_type_of_literal_type(fresh), one_a);

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
    let a = t.get_union_type(&[one, two], UnionReduction::Literal);
    let b = t.get_union_type(&[two, one], UnionReduction::Literal);
    assert_eq!(a, b);
    // Flattening: (1 | 2) | 2 == 1 | 2.
    assert_eq!(t.get_union_type(&[a, two], UnionReduction::Literal), a);
    // Singletons collapse; empties are never.
    assert_eq!(t.get_union_type(&[one], UnionReduction::Literal), one);
    assert_eq!(
        t.get_union_type(&[], UnionReduction::Literal),
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
        t.get_union_type(&[a, t.intrinsics.string], UnionReduction::Literal),
        t.intrinsics.string
    );
    assert_eq!(
        t.get_union_type(&[one, t.intrinsics.number], UnionReduction::Literal),
        t.intrinsics.number
    );
    // Fresh literal folds into its regular partner.
    let fresh = t.get_fresh_type_of_literal_type(one);
    assert_eq!(
        t.get_union_type(&[fresh, one], UnionReduction::Literal),
        one
    );
    // UnionReduction::None keeps the subsumed literal.
    let unreduced = t.get_union_type(&[a, t.intrinsics.string], UnionReduction::None);
    let TypeData::Union { types, .. } = &t.type_of(unreduced).data else {
        panic!("unreduced union stays a union");
    };
    assert_eq!(types.len(), 2);
}

#[test]
fn union_any_unknown_absorption() {
    let mut t = tables();
    let string = t.intrinsics.string;
    assert_eq!(
        t.get_union_type(&[t.intrinsics.any, string], UnionReduction::Literal),
        t.intrinsics.any
    );
    assert_eq!(
        t.get_union_type(&[t.intrinsics.unknown, string], UnionReduction::Literal),
        t.intrinsics.unknown
    );
    assert_eq!(
        t.get_union_type(&[t.intrinsics.wildcard, string], UnionReduction::Literal),
        t.intrinsics.wildcard
    );
    assert_eq!(
        t.get_union_type(&[t.intrinsics.error, string], UnionReduction::Literal),
        t.intrinsics.error
    );
    // never members vanish.
    assert_eq!(
        t.get_union_type(&[t.intrinsics.never, string], UnionReduction::Literal),
        string
    );
}

#[test]
fn union_folds_nullable_members_without_strict_null_checks() {
    let mut loose = TypeTables::new(false, false);
    let number = loose.intrinsics.number;
    let null = loose.intrinsics.null;
    // number | null collapses to number at construction (61347-61349).
    assert_eq!(
        loose.get_union_type(&[number, null], UnionReduction::Literal),
        number
    );
    // All-nullable sets fold to the (non-)widening singletons.
    assert_eq!(loose.get_union_type(&[null], UnionReduction::Literal), null);
    // A widening null plus a NON-widening undefined: the
    // IncludesNonWideningType bit is global, so the null branch
    // returns the non-widening nullType (61566-61568).
    let widening = loose.intrinsics.null_widening;
    assert_eq!(
        loose.get_union_type(
            &[widening, loose.intrinsics.undefined],
            UnionReduction::Literal
        ),
        loose.intrinsics.null
    );
    assert_eq!(
        loose.get_union_type(&[widening, widening], UnionReduction::Literal),
        widening
    );
    // Under strictNullChecks nullable members stay.
    let mut strict = tables();
    let strict_union = strict.get_union_type(
        &[strict.intrinsics.number, strict.intrinsics.null],
        UnionReduction::Literal,
    );
    assert!(strict.flags_of(strict_union).intersects(TypeFlags::UNION));
}

#[test]
fn union_dedups_missing_against_undefined() {
    // exactOptionalPropertyTypes tables: undefinedOrMissing = missing.
    let mut t = TypeTables::new(true, true);
    let missing = t.intrinsics.missing;
    let undefined = t.intrinsics.undefined;
    assert_ne!(missing, undefined);
    // undefined | missing folds to undefined (61540-61544).
    assert_eq!(
        t.get_union_type(&[undefined, missing], UnionReduction::Literal),
        undefined
    );
}

#[test]
fn two_union_fast_path_caches_by_reduction() {
    let mut t = tables();
    let one = t.get_number_literal_type(1.0);
    let two = t.get_number_literal_type(2.0);
    let string = t.intrinsics.string;
    let union = t.get_union_type(&[one, two], UnionReduction::Literal);
    let first = t.get_union_type(&[union, string], UnionReduction::Literal);
    let second = t.get_union_type(&[union, string], UnionReduction::Literal);
    assert_eq!(first, second);
    let reversed = t.get_union_type(&[string, union], UnionReduction::Literal);
    // Same worker result; the cache key is order-normalized.
    assert_eq!(first, reversed);
}

#[test]
fn named_union_members_denormalize_into_origin() {
    let mut t = tables();
    let one = t.get_number_literal_type(1.0);
    let two = t.get_number_literal_type(2.0);
    let named = t.get_union_type(&[one, two], UnionReduction::Literal);
    // Synthesize an alias (M4 machinery) to make the union "named".
    t.type_mut(named).alias_symbol = Some(crate::ty::SymbolId(0));
    // A union containing ONLY the named union returns it unchanged.
    let string = t.intrinsics.string;
    let widened = t.get_union_type(&[named, string], UnionReduction::Literal);
    let TypeData::Union { types, origin } = &t.type_of(widened).data else {
        panic!("union expected");
    };
    // typeSet is id-sorted: the string intrinsic precedes the
    // literal types allocated by this test.
    assert_eq!(types.as_ref(), [string, one, two]);
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
    assert_eq!(origin_types.as_ref(), [string, named]);
}

#[test]
fn get_type_list_id_compresses_consecutive_ids() {
    let t = tables();
    assert_eq!(
        t.get_type_list_id(&[TypeId(5), TypeId(6), TypeId(7), TypeId(9)]),
        "5:3,9"
    );
    assert_eq!(t.get_type_list_id(&[TypeId(3)]), "3");
    assert_eq!(t.get_type_list_id(&[]), "");
}

#[test]
fn tuple_targets_intern_by_flags_and_readonly() {
    let mut t = tables();
    let req = [ElementFlags::REQUIRED, ElementFlags::OPTIONAL];
    let req_flags = TupleTargetFlags::new(&req).expect("not single-rest");
    let a = t.get_tuple_target_type(req_flags, false, None);
    let b = t.get_tuple_target_type(req_flags, false, None);
    let readonly = t.get_tuple_target_type(req_flags, true, None);
    assert_eq!(a, b);
    assert_ne!(a, readonly);
    let TypeData::TupleTarget(data) = &t.type_of(a).data else {
        panic!("tuple target expected");
    };
    assert_eq!(data.min_length, 1);
    assert_eq!(data.fixed_length, 2);
    assert!(!data.has_rest_element);

    let rest = [ElementFlags::REQUIRED, ElementFlags::REST];
    let with_rest = t.get_tuple_target_type(
        TupleTargetFlags::new(&rest).expect("not single-rest"),
        false,
        None,
    );
    let TypeData::TupleTarget(data) = &t.type_of(with_rest).data else {
        panic!("tuple target expected");
    };
    assert_eq!(data.min_length, 1);
    assert_eq!(data.fixed_length, 1);
    assert!(data.has_rest_element);
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
            let target = t.get_tuple_target_type(flags, readonly, None);
            let TypeData::TupleTarget(data) = &t.type_of(target).data else {
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
                t.get_union_type(&literals, UnionReduction::Literal)
            );
            assert_eq!(target, t.get_tuple_target_type(flags, readonly, None));
        }
    }
    for tail in [ElementFlags::REST, ElementFlags::VARIADIC] {
        let mut t = tables();
        let flags = [ElementFlags::REQUIRED, tail];
        let target = t.get_tuple_target_type(TupleTargetFlags::new(&flags).unwrap(), false, None);
        let TypeData::TupleTarget(data) = &t.type_of(target).data else {
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
        .create_tuple_type(&[string, boolean], None, false, None)
        .expect("inner tuple");
    // [number, ...[string, boolean]] normalizes to [number, string, boolean].
    let outer_flags = [ElementFlags::REQUIRED, ElementFlags::VARIADIC];
    let outer_target = t.get_tuple_target_type(
        TupleTargetFlags::new(&outer_flags).expect("not single-rest"),
        false,
        None,
    );
    let outer = t
        .create_normalized_tuple_type(outer_target, &[number, inner])
        .expect("normalized");
    let direct = t
        .create_tuple_type(&[number, string, boolean], None, false, None)
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
        &t.type_of(folded_surrogate).data,
        &TypeData::Literal {
            value: LiteralValue::String(TemplateText::from_utf16(&[0xD800, b'x' as u16])),
        }
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
fn mapped_types_carry_root_and_instantiation_identity() {
    let mut t = tables();
    let symbol = SymbolId(7);
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

    let mapper = MapperId(3);
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
        alias_symbol: Some(SymbolId(7)),
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
        Some(SymbolId(7)),
        Some(&[check]),
    );
    assert_eq!(t.flags_of(conditional), TypeFlags::CONDITIONAL);
    let TypeData::Conditional(data) = &t.type_of(conditional).data else {
        panic!("conditional constructor must retain its root");
    };
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
    let a = t.create_type(TypeFlags::OBJECT, TypeData::Object);
    let b = t.create_type(TypeFlags::OBJECT, TypeData::Object);
    assert_ne!(a, b);
}

#[test]
fn optionality_follows_strict_null_checks() {
    let mut t = tables();
    let number = t.intrinsics.number;
    let optional = t.add_optionality(number, /*is_property*/ true, true);
    let TypeData::Union { types, .. } = &t.type_of(optional).data else {
        panic!("optional property type must union undefined");
    };
    assert!(types.contains(&t.intrinsics.undefined));
    assert_eq!(t.add_optionality(number, true, false), number);

    let mut loose = TypeTables::new(false, false);
    let number = loose.intrinsics.number;
    assert_eq!(loose.add_optionality(number, true, true), number);
}

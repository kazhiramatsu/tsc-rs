use tsc_binder::bind_source_file;
use tsc_syntax::{parse_source_file, LanguageVariant, ParseOptions, SourceFile};
use tsc_types::{
    CheckFlags, CompilerOptions, ElementFlags, ObjectFlags, SignatureFlags, TypeData, TypeFlags,
    TypeId,
};

use crate::links::LinkSlot;
use crate::relpin::find_probe_annotation;
use crate::speculate::SpeculationOutcome;
use crate::state::CheckerState;
use crate::{check_program, InputFile};

fn parse(text: &str) -> SourceFile {
    let source = parse_source_file(
        "annotate-test.ts".to_owned(),
        text.to_owned(),
        ParseOptions {
            language_variant: LanguageVariant::Standard,
            javascript_file: false,
            ..ParseOptions::default()
        },
        None,
    );
    assert!(
        source.parse_diagnostics.is_empty(),
        "test source must parse cleanly: {:?}",
        source.parse_diagnostics
    );
    source
}

fn annotation_type(state: &mut CheckerState, name: &str) -> TypeId {
    let annotation =
        find_probe_annotation(state.binder.source(0), name).expect("declared var with annotation");
    state
        .get_type_from_type_node(annotation)
        .expect("annotation resolves in the M3 slice")
}

fn with_state<R>(text: &str, run: impl FnOnce(&mut CheckerState) -> R) -> R {
    let options = CompilerOptions::default();
    let source = parse(text);
    let binder = bind_source_file(&source, &options);
    let mut state = CheckerState::new(&source, &binder, &options);
    run(&mut state)
}

#[test]
fn checked_js_non_jsdoc_base_constructor_error_is_published() {
    let result = check_program(
        &[
            InputFile::new(
                "first.js".to_owned(),
                "class Drakkhen extends Dragon {}\n".to_owned(),
            ),
            InputFile::new(
                "second.ts".to_owned(),
                "function Dragon(numberEaten: number) { void numberEaten; }\n".to_owned(),
            ),
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            target: Some(2),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diag| (
                diag.code(),
                diag.file_name.as_deref().unwrap_or_default(),
                diag.start.unwrap_or(u32::MAX),
                diag.length.unwrap_or(u32::MAX),
            ))
            .collect::<Vec<_>>(),
        [(2507, "first.js", 23, 6)]
    );
}

#[test]
fn base_constructor_type_argument_count_error_uses_expression_span() {
    let text = "interface Base<T, U> {}\n\
                interface BaseConstructor { new <T, U>(): Base<T, U>; }\n\
                declare function getBase(): BaseConstructor;\n\
                class D extends getBase() <string, string, string> {}\n";
    let result = check_program(
        &[InputFile::new("a.ts".to_owned(), text.to_owned())],
        &CompilerOptions {
            target: Some(2),
            ..CompilerOptions::default()
        },
    );
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code() == 2508)
        .expect("base constructor type-argument count error");
    let expected_start = text
        .find("getBase() <")
        .expect("heritage expression")
        .try_into()
        .expect("test offset fits u32");
    assert_eq!(
        (diagnostic.start, diagnostic.length),
        (Some(expected_start), Some(9))
    );
    assert_eq!(
        diagnostic.message.text,
        "No base constructor has the specified number of type arguments."
    );
}

#[test]
fn union_annotation_reduces_string_literals_matched_by_templates() {
    with_state(
        "declare var a: \"abc\" | `a${string}`;\ndeclare var b: `a${string}`;\n",
        |state| {
            let a = annotation_type(state, "a");
            let b = annotation_type(state, "b");
            // The annotation path routes through the checker union
            // twin, so removeStringLiteralsMatchedByTemplateLiterals
            // collapses the union to the template itself, like
            // getUnionTypeWorker (61547-61549) does.
            assert_eq!(a, b);
        },
    );
}

#[test]
fn distinct_type_literals_are_distinct_types() {
    with_state(
        "declare var a: { x: number };\ndeclare var b: { x: number };\n",
        |state| {
            let a = annotation_type(state, "a");
            let b = annotation_type(state, "b");
            assert_ne!(a, b, "anonymous object types never intern structurally");
            // Re-reading the SAME node returns the cached type.
            assert_eq!(annotation_type(state, "a"), a);
        },
    );
}

#[test]
fn literal_type_nodes_yield_regular_literals() {
    with_state("declare var a: 1;\ndeclare var b: \"x\";\n", |state| {
        let one = annotation_type(state, "a");
        assert!(!state.tables.is_fresh_literal_type(one));
        assert_eq!(state.tables.get_regular_type_of_literal_type(one), one);
        let x = annotation_type(state, "b");
        assert!(state
            .tables
            .flags_of(x)
            .intersects(TypeFlags::STRING_LITERAL));
    });
}

#[test]
fn union_annotations_intern_by_member_set() {
    with_state("declare var a: 1 | 2;\ndeclare var b: 2 | 1;\n", |state| {
        let a = annotation_type(state, "a");
        let b = annotation_type(state, "b");
        assert_eq!(a, b);
        assert!(state.tables.flags_of(a).intersects(TypeFlags::UNION));
    });
}

#[test]
fn tuple_annotations_build_normalized_references() {
    with_state(
        "declare var a: [number, string?];\ndeclare var b: readonly [number];\ndeclare var c: [number, ...[string, boolean]];\ndeclare var d: [number, string, boolean];\n",
        |state| {
            let a = annotation_type(state, "a");
            assert!(state.tables.is_tuple_type(a));
            let target = state.tables.reference_target(a);
            let TypeData::TupleTarget(data) = &state.tables.type_of(target).data else {
                panic!("tuple reference targets a tuple target");
            };
            assert_eq!(
                data.element_flags.as_ref(),
                [ElementFlags::REQUIRED, ElementFlags::OPTIONAL]
            );
            assert_eq!(data.min_length, 1);
            assert!(!data.readonly);
            // Optional element type widened with undefined (strict).
            let args = state.tables.type_arguments(a).to_vec();
            assert!(state.tables.flags_of(args[1]).intersects(TypeFlags::UNION));

            let b = annotation_type(state, "b");
            let b_target = state.tables.reference_target(b);
            let TypeData::TupleTarget(data) = &state.tables.type_of(b_target).data else {
                panic!("tuple reference targets a tuple target");
            };
            assert!(data.readonly);

            // Variadic tuple spread normalizes to the flat tuple.
            assert_eq!(annotation_type(state, "c"), annotation_type(state, "d"));
        },
    );
}

#[test]
fn recursive_interfaces_resolve_declared_types_and_members() {
    with_state(
        "interface A { next: B }\ninterface B { next: A }\ndeclare var a: A;\ndeclare var b: B;\n",
        |state| {
            let a = annotation_type(state, "a");
            let b = annotation_type(state, "b");
            assert_ne!(a, b);
            assert!(state
                .tables
                .object_flags_of(a)
                .intersects(ObjectFlags::INTERFACE));
            let members = state
                .resolve_structured_type_members(a)
                .expect("interface members resolve");
            let members = state.members_of(members).clone();
            assert_eq!(members.properties.len(), 1);
            let next = members.properties[0];
            let next_type = state.get_type_of_symbol(next).expect("property type");
            assert_eq!(next_type, b, "A.next is B's declared type");
        },
    );
}

#[test]
fn method_members_get_anonymous_types_with_call_signatures() {
    with_state(
        "declare var a: { m(x: 1): void, p: (x: number) => void };\n",
        |state| {
            let a = annotation_type(state, "a");
            let members_id = state
                .resolve_structured_type_members(a)
                .expect("type literal members resolve");
            let members = state.members_of(members_id).clone();
            assert_eq!(members.properties.len(), 2);

            let method_type = state
                .get_type_of_symbol(members.properties[0])
                .expect("method type");
            let method_members_id = state
                .resolve_structured_type_members(method_type)
                .expect("method members resolve");
            let method_members = state.members_of(method_members_id).clone();
            assert_eq!(method_members.call_signatures.len(), 1);
            let signature = state
                .signature_of(method_members.call_signatures[0])
                .clone();
            assert!(signature.from_method);
            assert!(signature.flags.contains(SignatureFlags::HAS_LITERAL_TYPES));
            assert_eq!(signature.min_argument_count, 1);

            let property_type = state
                .get_type_of_symbol(members.properties[1])
                .expect("function property type");
            let property_members_id = state
                .resolve_structured_type_members(property_type)
                .expect("function type members resolve");
            let property_members = state.members_of(property_members_id).clone();
            assert_eq!(property_members.call_signatures.len(), 1);
            assert!(
                !state
                    .signature_of(property_members.call_signatures[0])
                    .from_method
            );
        },
    );
}

#[test]
fn index_signatures_produce_index_infos() {
    with_state(
        "declare var a: { readonly [k: string]: number };\ndeclare var b: { [k: symbol]: number };\n",
        |state| {
            for (name, key) in [("a", TypeFlags::STRING), ("b", TypeFlags::ES_SYMBOL)] {
                let ty = annotation_type(state, name);
                let members_id = state
                    .resolve_structured_type_members(ty)
                    .expect("index members resolve");
                let members = state.members_of(members_id).clone();
                assert_eq!(members.index_infos.len(), 1);
                assert!(state
                    .tables
                    .flags_of(members.index_infos[0].key_type)
                    .intersects(key));
                assert_eq!(members.index_infos[0].is_readonly, name == "a");
            }
        },
    );
}

#[test]
fn template_annotations_fold_literal_spans() {
    with_state(
        "declare var a: `a${string}`;\ndeclare var b: `a${\"b\"}c`;\n",
        |state| {
            let a = annotation_type(state, "a");
            assert!(state
                .tables
                .flags_of(a)
                .intersects(TypeFlags::TEMPLATE_LITERAL));
            let b = annotation_type(state, "b");
            assert_eq!(b, state.tables.get_string_literal_type("abc"));
        },
    );
}

#[test]
fn intersection_normalization_matches_tsc() {
    with_state(
        concat!(
            "declare var a: string & number;\n",
            "declare var b: 1 & 2;\n",
            "declare var c: \"a\" & string;\n",
            "declare var d: string & {};\n",
            "declare var e: unknown & string;\n",
            "declare var f: (\"a\" | \"b\") & string;\n",
            "declare var g: (string | undefined) & (number | undefined);\n",
            "declare var h: boolean & true;\n",
            "declare var i: null & number;\n",
        ),
        |state| {
            let never = state.tables.intrinsics.never;
            // DisjointDomains: string & number = never (step 2).
            assert_eq!(annotation_type(state, "a"), never);
            // Unit ∧ Unit quirk: 1 & 2 = never.
            assert_eq!(annotation_type(state, "b"), never);
            // Supertype reduction: "a" & string = "a".
            let c = annotation_type(state, "c");
            assert_eq!(c, state.tables.get_string_literal_type("a"));
            // string & {} keeps both members (noSupertypeReduction).
            let d = annotation_type(state, "d");
            let TypeData::Intersection { types } = &state.tables.type_of(d).data else {
                panic!("string & {{}} stays an intersection");
            };
            assert_eq!(types.len(), 2);
            // unknown vanishes from intersections.
            assert_eq!(annotation_type(state, "e"), state.tables.intrinsics.string);
            // Union distribution: ("a"|"b") & string = "a" | "b".
            let f = annotation_type(state, "f");
            let a_lit = state.tables.get_string_literal_type("a");
            let b_lit = state.tables.get_string_literal_type("b");
            let expected = state
                .tables
                .get_union_type(&[a_lit, b_lit], tsc_types::UnionReduction::Literal);
            assert_eq!(f, expected);
            // The undefined pull-out: (string|undefined) & (number|undefined)
            // = (string & number) | undefined = undefined.
            assert_eq!(
                annotation_type(state, "g"),
                state.tables.intrinsics.undefined
            );
            // Cross product over the boolean primitive union.
            assert_eq!(
                annotation_type(state, "h"),
                state.tables.intrinsics.true_regular
            );
            // strictNullChecks default-on: null & number is never
            // via the nullable∧NumberLike disjoint check.
            assert_eq!(annotation_type(state, "i"), never);
        },
    );
}

#[test]
fn intersections_are_insertion_order_sensitive_and_never_structural() {
    with_state(
        concat!(
            "declare var a: { x: number } & { y: string };\n",
            "declare var b: { y: string } & { x: number };\n",
            "declare var c: { x: number } & { x: number };\n",
        ),
        |state| {
            // Member order is identity: A & B differs from B & A.
            assert_ne!(annotation_type(state, "a"), annotation_type(state, "b"));
            // Structurally identical anonymous literals never dedup:
            // both members survive (the typeMembershipMap is
            // identity-keyed — the steps-doc 4.3 pin).
            let c = annotation_type(state, "c");
            let TypeData::Intersection { types } = &state.tables.type_of(c).data else {
                panic!("distinct {{x}} literals stay an intersection");
            };
            assert_eq!(types.len(), 2);
            assert_ne!(types[0], types[1]);
        },
    );
}

#[test]
fn resolved_conditional_and_unresolved_name_shapes_are_sound() {
    with_state(
        concat!(
            "declare var b: number extends string ? 1 : 2;\n",
            "declare var c: Missing;\n",
            "declare var d: Missing.Scope<string>;\n",
            "declare var e: Missing.Scope<string>;\n",
        ),
        |state| {
            let annotation =
                find_probe_annotation(state.binder.source(0), "b").expect("annotation");
            let conditional = state
                .get_type_from_type_node(annotation)
                .expect("resolved conditional");
            assert_eq!(
                state
                    .type_to_string_slice(conditional)
                    .expect("resolved conditional display"),
                "2"
            );
            // Unresolved names are in-slice: resolveEntityName
            // reports 2304 and the reference types as
            // alias-bearing error intrinsics.
            let annotation =
                find_probe_annotation(state.binder.source(0), "c").expect("annotation");
            let suggestion_count = state.suggestion_count;
            let ty = state
                .get_type_from_type_node(annotation)
                .expect("unresolved names type as an alias-bearing error");
            assert_ne!(ty, state.tables.intrinsics.error);
            assert!(state.tables.is_error_type(ty));
            let alias = state
                .tables
                .type_of(ty)
                .alias_symbol
                .expect("unresolved error retains its alias symbol");
            assert!(state
                .get_check_flags(alias)
                .intersects(CheckFlags::UNRESOLVED));
            assert_eq!(state.symbol_display_name(alias), "Missing");
            assert_eq!(
                state
                    .type_to_string_slice(ty)
                    .expect("unresolved alias display"),
                "Missing"
            );
            assert_eq!(state.suggestion_count, suggestion_count + 1);
            assert_eq!(
                state
                    .get_type_from_type_node(annotation)
                    .expect("negative type reference resolution is cached"),
                ty
            );
            assert_eq!(
                state.suggestion_count,
                suggestion_count + 1,
                "cached unresolved annotations do not reburn suggestion budget"
            );
            assert!(matches!(
                state.links.node(annotation).resolved_symbol,
                LinkSlot::Resolved(symbol) if symbol == alias
            ));

            let d = annotation_type(state, "d");
            let e = annotation_type(state, "e");
            assert_eq!(d, e, "same unresolved path and arguments intern");
            assert!(state.tables.is_error_type(d));
            let qualified_alias = state
                .tables
                .type_of(d)
                .alias_symbol
                .expect("qualified unresolved error retains its alias symbol");
            assert_eq!(
                state.get_fully_qualified_name(qualified_alias),
                "Missing.Scope"
            );
            assert_eq!(
                state
                    .type_to_string_slice(d)
                    .expect("qualified unresolved alias display"),
                "Missing.Scope<string>"
            );
        },
    );
}

#[test]
fn type_node_resolution_is_trial_local_under_speculation() {
    with_state("declare var a: 1 | 2;\n", |state| {
        let annotation = find_probe_annotation(state.binder.source(0), "a")
            .expect("declared var with annotation");
        let resolved = state
            .speculate(|state| {
                let resolved = annotation_type(state, "a");
                assert!(matches!(
                    state.links.node(annotation).resolved_type,
                    LinkSlot::Resolved(cached) if cached == resolved
                ));
                Ok(SpeculationOutcome::Rollback(resolved))
            })
            .expect("trial resolves");
        assert!(matches!(
            state.links.node(annotation).resolved_type,
            LinkSlot::Vacant
        ));
        assert_eq!(annotation_type(state, "a"), resolved);
    });
}

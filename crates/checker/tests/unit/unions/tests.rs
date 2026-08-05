use tsc_binder::bind_source_file;
use tsc_syntax::{parse_source_file, LanguageVariant, ParseOptions};
use tsc_types::{CompilerOptions, TypeData, TypeFlags, UnionReduction};

use crate::relpin::find_probe_annotation;
use crate::relpin::{probe_relation, RelpinQuery, RelpinRelation, RelpinVerdict};
use crate::state::CheckerState;

fn with_state<R>(text: &str, run: impl FnOnce(&mut CheckerState) -> R) -> R {
    let options = CompilerOptions::default();
    let source = parse_source_file(
        "unions-test.ts".to_owned(),
        text.to_owned(),
        ParseOptions {
            language_variant: LanguageVariant::Standard,
            javascript_file: false,
            ..ParseOptions::default()
        },
        None,
    );
    assert!(source.parse_diagnostics.is_empty());
    let binder = bind_source_file(&source, &options);
    let mut state = CheckerState::new(&source, &binder, &options);
    run(&mut state)
}

fn annotation(state: &mut CheckerState, name: &str) -> tsc_types::TypeId {
    let node = find_probe_annotation(state.binder.source(0), name).expect("annotation");
    state.get_type_from_type_node(node).expect("resolves")
}

#[test]
fn subtype_reduction_drops_strict_subtypes() {
    with_state(
        "declare var a: { a: number, b: string };\ndeclare var b: { a: number };\n",
        |state| {
            let literal_a = state.tables.get_string_literal_type("a");
            let string = state.tables.intrinsics.string;
            // "a" is a strict subtype of string: Subtype reduction
            // collapses to string (Literal reduction already does
            // via removeRedundantLiteralTypes — exercise the
            // object case for removeSubtypes proper).
            assert_eq!(
                state
                    .get_union_type_ex(&[literal_a, string], UnionReduction::Subtype)
                    .expect("reduces"),
                string
            );
            let wide = annotation(state, "a");
            let narrow = annotation(state, "b");
            // { a, b } is a strict subtype of { a }: the union
            // subtype-reduces to { a } alone.
            assert_eq!(
                state
                    .get_union_type_ex(&[wide, narrow], UnionReduction::Subtype)
                    .expect("reduces"),
                narrow
            );
            // Literal reduction keeps both members.
            let unreduced = state
                .get_union_type_ex(&[wide, narrow], UnionReduction::Literal)
                .expect("constructs");
            assert!(state
                .tables
                .flags_of(unreduced)
                .intersects(TypeFlags::UNION));
            // reduceVoidUndefined: undefined folds into void under
            // Subtype reduction only.
            let void = state.tables.intrinsics.void;
            let undefined = state.tables.intrinsics.undefined;
            assert_eq!(
                state
                    .get_union_type_ex(&[void, undefined], UnionReduction::Subtype)
                    .expect("reduces"),
                void
            );
            let kept = state
                .get_union_type_ex(&[void, undefined], UnionReduction::Literal)
                .expect("constructs");
            assert!(state.tables.flags_of(kept).intersects(TypeFlags::UNION));
        },
    );
}

#[test]
fn checker_side_unions_reduce_template_matched_string_literals() {
    with_state("declare var t: `a${string}`;\n", |state| {
        let template = annotation(state, "t");
        let abc = state.tables.get_string_literal_type("abc");
        let xyz = state.tables.get_string_literal_type("xyz");
        // "abc" matches `a${string}` and is absorbed; "xyz" does
        // not and survives.
        assert_eq!(
            state
                .get_union_type_ex(&[abc, template], UnionReduction::Literal)
                .expect("reduces"),
            template
        );
        let mixed = state
            .get_union_type_ex(&[xyz, template], UnionReduction::Literal)
            .expect("constructs");
        let TypeData::Union { types, .. } = &state.tables.type_of(mixed).data else {
            panic!("unmatched literal survives the union");
        };
        assert_eq!(types.len(), 2);
    });
}

#[test]
fn intersections_extract_redundant_template_literals() {
    // Matching literal absorbs the template; a pattern template
    // with a non-matching literal collapses to never.
    assert!(matches!(
        probe_relation(&RelpinQuery {
            setup: "",
            source: "\"abc\" & `a${string}`",
            target: "\"abc\"",
            source_is_fresh: false,
            relation: RelpinRelation::Assignable,
            options: &CompilerOptions::default(),
        }),
        RelpinVerdict::Related
    ));
    assert!(matches!(
        probe_relation(&RelpinQuery {
            setup: "",
            source: "\"xyz\" & `a${string}`",
            target: "never",
            source_is_fresh: false,
            relation: RelpinRelation::Assignable,
            options: &CompilerOptions::default(),
        }),
        RelpinVerdict::Related
    ));
}

#[test]
fn common_supertype_prefers_single_supertypes_over_unions() {
    with_state("declare var u: string | null;\n", |state| {
        let a = state.tables.get_string_literal_type("a");
        let b = state.tables.get_string_literal_type("b");
        let string = state.tables.intrinsics.string;
        // Same-base literals join as a union.
        let union = state.get_common_supertype(&[a, b]).expect("join");
        assert!(state.tables.flags_of(union).intersects(TypeFlags::UNION));
        // A strict supertype in the set wins.
        assert_eq!(
            state.get_common_supertype(&[a, string]).expect("join"),
            string
        );
        // Nullable members re-add their nullability after the join
        // (strictNullChecks default-on).
        let string_or_null = annotation(state, "u");
        let joined = state
            .get_common_supertype(&[string_or_null, a])
            .expect("join");
        let TypeData::Union { types, .. } = &state.tables.type_of(joined).data else {
            panic!("nullable join is a union");
        };
        assert!(types.contains(&state.tables.intrinsics.null));
        assert!(types.contains(&string));
    });
}

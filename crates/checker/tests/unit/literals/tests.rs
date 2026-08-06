use tsc_diagnostics::DiagnosticCategory;
use tsc_syntax::SyntaxKind;
use tsc_types::CompilerOptions;

use crate::state::test_support::with_program_state;
use crate::state::CheckerState;
use crate::{check_program, InputFile};

/// Driver-level fixture check (expr.rs idiom): full
/// check_source_file, checker-sink rows as (code, start, length).
/// Every expectation below is oracle-pinned (tsc 6.0.3, noLib,
/// options {}) — scratchpad pins55c matrix, 2026-07-12.
fn checked_rows(text: &str) -> Vec<(u32, u32, u32)> {
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        rows(state)
    })
}

fn rows(state: &CheckerState) -> Vec<(u32, u32, u32)> {
    state
        .diagnostics
        .iter()
        .filter(|diag| diag.file_name.is_some())
        .map(|diag| {
            (
                diag.code(),
                diag.start.unwrap_or(u32::MAX),
                diag.length.unwrap_or(u32::MAX),
            )
        })
        .collect()
}

fn large_numeric_suggestion_spans(text: &str) -> Vec<(DiagnosticCategory, String)> {
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        state
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 80008)
            .map(|diagnostic| {
                let start = diagnostic.start.expect("spanned suggestion") as usize;
                let end = start + diagnostic.length.expect("spanned suggestion") as usize;
                (diagnostic.category(), text[start..end].to_owned())
            })
            .collect()
    })
}

#[test]
fn homomorphic_mapped_context_requests_tuple_literal_shape() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T>(value: { [K in keyof T]: T[K] }) {}\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let source = state.binder.source(0);
            let mapped_node = source
                .arena
                .node_ids()
                .find(|&node| source.arena.node(node).kind == SyntaxKind::MappedType)
                .expect("fixture has a mapped type");
            let mapped = state
                .get_type_from_type_node(mapped_node)
                .expect("mapped type resolves");
            assert!(state
                .is_tuple_context_constituent(mapped)
                .expect("mapped tuple-context predicate resolves"));
        },
    );
}

// ---- computed names: the 2464 legality band ----

#[test]
fn computed_name_of_object_type_reports_2464() {
    assert_eq!(
        checked_rows("declare const o: object;\n({ [o]: 1 });\n"),
        [(2464, 28, 3)]
    );
}

#[test]
fn computed_name_of_boolean_reports_2464() {
    assert_eq!(
        checked_rows("declare const b: boolean;\n({ [b]: 1 });\n"),
        [(2464, 29, 3)]
    );
}

#[test]
fn computed_name_of_null_reports_2464() {
    assert_eq!(
        checked_rows("declare const u: null;\n({ [u]: 1 });\n"),
        [(2464, 26, 3)]
    );
}

#[test]
fn computed_name_of_undefined_reports_2464() {
    assert_eq!(
        checked_rows("declare const u: undefined;\n({ [u]: 1 });\n"),
        [(2464, 31, 3)]
    );
}

#[test]
fn nullable_first_wins_over_string_part_in_2464() {
    // `string | undefined` — the Nullable test fires BEFORE the
    // StringLike kind test (74076).
    assert_eq!(
        checked_rows("declare const su: string | undefined;\n({ [su]: 1 });\n"),
        [(2464, 41, 4)]
    );
}

#[test]
fn legal_computed_names_are_silent() {
    assert_eq!(
        checked_rows("declare const n: number;\n({ [n]: 1 });\n"),
        []
    );
    assert_eq!(checked_rows("declare const a: any;\n({ [a]: 1 });\n"), []);
    assert_eq!(
        checked_rows("declare const sym: unique symbol;\n({ [sym]: 1 });\n"),
        []
    );
    assert_eq!(
        checked_rows("declare enum E { A }\ndeclare const e: E;\n({ [e]: 1 });\n"),
        []
    );
}

// ---- spreads: the 2698 validity band ----

#[test]
fn rest_binding_pattern_reports_2501_on_the_original_expression() {
    assert_eq!(checked_rows("({...{}} = {});\n"), [(2501, 5, 2)]);
    assert_eq!(
        checked_rows("({...({})} = {});\n"),
        [(2501, 5, 4), (2701, 5, 4)]
    );
}

#[test]
fn spreading_a_number_reports_2698() {
    // Error node = the SpreadAssignment (74240).
    assert_eq!(checked_rows("({ ...42 });\n"), [(2698, 3, 5)]);
}

#[test]
fn spreading_a_boolean_reports_2698() {
    assert_eq!(checked_rows("({ ...true });\n"), [(2698, 3, 7)]);
}

#[test]
fn spreading_a_string_reports_2698() {
    assert_eq!(
        checked_rows("declare const s: string;\n({ ...s });\n"),
        [(2698, 28, 4)]
    );
}

#[test]
fn spreading_null_and_undefined_reports_2698() {
    // The falsy-strip leaves never — INVALID, not silent (oracle
    // pin f08; the strip matters the other way: nullable UNIONS
    // spread cleanly, below).
    assert_eq!(
        checked_rows("({ ...null });\n({ ...undefined });\n"),
        [(2698, 3, 7), (2698, 18, 12)]
    );
}

#[test]
fn spreading_never_and_unknown_report_2698() {
    assert_eq!(
        checked_rows("declare const nv: never;\n({ ...nv });\n"),
        [(2698, 28, 5)]
    );
    assert_eq!(
        checked_rows("declare const uk: unknown;\n({ ...uk });\n"),
        [(2698, 30, 5)]
    );
}

#[test]
fn spreading_a_nullable_object_union_is_silent() {
    // THE risk-#4-adjacent verdict pin: removeDefinitelyFalsyTypes
    // strips `null` from the union before the validity test — an
    // identity stub would emit a spurious 2698 here.
    assert_eq!(
        checked_rows("declare const x: { a: number } | null;\n({ ...x });\n"),
        []
    );
}

#[test]
fn spreading_an_object_union_is_silent() {
    assert_eq!(
        checked_rows("declare const u2: { a: number } | { b: string };\n({ ...u2 });\n"),
        []
    );
}

// ---- spread overrides: 2783 + related 2785 ----

#[test]
fn spread_overriding_a_property_reports_2783_with_related_2785() {
    let text = "({ a: 1, ...{ a: 2 } });\n";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        assert_eq!(rows(state), [(2783, 3, 4)]);
        let related = &state.diagnostics[0].related;
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].message.code, 2785);
        assert_eq!(related[0].start, Some(9));
        assert_eq!(related[0].length, Some(11));
    });
}

#[test]
fn spread_of_required_declared_prop_reports_2783() {
    assert_eq!(
        checked_rows("declare const src: { a: number };\n({ a: 1, ...src });\n"),
        [(2783, 37, 4)]
    );
}

#[test]
fn optional_spread_props_do_not_override() {
    assert_eq!(
        checked_rows("declare const src2: { a?: number };\n({ a: 1, ...src2 });\n"),
        []
    );
}

#[test]
fn spread_before_the_property_is_silent() {
    assert_eq!(checked_rows("({ ...{ a: 2 }, a: 1 });\n"), []);
}

// ---- containment lift: literals now surface inner diagnostics ----

#[test]
fn array_literal_elements_are_forced() {
    assert_eq!(checked_rows("[missingName];\n"), [(2304, 1, 11)]);
}

#[test]
fn object_literal_initializers_are_forced() {
    assert_eq!(checked_rows("({ a: missingName });\n"), [(2304, 6, 11)]);
}

#[test]
fn clean_literals_are_silent() {
    assert_eq!(checked_rows("[1, 2, 3];\n"), []);
    assert_eq!(checked_rows("[, 1];\n"), []);
    assert_eq!(checked_rows("[...[1, 2]];\n"), []);
    assert_eq!(checked_rows("declare const a: number;\n({ a });\n"), []);
}

// ---- M7 8.1b: object-literal grammar owner cluster ----

#[test]
fn large_integer_suggestion_observes_threshold_and_literal_form() {
    let text = "9007199254740991;\n\
                9007199254740992;\n\
                -9007199254740992;\n\
                9007199254740992.0;\n\
                9.007199254740992e15;\n";
    assert_eq!(
        large_numeric_suggestion_spans(text),
        [
            (
                DiagnosticCategory::Suggestion,
                "9007199254740992".to_owned()
            ),
            (
                DiagnosticCategory::Suggestion,
                "9007199254740992".to_owned()
            ),
        ]
    );
}

#[test]
fn large_radix_integers_are_not_mistaken_for_scientific_literals() {
    let text = "0x20000000000000;\n\
                0xEEEEEEEEEEEEEE;\n\
                0b100000000000000000000000000000000000000000000000000000;\n\
                0o400000000000000000;\n\
                9_007_199_254_740_992;\n\
                0xDEAD;\n";
    assert_eq!(
        large_numeric_suggestion_spans(text)
            .into_iter()
            .map(|(_, span)| span)
            .collect::<Vec<_>>(),
        [
            "0x20000000000000",
            "0xEEEEEEEEEEEEEE",
            "0b100000000000000000000000000000000000000000000000000000",
            "0o400000000000000000",
            "9_007_199_254_740_992",
        ]
    );
}

#[test]
fn object_property_values_and_names_use_the_exact_grammar_faces() {
    let text = "({ 9007199254740992: 9007199254740994,\n\
                   9007199254740996() {} });\n";
    assert_eq!(
        large_numeric_suggestion_spans(text)
            .into_iter()
            .map(|(_, span)| span)
            .collect::<Vec<_>>(),
        ["9007199254740992", "9007199254740994"]
    );
}

#[test]
fn duplicate_object_props_report_1117_on_the_second_name() {
    assert_eq!(checked_rows("({ a: 1, a: 2 });\n"), [(1117, 9, 1)]);
    assert_eq!(checked_rows("({ a: 1, b: 2 });\n"), []);
}

#[test]
fn duplicate_object_accessors_match_the_grammar_table() {
    // tsc also reports separately owned TS2300 rows on both
    // names; those remain outside this producer-owned 8.1b slice.
    assert_eq!(
        checked_rows("({ get a() { return 1; }, get a() { return 2; } });\n"),
        [(1118, 30, 1)]
    );
    assert_eq!(
        checked_rows("({ a: 1, get a() { return 2; } });\n"),
        [(1119, 13, 1)]
    );
}

#[test]
fn object_literal_modifier_and_method_grammar_order_matches_oracle() {
    assert_eq!(
        checked_rows("({ public get a() { return 1; } });\n"),
        [(1042, 3, 6)]
    );
    assert_eq!(checked_rows("({ get a() { return 1; } });\n"), []);
    assert_eq!(
        checked_rows("({ static m() {} });\n"),
        [(1042, 3, 6), (1184, 3, 6)]
    );
}

#[test]
fn private_object_literal_names_report_18016_in_ts_and_checked_js() {
    assert_eq!(checked_rows("({ #name: 1 });\n"), [(18016, 3, 5)]);

    let result = check_program(
        &[InputFile {
            name: "a.js".to_owned(),
            text: "({ #name: 1 });\n".to_owned(),
        }],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| (
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
            ))
            .collect::<Vec<_>>(),
        [(18016, 3, 5)]
    );
}

#[test]
fn duplicate_object_methods_report_2300_on_the_second_name() {
    assert_eq!(
        checked_rows("({ foo(x: 'hi') {}, foo(x: 'a') {} });\n"),
        [(2300, 20, 3), (6133, 7, 1), (6133, 24, 1)]
    );
}

#[test]
fn non_array_spread_in_array_literal_is_contained() {
    // Oracle (noLib, re-probed 2026-07-14): silent — noLib's
    // anyReadonlyArrayType degenerates to emptyObjectType, so
    // isArrayLikeType(42) is TRUE and the literal tuples up
    // without touching the (now live) iteration protocol.
    assert_eq!(checked_rows("[...42];\n"), []);
}

#[test]
fn object_literal_method_bodies_check_since_5_5f() {
    // Oracle: 2304 @16+11 — the deferred body pass drives the
    // method's return expression through checkExpression.
    assert_eq!(
        checked_rows("({ m() { return missingName; } });\n"),
        [(2304, 16, 11)]
    );
}

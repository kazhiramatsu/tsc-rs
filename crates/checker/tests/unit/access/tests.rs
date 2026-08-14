use tsc_types::{CompilerOptions, ScriptTarget};

use crate::state::test_support::with_program_state;
use crate::state::CheckerState;
use crate::{check_program, InputFile};

/// Driver-level fixture check (literals.rs idiom): oracle-pinned
/// rows (tsc 6.0.3, noLib, options {} unless stated) — scratchpad
/// matrix-risk{1,4}.out, 2026-07-12.
#[test]
fn double_narrowed_never_receiver_reports_2339() {
    // reportNonexistentProperty reports on never receivers
    // (75416), including values narrowed from a non-never
    // declaration. Oracle-pinned vs vendored tsc 6.0.3, noLib.
    assert_eq!(
            checked_rows(
                "declare const x: string | number;\nif (typeof x === \"string\") { if (typeof x === \"number\") { x.toFixed; } }\n"
            ),
            [(2339, 94, 7)]
        );
}

#[test]
fn never_reduced_intersection_receiver_reports_2339() {
    // m6 7.6: the getReducedType never-reduction consult — a
    // conflicting-discriminant intersection receiver collapses to
    // never in tsc's own lookup (59287-59297), so the 2339 row
    // proceeds instead of containing. tsc-probed (scratchpad p6,
    // vendored 6.0.3 noLib): container renders 'never'.
    let text = "type AB = { kind: \"a\" } & { kind: \"b\" };\ndeclare const x: AB;\nx.q;\n";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        let diag = state
            .diagnostics
            .iter()
            .find(|diag| diag.file_name.is_some() && diag.code() == 2339)
            .expect("property miss");
        assert_eq!((diag.start, diag.length), (Some(64), Some(1)));
        assert_eq!(
            diag.message.text,
            "Property 'q' does not exist on type 'never'."
        );
        assert_eq!(diag.message.next.len(), 1);
        assert_eq!(
                diag.message.next[0].text,
                "The intersection 'AB' was reduced to 'never' because property 'kind' has conflicting types in some constituents."
            );
    });
}

#[test]
fn never_narrowed_union_with_reduced_member_reports_2339() {
    // getReducedUnionType drops the impossible `Bad` member, after
    // which the negative discriminant branch narrows the surviving
    // member to never. tsc still runs reportNonexistentProperty on
    // that never receiver; this used to stop at the M8 narrowing
    // shield instead.
    let text = "type Bad = { kind: \"a\" } & { kind: \"b\" };\n\
                    type U = Bad | { kind: \"c\" };\n\
                    declare const x: U;\n\
                    if (x.kind !== \"c\") { x.q; }\n";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        assert!(
            state.diagnostics.iter().any(|diag| diag.code() == 2339),
            "{:#?}",
            state.diagnostics
        );
        assert!(
            state.partial_check_records.is_empty(),
            "{:#?}",
            state.partial_check_records
        );
    });
}

fn checked_rows(text: &str) -> Vec<(u32, u32, u32)> {
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        rows(state)
    })
}

fn checked_js_rows(text: &str, check_js: Option<bool>) -> Vec<(u32, u32, u32)> {
    with_program_state(
        &[("a.js", text)],
        &CompilerOptions {
            allow_js: true,
            check_js,
            ..CompilerOptions::default()
        },
        |state| {
            state.check_source_file(0);
            rows(state)
        },
    )
}

fn published_js_rows(text: &str, check_js: Option<bool>) -> Vec<(u32, u32, u32)> {
    check_program(
        &[InputFile::new("a.js".to_owned(), text.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js,
            ..CompilerOptions::default()
        },
    )
    .diagnostics
    .iter()
    .filter(|diagnostic| diagnostic.file_name.is_some())
    .map(|diagnostic| {
        (
            diagnostic.code(),
            diagnostic.start.unwrap_or(u32::MAX),
            diagnostic.length.unwrap_or(u32::MAX),
        )
    })
    .collect()
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

// ---- risk-#4 selection matrix ----

#[test]
fn nullable_union_receiver_reports_18047_and_member_miss() {
    // Un-gated at 6.6f (oracle-exact rows).
    assert_eq!(
        checked_rows("declare const x: string | null;\nx.length;\n"),
        [(18047, 32, 1), (2339, 34, 6)]
    );
}

#[test]
fn undefined_union_receiver_reports_18048_and_member_miss() {
    // Un-gated at 6.6f (oracle-exact rows).
    assert_eq!(
        checked_rows("declare const x: string | undefined;\nx.length;\n"),
        [(18048, 37, 1), (2339, 39, 6)]
    );
}

#[test]
fn checked_js_jsdoc_parameter_flow_publishes_undefined_only_receivers() {
    let constructor = "/** @param {number} x */\n\
function Point(x) {\n\
    if (!(this instanceof Point)) return new Point(x);\n\
    this.x = x;\n\
}\n\
/** @param {Point} p */\n\
function magnitude(p) { return p.x ** 2; }\n";
    let point = constructor.rfind("p.x").unwrap() as u32;
    assert_eq!(
        published_js_rows(constructor, Some(true)),
        [(18048, point, 3)]
    );

    let destructuring = "/**\n\
 * @param {object} opts\n\
 * @param {number} opts.a\n\
 * @param {number} [opts.b]\n\
 */\n\
function sum({ a, b }) { return a + b; }\n";
    let b = destructuring.rfind("b;").unwrap() as u32;
    assert_eq!(
        published_js_rows(destructuring, Some(true)),
        [(18048, b, 1)]
    );

    let required = destructuring.replace("[opts.b]", "opts.b");
    assert_eq!(published_js_rows(&required, Some(true)), []);
    assert!(check_program(
        &[InputFile::new("a.js".to_owned(), destructuring.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict_null_checks: Some(false),
            ..CompilerOptions::default()
        },
    )
    .diagnostics
    .is_empty());
}

#[test]
fn checked_js_jsdoc_commonjs_optional_export_publishes_18048() {
    let mod_text = "/** @typedef {number} Baz */\n\
module.exports = { Baz: class {} };\n\
/** @typedef {number} Quack */\n\
module.exports = { Quack: 2 };\n";
    let use_text = "var mod = require('./mod1.js');\nnew mod.Baz();\n";
    let result = check_program(
        &[
            InputFile::new("mod1.js".to_owned(), mod_text.to_owned()),
            InputFile::new("use.js".to_owned(), use_text.to_owned()),
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
            .map(|diagnostic| (
                diagnostic.file_name.as_deref(),
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
            ))
            .collect::<Vec<_>>(),
        [
            (
                Some("mod1.js"),
                2300,
                mod_text.find("Baz").unwrap() as u32,
                "Baz".len() as u32,
            ),
            (
                Some("mod1.js"),
                2300,
                mod_text.rfind("Baz").unwrap() as u32,
                "Baz".len() as u32,
            ),
            (
                Some("use.js"),
                18048,
                use_text.find("mod.Baz").unwrap() as u32,
                "mod.Baz".len() as u32,
            ),
        ]
    );
}

#[test]
fn checked_js_jsdoc_unknown_catch_receivers_publish_18046_only() {
    let text = "/** @typedef {unknown} Unknown */\n\
try {} catch (/** @type {unknown} */ err) { err.foo; }\n\
try {} catch (/** @type {Unknown} */ other) { other.foo; }\n\
try {} catch (/** @type {any} */ anyErr) { anyErr.foo; }\n\
try {} catch (plain) { plain.foo; }\n";
    let result = check_program(
        &[InputFile::new("a.js".to_owned(), text.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(true),
            use_unknown_in_catch_variables: Some(false),
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
        [
            (
                18046,
                text.find("err.foo").unwrap() as u32,
                "err".len() as u32,
            ),
            (
                18046,
                text.find("other.foo").unwrap() as u32,
                "other".len() as u32,
            ),
        ]
    );
}

#[test]
fn both_nullable_receiver_reports_18049_and_member_miss() {
    // Un-gated at 6.6f (oracle-exact rows).
    assert_eq!(
        checked_rows("declare const x: string | null | undefined;\nx.length;\n"),
        [(18049, 44, 1), (2339, 46, 6)]
    );
}

#[test]
fn unknown_receiver_reports_18046() {
    // Un-gated at 6.6f (oracle-exact row).
    assert_eq!(
        checked_rows("declare const x: unknown;\nx.length;\n"),
        [(18046, 26, 1)]
    );
}

#[test]
fn void_receiver_reports_plain_2339_on_void() {
    // facts ∌ void: NOT an 18048-family report (oracle-pinned).
    assert_eq!(
        checked_rows("declare const x: void;\nx.foo;\n"),
        [(2339, 25, 3)]
    );
}

#[test]
fn never_receiver_reports_plain_2339() {
    assert_eq!(
        checked_rows("declare const x: never;\nx.foo;\n"),
        [(2339, 26, 3)]
    );
}

#[test]
fn null_literal_receiver_reports_18050() {
    assert_eq!(checked_rows("null.foo;\n"), [(18050, 0, 4)]);
}

#[test]
fn parenthesized_null_receiver_reports_2531() {
    // Parens defeat BOTH the NullKeyword kind test and the
    // entity-name test (oracle-pinned).
    assert_eq!(checked_rows("(null).foo;\n"), [(2531, 0, 6)]);
}

#[test]
fn chained_entity_name_reports_18047_with_entity_text() {
    // Un-gated at 6.6f: "'x.a' is possibly 'null'." (oracle-exact).
    assert_eq!(
        checked_rows("declare const x: { a: { b: number } | null };\nx.a.b;\n"),
        [(18047, 46, 3)]
    );
}

#[test]
fn optional_chain_root_silences_nullable_receiver() {
    assert_eq!(
        checked_rows("declare const x: { a: number } | null;\nx?.a;\n"),
        []
    );
}

#[test]
fn optional_root_then_plain_link_reports_18047_over_the_link() {
    // Un-gated at 6.6f: span includes the `?.`, message renders
    // 'x.a' (entityNameToString; oracle-exact).
    assert_eq!(
        checked_rows("declare const x: { a: { b: number } | null } | undefined;\nx?.a.b;\n"),
        [(18047, 58, 4)]
    );
}

#[test]
fn nonnull_assertion_strips_silently() {
    assert_eq!(
        checked_rows("declare const x: { a: number } | null;\nx!.a;\n"),
        []
    );
    // `x!` on void: silent, downstream never (oracle-pinned).
    assert_eq!(
        checked_rows("declare const x: void;\nx!.foo;\n"),
        [(2339, 26, 3)]
    );
}

// ---- risk-#1 spelling matrix (property side) ----

#[test]
fn insertion_typo_reports_2551_with_related_2728() {
    // Anonymous-receiver flavors stay contained until the
    // nodeBuilder display slice (T2) — interface receivers pin the
    // band (oracle re-probed 2026-07-12).
    let text = "interface O { hello: string }\ndeclare const o: O;\no.helo;\n";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        let diags: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| d.file_name.is_some())
            .collect();
        assert_eq!(diags.len(), 1, "{diags:?}");
        let diag = diags[0];
        assert_eq!(
            (diag.code(), diag.start, diag.length),
            (2551, Some(52), Some(4))
        );
        assert_eq!(
            diag.message_text(),
            "Property 'helo' does not exist on type 'O'. Did you mean 'hello'?"
        );
        assert_eq!(diag.related.len(), 1);
        assert_eq!(diag.related[0].message.code, 2728);
        assert_eq!(diag.related[0].start, Some(14));
        assert_eq!(diag.related[0].length, Some(5));
    });
}

#[test]
fn substitution_typo_len3_gets_no_suggestion() {
    assert_eq!(
        checked_rows("interface O { abc: string }\ndeclare const o: O;\no.abd;\n"),
        [(2339, 50, 3)]
    );
}

#[test]
fn substitution_typo_len5_suggests() {
    assert_eq!(
        checked_rows("interface O { world: string }\ndeclare const o: O;\no.worls;\n"),
        [(2551, 52, 5)]
    );
}

#[test]
fn case_flip_suggests() {
    assert_eq!(
        checked_rows("interface O { hello: string }\ndeclare const o: O;\no.HELLO;\n"),
        [(2551, 52, 5)]
    );
}

#[test]
fn short_candidate_needs_case_insensitive_match() {
    assert_eq!(
        checked_rows("interface O { ab: number }\ndeclare const o: O;\no.ax;\no.AB;\n"),
        [(2339, 49, 2), (2551, 55, 2)]
    );
}

#[test]
fn union_chain_names_first_lacking_constituent() {
    let text = "interface A { a: number; c: string }\ninterface B { b: number; c: string }\ndeclare const o: A | B;\no.d;\n";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        let diags: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| d.file_name.is_some())
            .collect();
        assert_eq!(diags.len(), 1);
        let diag = diags[0];
        assert_eq!((diag.code(), diag.start), (2339, Some(100)));
        assert_eq!(
            diag.message_text(),
            "Property 'd' does not exist on type 'A | B'."
        );
        assert_eq!(diag.message.next.len(), 1);
        assert_eq!(
            diag.message.next[0].text,
            "Property 'd' does not exist on type 'A'."
        );
    });
}

#[test]
fn empty_dom_intersection_gets_the_missing_dom_library_hint() {
    // everyContainedType includes intersections as well as unions.  Both
    // empty DOM-shaped constituents must therefore retain the specialized
    // 2812 diagnostic instead of falling back to the ordinary 2339 row.
    let text = "interface EventTarget {}\n\
                interface HTMLInputElement {}\n\
                declare const input: EventTarget & HTMLInputElement;\n\
                input.value;\n";
    with_program_state(
        &[("a.ts", text)],
        &CompilerOptions {
            lib: Some(vec!["es6".to_owned()]),
            ..CompilerOptions::default()
        },
        |state| {
            state.check_source_file(0);
            let diagnostics = state
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.file_name.is_some())
                .collect::<Vec<_>>();
            assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
            assert_eq!(diagnostics[0].code(), 2812);
            assert_eq!(
                diagnostics[0].message_text(),
                "Property 'value' does not exist on type 'EventTarget & HTMLInputElement'. Try changing the 'lib' compiler option to include 'dom'."
            );
        },
    );
}

#[test]
fn static_member_suggestion_reports_2576() {
    assert_eq!(
        checked_rows("class C { static s = 1; }\ndeclare const c: C;\nc.s;\n"),
        [(2576, 48, 1)]
    );
}

#[test]
fn thenable_miss_gets_await_hint_2773() {
    let text =
        "interface P { then(cb: (x: { a: number }) => void): void }\ndeclare const p: P;\np.a;\n";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        let diags: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| d.file_name.is_some())
            .collect();
        assert_eq!(diags.len(), 1, "{diags:?}");
        let diag = diags[0];
        assert_eq!((diag.code(), diag.start), (2339, Some(81)));
        assert_eq!(diag.related.len(), 1);
        assert_eq!(diag.related[0].message.code, 2773);
        assert_eq!(diag.related[0].start, Some(81));
    });
}

#[test]
fn never_intersection_elaborates_reduction_reason() {
    let text = "interface A { a: 1 }\ninterface B { a: 2 }\ndeclare const o: A & B;\no.b;\n";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        let diags: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| d.file_name.is_some())
            .collect();
        assert_eq!(diags.len(), 1, "{diags:?}");
        let diag = diags[0];
        assert_eq!(
            (diag.code(), diag.start, diag.length),
            (2339, Some(68), Some(1))
        );
        assert_eq!(diag.message.next.len(), 1);
        assert_eq!(
                diag.message.next[0].text,
                "The intersection 'A & B' was reduced to 'never' because property 'a' has conflicting types in some constituents."
            );
    });
}

// ---- name-side suggestion budget (noLib burn) ----

// ---- element-access ladder (risk-#1 order; oracle re-probed
// with named receivers 2026-07-12) ----

#[test]
fn element_spelling_2551_has_no_related() {
    let text = "interface O { hello: number }\ndeclare const o: O;\no[\"helo\"];\n";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        let diags: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| d.file_name.is_some())
            .collect();
        assert_eq!(diags.len(), 1, "{diags:?}");
        let diag = diags[0];
        assert_eq!(
            (diag.code(), diag.start, diag.length),
            (2551, Some(52), Some(6))
        );
        // The element-side flavor carries NO related 2728
        // (oracle-pinned asymmetry vs the property side).
        assert_eq!(diag.related.len(), 0);
    });
}

#[test]
fn element_literal_miss_reports_7053_chain() {
    let text = "interface O { hello: number }\ndeclare const o: O;\no[\"xyz\"];\n";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        let diags: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| d.file_name.is_some())
            .collect();
        assert_eq!(diags.len(), 1, "{diags:?}");
        let diag = diags[0];
        assert_eq!(
            (diag.code(), diag.start, diag.length),
            (7053, Some(50), Some(8))
        );
        assert_eq!(
                diag.message_text(),
                "Element implicitly has an 'any' type because expression of type '\"xyz\"' can't be used to index type 'O'."
            );
        assert_eq!(diag.message.next.len(), 1);
        assert_eq!(
            diag.message.next[0].text,
            "Property 'xyz' does not exist on type 'O'."
        );
    });
}

#[test]
fn element_number_index_reports_7015_on_index_expression() {
    assert_eq!(
            checked_rows(
                "interface O { [n: number]: string }\ndeclare const o: O;\ndeclare const s: string;\no[s];\n"
            ),
            [(7015, 83, 1)]
        );
}

#[test]
fn element_get_method_probe_reports_7052() {
    let text = "interface O { get(k: string): number }\ndeclare const o: O;\ndeclare const k: string;\no[k];\n";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        let diags: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| d.file_name.is_some())
            .collect();
        assert_eq!(diags.len(), 1, "{diags:?}");
        let diag = diags[0];
        assert_eq!(
            (diag.code(), diag.start, diag.length),
            (7052, Some(84), Some(4))
        );
        assert!(diag
            .message_text()
            .ends_with("Did you mean to call 'o.get'?"));
    });
}

#[test]
fn element_set_method_probe_omits_non_entity_receiver_text() {
    let text = "({ get: (key: string) => '', set: (key: string, value: string) => {} })['hello'] = 'modified';\n";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        let diagnostic = state
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.file_name.is_some())
            .expect("nonexistent index signature diagnostic");

        assert_eq!(diagnostic.code(), 7052);
        assert!(
            diagnostic
                .message_text()
                .ends_with("Did you mean to call 'set'?"),
            "{}",
            diagnostic.message_text(),
        );
    });
}

#[test]
fn element_string_key_reports_7053_no_index_signature() {
    let text = "interface O { a: number }\ndeclare const o: O;\ndeclare const k: string;\no[k];\n";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        let diags: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| d.file_name.is_some())
            .collect();
        assert_eq!(diags.len(), 1, "{diags:?}");
        let diag = diags[0];
        assert_eq!(
            (diag.code(), diag.start, diag.length),
            (7053, Some(71), Some(4))
        );
        assert_eq!(
            diag.message.next[0].text,
            "No index signature with a parameter of type 'string' was found on type 'O'."
        );
    });
}

#[test]
fn element_static_member_reports_2576_with_bracket_text() {
    let text = "class C { static s = 1; }\ndeclare const c: C;\nc[\"s\"];\n";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        let diags: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| d.file_name.is_some())
            .collect();
        assert_eq!(diags.len(), 1, "{diags:?}");
        let diag = diags[0];
        assert_eq!(
            (diag.code(), diag.start, diag.length),
            (2576, Some(46), Some(6))
        );
        assert!(
            diag.message_text()
                .ends_with("Did you mean to access the static member 'C[\"s\"]' instead?"),
            "{}",
            diag.message_text()
        );
    });
}

#[test]
fn element_union_receiver_reports_single_7053() {
    let text = "interface A { a: number }\ninterface B { b: number }\ndeclare const o: A | B;\ndeclare const k: string;\no[k];\n";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        let diags: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| d.file_name.is_some())
            .collect();
        assert_eq!(diags.len(), 1, "{diags:?}");
        let diag = diags[0];
        assert_eq!(
            (diag.code(), diag.start, diag.length),
            (7053, Some(101), Some(4))
        );
        assert_eq!(
            diag.message.next[0].text,
            "No index signature with a parameter of type 'string' was found on type 'A | B'."
        );
    });
}

#[test]
fn tuple_out_of_range_reports_with_the_tuple_display() {
    // Oracle: (2493, 37, 1) "Tuple type '[string, number]' of
    // length '2' has no element at index '5'." — flipped live at
    // phase-9 9.3a (tuple renderer).
    assert_eq!(
        checked_rows("declare const t: [string, number];\nt[5];\n"),
        [(2493, 37, 1)]
    );
}

#[test]
fn string_index_signature_hit_is_silent() {
    assert_eq!(
        checked_rows(
            "interface O { [k: string]: number }\ndeclare const o: O;\no[\"anything\"];\n"
        ),
        []
    );
}

#[test]
fn nolib_burn_exhausts_name_suggestions() {
    // Bootstrap burns all 10 slots: near-miss names degrade to
    // plain 2304 (oracle-pinned; the LIB-LOADED 2552 flavor is
    // conformance-gated).
    assert_eq!(checked_rows("const hello = 1;\nhelo;\n"), [(2304, 17, 4)]);
}

#[test]
fn strict_bind_call_apply_off_frees_two_slots() {
    // burn=8 ⇒ suggestions #9/#10 live, #11 degrades (the full
    // budget mechanics in one noLib pin).
    let options = CompilerOptions {
        strict_bind_call_apply: Some(false),
        ..CompilerOptions::default()
    };
    let text = "const hello = 1;\nconst world = 1;\nconst tiger = 1;\nhelo;\nworl;\ntige;\n";
    with_program_state(&[("a.ts", text)], &options, |state| {
        state.check_source_file(0);
        let codes: Vec<u32> = state
            .diagnostics
            .iter()
            .filter(|d| d.file_name.is_some())
            .map(|d| d.code())
            .collect();
        assert_eq!(codes, [2552, 2552, 2304]);
    });
}

#[test]
fn guard_arm_2693_does_not_consume_budget() {
    // A guard-chain arm (the primitive-name 2693 flavor; the
    // 2662/2663 MissingPrefix arms need class-body checking, 5.8)
    // returns BEFORE the budget block — both later near-misses
    // still suggest (oracle-pinned under strictBindCallApply:false).
    let options = CompilerOptions {
        strict_bind_call_apply: Some(false),
        ..CompilerOptions::default()
    };
    let text = "const hello = 1;\nconst world = 1;\nstring;\nhelo;\nworl;\n";
    with_program_state(&[("a.ts", text)], &options, |state| {
        state.check_source_file(0);
        let codes: Vec<u32> = state
            .diagnostics
            .iter()
            .filter(|d| d.file_name.is_some())
            .map(|d| d.code())
            .collect();
        assert_eq!(codes, [2693, 2552, 2552]);
    });
}

#[test]
fn no_suggestion_failure_still_consumes_budget() {
    let options = CompilerOptions {
        strict_bind_call_apply: Some(false),
        ..CompilerOptions::default()
    };
    let text = "const hello = 1;\nconst world = 1;\nxyzzy;\nhelo;\nworl;\n";
    with_program_state(&[("a.ts", text)], &options, |state| {
        state.check_source_file(0);
        let codes: Vec<u32> = state
            .diagnostics
            .iter()
            .filter(|d| d.file_name.is_some())
            .map(|d| d.code())
            .collect();
        assert_eq!(codes, [2304, 2552, 2304]);
    });
}

// ---- 5.7b: the 2729 declared-before-use band (scratchpad
// pins/r{1,2}.ts, oracle-probed 2026-07-13) ----

#[test]
fn static_property_used_before_initialization_reports_2729() {
    assert_eq!(
        checked_rows("class C {\n    static a = C.b;\n    static b = 1;\n}\nC.a;\n"),
        [(2729, 27, 1)]
    );
}

#[test]
fn instance_property_used_before_initialization_stays_contained() {
    // Oracle: 2729 at `b` (23+1) — LIVE since the VALUE_MODULE
    // getTypeOfSymbol arm landed (5.8d) un-contained the strict
    // noImplicitThis globalThis probe on the `this` receiver.
    assert_eq!(
        checked_rows("class E {\n    a = this.b;\n    b = 1;\n}\ndeclare const e: E;\ne.a;\n"),
        [(2729, 23, 1)]
    );
}

#[test]
fn static_block_initialization_probe_is_flow_exact() {
    // emitStandardClassFields=false regime (useDefineForClassFields
    // off): the static-block probe's verdict solely decides the
    // 2729 walk (M5 post-close review — the declared-type stub
    // diverged both ways here). Oracle: tsc 6.0.3, probes
    // c3_static_block_{fn,fp}.ts 2026-07-19.
    let udfcf_off = CompilerOptions {
        use_define_for_class_fields: Some(false),
        ..CompilerOptions::default()
    };
    // FN face: the empty block initializes nothing — 2729 at the
    // S7.a read in b's initializer (offset 43).
    let fn_shape =
        "class S7 {\n    static {}\n    static b = S7.a + 1;\n    static a: number;\n}\n";
    assert_eq!(
        with_program_state(&[("a.ts", fn_shape)], &udfcf_off, |state| {
            state.check_source_file(0);
            rows(state)
        }),
        [(2729, 43, 1)]
    );
    // FP face: the block's `this.a = 1` write proves
    // initialization for the S8.a! read (no second 2729); the
    // write itself still reports (2,19 → offset 29).
    let fp_shape = "class S8 {\n    static { this.a = 1; }\n    static b = S8.a! + 1;\n    static a: number | undefined;\n}\n";
    assert_eq!(
        with_program_state(&[("a.ts", fp_shape)], &udfcf_off, |state| {
            state.check_source_file(0);
            rows(state)
        }),
        [(2729, 29, 1)]
    );
}

#[test]
fn property_used_after_its_declaration_is_clean() {
    // The positional walk's other face: b precedes a (static so
    // the receiver stays this-free).
    assert_eq!(
        checked_rows("class G {\n    static b = 1;\n    static a = G.b;\n}\nG.a;\n"),
        []
    );
}

#[test]
fn function_expando_uses_assignment_declaration_flow() {
    let text = "function f(b: boolean) { function d() {} if (b) { d.q = false; } d.q; if (b) { d.r = 1; } else { d.r = 2; } d.r; }\n";
    let options = CompilerOptions {
        strict: Some(true),
        ..CompilerOptions::default()
    };
    assert_eq!(
        with_program_state(&[("a.ts", text)], &options, |state| {
            state.check_source_file(0);
            rows(state)
        }),
        [(2565, 67, 1)]
    );
}

#[test]
fn decorator_argument_class_expression_contains_its_privates() {
    // m4-review S2 (oracle: vendored tsc 6.0.3, noLib, strict,
    // 2026-07-19): clean — findAncestor's isClassLike "quit"
    // keeps Inner as the containing class for `this.#p`. Pre-fix
    // the walk escaped past Inner to Outer's decorator → 18013.
    assert_eq!(
            checked_rows(
                "function dec(x: any): any { return x; }\n@dec(class Inner { #p = 1; m() { return this.#p; } })\nclass Outer {}\n"
            ),
            []
        );
}

#[test]
fn private_access_diagnostic_uses_the_anonymous_class_sentinel() {
    let text = "const Anonymous = class {\n\
                        #field = 1;\n\
                        static getInstance() { return new Anonymous(); }\n\
                    };\n\
                    Anonymous.getInstance().#field;\n\
                    class Named {\n\
                        #field = 1;\n\
                        static getInstance() { return new Named(); }\n\
                    }\n\
                    Named.getInstance().#field;\n";
    let messages = with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        state
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 18013)
            .map(|diagnostic| diagnostic.message_text().to_owned())
            .collect::<Vec<_>>()
    });
    assert_eq!(
            messages,
            [
                "Property '#field' is not accessible outside class '(anonymous)' because it has a private identifier.".to_owned(),
                "Property '#field' is not accessible outside class 'Named' because it has a private identifier.".to_owned(),
            ]
        );
}

#[test]
fn shadowed_private_access_reports_both_declaration_sites() {
    let text = "class Base {\n\
                        #x = 1;\n\
                        m() {\n\
                            class Derived {\n\
                                #x = 2;\n\
                                f(x: Base) { return x.#x; }\n\
                            }\n\
                        }\n\
                    }\n";
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        ..CompilerOptions::default()
    };
    with_program_state(&[("a.ts", text)], &options, |state| {
        state.check_source_file(0);
        let diagnostic = state
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code() == 18014)
            .expect("shadowed private access");
        assert_eq!(
            diagnostic
                .related
                .iter()
                .map(|related| related.message.code)
                .collect::<Vec<_>>(),
            [18017, 18018]
        );
        assert_eq!(
            diagnostic
                .related
                .iter()
                .map(|related| related.message.text.as_str())
                .collect::<Vec<_>>(),
            [
                "The shadowing declaration of '#x' is defined here",
                "The declaration of '#x' that you probably intended to use is defined here",
            ]
        );
    });
}

#[test]
fn undeclared_private_field_in_plain_js_reports_1111() {
    let text = "class A {\n    #a;\n    m() {\n        this.#a;\n        this.#b;\n    }\n}\n";
    assert_eq!(
        published_js_rows(text, None),
        [(
            1111,
            text.find("#b").expect("undeclared private field") as u32,
            2,
        )]
    );
}

#[test]
fn checked_js_does_not_use_plain_js_private_field_row() {
    let text = "class A { m() { this.#b; } }\n";
    for diagnostics in [
        checked_js_rows(text, Some(true)),
        checked_js_rows(&format!("// @ts-check\n{text}"), None),
    ] {
        assert!(
            diagnostics.iter().all(|(code, _, _)| *code != 1111),
            "{diagnostics:?}"
        );
    }
}

#[test]
fn checked_js_global_this_assignment_uses_the_merged_augmentation() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        strict: Some(true),
        target: Some(ScriptTarget::ES2015.bits()),
        ..CompilerOptions::default()
    };
    let clean = "globalThis.alpha = 4;\nglobalThis.alpha;\n";
    assert_eq!(
        with_program_state(&[("a.js", clean)], &options, |state| {
            state.check_source_file(0);
            rows(state)
        }),
        []
    );

    let siblings = "globalThis.missing;\nlet scoped = 1;\nglobalThis.scoped;\n";
    assert_eq!(
        with_program_state(&[("a.js", siblings)], &options, |state| {
            state.check_source_file(0);
            rows(state)
                .into_iter()
                .filter(|(code, _, _)| matches!(code, 2339 | 7017))
                .collect::<Vec<_>>()
        }),
        [
            (
                7017,
                siblings.find("missing").expect("missing property") as u32,
                "missing".len() as u32,
            ),
            (
                2339,
                siblings.rfind("scoped").expect("block-scoped property") as u32,
                "scoped".len() as u32,
            ),
        ]
    );
}

#[test]
fn jsdoc_deprecated_symbols_and_selected_signatures_match_tsc() {
    let text = "/** @deprecated use current */\n\
                    function old(value: string): string { return value; }\n\
                    const ref = old;\n\
                    old(\"x\");\n\
                    interface API {\n\
                      /** @deprecated */\n\
                      old(): void;\n\
                    }\n\
                    declare const api: API;\n\
                    api.old;\n\
                    api.old();\n\
                    api[\"old\"];\n\
                    interface Indexed {\n\
                      /** @deprecated */\n\
                      [key: string]: number;\n\
                    }\n\
                    declare const indexed: Indexed;\n\
                    indexed.foo;\n\
                    interface DeprecatedCtor {\n\
                      /** @deprecated */\n\
                      new (): object;\n\
                    }\n\
                    declare const C: DeprecatedCtor;\n\
                    new C();\n";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        let rows = state
            .diagnostics
            .iter()
            .filter(|diagnostic| matches!(diagnostic.code(), 6385 | 6387))
            .map(|diagnostic| {
                (
                    diagnostic.code(),
                    diagnostic.category(),
                    diagnostic.message_text().to_owned(),
                    diagnostic
                        .related
                        .iter()
                        .map(|related| related.message.code)
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            rows,
            [
                (
                    6385,
                    tsc_diagnostics::DiagnosticCategory::Suggestion,
                    "'old' is deprecated.".to_owned(),
                    vec![2798],
                ),
                (
                    6387,
                    tsc_diagnostics::DiagnosticCategory::Suggestion,
                    "The signature '(value: string): string' of 'old' is deprecated.".to_owned(),
                    vec![2798],
                ),
                (
                    6385,
                    tsc_diagnostics::DiagnosticCategory::Suggestion,
                    "'old' is deprecated.".to_owned(),
                    vec![2798],
                ),
                (
                    6387,
                    tsc_diagnostics::DiagnosticCategory::Suggestion,
                    "The signature '(): void' of 'api.old' is deprecated.".to_owned(),
                    vec![2798],
                ),
                (
                    6385,
                    tsc_diagnostics::DiagnosticCategory::Suggestion,
                    "'old' is deprecated.".to_owned(),
                    vec![2798],
                ),
                (
                    6385,
                    tsc_diagnostics::DiagnosticCategory::Suggestion,
                    "'foo' is deprecated.".to_owned(),
                    vec![2798],
                ),
                (
                    6387,
                    tsc_diagnostics::DiagnosticCategory::Suggestion,
                    "The signature '(): object' of 'C' is deprecated.".to_owned(),
                    vec![2798],
                ),
            ]
        );
    });
}

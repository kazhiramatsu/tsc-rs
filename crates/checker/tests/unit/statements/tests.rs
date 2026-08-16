use crate::state::test_support::with_program_state;
use crate::{check_program, CompilerOptions, InputFile};

/// Checker-sink rows as (code, start, length) — noLib unit parity
/// (scratchpad p1-p6 oracle probes, 2026-07-14).
fn checked_rows(text: &str) -> Vec<(u32, u32, u32)> {
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        state
            .diagnostics
            .iter()
            .filter(|diag| {
                diag.file_name.is_some()
                    && diag.category() == tsc_diagnostics::DiagnosticCategory::Error
            })
            .map(|diag| {
                (
                    diag.code(),
                    diag.start.unwrap_or(u32::MAX),
                    diag.length.unwrap_or(u32::MAX),
                )
            })
            .collect()
    })
}

fn checked_rows_with(text: &str, options: &CompilerOptions) -> Vec<(u32, u32, u32)> {
    with_program_state(&[("a.ts", text)], options, |state| {
        state.check_source_file(0);
        state
            .diagnostics
            .iter()
            .filter(|diag| {
                diag.file_name.is_some()
                    && diag.category() == tsc_diagnostics::DiagnosticCategory::Error
            })
            .map(|diag| {
                (
                    diag.code(),
                    diag.start.unwrap_or(u32::MAX),
                    diag.length.unwrap_or(u32::MAX),
                )
            })
            .collect()
    })
}

fn checked_js_rows(text: &str) -> Vec<(u32, u32, u32)> {
    with_program_state(
        &[("a.js", text)],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(false),
            ..CompilerOptions::default()
        },
        |state| {
            state.check_source_file(0);
            state
                .diagnostics
                .iter()
                .filter(|diag| {
                    diag.file_name.is_some()
                        && diag.category() == tsc_diagnostics::DiagnosticCategory::Error
                })
                .map(|diag| {
                    (
                        diag.code(),
                        diag.start.unwrap_or(u32::MAX),
                        diag.length.unwrap_or(u32::MAX),
                    )
                })
                .collect()
        },
    )
}

fn unused_label_rows(
    text: &str,
    options: &CompilerOptions,
) -> Vec<(tsc_diagnostics::DiagnosticCategory, u32, u32)> {
    with_program_state(&[("a.ts", text)], options, |state| {
        state.check_source_file(0);
        state
            .diagnostics
            .iter()
            .filter(|diag| diag.code() == 7028)
            .map(|diag| {
                (
                    diag.category(),
                    diag.start.unwrap_or(u32::MAX),
                    diag.length.unwrap_or(u32::MAX),
                )
            })
            .collect()
    })
}

fn unreachable_rows(
    text: &str,
    options: &CompilerOptions,
) -> Vec<(tsc_diagnostics::DiagnosticCategory, u32, u32)> {
    with_program_state(&[("a.ts", text)], options, |state| {
        state.check_source_file(0);
        state
            .diagnostics
            .iter()
            .filter(|diag| diag.code() == 7027)
            .map(|diag| {
                (
                    diag.category(),
                    diag.start.unwrap_or(u32::MAX),
                    diag.length.unwrap_or(u32::MAX),
                )
            })
            .collect()
    })
}

fn checked_js_unreachable_rows(text: &str) -> Vec<(tsc_diagnostics::DiagnosticCategory, u32, u32)> {
    check_program(
        &[InputFile::new("a.js".to_owned(), text.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            allow_unreachable_code: Some(false),
            target: Some(2),
            ..CompilerOptions::default()
        },
    )
    .diagnostics
    .into_iter()
    .filter(|diagnostic| diagnostic.code() == 7027)
    .map(|diagnostic| {
        (
            diagnostic.category(),
            diagnostic.start.unwrap_or(u32::MAX),
            diagnostic.length.unwrap_or(u32::MAX),
        )
    })
    .collect()
}

#[test]
fn duplicate_literal_members_resolve_last_wins() {
    // The M7 8.1b grammar producer now reports tsc's 1117 row.
    // The RELATION must still see the table's last-wins member —
    // the stale first duplicate in resolved.properties was the
    // 6.6f objectLiteralErrors 2322 FP face.
    assert_eq!(checked_rows("var e3 = { a: 0, a: '' };\n"), [(1117, 17, 1)]);
}

#[test]
fn probe_priv() {
    assert_eq!(
        checked_rows("class A { #foo = 1; }\nclass B { #foo = 1; }\nconst b: B = new A();\n"),
        [(2322, 50, 1)]
    );
}

#[test]
fn impossible_intersection_member_does_not_create_false_discriminant() {
    // tsc 59109 reduces each constituent before synthesizing an
    // outer union property.  The impossible string arm therefore
    // contributes no `type` property, so the partial property is
    // not a discriminant and the else arm remains accessible.
    // Oracle-pinned vs vendored tsc 6.0.3, noLib.
    assert_eq!(
            checked_rows("type RV = { type: 'number', value: number } | { type: 'string', value: string };\nfunction foo1(x: RV & { type: 'number' }) {\n  if (x.type === 'number') { x.value; }\n  else { x.value; }\n}\n"),
            []
        );
}

#[test]
fn probe_pattern() {
    assert_eq!(
            checked_rows("let a: 0 | 1 = 0;\nlet b: 0 | 1 | 9;\n[{ [(a = 1)]: b } = [9, a] as const] = [];\nconst bb: 0 = b;\n"),
            []
        );
}

// ---- private-twin heads + the non-augmenting substitution
// (6.6 review; rows oracle-pinned vs vendored tsc 6.0.3 noLib,
// 2026-07-19) ----

#[test]
fn private_twin_first_unmatched_beside_others_keeps_2322() {
    // reportUnmatchedProperty's private arm precedes the
    // props-count dispatch (66710-66724) — and an EMPTY subclass
    // hits its base's twin through the non-augmenting
    // substitution (getNormalizedType 64809).
    assert_eq!(
            checked_rows(
                "class A { #x = 1; }\nclass B extends A { }\nclass C { #x = 2; y = 0; }\ndeclare const b: B;\nconst c: C = b;\n"
            ),
            [(2322, 95, 1)]
        );
}

#[test]
fn inherited_private_of_augmenting_subclass_keeps_2741() {
    // An AUGMENTING subclass is never substituted and the keyed
    // twin lookup cannot see the base's private — 2741 like tsc.
    assert_eq!(
            checked_rows(
                "class A { #x = 1; }\nclass B extends A { y = 0; }\nclass C { #x = 2; }\ndeclare const b: B;\nconst c: C = b;\n"
            ),
            [(2741, 95, 1)]
        );
}

#[test]
fn empty_subclass_missing_property_reports_base_display() {
    // The 2741 walk and display run over the substituted BASE
    // ('A'); only the plain relation head keeps the original name
    // (reportErrorResults 65250-65253).
    let text = "class A { z = 1; }\nclass B extends A { }\nclass C { y = 0; }\ndeclare const b: B;\nconst c: C = b;\n";
    assert_eq!(checked_rows(text), [(2741, 86, 1)]);
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        let message = &state.diagnostics.last().expect("2741 row").message;
        assert!(
            message.text.contains("in type 'A'"),
            "substituted display: {}",
            message.text
        );
    });
}

// ---- body-inferred predicate declaration rows (tsc 6.0.3 noLib
// is clean on each) ----

#[test]
fn body_predicate_narrows_binding_pattern_initializer() {
    assert_eq!(
            checked_rows(
                "interface T { a: string }\nfunction isObj(x: T | null) { return x !== null; }\ndeclare const u: T | null;\nif (isObj(u)) { const { a }: T = u; }\n"
            ),
            []
        );
}

#[test]
fn body_predicate_narrows_merged_declaration_initializer() {
    assert_eq!(
            checked_rows(
                "interface T { a: string }\nfunction isObj(x: T | null) { return x !== null; }\ndeclare const w: T | null;\nfunction h() { if (isObj(w)) { var v: T; var v = w; } }\n"
            ),
            []
        );
}

#[test]
fn checked_js_empty_container_includes_later_expando_exports() {
    let result = check_program(
        &[
            InputFile::new("a.d.ts".to_owned(), "declare class A {}\n".to_owned()),
            InputFile::new("b.js".to_owned(), "const A = { };\nA.d = { };\n".to_owned()),
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
                diagnostic.message_text(),
            ))
            .collect::<Vec<_>>(),
        [(
            Some("b.js"),
            2739,
            6,
            1,
            "Type '{}' is missing the following properties from type 'typeof A': prototype, d",
        )]
    );
}

#[test]
fn checked_js_define_property_container_preserves_readonly_descriptors() {
    let js = "const x = {};\n\
Object.defineProperty(x, \"writable\", { value: \"\", writable: true });\n\
Object.defineProperty(x, \"implicit\", { value: \"\" });\n\
Object.defineProperty(x, \"explicit\", { value: \"\", writable: false });\n\
Object.defineProperty(x, \"getter\", { get() { return 1; } });\n\
Object.defineProperty(x, \"accessor\", { get() { return 1; }, set(_v) {} });\n";
    let result = check_program(
            &[
                InputFile::new("globals.d.ts".to_owned(), "declare var Object: { defineProperty(target: any, name: string, descriptor: any): any };\n"
                        .to_owned()),
                InputFile::new("a.js".to_owned(), js.to_owned()),
                InputFile::new("b.ts".to_owned(), "x.writable = \"\";\n\
x.implicit = \"\";\n\
x.explicit = \"\";\n\
x.getter = 1;\n\
x.accessor = 1;\n"
                        .to_owned()),
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
                diagnostic.message_text(),
            ))
            .collect::<Vec<_>>(),
        [
            (
                Some("a.js"),
                7006,
                js.find("_v").expect("implicit-any setter parameter") as u32,
                "_v".len() as u32,
                "Parameter '_v' implicitly has an 'any' type.",
            ),
            (
                Some("b.ts"),
                2540,
                19,
                8,
                "Cannot assign to 'implicit' because it is a read-only property.",
            ),
            (
                Some("b.ts"),
                2540,
                36,
                8,
                "Cannot assign to 'explicit' because it is a read-only property.",
            ),
            (
                Some("b.ts"),
                2540,
                53,
                6,
                "Cannot assign to 'getter' because it is a read-only property.",
            ),
        ]
    );
}

#[test]
fn body_predicate_narrows_empty_pattern_initializer() {
    assert_eq!(
            checked_rows(
                "interface T { a: string }\nfunction isObj(x: T | null) { return x !== null; }\ndeclare const u: T | null;\nif (isObj(u)) { const {} = u; }\n"
            ),
            []
        );
}

#[test]
fn unreachable_const_enum_under_isolated_modules_reports_7027() {
    // shouldPreserveConstEnums' computed isolatedModules arm
    // (18157 + 18160-18162; 6.6-review D4).
    let options = CompilerOptions {
        isolated_modules: Some(true),
        allow_unreachable_code: Some(false),
        ..CompilerOptions::default()
    };
    assert_eq!(
        checked_rows_with(
            "function f() {\n    return;\n    const enum E { A }\n}\n",
            &options
        ),
        [(7027, 31, 18)]
    );
}

// ---- unreachable code / fallthrough (6.6b; rows oracle-pinned
// vs vendored tsc 6.0.3 noLib per shape, 2026-07-19) ----

#[test]
fn unreachable_code_reports_7027_under_explicit_false() {
    let options = CompilerOptions {
        allow_unreachable_code: Some(false),
        ..CompilerOptions::default()
    };
    // Bind-time Unreachable flag face.
    assert_eq!(
        checked_rows_with("function f() { return; let x = 1; }\n", &options),
        [(7027, 23, 10)]
    );
    // Aggregation: adjacent unreachable statements collapse into
    // ONE range diagnostic (86775-86803).
    assert_eq!(
            checked_rows_with(
                "declare function a(): void;\ndeclare function b(): void;\nfunction f() { return; a(); b(); }\n",
                &options
            ),
            [(7027, 79, 9)]
        );
    // FLOW face: the binder cannot see the never-returning call —
    // isSourceElementUnreachable's isReachableFlowNode arm drives
    // this row (86818-86819).
    assert_eq!(
        checked_rows_with(
            "declare function fail(): never;\nfunction f() { fail(); let x = 1; }\n",
            &options
        ),
        [(7027, 55, 10)]
    );
    // A nested block reports as ONE statement; its contents stay
    // silent (withinUnreachableCode).
    assert_eq!(
        checked_rows_with(
            "function f() { return; { let a = 1; let b = 2; } }\n",
            &options
        ),
        [(7027, 25, 21)]
    );
    // A type alias is not potentially executable: skipped AND a
    // range breaker.
    assert_eq!(
        checked_rows_with(
            "function f() { return; type T = number; let y = 1; }\n",
            &options
        ),
        [(7027, 40, 10)]
    );
    // const enum in unreachable code: the isEnumConst arm keeps it
    // un-reported (no preserveConstEnums).
    assert_eq!(
        checked_rows_with("function f() { return; const enum E { A } }\n", &options),
        []
    );
}

#[test]
fn unreachable_code_preserves_allow_unreachable_code_tri_state() {
    let text = "function f() { return; let x = 1; }\n";
    assert_eq!(
        unreachable_rows(text, &CompilerOptions::default()),
        [(tsc_diagnostics::DiagnosticCategory::Suggestion, 23, 10)]
    );
    assert_eq!(
        unreachable_rows(
            text,
            &CompilerOptions {
                allow_unreachable_code: Some(false),
                ..CompilerOptions::default()
            }
        ),
        [(tsc_diagnostics::DiagnosticCategory::Error, 23, 10)]
    );
    assert_eq!(
        unreachable_rows(
            text,
            &CompilerOptions {
                allow_unreachable_code: Some(true),
                ..CompilerOptions::default()
            }
        ),
        []
    );
    // The error-only helper must continue to exclude the default
    // suggestion face.
    assert_eq!(checked_rows("function f() { return; let x = 1; }\n"), []);
}

#[test]
fn plain_js_publishes_unreachable_code_suggestion() {
    let result = check_program(
        &[InputFile::new(
            "a.js".to_owned(),
            "function f() { return; let x = 1; }\n".to_owned(),
        )],
        &CompilerOptions {
            allow_js: true,
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diag| diag.code() == 7027)
            .map(|diag| (
                diag.category(),
                diag.start.unwrap_or(u32::MAX),
                diag.length.unwrap_or(u32::MAX),
            ))
            .collect::<Vec<_>>(),
        [(tsc_diagnostics::DiagnosticCategory::Suggestion, 23, 10)]
    );
}

#[test]
fn checked_js_jsdoc_never_and_boolean_publish_flow_unreachable_rows() {
    let never_text = "/** @returns {never} */\n\
function fail() { throw \"x\"; }\n\
function f() { fail(); x; }\n";
    assert_eq!(
        checked_js_unreachable_rows(never_text),
        [(
            tsc_diagnostics::DiagnosticCategory::Error,
            never_text.rfind("x;").unwrap() as u32,
            2,
        )]
    );

    let boolean_text = "/** @param {boolean} b */\n\
function f(b) {\n\
    switch (b) {\n\
        case true: return 1;\n\
        case false: return 0;\n\
    }\n\
    b;\n\
}\n";
    assert_eq!(
        checked_js_unreachable_rows(boolean_text),
        [(
            tsc_diagnostics::DiagnosticCategory::Error,
            boolean_text.rfind("b;").unwrap() as u32,
            2,
        )]
    );

    let void_control = never_text.replace("{never}", "{void}");
    assert_eq!(checked_js_unreachable_rows(&void_control), []);
    let any_control = boolean_text.replace("{boolean}", "{*}");
    assert_eq!(checked_js_unreachable_rows(&any_control), []);
}

#[test]
fn unused_label_preserves_allow_unused_labels_tri_state() {
    let text = "unused: { let x = 1; }\n";
    assert_eq!(
        unused_label_rows(text, &CompilerOptions::default()),
        [(tsc_diagnostics::DiagnosticCategory::Suggestion, 0, 6)]
    );
    assert_eq!(
        unused_label_rows(
            text,
            &CompilerOptions {
                allow_unused_labels: Some(false),
                ..CompilerOptions::default()
            }
        ),
        [(tsc_diagnostics::DiagnosticCategory::Error, 0, 6)]
    );
    assert_eq!(
        unused_label_rows(
            text,
            &CompilerOptions {
                allow_unused_labels: Some(true),
                ..CompilerOptions::default()
            }
        ),
        []
    );
    assert_eq!(
        unused_label_rows("used: { break used; }\n", &CompilerOptions::default()),
        []
    );
}

#[test]
fn plain_js_publishes_unused_label_suggestion() {
    let result = check_program(
        &[InputFile::new(
            "a.js".to_owned(),
            "unused: { let x = 1; }\n".to_owned(),
        )],
        &CompilerOptions {
            allow_js: true,
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diag| diag.code() == 7028)
            .map(|diag| (
                diag.category(),
                diag.start.unwrap_or(u32::MAX),
                diag.length.unwrap_or(u32::MAX),
            ))
            .collect::<Vec<_>>(),
        [(tsc_diagnostics::DiagnosticCategory::Suggestion, 0, 6)]
    );
}

#[test]
fn fallthrough_case_reports_7029() {
    let options = CompilerOptions {
        no_fallthrough_cases_in_switch: Some(true),
        ..CompilerOptions::default()
    };
    assert_eq!(
            checked_rows_with(
                "declare function g(x: number): void;\nfunction f(x: number) { switch (x) { case 0: g(0); case 1: g(1); break; } }\n",
                &options
            ),
            [(7029, 74, 7)]
        );
}

// ---- §2 variable band (oracle p1) ----

#[test]
fn declaration_initializer_2322_reports_at_the_name_span() {
    // getErrorSpanForNode's VariableDeclaration arm → the NAME.
    assert_eq!(checked_rows("const x: string = 1;\n"), [(2322, 6, 1)]);
}

#[test]
fn js_defaulted_expando_variable_checks_the_effective_initializer() {
    // getEffectiveInitializer unwraps both an unqualified and a
    // global-object-qualified self-reference before the empty
    // JS-container exemption. The expando members make each
    // effective `{}` initializer an assignment declaration.
    assert_eq!(
        checked_js_rows(
            "var my = my || {};\nmy.app = {};\nvar min = this.min || {};\nmin.app = {};\n"
        ),
        []
    );
    assert_eq!(
        checked_js_rows(
            "var my = my ?? {};\nmy.app = {};\nvar min = this.min ?? {};\nmin.app = {};\n"
        ),
        []
    );
}

#[test]
fn subsequent_variable_declaration_reports_2403_with_related() {
    let rows = with_program_state(
        &[("a.ts", "var y: string;\nvar y: number;\n")],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            state
                .diagnostics
                .iter()
                .map(|diag| {
                    (
                        diag.code(),
                        diag.start.unwrap_or(u32::MAX),
                        diag.length.unwrap_or(u32::MAX),
                        diag.message_text().to_owned(),
                        diag.related.len(),
                    )
                })
                .collect::<Vec<_>>()
        },
    );
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        (rows[0].0, rows[0].1, rows[0].2, rows[0].4),
        (2403, 19, 1, 1)
    );
    // tsc's double space after the first sentence is load-bearing.
    assert_eq!(
            rows[0].3,
            "Subsequent variable declarations must have the same type.  Variable 'y' must be of type 'string', but here has type 'number'."
        );
}

#[test]
fn unresolved_assertion_alias_suppresses_redeclaration_type_comparison() {
    let rows = checked_rows("var v = <T>() => 1;\nvar v = <T>a;\n");
    assert!(
        rows.iter().all(|row| row.0 != 2403),
        "isErrorType must recognize alias-bearing unresolved types: {rows:?}"
    );
}

#[test]
fn unrelated_guard_does_not_suppress_2339() {
    // PR #6 review P1: the [FLOW M5] gates key on a narrowing
    // construct RELATED to the reference — `if (true)` mentions
    // no root of `x`, so the report stands. Oracle: 2339 @61+7.
    assert_eq!(
        checked_rows("interface I { a: number }\ndeclare const x: I;\nif (true) {}\nx.missing;\n"),
        [(2339, 61, 7)]
    );
}

#[test]
fn unrelated_guard_does_not_suppress_2345() {
    // Oracle: 2345 @86+1 — the argument gate requires a guard
    // mentioning `s`.
    assert_eq!(
            checked_rows(
                "if (true) {}\ndeclare function f(n: number): void;\ndeclare const s: string | number;\nf(s);\n"
            ),
            [(2345, 86, 1)]
        );
}

#[test]
fn unrelated_guard_does_not_suppress_2322() {
    // Oracle: 2322 @70+6 — the return gate requires a guard
    // mentioning `u`.
    assert_eq!(
        checked_rows(
            "if (true) {}\ndeclare const u: string | number;\nfunction g(): string { return u; }\n"
        ),
        [(2322, 70, 6)]
    );
}

#[test]
fn related_guard_contains_the_narrowed_argument() {
    // The positive face: `typeof x === 'object'` mentions `x`, so
    // the failed declared-type verdict contains ([FLOW M5]; tsc
    // narrows x to `object` here and reports nothing).
    assert_eq!(
            checked_rows(
                "declare function obj(o: object): void;\nfunction f(x: unknown) {\n    if (!x) { return; }\n    if (typeof x === 'object') { obj(x); }\n}\n"
            ),
            []
        );
}

#[test]
fn later_guard_does_not_suppress_2345() {
    // PR #6 review round 2 P1: a guard AFTER the read cannot
    // narrow it — the [FLOW M5] reach face keys on forward flow.
    // Oracle: 2345 @74+1.
    assert_eq!(
            checked_rows(
                "declare function f(n: number): void;\ndeclare const x: string | number;\n\nf(x);\nif (typeof x === \"string\") {}\n"
            ),
            [(2345, 74, 1)]
        );
}

#[test]
fn shadowed_binding_guard_does_not_suppress_2345() {
    // PR #6 review round 2 P1: the guard's `x` is the block-scoped
    // shadow, a different BINDING than the outer argument — the
    // root match compares symbols, not spellings. Oracle: 2345
    // @130+1.
    assert_eq!(
            checked_rows(
                "declare function f(n: number): void;\ndeclare const x: string | number;\n{\n    const x = \"s\";\n    if (typeof x === \"string\") {}\n}\nf(x);\n"
            ),
            [(2345, 130, 1)]
        );
}

#[test]
fn unrelated_limb_does_not_suppress_2339() {
    // PR #6 review round 2 P1: sitting inside `if (true) { ... }`
    // is not a guarded position for `x` — the limb probe now
    // requires the CONDITION to mention a root. Oracle: 2339
    // @60+7.
    assert_eq!(
        checked_rows("interface I { a: number }\ndeclare const x: I;\nif (true) { x.missing; }\n"),
        [(2339, 60, 7)]
    );
}

#[test]
fn else_sibling_guard_does_not_suppress_2345() {
    // Flow out of a then-limb guard rejoins unnarrowed before the
    // else-limb — the reach face excludes sibling if limbs.
    // Oracle: 2345 @147+1.
    assert_eq!(
            checked_rows(
                "declare function f(n: number): void;\ndeclare const x: string | number;\ndeclare const c: boolean;\nif (c) { if (typeof x === \"string\") {} } else { f(x); }\n"
            ),
            [(2345, 147, 1)]
        );
}

#[test]
fn aliased_predicate_call_contains_the_narrowed_argument() {
    // PR #6 review round 2 P1 (the FP face): tsc narrows `x`
    // through the aliased predicate call (`const ok = isString(x);
    // if (ok)`), so the 2345 verdict must contain. Oracle: silent.
    assert_eq!(
            checked_rows(
                "declare function isString(x: unknown): x is string;\ndeclare function take(s: string): void;\ndeclare const x: unknown;\nconst ok = isString(x);\nif (ok) { take(x); }\n"
            ),
            []
        );
}

#[test]
fn non_predicate_call_alias_does_not_suppress_2345() {
    // The counter-face: `notPred` resolves to a plain boolean
    // signature — not every call is a guard, and the report
    // stands. Oracle: 2345 @141+1.
    assert_eq!(
            checked_rows(
                "declare function notPred(x: unknown): boolean;\ndeclare function take(s: string): void;\ndeclare const x: unknown;\nconst ok = notPred(x);\ntake(x);\n"
            ),
            [(2345, 141, 1)]
        );
}

#[test]
fn loop_back_edge_guard_reports_2345() {
    // Un-gated at 6.6f: the rejoin kills the narrowing, so the
    // loop-crossing argument reports (oracle-exact row).
    assert_eq!(
            checked_rows(
                "declare function f(n: number): void;\ndeclare const x: string | number;\ndeclare const c: boolean;\nwhile (c) { f(x); if (typeof x === \"string\") {} }\n"
            ),
            [(2345, 111, 1)]
        );
}

#[test]
fn guard_before_reference_reports_2345() {
    // Un-gated at 6.6f: the empty limb rejoins, so the read past
    // the guard keeps the union and reports (oracle-exact row).
    assert_eq!(
            checked_rows(
                "declare function f(n: number): void;\ndeclare const x: string | number;\nif (typeof x === \"string\") {}\nf(x);\n"
            ),
            [(2345, 103, 1)]
        );
}

#[test]
fn predicate_type_alias_condition_contains() {
    assert_eq!(
            checked_rows(
                "type Pred = (x: unknown) => x is string;\ndeclare const pred: Pred;\ndeclare function take(s: string): void;\ndeclare const x: unknown;\nconst ok = pred(x);\nif (ok) { take(x); }\n"
            ),
            []
        );
}

#[test]
fn parenthesized_condition_alias_contains() {
    assert_eq!(
            checked_rows(
                "declare function take(s: string): void;\ndeclare const x: unknown;\nconst ok = (typeof x === \"string\");\nif (ok) { take(x); }\n"
            ),
            []
        );
}

#[test]
fn condition_alias_chain_contains() {
    // The alias definition is only an edge; the final `if (ok)` is
    // the reaching guard. Oracle: silent.
    assert_eq!(
            checked_rows(
                "declare function take(s: string): void;\ndeclare const x: unknown;\nconst isString = typeof x === \"string\";\nconst ok = isString;\nif (ok) { take(x); }\n"
            ),
            []
        );
}

#[test]
fn mutable_condition_aliases_do_not_narrow() {
    assert_eq!(
            checked_rows(
                "declare function take(s: string): void;\ndeclare const x: unknown;\nlet ok = typeof x === \"string\";\nif (ok) { take(x); }\n"
            ),
            [(2345, 113, 1)]
        );
    assert_eq!(
            checked_rows(
                "declare function take(s: string): void;\ndeclare const x: unknown;\nvar ok = typeof x === \"string\";\nif (ok) { take(x); }\n"
            ),
            [(2345, 113, 1)]
        );
}

#[test]
fn annotated_const_condition_alias_does_not_narrow() {
    assert_eq!(
            checked_rows(
                "declare function take(s: string): void;\ndeclare const x: unknown;\nconst ok: boolean = typeof x === \"string\";\nif (ok) { take(x); }\n"
            ),
            [(2345, 124, 1)]
        );
}

#[test]
fn condition_alias_inline_depth_matches_tsc() {
    assert_eq!(
            checked_rows(
                "declare function take(s: string): void;\ndeclare const x: unknown;\nconst a0 = typeof x === \"string\";\nconst a1 = a0;\nconst a2 = a1;\nconst a3 = a2;\nconst a4 = a3;\nif (a4) { take(x); }\n"
            ),
            []
        );
    assert_eq!(
            checked_rows(
                "declare function take(s: string): void;\ndeclare const x: unknown;\nconst a0 = typeof x === \"string\";\nconst a1 = a0;\nconst a2 = a1;\nconst a3 = a2;\nconst a4 = a3;\nconst a5 = a4;\nif (a5) { take(x); }\n"
            ),
            [(2345, 190, 1)]
        );
}

#[test]
fn predicate_only_narrows_its_target_argument() {
    assert_eq!(
            checked_rows(
                "declare function isString(value: unknown, other: unknown): value is string;\ndeclare function take(s: string): void;\ndeclare const x: unknown, y: unknown;\nconst ok = isString(y, x);\nif (ok) { take(x); }\n"
            ),
            [(2345, 196, 1)]
        );
}

#[test]
fn enclosing_predicate_guard_reaches_captured_this_in_arrow() {
    assert_eq!(
            checked_rows(
                "type Wrong = { value: number };\ntype Correct = { name: string };\ndeclare function isCorrect(value: unknown): value is Correct;\ndeclare function callback(cb: () => void): void;\nfunction f(this: Correct | Wrong): void {\n  if (!isCorrect(this)) return;\n  callback(() => { this.name; });\n}\n"
            ),
            []
        );
}

#[test]
fn predicate_certainty_does_not_union_overloads() {
    assert_eq!(
            checked_rows(
                "declare function pred(x: string): x is \"a\";\ndeclare function pred(x: number): boolean;\ndeclare function take(s: string): void;\ndeclare const x: number;\nconst ok = pred(x);\nif (ok) { take(x); }\n"
            ),
            [(2345, 187, 1)]
        );
}

#[test]
fn member_predicate_condition_alias_contains() {
    assert_eq!(
            checked_rows(
                "declare const guards: { isString(x: unknown): x is string };\ndeclare function take(s: string): void;\ndeclare const x: unknown;\nconst ok = guards.isString(x);\nif (ok) { take(x); }\n"
            ),
            []
        );
}

#[test]
fn unused_condition_alias_does_not_suppress_2345() {
    assert_eq!(
            checked_rows(
                "declare function isString(x: unknown): x is string;\ndeclare function take(s: string): void;\ndeclare const x: unknown;\nconst ok = isString(x);\ntake(x);\n"
            ),
            [(2345, 147, 1)]
        );
}

#[test]
fn later_var_alias_does_not_retroactively_narrow() {
    // 2454 on `ok` rides along since 6.2 (flipped initialType
    // ladder; oracle parity — the alias read precedes its
    // assignment).
    assert_eq!(
            checked_rows(
                "declare function take(s: string): void;\ndeclare const x: unknown;\nif (ok) { take(x); }\nvar ok = typeof x === \"string\";\n"
            ),
            [(2454, 70, 2), (2345, 81, 1)]
        );
}

#[test]
fn unrelated_property_path_does_not_suppress_2345() {
    assert_eq!(
            checked_rows(
                "declare function take(n: number): void;\ndeclare const obj: { value: string | number };\ndeclare const other: { value: boolean };\nif (other.value) {}\ntake(obj.value);\n"
            ),
            [(2345, 153, 9)]
        );
}

#[test]
fn direct_non_predicate_condition_does_not_suppress_2345() {
    assert_eq!(
            checked_rows(
                "declare function notPred(x: unknown): boolean;\ndeclare function take(s: string): void;\ndeclare const x: unknown;\nif (notPred(x)) { take(x); }\n"
            ),
            [(2345, 136, 1)]
        );
}

#[test]
fn try_guard_does_not_reach_catch() {
    assert_eq!(
            checked_rows(
                "declare function take(n: number): void;\ndeclare const x: string | number;\ntry { if (typeof x === \"string\") {} } catch { take(x); }\n"
            ),
            [(2345, 125, 1)]
        );
}

#[test]
fn for_await_over_a_sync_only_iterable_falls_back_without_panicking() {
    // PR #6 review P1: the worker caches AsyncIterable = No on the
    // async miss, then the sync branch OVERWRITES that key with
    // the awaited sync-derived triple (tsc worker 84139-84174;
    // setCachedIterationTypes is a plain assignment) — the old
    // write-once setter panicked here. Oracle (noLib, 2026-07-14):
    // silent.
    assert_eq!(
            checked_rows(
                "declare var Symbol: { readonly iterator: unique symbol; readonly asyncIterator: unique symbol };\nclass C {\n    [Symbol.iterator]() { return { next() { return { done: false, value: 1 }; } }; }\n}\nasync function f() {\n    for await (const x of new C()) { x; }\n}\n"
            ),
            []
        );
}

#[test]
fn renamed_signature_binding_2842_reports_since_the_parameter_arm() {
    // Oracle p5: tsc reports 2842 at `b` (offset 24) and the
    // isReferenced gate suppresses h2's `c`. Drain + gate landed
    // at 5.8a (risk §14.16); the pusher (checkParameter →
    // checkVariableLikeDeclaration) went live at 5.8b and flipped
    // this pin from [].
    assert_eq!(
            checked_rows(
                "declare function h({ a: b }: { a: number }): void;\ndeclare function h2({ a: c }: { a: number }, d: typeof c): void;\n"
            ),
            [(2842, 24, 1)]
        );
}

// ---- §2 collisions band under skippedOn(noEmit) (oracle p4) ----

fn commonjs_rows(no_emit: Option<bool>) -> Vec<(u32, u32, u32)> {
    let options = CompilerOptions {
        module: Some(1),
        no_emit,
        ..CompilerOptions::default()
    };
    let result = check_program(
        &[InputFile::new(
            "a.ts".to_owned(),
            "export {};\nvar require: number;\n".to_owned(),
        )],
        &options,
    );
    result
        .diagnostics
        .iter()
        .filter(|diag| diag.category() == tsc_diagnostics::DiagnosticCategory::Error)
        .map(|diag| {
            (
                diag.code(),
                diag.start.unwrap_or(u32::MAX),
                diag.length.unwrap_or(u32::MAX),
            )
        })
        .collect()
}

#[test]
fn require_collision_reports_2441_and_no_emit_filters_it() {
    assert_eq!(commonjs_rows(None), [(2441, 15, 7)]);
    assert_eq!(commonjs_rows(Some(true)), []);
}

#[test]
fn commonjs_import_equals_value_aliases_reserve_require_but_type_aliases_do_not() {
    let check = |type_only: bool| {
        let qualifier = if type_only { "type " } else { "" };
        let result = check_program(
            &[
                InputFile::new("dep.ts".to_owned(), "export const value = 1;\n".to_owned()),
                InputFile::new(
                    "main.ts".to_owned(),
                    format!(
                        "import {qualifier}require = require('./dep');\nexport {{ require }};\n"
                    ),
                ),
            ],
            &CompilerOptions {
                module: Some(1),
                target: Some(2),
                ..CompilerOptions::default()
            },
        );
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 2441)
            .map(|diagnostic| {
                (
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                )
            })
            .collect::<Vec<_>>()
    };

    assert_eq!(check(false), [(2441, 7, 7)]);
    assert_eq!(check(true), []);
}

#[test]
fn node_commonjs_format_reports_generated_name_collisions() {
    let text = "function require() {}\nconst exports = {};\nclass Object {}\nexport const __esModule = false;\nexport {require, exports, Object};\n";
    for (extension, allow_js, check_js) in [("ts", false, None), ("js", true, Some(true))] {
        let result = check_program(
            &[
                InputFile::new(format!("subfolder/index.{extension}"), text.to_owned()),
                InputFile::new(
                    "subfolder/package.json".to_owned(),
                    "{\"type\":\"commonjs\"}".to_owned(),
                ),
            ],
            &CompilerOptions {
                module: Some(100),
                target: Some(9),
                allow_js,
                check_js,
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diag| (
                    diag.code(),
                    diag.start.unwrap_or(u32::MAX),
                    diag.length.unwrap_or(u32::MAX),
                ))
                .collect::<Vec<_>>(),
            [(2441, 9, 7), (2441, 28, 7), (2725, 48, 6), (1216, 71, 10),]
        );

        let esm_result = check_program(
            &[
                InputFile::new(format!("index.{extension}"), text.to_owned()),
                InputFile::new(
                    "package.json".to_owned(),
                    "{\"type\":\"module\"}".to_owned(),
                ),
            ],
            &CompilerOptions {
                module: Some(100),
                target: Some(9),
                allow_js,
                check_js,
                ..CompilerOptions::default()
            },
        );
        assert_eq!(esm_result.diagnostics, []);
    }
}

// ---- §3 control statements (oracle p2/p3) ----

#[test]
fn condition_bands_report_2774_2873_1313() {
    assert_eq!(
        checked_rows("declare function f(): void;\nif (f) {}\nif (void 0) {}\nif (1) ;\n"),
        [(2774, 32, 1), (2873, 42, 6), (1313, 60, 1)]
    );
}

#[test]
fn switch_case_2678_uses_the_case_type_as_source() {
    assert_eq!(
        checked_rows("switch (\"a\") { case 1: break; }\n"),
        [(2678, 20, 1)]
    );
}

#[test]
fn switch_case_2678_preserves_eager_zero_union_order() {
    let text = "function earlier(x: 2 | 3) { x = 2; }\n\
                    function f(x: 0 | 2 | 4) { switch (x) { case 1: return; } }\n";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        let diagnostic = state
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code() == 2678)
            .expect("incomparable switch case");
        assert_eq!(
            diagnostic.message.text,
            "Type '1' is not comparable to type '0 | 2 | 4'."
        );
    });
}

#[test]
fn catch_clause_block_scoped_shadow_reports_2492() {
    assert_eq!(
        checked_rows("try {} catch (q) { let q: number; }\n"),
        [(2492, 23, 1)]
    );
}

#[test]
fn checked_js_catch_type_tags_require_any_or_unknown() {
    let source = "class Error {}\n\
                      /** @typedef {any} Any */\n\
                      /** @typedef {unknown} Unknown */\n\
                      try {} catch (/** @type {any} */ err) {}\n\
                      try {} catch (/** @type {unknown} */ err) {}\n\
                      try {} catch (/** @type {Any} */ err) {}\n\
                      try {} catch (/** @type {Unknown} */ err) {}\n\
                      try {} catch (/** @type {Error} */ err) {}\n\
                      try {} catch (/** @type {object} */ err) {}\n\
                      try {} catch (/** @type {Error} */ { x }) {}\n\
                      try {} catch (/** @type {object} */ { x }) {}\n";
    let result = check_program(
        &[InputFile::new("a.js".to_owned(), source.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            target: Some(99),
            use_unknown_in_catch_variables: Some(false),
            ..CompilerOptions::default()
        },
    );
    let rows = result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code() == 1196)
        .map(|diagnostic| {
            (
                diagnostic.start.expect("TS1196 has a start"),
                diagnostic.length.expect("TS1196 has a length"),
            )
        })
        .collect::<Vec<_>>();
    let expected = ["Error", "object", "Error", "object"]
        .into_iter()
        .scan(0usize, |cursor, name| {
            let relative = source[*cursor..]
                .find(&format!("@type {{{name}}}"))
                .expect("invalid catch type exists");
            let start = *cursor + relative + "@type {".len();
            *cursor = start + name.len();
            Some((start as u32, name.len() as u32))
        })
        .collect::<Vec<_>>();
    assert_eq!(rows, expected);
}

#[test]
fn block_scoped_statement_in_do_body_reports_1156() {
    assert_eq!(
        checked_rows("if (2) { let z = 1; }\ndo let v = 1; while (0);\n"),
        [(2872, 4, 1), (1156, 25, 10)]
    );
}

// ---- PR #5 review-round pins (oracle r1-r4 probes) ----

#[test]
fn comma_in_computed_property_name_reports_1171() {
    // checkGrammarObjectLiteralExpression's computed-name row
    // (live in the 8.1b producer) + the 5.5e comma-operator 2695.
    let rows = checked_rows("const a = { [0, 1]: {} };\n");
    assert!(rows.contains(&(1171, 13, 4)), "{rows:?}");
    assert!(rows.contains(&(2695, 13, 1)), "{rows:?}");
    assert_eq!(rows.len(), 2, "{rows:?}");
}

#[test]
fn import_type_assert_form_reports_2880_and_with_form_stays_silent() {
    // The parser threads the consumed keyword into
    // ImportAttributesData.token (review find: the source form is
    // unrecoverable after the parse). getResolutionModeOverride
    // is LIVE (5.8d): zero attribute entries draw the
    // exactly-one-resolution-mode-key rows (oracle probe58d p5;
    // the 2307 is the import-type resolution seam, LIVE since
    // 5.9d's getTypeFromImportTypeNode).
    assert_eq!(
        checked_rows("type T = typeof import(\"./m\", { assert: {} });\n"),
        [(2880, 40, 1), (1456, 40, 2), (2307, 23, 5)]
    );
    assert_eq!(
        checked_rows("type U = typeof import(\"./m\", { with: {} });\n"),
        [(1464, 38, 2), (2307, 23, 5)]
    );
}

fn module_target_rows(module: i32, target: i32, text: &str) -> Vec<(u32, u32, u32)> {
    let options = CompilerOptions {
        module: Some(module),
        target: Some(target),
        ..CompilerOptions::default()
    };
    program_rows_with_file("a.ts", text, &options)
}

fn program_rows_with_file(
    name: &str,
    text: &str,
    options: &CompilerOptions,
) -> Vec<(u32, u32, u32)> {
    let result = check_program(&[InputFile::new(name.to_owned(), text.to_owned())], options);
    result
        .diagnostics
        .iter()
        .filter(|diag| {
            diag.file_name.is_some()
                && diag.category() == tsc_diagnostics::DiagnosticCategory::Error
        })
        .map(|diag| {
            (
                diag.code(),
                diag.start.unwrap_or(u32::MAX),
                diag.length.unwrap_or(u32::MAX),
            )
        })
        .collect()
}

#[test]
fn top_level_for_await_in_node_commonjs_reports_1309() {
    for module in [100, 101, 102, 199] {
        for (extension, allow_js, check_js) in [("cts", false, None), ("cjs", true, Some(true))] {
            let options = CompilerOptions {
                module: Some(module),
                target: Some(tsc_types::ScriptTarget::ES2022.bits()),
                allow_js,
                check_js,
                ..CompilerOptions::default()
            };
            assert_eq!(
                program_rows_with_file(
                    &format!("a.{extension}"),
                    "export {};\nfor await (const x of []) { x; }\n",
                    &options,
                ),
                [(1309, 15, 5)]
            );
        }
    }
}

#[test]
fn top_level_for_await_in_node_esm_remains_allowed() {
    for module in [100, 101, 102, 199] {
        for (extension, allow_js, check_js) in [("mts", false, None), ("mjs", true, Some(true))] {
            let options = CompilerOptions {
                module: Some(module),
                target: Some(tsc_types::ScriptTarget::ES2022.bits()),
                allow_js,
                check_js,
                ..CompilerOptions::default()
            };
            assert_eq!(
                program_rows_with_file(
                    &format!("a.{extension}"),
                    "export {};\nfor await (const x of []) { x; }\n",
                    &options,
                ),
                []
            );
        }
    }
}

#[test]
fn top_level_for_await_node_commonjs_stops_before_target_gate() {
    let options = CompilerOptions {
        module: Some(100),
        target: Some(tsc_types::ScriptTarget::ES2015.bits()),
        ..CompilerOptions::default()
    };
    assert_eq!(
        program_rows_with_file(
            "a.cts",
            "export {};\nfor await (const x of []) { x; }\n",
            &options,
        ),
        [(1309, 15, 5)]
    );
}

#[test]
fn top_level_await_module_ladder_reports_1378() {
    // module=es2020 never satisfies the ladder (oracle r3).
    assert_eq!(
        module_target_rows(6, 9, "export {};\nawait 1;\n"),
        [(1378, 11, 5)]
    );
    // module=esnext + target>=es2017 is clean.
    assert_eq!(module_target_rows(99, 9, "export {};\nawait 1;\n"), []);
}

#[test]
fn top_level_await_using_ladder_reports_2854_on_low_targets() {
    // module=esnext + target=es2015 fails on the target half
    // (oracle r4; the Disposable 2318 probes are file-less and
    // stay out of per-file output).
    assert_eq!(
        module_target_rows(99, 2, "export {};\nawait using x = null;\n"),
        [(2854, 11, 5)]
    );
    assert_eq!(
        module_target_rows(99, 9, "export {};\nawait using x = null;\n"),
        []
    );
}

// ---- §11 tuple type-node rows (oracle p6) ----

#[test]
fn tuple_element_order_rows_1266_1257_2574() {
    assert_eq!(
            checked_rows(
                "interface Array<T> { length: number }\ntype T1 = [...string[], number?];\ntype T2 = [number?, string];\ntype T3 = [...number, string];\n"
            ),
            [(1266, 62, 7), (1257, 92, 6), (2574, 112, 9)]
        );
    assert_eq!(
        checked_rows("interface Array<T> { length: number }\ntype T4 = [...string?];\n"),
        [(2574, 49, 10), (17019, 52, 7)]
    );
}

// ---- m4-review B28: checkGrammarVariableDeclaration's ambient
// report falls through (oracle: vendored tsc 6.0.3, noLib,
// strict, 2026-07-19) — the let-name 2480 and the
// definite-assignment 1263 tails still run after 1039.

#[test]
fn ambient_initializer_falls_through_to_let_name_2480() {
    // Insertion order: the ambient 1039 lands before the let-name
    // tail; the program layer sorts by position like tsc.
    assert_eq!(
        checked_rows("declare const let: number = 1;\n"),
        [(1039, 28, 1), (2480, 14, 3)]
    );
}

#[test]
fn ambient_initializer_falls_through_to_definite_assignment_1263() {
    assert_eq!(
        checked_rows("declare let x!: string = \"a\";\n"),
        [(1039, 25, 3), (1263, 13, 1)]
    );
}

#[test]
fn ambient_definite_assignment_without_initializer_reports_1255() {
    assert_eq!(checked_rows("declare var v!: number;\n"), [(1255, 13, 1)]);
}

#[test]
fn ambient_export_initializer_reports_1039_only() {
    assert_eq!(
        checked_rows("namespace N { export declare let w: string = \"x\"; }\n"),
        [(1039, 45, 3)]
    );
}

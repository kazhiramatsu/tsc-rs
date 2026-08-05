use tsc_types::CompilerOptions;

use crate::state::test_support::{with_program_state, with_program_state_allow_parse_diagnostics};
use crate::state::CheckerState;
use crate::{check_program, InputFile};

/// Driver-level fixture check (operators.rs idiom): oracle-pinned
/// rows (tsc 6.0.3, noLib, options per test) — scratchpad
/// pin56_{a..e}.ts probes, 2026-07-13.
fn checked_rows_with(text: &str, options: &CompilerOptions) -> Vec<(u32, u32, u32)> {
    with_program_state(&[("a.ts", text)], options, |state| {
        state.check_source_file(0);
        rows(state)
    })
}

fn checked_rows(text: &str) -> Vec<(u32, u32, u32)> {
    checked_rows_with(text, &CompilerOptions::default())
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

// ---- the auto-type family (getTypeForVariableLikeDeclaration
// auto arm — flow-evolved, live since 6.2/6.6) ----

#[test]
fn auto_family_renders_no_false_relations() {
    // Oracle rows: 2339 toFixed-on-number (flow-evolved — LIVE
    // since the 6.6f gate retirement), 7053 on c[0] (LIVE since
    // M6 7.5's "{}" display arm — the evolving never[] index
    // renders '{}' under the noLib Array miss; re-probed
    // probe75d.mjs), 7005 ×2 (live). This pin is ORACLE-EXACT
    // (7.5d re-probe: no 6133 rows exist here — the historical
    // "6133 ×2 (M7)" claim was stale; 6133 needs noUnusedLocals
    // and everything in this fixture is read). It also asserts
    // the FP face: NO 2322 from `b = 5` against a null-typed b.
    assert_eq!(
            checked_rows(
                "let b = null;\nb = 5;\nb.toFixed();\nlet c = [];\nc[0] = 1;\nexport let v1;\nv1;\ndeclare let d1;\nd1;\n"
            ),
            [(2339, 23, 7), (7053, 46, 4), (7005, 67, 2), (7005, 87, 2)]
        );
}

#[test]
fn const_null_keeps_the_null_type_and_reports_implicit_any_bands() {
    // The function-expression-owned 6133 suggestions are live in
    // M7 8.4j alongside the implicit-any rows.
    assert_eq!(
            checked_rows(
                "const b2 = null;\ndeclare let n1: number;\nn1 = b2;\nconst f = function (x) { return 1; };\nf;\nconst fb = function ({ c }, [d]) { return 1; };\nfb;\n"
            ),
            [
                (2322, 41, 2),
                (7006, 70, 1),
                (7031, 114, 1),
                (7031, 120, 1),
                (6133, 70, 1),
                (6133, 112, 5),
                (6133, 119, 3),
            ]
        );
}

// ---- sibling-context widening (getWidenedTypeOfObjectLiteral +
// getUndefinedProperty) and fresh/regular round-trip ----

#[test]
fn union_widening_synthesizes_optional_undefined_siblings() {
    // ABSENCE pin (oracle: no diagnostics): without the sibling
    // context (getUndefinedProperty), `t.b` / `nested.p.y` would
    // render 2339 on the arm that lacks the property — the FP
    // shape the context machinery exists to prevent. The positive
    // faces (2322 `number | undefined`, 2741/2353 displays) sit
    // behind the M5 narrowable-union assignment gate and the T2
    // anonymous display slice.
    assert_eq!(
            checked_rows(
                "declare const cond: boolean;\nconst t = cond ? { a: 1 } : { b: 2 };\nt.a;\nt.b;\nconst nested = cond ? { p: { x: 1 } } : { p: { y: \"s\" } };\nnested.p.x;\nnested.p.y;\n"
            ),
            []
        );
}

// ---- reportImplicitAny suggestion band (!noImplicitAny) ----

#[test]
fn implicit_any_recovery_names_use_the_missing_declaration_face() {
    let text = "\
function f(`hello`);
function f(x: string);
function f(x: string) {}
function g(value) { return value; }
";
    let options = CompilerOptions {
        no_implicit_any: Some(true),
        ..CompilerOptions::default()
    };
    with_program_state_allow_parse_diagnostics(&[("a.ts", text)], &options, |state| {
        state.check_source_file(0);
        let messages = state
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 7006)
            .map(|diagnostic| diagnostic.message_text().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            messages,
            [
                "Parameter '(Missing)' implicitly has an 'any' type.",
                "Parameter 'value' implicitly has an 'any' type.",
            ]
        );
    });
}

#[test]
fn loose_mode_reports_suggestion_variants() {
    // 6133, 7043, and 7044 are Suggestion-category rows that ride
    // the same T0 key space.
    let options = CompilerOptions {
        strict: Some(false),
        ..CompilerOptions::default()
    };
    assert_eq!(
        checked_rows_with(
            "let a;\na = 1;\nconst f = function (x) { return 1; };\nf;\n",
            &options
        ),
        [(7043, 4, 1), (7044, 34, 1), (6133, 34, 1)]
    );
}

#[test]
fn named_parameter_without_type_is_an_error_or_suggestion() {
    let source = "type F = (string) => void;\n";
    for (options, expected_category) in [
        (
            CompilerOptions {
                strict: Some(false),
                ..CompilerOptions::default()
            },
            tsc_diagnostics::DiagnosticCategory::Suggestion,
        ),
        (
            CompilerOptions {
                no_implicit_any: Some(true),
                ..CompilerOptions::default()
            },
            tsc_diagnostics::DiagnosticCategory::Error,
        ),
    ] {
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: source.to_owned(),
            }],
            &options,
        );
        let diagnostic = result
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code() == 7051)
            .expect("TS7051");
        assert_eq!(diagnostic.category(), expected_category);
        assert_eq!(
            (
                diagnostic.start,
                diagnostic.length,
                diagnostic.message.text.as_str()
            ),
            (
                Some(10),
                Some(6),
                "Parameter has a name but no type. Did you mean 'arg0: string'?"
            )
        );
    }
}

#[test]
fn checked_js_publishes_loose_parameter_suggestions() {
    let options = CompilerOptions {
        strict: Some(false),
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let files = [
        (
            "parameter.js",
            "/** @constructor */\nfunction Dependency(j) { return j; }\nDependency({});\n",
        ),
        (
            "inner-namepath.js",
            "class C {\n/** @param {C~A} value */\nconstructor(value) {}\n}\n",
        ),
    ];

    with_program_state(&files, &options, |state| {
        for file_index in 0..files.len() {
            state.check_source_file(file_index);
        }
        let emitted = state
            .diagnostics
            .iter()
            .filter(|diagnostic| matches!(diagnostic.code(), 7044 | 7045))
            .map(|diagnostic| {
                (
                    diagnostic.file_name.clone().expect("file diagnostic"),
                    diagnostic.start.expect("spanned diagnostic"),
                    diagnostic.length.expect("spanned diagnostic"),
                    diagnostic.code(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            emitted.iter().map(|row| row.3).collect::<Vec<_>>(),
            [7044, 7044]
        );
    });

    let inputs = files.map(|(name, text)| InputFile {
        name: name.to_owned(),
        text: text.to_owned(),
    });
    let published = check_program(&inputs, &options)
        .diagnostics
        .iter()
        .filter(|diagnostic| matches!(diagnostic.code(), 7044 | 7045))
        .map(|diagnostic| diagnostic.code())
        .collect::<Vec<_>>();
    assert_eq!(published, [7044, 7044]);
}

#[test]
fn unchecked_js_does_not_publish_loose_implicit_any_suggestions() {
    let options = CompilerOptions {
        strict: Some(false),
        allow_js: true,
        check_js: Some(false),
        ..CompilerOptions::default()
    };
    let result = check_program(
        &[InputFile {
            name: "a.js".to_owned(),
            text: "function f(value) { return value; }\nf(1);\n".to_owned(),
        }],
        &options,
    );
    assert!(result
        .diagnostics
        .iter()
        .all(|diagnostic| !matches!(diagnostic.code(), 7044 | 7045)));
}

#[test]
fn checked_js_publishes_constructor_flow_implicit_any_members() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        no_implicit_any: Some(true),
        strict_null_checks: Some(true),
        ..CompilerOptions::default()
    };
    let result = check_program(
        &[InputFile {
            name: "a.js".to_owned(),
            text: "function A() {\n\
                       this.unknown = null;\n\
                       this.unknowable = undefined;\n\
                       this.empty = [];\n\
                       }\n\
                       const a = new A();\n\
                       a.unknown = 1;\n\
                       a.unknowable = 1;\n\
                       a.empty.push(1);\n"
                .to_owned(),
        }],
        &options,
    );
    let rows = result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code() == 7008)
        .map(|diagnostic| {
            (
                diagnostic.start.expect("spanned diagnostic"),
                diagnostic.length.expect("spanned diagnostic"),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 3);

    let sibling = check_program(
        &[InputFile {
            name: "sibling.js".to_owned(),
            text: "function Installer() { this.args = 0; }\n\
                       Installer.prototype.load = function () {\n\
                       (() => { this.newProperty = 1; });\n\
                       };\n"
                .to_owned(),
        }],
        &options,
    );
    assert!(sibling
        .diagnostics
        .iter()
        .all(|diagnostic| diagnostic.code() != 7008));

    let annotated = check_program(
        &[InputFile {
            name: "annotated.js".to_owned(),
            text: "class Render {\n\
                       constructor() {\n\
                       /** @type {number[]} */\n\
                       this.objects = [];\n\
                       }\n\
                       }\n"
            .to_owned(),
        }],
        &options,
    );
    assert!(annotated
        .diagnostics
        .iter()
        .all(|diagnostic| diagnostic.code() != 7008));
}

#[test]
fn checked_js_ports_direct_inline_and_method_level_parameter_types() {
    let options = CompilerOptions {
        strict: Some(false),
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/** @param {number} value */\n\
                    function f(value) { return value; }\n\
                    ({\n\
                      /** @type {() => void} */\n\
                      method(more) {}\n\
                    });\n\
                    const inline = (/** @type {number} */ prop) => prop;\n\
                    inline(1);\n";

    with_program_state(&[("a.js", text)], &options, |state| {
        let method = state
            .binder
            .source(0)
            .arena
            .node_ids()
            .find(|&node| state.kind_of(node) == tsc_syntax::SyntaxKind::MethodDeclaration)
            .expect("object-literal method");
        assert!(
            state.get_jsdoc_type(method).is_some(),
            "method tags: {:?}",
            state
                .get_jsdoc_tags(method)
                .into_iter()
                .map(|tag| state.kind_of(tag))
                .collect::<Vec<_>>()
        );
        state.check_source_file(0);
        let emitted = state
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 7044)
            .map(|diagnostic| {
                (
                    diagnostic.file_name.clone().expect("file diagnostic"),
                    diagnostic.start.expect("spanned diagnostic"),
                    diagnostic.length.expect("spanned diagnostic"),
                    diagnostic.code(),
                )
            })
            .collect::<Vec<_>>();
        // getSignatureOfTypeTag contextually types the whole
        // object-literal method signature, including `more`.
        assert!(emitted.is_empty(), "{emitted:?}");
    });

    let published = check_program(
        &[InputFile {
            name: "a.js".to_owned(),
            text: text.to_owned(),
        }],
        &options,
    );
    let published_7044 = published
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code() == 7044)
        .map(|diagnostic| {
            (
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
            )
        })
        .collect::<Vec<_>>();
    assert!(published_7044.is_empty(), "{published_7044:?}");
}

// ---- reportWideningErrorsInType / reportErrorsFromWidening under
// noImplicitAny + strictNullChecks:false (nullWideningType) ----

#[test]
fn null_widening_reports_7018_and_7011() {
    let options = CompilerOptions {
        no_implicit_any: Some(true),
        strict_null_checks: Some(false),
        ..CompilerOptions::default()
    };
    // The arr row is an ABSENCE pin: under noLib the oracle emits
    // nothing for `const arr = [null]` (no 7005).
    assert_eq!(
            checked_rows_with(
                "const o1 = { a: null };\no1;\nconst h = function () { return null; };\nh;\nconst k = function () { return { a: null }; };\nk;\nconst arr = [null];\narr;\n",
                &options
            ),
            [(7018, 13, 7), (7011, 38, 8), (7018, 104, 7)]
        );
}

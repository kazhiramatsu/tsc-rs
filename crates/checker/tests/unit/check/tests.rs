use tsc_binder::node_util;
use tsc_syntax::{NodeData, SyntaxKind};
use tsc_types::{CompilerOptions, ObjectFlags, ScriptTarget, SymbolFlags, TypeData, TypeFlags};

use crate::state::test_support::{with_program_state, with_program_state_allow_parse_diagnostics};
use crate::state::CheckerState;

#[test]
fn recursive_getter_type_survives_completed_overload_trials() {
    // Vendored tsc 6.0.3, strict/noLib/ESNext: selected trial, rejected
    // predecessor, and nested trials all leave this recursive getter clean.
    let declarations = "interface Box<T> { value: T }; type RecBox<T> = T | Box<RecBox<T>>;";
    let object = "{ value: { value: { get value() { return this; } }}}";
    let overloads = "declare function unbox<T>(box: RecBox<T>, tag: \"first\"): T; declare function unbox<T>(box: RecBox<T>, tag: \"second\"): T;";
    for source in [
        format!("{declarations} declare function unbox<T>(box: RecBox<T>): T; unbox({object});"),
        format!("{declarations}{overloads}unbox({object}, \"second\");"),
        format!("{declarations}{overloads}declare function wrap<T>(value:T):T; wrap(unbox({object}, \"second\"));"),
    ] {
        assert_eq!(checked_diags(&source), [], "{source}");
    }
}

#[test]
fn completed_getter_trials_preserve_real_circularity_diagnostics() {
    let declarations = "declare function pick<T>(value:T, tag:\"first\"):T; declare function pick<T>(value:T, tag:\"second\"):T;";
    for (tag, expected) in [("second", vec![7023]), ("neither", vec![2769, 7023])] {
        let source =
            format!("{declarations} pick({{get value(){{return this.value;}}}}, \"{tag}\");");
        let mut codes = checked_diags(&source)
            .iter()
            .map(|row| row.0)
            .collect::<Vec<_>>();
        codes.sort_unstable();
        assert_eq!(codes, expected, "{source}");
    }
}

#[test]
fn computed_return_diagnostics_follow_syntactic_expression_inference() {
    // Complete messages oracle-checked against vendored tsc 6.0.3,
    // strict/noLib/ESNext. Supported literal properties keep the semantic
    // return fallback; unsupported computed names infer the expression.
    for (expression, return_type) in [
        ("{ [this.a]: \"\" }", "{ [x: number]: string; }"),
        ("({ [this.a]: \"\" })", "{ [x: number]: string; }"),
        (
            "({ [this.a]: \"\" } as const)",
            "{ readonly [x: number]: \"\"; }",
        ),
        ("{ a: this.a }", "any"),
        ("{ [+1]: this.a }", "any"),
        ("{ [-1]: this.a }", "any"),
        ("{ ...this.a }", "any"),
        ("[this.a]", "{}"),
    ] {
        let source = format!("export const thing = {{ doit() {{ return {expression}; }} }};");
        let rows = checked_diags(&source);
        let mut codes = rows.iter().map(|row| row.0).collect::<Vec<_>>();
        codes.sort_unstable();
        assert_eq!(codes, [2339, 7023], "{source}");
        assert_eq!(
            rows.iter().find(|row| row.0 == 2339).unwrap().3,
            format!("Property 'a' does not exist on type '{{ doit(): {return_type}; }}'."),
            "{source}"
        );
    }
    let contextual = "declare function call<T>(f:T):T; export const thing=call({doit(){return {[this.a]: \"\"};}});";
    let rows = checked_diags(contextual);
    assert_eq!(
        rows.iter().find(|row| row.0 == 2339).unwrap().3,
        "Property 'a' does not exist on type '{ doit(): any; }'."
    );
}

/// Drive the check driver over a single-file program and return
/// the checker sink as (code, start, length, head message) rows.
fn checked_diags(text: &str) -> Vec<(u32, u32, u32, String)> {
    checked_diags_with(text, &CompilerOptions::default())
}

fn checked_diags_with(text: &str, options: &CompilerOptions) -> Vec<(u32, u32, u32, String)> {
    checked_file_diags_with("a.ts", text, options)
}

fn checked_file_diags_with(
    file_name: &str,
    text: &str,
    options: &CompilerOptions,
) -> Vec<(u32, u32, u32, String)> {
    with_program_state(&[(file_name, text)], options, |state| {
        state.check_source_file(0);
        diag_rows(state)
    })
}

/// Parser-owned JSDoc diagnostics are stored separately from ordinary
/// parse diagnostics and the checker sink. The Program driver merges
/// this stream only for checked JavaScript files.
fn jsdoc_parse_diag_rows(
    file_name: &str,
    text: &str,
    options: &CompilerOptions,
) -> Vec<(u32, u32, u32, String)> {
    with_program_state(&[(file_name, text)], options, |state| {
        state
            .binder
            .source(0)
            .js_doc_diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.category() == tsc_diagnostics::DiagnosticCategory::Error
            })
            .map(|diagnostic| {
                (
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                    diagnostic.message_text().to_owned(),
                )
            })
            .collect()
    })
}

fn diag_rows(state: &CheckerState) -> Vec<(u32, u32, u32, String)> {
    state
        .diagnostics
        .iter()
        // File-less program diagnostics (the lazy missing-global
        // 2318 band these no-lib fixtures trip on Array probes)
        // are excluded from per-file output — same rule as
        // check_program's assembly.
        .filter(|diag| {
            diag.file_name.is_some()
                && diag.category() == tsc_diagnostics::DiagnosticCategory::Error
        })
        .map(|diag| {
            (
                diag.code(),
                diag.start.unwrap_or(u32::MAX),
                diag.length.unwrap_or(u32::MAX),
                diag.message_text().to_owned(),
            )
        })
        .collect()
}

fn checked_chain_codes(text: &str) -> Vec<Vec<u32>> {
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        state
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.file_name.is_some()
                    && diagnostic.category() == tsc_diagnostics::DiagnosticCategory::Error
            })
            .map(|diagnostic| {
                fn visit(chain: &tsc_diagnostics::MessageChain, codes: &mut Vec<u32>) {
                    codes.push(chain.code);
                    for next in &chain.next {
                        visit(next, codes);
                    }
                }

                let mut codes = Vec::new();
                visit(&diagnostic.message, &mut codes);
                codes
            })
            .collect()
    })
}

#[test]
fn captured_relation_diagnostic_is_owned_independently_of_the_program_sink() {
    with_program_state(
        &[("a.ts", "const marker = 0;")],
        &CompilerOptions::default(),
        |state| {
            let error_node = state.binder.source(0).root;
            let source = state.tables.intrinsics.number;
            let target = state.tables.intrinsics.string;
            let (related, first) = state
                .capture_type_assignable_to_diagnostic(
                    source,
                    target,
                    error_node,
                    &tsc_diagnostics::gen::Type_0_is_not_assignable_to_type_1,
                )
                .expect("primitive relation report");
            assert!(!related);
            let first = first.expect("failed reporting relation owns a diagnostic");

            state.push_error_diagnostic(first.clone());
            let program_count = state.diagnostics.len();
            let (related, captured) = state
                .capture_type_assignable_to_diagnostic(
                    source,
                    target,
                    error_node,
                    &tsc_diagnostics::gen::Type_0_is_not_assignable_to_type_1,
                )
                .expect("cached failed relation is replayable for reporting");

            assert!(!related);
            assert_eq!(captured.as_ref(), Some(&first));
            assert_eq!(state.diagnostics.len(), program_count);
        },
    );
}

#[test]
fn erasable_syntax_only_reports_each_runtime_syntax_at_its_owner_span() {
    let text = "class C {\n\
                    constructor(public x: string) {}\n\
                }\n\
                namespace Runtime {\n\
                    export const x = 1;\n\
                }\n\
                namespace Erased {\n\
                    export interface Shape {}\n\
                }\n\
                enum E { A }\n\
                declare enum AmbientEnum { A }\n\
                import Alias = E.A;\n\
                declare namespace Ambient {\n\
                    import Fine = AmbientEnum.A;\n\
                }\n\
                const value = 0;\n\
                export = value;\n\
                const angle = <number>value;\n\
                const asExpression = value as number;\n";
    let options = CompilerOptions {
        erasable_syntax_only: Some(true),
        ..CompilerOptions::default()
    };
    let rows = checked_diags_with(text, &options)
        .into_iter()
        .filter(|row| row.0 == 1294)
        .collect::<Vec<_>>();
    let start = |needle: &str| text.find(needle).expect("fixture token") as u32;
    let enum_start = start("enum E") + "enum ".len() as u32;
    let message = "This syntax is not allowed when 'erasableSyntaxOnly' is enabled.";
    let row = |start: u32, length: u32| (1294, start, length, message.to_owned());

    assert_eq!(
        rows,
        [
            row(start("public x: string"), "public x: string".len() as u32),
            row(start("Runtime"), "Runtime".len() as u32),
            row(enum_start, 1),
            row(
                start("import Alias = E.A;"),
                "import Alias = E.A;".len() as u32,
            ),
            row(start("export = value;"), "export = value;".len() as u32),
            row(start("<number>"), "<number>".len() as u32),
        ]
    );

    assert!(
        checked_diags(text).iter().all(|row| row.0 != 1294),
        "the option must be the sole gate"
    );
}

#[test]
fn eager_unused_callback_precedes_source_file_collision_drains() {
    let text = "export {}; const WeakMap = 1; class C { #x = 1; }";
    let options = CompilerOptions {
        no_unused_locals: Some(true),
        target: Some(ScriptTarget::ES2015.bits()),
        ..CompilerOptions::default()
    };
    with_program_state(&[("a.ts", text)], &options, |state| {
        state.check_source_file(0);
        let relevant = state
            .diagnostics
            .iter()
            .filter_map(|diagnostic| {
                matches!(diagnostic.code(), 6133 | 6196 | 18027).then_some(diagnostic.code())
            })
            .collect::<Vec<_>>();
        let collision = relevant
            .iter()
            .position(|&code| code == 18027)
            .expect("WeakMap collision");
        assert!(
            relevant[..collision]
                .iter()
                .any(|&code| matches!(code, 6133 | 6196)),
            "{relevant:?}"
        );
        assert!(
            relevant[collision + 1..]
                .iter()
                .all(|&code| !matches!(code, 6133 | 6196)),
            "{relevant:?}"
        );
    });
}

#[test]
fn no_infer_relation_reports_use_the_write_normalized_target() {
    let rows = checked_diags(
        "type NoInfer<T> = intrinsic;\n\
             declare function assertEqual<T>(actual: T, expected: NoInfer<T>): boolean;\n\
             const g = { x: 3, y: 2 };\n\
             assertEqual(g, { x: 3 });\n\
             declare function invoke<T, R>(func: (value: T) => R, value: NoInfer<T>): R;\n\
             declare function test(value: { x: number }): number;\n\
             invoke(test, { x: 1, y: 2 });\n",
    );
    let messages = rows
        .into_iter()
        .filter(|row| matches!(row.0, 2345 | 2353))
        .map(|row| (row.0, row.3))
        .collect::<Vec<_>>();
    assert_eq!(
            messages,
            [
                (
                    2345,
                    "Argument of type '{ x: number; }' is not assignable to parameter of type '{ x: number; y: number; }'."
                        .to_owned(),
                ),
                (
                    2353,
                    "Object literal may only specify known properties, and 'y' does not exist in type '{ x: number; }'."
                        .to_owned(),
                ),
            ]
        );
}

#[test]
fn relation_reports_use_normalized_pair_then_restore_alias_faces() {
    let options = CompilerOptions {
        strict_null_checks: Some(true),
        ..CompilerOptions::default()
    };
    let messages = checked_diags_with(
        "type Partial<T> = { [P in keyof T]?: T[P] };\n\
             type Readonly<T> = { readonly [P in keyof T]: T[P] };\n\
             type Named<T> = T & {};\n\
             function read<T>(x: T, p: Partial<T>, k: keyof T) { x[k] = p[k]; }\n\
             function write<T, U extends T>(x: T, r: Readonly<U>, k: keyof T) { r[k] = x[k]; }\n\
             function alias<T>(x: T, n: Named<T>) { n = x; }\n",
        &options,
    )
    .into_iter()
    .filter(|row| row.0 == 2322)
    .map(|row| row.3)
    .collect::<Vec<_>>();
    assert_eq!(
        messages,
        [
            "Type 'T[keyof T] | undefined' is not assignable to type 'T[keyof T]'.",
            "Type 'T[keyof T]' is not assignable to type 'U[keyof T]'.",
            "Type 'T' is not assignable to type 'Named<T>'.",
        ]
    );
}

#[test]
fn report_only_refinement_does_not_erase_variadic_key_assignment() {
    let rows = checked_diags(
        "function f<T extends string[]>(k: keyof [1, 2, ...T]) {\n\
                 k = '2';\n\
             }\n",
    );
    assert_eq!(
        rows.into_iter()
            .filter(|row| row.0 == 2322)
            .collect::<Vec<_>>(),
        [(
            2322,
            56,
            1,
            "Type 'string' is not assignable to type 'keyof [1, 2, ...T]'.".to_owned(),
        )]
    );
}

#[test]
fn unresolved_type_aliases_keep_written_return_faces_in_relation_reports() {
    let options = CompilerOptions {
        strict_null_checks: Some(true),
        ..CompilerOptions::default()
    };
    let messages = checked_diags_with(
        "let a: () => Missing = null;\n\
             let b: () => Missing.Scope<string> = null;\n",
        &options,
    )
    .into_iter()
    .filter(|row| row.0 == 2322)
    .map(|row| row.3)
    .collect::<Vec<_>>();
    assert_eq!(
        messages,
        [
            "Type 'null' is not assignable to type '() => Missing'.",
            "Type 'null' is not assignable to type '() => Missing.Scope<string>'.",
        ]
    );
}

#[test]
fn duplicate_recovered_type_parameter_uses_the_missing_name_face() {
    let text = "type T<in in> = T;\n";
    with_program_state_allow_parse_diagnostics(
        &[("a.ts", text)],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            let diagnostic = state
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code() == 2300)
                .expect("duplicate recovered type parameter");
            assert_eq!(
                diagnostic.message_text(),
                "Duplicate identifier '(Missing)'."
            );
        },
    );
}

#[test]
fn circularity_and_unassigned_property_diagnostics_use_written_names() {
    let options = CompilerOptions {
        strict: Some(true),
        target: Some(ScriptTarget::ES2015.bits()),
        ..CompilerOptions::default()
    };
    let rows = checked_diags_with(
        "class A {\n\
                 #foo = this.#bar;\n\
                 #bar = this.#foo;\n\
                 [\"#baz\"] = this[\"#baz\"];\n\
             }\n\
             class B {\n\
                 #d: number;\n\
                 constructor() {\n\
                     this.#d;\n\
                     this.#d = 1;\n\
                 }\n\
             }\n",
        &options,
    );
    let messages = rows
        .into_iter()
        .filter(|row| matches!(row.0, 7022 | 2565))
        .map(|row| (row.0, row.3))
        .collect::<Vec<_>>();
    assert_eq!(
            messages,
            [
                (
                    7022,
                    "'#foo' implicitly has type 'any' because it does not have a type annotation and is referenced directly or indirectly in its own initializer.".to_owned(),
                ),
                (
                    7022,
                    "'#bar' implicitly has type 'any' because it does not have a type annotation and is referenced directly or indirectly in its own initializer.".to_owned(),
                ),
                (
                    7022,
                    "'[\"#baz\"]' implicitly has type 'any' because it does not have a type annotation and is referenced directly or indirectly in its own initializer.".to_owned(),
                ),
                (
                    2565,
                    "Property '#d' is used before being assigned.".to_owned(),
                ),
            ]
        );
}

// ---- checked-JS checkJSDocTypeAliasTag AST path ----

#[test]
fn jsdoc_typedef_template_before_properties_reports_8021_on_the_name() {
    let text = "/**\n\
                    * @typedef Oops\n\
                    * @template T\n\
                    * @property {T} value\n\
                    */\n\
                    const host = {};\n";
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        strict: Some(true),
        ..CompilerOptions::default()
    };
    let rows: Vec<_> = checked_file_diags_with("a.js", text, &options)
        .into_iter()
        .filter(|row| row.0 == 8021)
        .collect();
    assert_eq!(
            rows,
            [(
                8021,
                text.find("Oops").unwrap() as u32,
                4,
                "JSDoc '@typedef' tag should either have a type annotation or be followed by '@property' or '@member' tags.".to_owned(),
            )]
        );
}

#[test]
fn jsdoc_typedef_type_and_property_siblings_do_not_report_8021() {
    let text = "/** @typedef {(x: number) => string} Explicit */\n\
                    /**\n\
                    * @typedef ObjectLike\n\
                    * @property {number} value\n\
                    */\n\
                    /**\n\
                    * @typedef Nested\n\
                    * @property {Object} child\n\
                    * @template T\n\
                    * @property {T} child.value\n\
                    */\n\
                    const host = {};\n";
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        strict: Some(true),
        ..CompilerOptions::default()
    };
    assert!(checked_file_diags_with("a.js", text, &options)
        .into_iter()
        .all(|row| row.0 != 8021));
}

#[test]
fn jsdoc_value_references_use_the_initializer_expando_symbol() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        strict: Some(false),
        target: Some(ScriptTarget::ES2015.bits()),
        ..CompilerOptions::default()
    };
    let class_text = "var Outer = class O {\n\
                              m(x, y) { }\n\
                          }\n\
                          Outer.Inner = class I {\n\
                              n(a, b) { }\n\
                          }\n\
                          /** @type {Outer} */\n\
                          var outer\n\
                          outer.m\n\
                          /** @type {Outer.Inner} */\n\
                          var inner\n\
                          inner.n\n";
    let function_text = "var Outer = function O() {\n\
                                 this.y = 2\n\
                             }\n\
                             Outer.Inner = class I {\n\
                                 constructor() { this.x = 1 }\n\
                             }\n\
                             /** @type {Outer} */\n\
                             var outer\n\
                             outer.y\n\
                             /** @type {Outer.Inner} */\n\
                             var inner\n\
                             inner.x\n";
    for text in [class_text, function_text] {
        let diagnostics = checked_file_diags_with("a.js", text, &options);
        assert!(
            diagnostics.iter().all(|row| row.0 != 2339),
            "JSDoc value references must expose initializer instance members: {diagnostics:?}"
        );
    }
}

// ---- checked-JS reportImplicitAny through materialized JSDoc ----

#[test]
fn jsdoc_implicit_any_honors_ts_check_and_ast_spans() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(false),
        no_implicit_any: Some(true),
        ..CompilerOptions::default()
    };
    let text = "// @ts-check\n\
                    /** @type {Function} */\n\
                    const x = a => a;\n\
                    /** @type {function (number)} */\n\
                    const y = n => n;\n";
    let diagnostics = checked_file_diags_with("a.js", text, &options);
    assert!(diagnostics.iter().any(|row| {
        row.0 == 7006
            && row.1 == text.find("a =>").expect("plain Function parameter") as u32
            && row.2 == 1
    }));
    let function_type = "function (number)";
    assert!(diagnostics.iter().any(|row| {
        row.0 == 7014
            && row.1 == text.find(function_type).expect("JSDoc function type") as u32
            && row.2 == function_type.len() as u32
    }));
}

// ---- checkUnmatchedJSDocParameters through materialized tags ----

#[test]
fn jsdoc_unmatched_parameters_preserve_owner_spans_and_nested_boundaries() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/** @param {string} s */\n\
                    var one = function (s) {}, two = function (untyped) {};\n\
                    /**\n\
                     * @param {object} xyz\n\
                     * @param {number} xyz.bar.p\n\
                     */\n\
                    function qualified(xyz) {}\n\
                    /** @param {number?[]} a */\n\
                    function recovered(a) {}\n";
    let diagnostics = checked_file_diags_with("a.js", text, &options)
        .into_iter()
        .filter(|row| matches!(row.0, 8024 | 8032))
        .collect::<Vec<_>>();
    assert_eq!(
            diagnostics,
            [
                (
                    8024,
                    text.find("s */").expect("shared unmatched tag") as u32,
                    1,
                    "JSDoc '@param' tag has name 's', but there is no parameter with that name."
                        .to_owned(),
                ),
                (
                    8032,
                    text.find("xyz.bar.p").expect("qualified tag") as u32,
                    "xyz.bar.p".len() as u32,
                    "Qualified name 'xyz.bar.p' is not allowed without a leading '@param {object} xyz.bar'."
                        .to_owned(),
                ),
                (
                    8024,
                    text.find("?[]").expect("JSDoc type recovery") as u32,
                    0,
                    "JSDoc '@param' tag has name '', but there is no parameter with that name."
                        .to_owned(),
                ),
            ]
        );
}

#[test]
fn jsdoc_unmatched_parameters_do_not_escape_nested_or_arguments_faces() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/**\n\
                     * @param {Object} obj\n\
                     * @param {string} obj.value\n\
                     */\n\
                    function nested({ value }) {}\n\
                    /** @param {number} missing */\n\
                    function argumentsOwner(x) { return arguments; }\n";
    let diagnostics = checked_file_diags_with("a.js", text, &options);
    assert!(
        diagnostics.iter().all(|row| !matches!(row.0, 8024 | 8032)),
        "{diagnostics:?}"
    );
}

// ---- M8-P19 checkUnmatchedJSDocParameters arguments branch ----

#[test]
fn jsdoc_arguments_owner_reports_8029_for_the_last_non_array_parameter() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        strict: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/** @param {string} first */\n\
                    function concat() { return arguments.length; }\n";
    let rows: Vec<_> = checked_file_diags_with("a.js", text, &options)
        .into_iter()
        .filter(|row| row.0 == 8029)
        .collect();
    assert_eq!(
            rows,
            [(
                8029,
                text.find("first").unwrap() as u32,
                5,
                "JSDoc '@param' tag has name 'first', but there is no parameter with that name. It would match 'arguments' if it had an array type.".to_owned(),
            )]
        );
}

#[test]
fn jsdoc_arguments_owner_preserves_array_match_and_binding_siblings() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        strict: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/**\n\
                     * @param {string} ignored\n\
                     * @param {...string} values\n\
                     */\n\
                    function variadic() { return arguments; }\n\
                    /** @param {string[]} values */\n\
                    function array() { return arguments; }\n\
                    /** @param {string} present */\n\
                    function matching(present) { return arguments; }\n\
                    /** @param {string} excluded */\n\
                    function binding({ value }) { return arguments; }\n";
    // `isArrayType` is a type-identity query. Supply the global
    // Array declaration that the ordinary Program lib prefix owns;
    // a no-lib fixture intentionally treats `string[]` as an error
    // type and tsc reports TS8029 for that world.
    with_program_state(
        &[
            (
                "lib.d.ts",
                "interface Array<T> { readonly length: number; }\n",
            ),
            ("a.js", text),
        ],
        &options,
        |state| {
            state.check_source_file(1);
            assert!(state
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code() != 8029));
        },
    );
}

// ---- M8-P20 parseTypedefTag duplicate type child ----

#[test]
fn jsdoc_typedef_duplicate_type_reports_8033_with_detached_related() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/**\n * @typedef Name\n * @type {string}\n * @type {Oops}\n */";
    with_program_state(&[("a.js", text)], &options, |state| {
        let diagnostic = state
            .binder
            .source(0)
            .js_doc_diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code() == 8033)
            .expect("TS8033");
        assert_eq!((diagnostic.start, diagnostic.length), (Some(54), Some(1)));
        assert_eq!(
            diagnostic.message_text(),
            "A JSDoc '@typedef' comment may not contain multiple '@type' tags."
        );
        assert_eq!(diagnostic.related.len(), 1);
        let related = &diagnostic.related[0];
        assert_eq!(related.file_name.as_deref(), Some("a.js"));
        assert_eq!((related.start, related.length), (Some(0), Some(0)));
        assert_eq!(related.message.code, 8034);
        assert_eq!(related.message.text, "The tag was first specified here.");
    });
}

#[test]
fn jsdoc_typedef_duplicate_type_preserves_explicit_type_sibling() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "class C {\n\
                    /**\n\
                     * @typedef {C~A} C~B\n\
                     * @typedef {object} C~A\n\
                     */\n\
                    /** @param {C~A} o */\n\
                    constructor(o) {}\n\
                    }\n";
    assert!(jsdoc_parse_diag_rows("a.js", text, &options)
        .into_iter()
        .all(|row| row.0 != 8033));
}

// ---- M8-P21 invalid template child tags ----

#[test]
fn jsdoc_callback_overload_and_nested_property_report_8039() {
    let fixture = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../ts-tests/tests/cases/conformance/jsdoc/templateInsideCallback.ts"
    ));
    let text = fixture
        .split_once("// @filename: templateInsideCallback.js\n")
        .expect("fixture file section")
        .1;
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        strict: Some(true),
        ..CompilerOptions::default()
    };
    with_program_state(&[("templateInsideCallback.js", text)], &options, |state| {
        let rows = state
            .binder
            .source(0)
            .js_doc_diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 8039)
            .map(|diagnostic| {
                (
                    diagnostic.start,
                    diagnostic.length,
                    diagnostic.message_text(),
                    diagnostic.related.len(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
                rows,
                [
                    (
                        Some(104),
                        Some(8),
                        "A JSDoc '@template' tag may not follow a '@typedef', '@callback', or '@overload' tag",
                        0,
                    ),
                    (
                        Some(299),
                        Some(8),
                        "A JSDoc '@template' tag may not follow a '@typedef', '@callback', or '@overload' tag",
                        0,
                    ),
                    (
                        Some(370),
                        Some(8),
                        "A JSDoc '@template' tag may not follow a '@typedef', '@callback', or '@overload' tag",
                        0,
                    ),
                    (
                        Some(496),
                        Some(8),
                        "A JSDoc '@template' tag may not follow a '@typedef', '@callback', or '@overload' tag",
                        0,
                    ),
                ]
            );
    });
}

#[test]
fn jsdoc_invalid_template_preserves_frozen_overload_sibling() {
    let fixture = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../ts-tests/tests/cases/conformance/jsdoc/overloadTag2.ts"
    ));
    let text = fixture
        .split_once("// @filename: overloadTag2.js\n")
        .expect("fixture file section")
        .1;
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        strict: Some(true),
        ..CompilerOptions::default()
    };
    assert!(jsdoc_parse_diag_rows("overloadTag2.js", text, &options)
        .into_iter()
        .all(|row| row.0 != 8039));
}

// ---- M7 8.1m JSDoc unique-symbol property grammar ----

#[test]
fn jsdoc_unique_symbol_properties_require_static_and_effective_readonly() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "class C {\n\
                      /** @type {unique symbol} */\n\
                      static missingReadonly;\n\
                      /**\n\
                       * @type {unique symbol}\n\
                       * @readonly\n\
                       */\n\
                      instance;\n\
                      /** @type {unique symbol}\n\
                       * @readonly */\n\
                      static valid;\n\
                      /** prose `@type {unique symbol}` */\n\
                      static prose;\n\
                      /** @type {unique symbolic} */\n\
                      static other;\n\
                    }\n";
    with_program_state(&[("a.js", text)], &options, |state| {
        state.check_source_file(0);
        let diagnostics = state
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 1331)
            .map(|diagnostic| {
                (
                    diagnostic.start.expect("TS1331 start"),
                    diagnostic.length.expect("TS1331 length"),
                )
            })
            .collect::<Vec<_>>();
        let expected = ["missingReadonly", "instance"].map(|name| {
            (
                text.find(name).expect("property name") as u32,
                name.len() as u32,
            )
        });
        assert_eq!(diagnostics, expected);
    });
}

// ---- M7 8.1n JSDoc parameter type-argument grammar ----

#[test]
fn jsdoc_parameter_dot_type_arguments_report_empty_and_trailing_comma() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/** @param {C.<>} x\n\
                     * @param {C.<number,>} y */\n\
                    function f(x, y) {}\n";
    with_program_state(&[("a.js", text)], &options, |state| {
        state.check_source_file(0);
        let diagnostics = state
            .diagnostics
            .iter()
            .filter(|diagnostic| matches!(diagnostic.code(), 1009 | 1099))
            .map(|diagnostic| {
                (
                    diagnostic.code(),
                    diagnostic.start.expect("grammar diagnostic start"),
                    diagnostic.length.expect("grammar diagnostic length"),
                )
            })
            .collect::<Vec<_>>();
        let empty = text.find("<>").expect("empty type arguments") as u32;
        let comma = text.find(",>").expect("trailing comma") as u32;
        assert_eq!(diagnostics, [(1099, empty, 2), (1009, comma, 1)]);
    });
}

#[test]
fn jsdoc_parameter_type_arguments_reject_other_comment_faces() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/** @return {(Array.<> | null)} */\n\
                    function returns() {}\n\
                    /** prose `@param {C.<>} x` */\n\
                    function prose(x) {}\n\
                    /** @parameter {C.<number,>} x */\n\
                    function otherTag(x) {}\n\
                    const text = \"/** @param {C.<>} x */\";\n\
                    /** @param {C.<number>} x */\n\
                    function valid(x) {}\n";
    let diagnostics = jsdoc_parse_diag_rows("a.js", text, &options);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| !matches!(diagnostic.0, 1009 | 1099)),
        "non-parameter/valid faces must not produce type-argument grammar diagnostics: \
             {diagnostics:?}"
    );
}

// ---- M7 8.1p JSDoc template-modifier grammar ----

#[test]
fn jsdoc_template_modifiers_follow_effective_host_grammar() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/**\n\
                     * @template const T\n\
                     * @typedef {[T]} X\n\
                     */\n\
                    /** @template private T */\n\
                    function f() {}\n\
                    /** @template in T */\n\
                    function g() {}\n";
    with_program_state(&[("a.js", text)], &options, |state| {
        state.check_source_file(0);
        let diagnostics = state
            .diagnostics
            .iter()
            .filter(|diagnostic| matches!(diagnostic.code(), 1273 | 1274 | 1277))
            .map(|diagnostic| {
                (
                    diagnostic.code(),
                    diagnostic.start.expect("template modifier start"),
                    diagnostic.length.expect("template modifier length"),
                    diagnostic.message_text().to_owned(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
                diagnostics,
                [
                    (
                        1277,
                        text.find("const").expect("const modifier") as u32,
                        "const".len() as u32,
                        "'const' modifier can only appear on a type parameter of a function, method or class"
                            .to_owned(),
                    ),
                    (
                        1273,
                        text.find("private").expect("private modifier") as u32,
                        "private".len() as u32,
                        "'private' modifier cannot appear on a type parameter".to_owned(),
                    ),
                    (
                        1274,
                        text.find("@template in").expect("variance tag") as u32
                            + "@template ".len() as u32,
                        "in".len() as u32,
                        "'in' modifier can only appear on a type parameter of a class, interface or type alias"
                            .to_owned(),
                    ),
                ]
            );
    });
}

#[test]
fn jsdoc_template_modifiers_preserve_valid_and_non_tag_faces() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/** @template const T */\n\
                    class C {}\n\
                    /** @template in T\n\
                     * @typedef {Object} In */\n\
                    /** @template out T\n\
                     * @typedef {Object} Out */\n\
                    /** @template T */\n\
                    function valid() {}\n\
                    /** prose `@template private T` */\n\
                    function prose() {}\n\
                    /** @templates private T */\n\
                    function otherTag() {}\n\
                    const text = \"/** @template private T */\";\n\
                    /** @template privateish T */\n\
                    function boundary() {}\n";
    let diagnostics = checked_file_diags_with("a.js", text, &options);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| !matches!(diagnostic.0, 1273 | 1274 | 1277)),
        "valid/non-tag faces must not produce template-modifier grammar diagnostics: \
             {diagnostics:?}"
    );
}

// ---- M7 8.1q JSDoc satisfies-tag duplicate grammar ----

#[test]
fn jsdoc_satisfies_duplicates_report_every_tag_after_the_first_per_host() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/** @satisfies {number}\n\
                     * @satisfies {number} */\n\
                    const first = 1;\n\
                    /** @satisfies {number} */\n\
                    const second = /** @satisfies {number} */ (1);\n\
                    /** @satisfies {number}\n\
                     * @satisfies {number}\n\
                     * @satisfies {number} */\n\
                    const third = 1;\n";
    with_program_state(&[("a.js", text)], &options, |state| {
        state.check_source_file(0);
        let tags = text
            .match_indices("@satisfies")
            .map(|(start, _)| ((start + 1) as u32, "satisfies".len() as u32))
            .collect::<Vec<_>>();
        // Only the declaration-level and inline comments for `second`
        // collapse onto one effective initializer host. getAllJSDocTags
        // orders the inline tag first, so the declaration tag is the
        // duplicate reported by tsc.
        let expected = [tags[2]];
        let diagnostics = state
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 1223)
            .map(|diagnostic| {
                (
                    diagnostic.start.expect("duplicate tag start"),
                    diagnostic.length.expect("duplicate tag length"),
                    diagnostic.message_text().to_owned(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            diagnostics,
            expected
                .map(|(start, length)| (
                    start,
                    length,
                    "'satisfies' tag already specified.".to_owned(),
                ))
                .to_vec()
        );
    });
}

#[test]
fn jsdoc_satisfies_duplicates_preserve_distinct_hosts_and_non_tags() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/** @satisfies {number} */\n\
                    const first = 1;\n\
                    const inline = /** @satisfies {number} */ (1);\n\
                    /** @satisfies {number} */\n\
                    const left = 1, right = /** @satisfies {number} */ (2);\n\
                    /** prose `@satisfies {number}` */\n\
                    const prose = 1;\n\
                    /** @satisfiesElse {number} */\n\
                    const boundary = 1;\n\
                    const text = \"/** @satisfies {number} */\";\n\
                    /* @satisfies {number} */\n\
                    const ordinary = 1;\n";
    let diagnostics = checked_file_diags_with("a.js", text, &options);
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.0 != 1223),
        "distinct-host/non-tag faces must not produce duplicate-tag diagnostics: \
             {diagnostics:?}"
    );
}

// ---- M7 8.1t JSDoc variadic-parameter grammar ----

#[test]
fn jsdoc_variadic_types_require_the_final_host_parameter() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/**\n\
                     * @param {...?number} e\n\
                     * @param {...number?} f\n\
                     * @param {...number!?} g\n\
                     * @param {...number?!} h\n\
                     * @param {...number[]} i\n\
                     * @param {...number![]?} j\n\
                     * @param {...number?[]!} k\n\
                     * @param {...number} m\n\
                     */\n\
                    function f(e, f, g, h, i, j, k, m) {}\n";
    with_program_state(&[("a.js", text)], &options, |state| {
        state.check_source_file(0);
        let diagnostics = state
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 1014)
            .map(|diagnostic| {
                (
                    diagnostic.start.expect("TS1014 start"),
                    diagnostic.length.expect("TS1014 length"),
                    diagnostic.message_text().to_owned(),
                )
            })
            .collect::<Vec<_>>();
        let expected = [
            "...?number",
            "...number?",
            "...number!?",
            "...number[]",
            "...number![]?",
        ]
        .map(|variadic| {
            (
                text.find(variadic).expect("variadic type") as u32,
                variadic.len() as u32,
                "A rest parameter must be last in a parameter list.".to_owned(),
            )
        });
        assert_eq!(diagnostics, expected);
    });
}

#[test]
fn jsdoc_variadic_types_preserve_last_malformed_and_non_tag_faces() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/** @param {...number} value */\n\
                    function last(value) {}\n\
                    /** @param {...number?!} bad\n\
                     * @param {...number?[]!} alsoBad\n\
                     * @param {number} final */\n\
                    function malformed(bad, alsoBad, final) {}\n\
                    /** prose `@param {...number} value` */\n\
                    function prose(value) {}\n\
                    /** @parameter {...number} value */\n\
                    function other(value) {}\n";
    let diagnostics = checked_file_diags_with("a.js", text, &options);
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.0 != 1014),
        "last/malformed/non-tag faces must not produce TS1014: {diagnostics:?}"
    );
}

// ---- M7 8.1u JSDoc effective optional-parameter grammar ----

#[test]
fn jsdoc_optional_parameters_reject_a_following_required_parameter() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/**\n\
                     * @param {number} a\n\
                     * @param {number} [b]\n\
                     * @param {number} c\n\
                     */\n\
                    function first(a, b, c) {}\n\
                    /**\n\
                     * @param {string=} `args`\n\
                     * @param `bwarg` {?number?}\n\
                     */\n\
                    function second(args, bwarg) {}\n";
    with_program_state(&[("a.js", text)], &options, |state| {
        state.check_source_file(0);
        let mut diagnostics = state
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 1016)
            .map(|diagnostic| {
                (
                    diagnostic.start.expect("TS1016 start"),
                    diagnostic.length.expect("TS1016 length"),
                    diagnostic.message_text().to_owned(),
                )
            })
            .collect::<Vec<_>>();
        diagnostics.sort_by_key(|diagnostic| diagnostic.0);
        let expected = [
            ("function first(a, b, c)", "c"),
            ("function second(args, bwarg)", "bwarg"),
        ]
        .map(|(signature, name)| {
            let signature_start = text.find(signature).expect("host signature");
            let relative_name = signature.rfind(name).expect("parameter name");
            (
                (signature_start + relative_name) as u32,
                name.len() as u32,
                "A required parameter cannot follow an optional parameter.".to_owned(),
            )
        });
        assert_eq!(diagnostics, expected);
    });
}

#[test]
fn jsdoc_optional_parameters_preserve_adjacent_negative_faces() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/** @param {number} a\n\
                     * @param {number} [b] */\n\
                    function ordered(a, b) {}\n\
                    /** @param {number} [a]\n\
                     * @param {number} b */\n\
                    function initialized(a, b = 0) {}\n\
                    /** @param {object} opts\n\
                     * @param {number} [opts.value]\n\
                     * @param {number} tail */\n\
                    function property(opts, tail) {}\n\
                    /** prose `@param {number} [a]` */\n\
                    function prose(a, b) {}\n";
    let diagnostics = checked_file_diags_with("a.js", text, &options);
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.0 != 1016),
        "ordered/initialized/property/prose faces must not produce TS1016: {diagnostics:?}"
    );

    let ts_diagnostics = checked_file_diags_with(
        "a.ts",
        "/** @param {number} [a] */\nfunction typed(a: number, b: number) {}\n",
        &options,
    );
    assert!(
        ts_diagnostics.iter().all(|diagnostic| diagnostic.0 != 1016),
        "JSDoc optionality is a JavaScript-only effective token: {ts_diagnostics:?}"
    );
}

// ---- M7 8.1v JSDoc template missing-name grammar ----

#[test]
fn jsdoc_template_constraint_requires_a_parameter_name() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/** @template {T} */\n\
                    function inline() {}\n\
                    /**\n\
                     * @template {NoLongerAllowed}\n\
                     * @template U\n\
                     */\n\
                    function multiline() {}\n";
    with_program_state(&[("a.js", text)], &options, |state| {
        let mut diagnostics = state
            .binder
            .source(0)
            .js_doc_diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 1069)
            .map(|diagnostic| {
                (
                    diagnostic.start.expect("TS1069 start"),
                    diagnostic.length.expect("TS1069 length"),
                    diagnostic.message_text().to_owned(),
                )
            })
            .collect::<Vec<_>>();
        diagnostics.sort_by_key(|diagnostic| diagnostic.0);
        let inline_start = text.find("{T}").expect("inline constraint") + "{T}".len();
        let next_tag_start = text.find("\n* @template U").expect("next template tag") + "\n".len();
        let expected = [inline_start, next_tag_start].map(|start| {
            (
                start as u32,
                1,
                "Unexpected token. A type parameter name was expected without curly braces."
                    .to_owned(),
            )
        });
        assert_eq!(diagnostics, expected);
    });
}

#[test]
fn jsdoc_template_missing_name_preserves_valid_and_non_tag_faces() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/** @template {{ value: number }} T,U */\n\
                    function constrained() {}\n\
                    /** @template {number} [T=number] */\n\
                    function defaulted() {}\n\
                    /** @template T */\n\
                    function plain() {}\n\
                    /** prose `@template {T}` */\n\
                    function prose() {}\n";
    let diagnostics = jsdoc_parse_diag_rows("a.js", text, &options);
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.0 != 1069),
        "valid/non-tag template faces must not produce TS1069: {diagnostics:?}"
    );

    let ts_diagnostics = jsdoc_parse_diag_rows(
        "a.ts",
        "/** @template {T} */\nfunction typed() {}\n",
        &options,
    );
    assert!(
        ts_diagnostics.iter().all(|diagnostic| diagnostic.0 != 1069),
        "JSDoc parser diagnostics are JavaScript-only: {ts_diagnostics:?}"
    );
}

// ---- M7 8.1w JSDoc identifier-name recovery grammar ----

#[test]
fn jsdoc_identifier_name_recovery_reports_missing_and_invalid_names() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/** @augments */\n\
                    class Augments {}\n\
                    /** @implements */\n\
                    class Implements {}\n\
                    /**\n\
                     * @property {string} #id\n\
                     * @param *\n\
                     * @param {number}\n\
                     * * y\n\
                     * @param {number} * z\n\
                     */\n\
                    function invalid(x, y, z) {}\n";
    with_program_state(&[("a.js", text)], &options, |state| {
        let mut diagnostics = state
            .binder
            .source(0)
            .js_doc_diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 1003)
            .map(|diagnostic| {
                (
                    diagnostic.start.expect("TS1003 start"),
                    diagnostic.length.expect("TS1003 length"),
                    diagnostic.message_text().to_owned(),
                )
            })
            .collect::<Vec<_>>();
        diagnostics.sort_by_key(|diagnostic| diagnostic.0);
        let expected_starts = [
            text.find("@augments").expect("augments tag") + "@augments".len(),
            text.find("@implements").expect("implements tag") + "@implements".len(),
            text.find("@param *").expect("inline star parameter") + "@param ".len(),
            text.find("\n* * y").expect("wrapped star parameter") + "\n* ".len(),
            text.find("@param {number} * z")
                .expect("typed star parameter")
                + "@param {number} ".len(),
        ];
        assert_eq!(
            diagnostics,
            expected_starts
                .map(|start| (start as u32, 0, "Identifier expected.".to_owned()))
                .to_vec()
        );
    });
}

#[test]
fn jsdoc_identifier_name_recovery_preserves_valid_wrapping_and_non_tags() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/** @augments {Base} */\n\
                    class Augments {}\n\
                    /** @implements Base */\n\
                    class Implements {}\n\
                    /**\n\
                     * @property {string} id\n\
                     * @param\n\
                     * {number} x\n\
                     * @param {number}\n\
                     * y\n\
                     * @param {number} z\n\
                     * argument z\n\
                     */\n\
                    function valid(x, y, z) {}\n\
                    /** prose `@param *` */\n\
                    function prose(value) {}\n\
                    /** @parameter * */\n\
                    function boundary(value) {}\n";
    let diagnostics = jsdoc_parse_diag_rows("a.js", text, &options);
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.0 != 1003),
        "valid/non-tag JSDoc faces must not produce TS1003: {diagnostics:?}"
    );

    let ts_diagnostics = jsdoc_parse_diag_rows(
        "a.ts",
        "/** @implements */\nclass Typed {}\n/** @param * */\nfunction f(x: number) {}\n",
        &options,
    );
    assert!(
        ts_diagnostics.iter().all(|diagnostic| diagnostic.0 != 1003),
        "JSDoc parser diagnostics are JavaScript-only: {ts_diagnostics:?}"
    );
}

// ---- M7 8.1x JSDoc satisfies required-brace grammar ----

#[test]
fn jsdoc_satisfies_type_expression_requires_braces() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/**\n * @satisfies T1\n */\nconst first = {};\n\
                    const second = /** @satisfies T2 */ ({});\n";
    with_program_state(&[("a.js", text)], &options, |state| {
        let mut diagnostics = state
            .binder
            .source(0)
            .js_doc_diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 1005)
            .map(|diagnostic| {
                (
                    diagnostic.start.expect("TS1005 start"),
                    diagnostic.length.expect("TS1005 length"),
                    diagnostic.message_text().to_owned(),
                )
            })
            .collect::<Vec<_>>();
        diagnostics.sort_by_key(|diagnostic| diagnostic.0);
        let comment_closes = text
            .match_indices("*/")
            .map(|(start, _)| start)
            .collect::<Vec<_>>();
        let first_type =
            text.find("@satisfies T1").expect("multiline satisfies tag") + "@satisfies ".len();
        let second_type =
            text.find("@satisfies T2").expect("inline satisfies tag") + "@satisfies ".len();
        let expected = [
            (first_type, "T1".len(), "'{' expected."),
            (comment_closes[0], 0, "'}' expected."),
            (second_type, "T2".len(), "'{' expected."),
            (comment_closes[1], 0, "'}' expected."),
        ];
        assert_eq!(
            diagnostics,
            expected
                .map(|(start, length, message)| {
                    (start as u32, length as u32, message.to_owned())
                })
                .to_vec()
        );
    });
}

#[test]
fn jsdoc_satisfies_braces_preserve_valid_and_non_tag_faces() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/** @satisfies {T1} */\n\
                    const valid = {};\n\
                    /** prose `@satisfies T1` */\n\
                    const prose = {};\n\
                    /** @satisfiesElse T1 */\n\
                    const boundary = {};\n";
    let diagnostics = jsdoc_parse_diag_rows("a.js", text, &options);
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.0 != 1005),
        "valid/non-tag satisfies faces must not produce TS1005: {diagnostics:?}"
    );

    let ts_diagnostics = jsdoc_parse_diag_rows(
        "a.ts",
        "/** @satisfies T1 */\nconst typed = {};\n",
        &options,
    );
    assert!(
        ts_diagnostics.iter().all(|diagnostic| diagnostic.0 != 1005),
        "JSDoc parser diagnostics are JavaScript-only: {ts_diagnostics:?}"
    );
}

// ---- M7 8.1y JSDoc import-clause `from` grammar ----

#[test]
fn jsdoc_default_import_clause_requires_from_keyword() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/** @import defer * as ns from \"./types\" */\n\
                    /**\n * @import foo\n */\n\
                    /** @import x = require(\"types\") */\n";
    with_program_state(&[("a.js", text)], &options, |state| {
        let mut diagnostics = state
            .binder
            .source(0)
            .js_doc_diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 1005)
            .map(|diagnostic| {
                (
                    diagnostic.start.expect("TS1005 start"),
                    diagnostic.length.expect("TS1005 length"),
                    diagnostic.message_text().to_owned(),
                )
            })
            .collect::<Vec<_>>();
        diagnostics.sort_by_key(|diagnostic| diagnostic.0);
        let defer_star =
            text.find("@import defer *").expect("defer import") + "@import defer ".len();
        let foo_tag = text.find("@import foo").expect("missing-from import");
        let foo_close = foo_tag + text[foo_tag..].find("*/").expect("foo comment close");
        let import_equals =
            text.find("@import x =").expect("import-equals spelling") + "@import x ".len();
        let expected = [(defer_star, 1), (foo_close, 0), (import_equals, 1)];
        assert_eq!(
            diagnostics,
            expected
                .map(|(start, length)| { (start as u32, length, "'from' expected.".to_owned()) })
                .to_vec()
        );
    });
}

#[test]
fn jsdoc_import_from_preserves_valid_and_non_tag_faces() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/** @import Foo from \"./foo\" */\n\
                    /** @import * as ns from \"./foo\" */\n\
                    /** @import { Bar } from \"./foo\" */\n\
                    /** @import Foo, { Bar } from \"./foo\" */\n\
                    /** prose `@import foo` */\n\
                    /** @imports foo */\n";
    let diagnostics = jsdoc_parse_diag_rows("a.js", text, &options);
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.0 != 1005),
        "valid/non-tag import faces must not produce TS1005: {diagnostics:?}"
    );

    let ts_diagnostics = jsdoc_parse_diag_rows("a.ts", "/** @import foo */\n", &options);
    assert!(
        ts_diagnostics.iter().all(|diagnostic| diagnostic.0 != 1005),
        "JSDoc parser diagnostics are JavaScript-only: {ts_diagnostics:?}"
    );
}

// ---- M7 8.1aa JSDoc import module-specifier expression grammar ----

#[test]
fn jsdoc_import_module_specifier_requires_an_expression() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = concat!(
        "/**\n",
        " * @import\n",
        " */\n",
        "/**\n",
        " * @import foo\n",
        " */\n",
        "/**\n",
        " * @import foo from\n",
        " */\n",
    );
    with_program_state(&[("a.js", text)], &options, |state| {
        let mut diagnostics = state
            .binder
            .source(0)
            .js_doc_diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 1109)
            .map(|diagnostic| {
                (
                    diagnostic.start.expect("TS1109 start"),
                    diagnostic.length.expect("TS1109 length"),
                    diagnostic.message_text().to_owned(),
                )
            })
            .collect::<Vec<_>>();
        diagnostics.sort_by_key(|diagnostic| diagnostic.0);
        let bare = text.find("@import\n").expect("bare import tag") + "@import".len();
        let default = text.find("@import foo\n").expect("default import tag") + "@import foo".len();
        let from = text
            .find("@import foo from\n")
            .expect("missing module specifier")
            + "@import foo from".len();
        let expected = [(bare, 1), (default, 0), (from, 0)];
        assert_eq!(
            diagnostics,
            expected
                .map(|(start, length)| {
                    (start as u32, length, "Expression expected.".to_owned())
                })
                .to_vec()
        );
    });
}

#[test]
fn jsdoc_import_module_specifier_preserves_valid_and_non_tag_faces() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = concat!(
        "/** @import \"./side-effect\" */\n",
        "/** @import Foo from \"./foo\" */\n",
        "/** @import * as ns from \"./foo\" */\n",
        "/** @import { Bar } from \"./foo\" */\n",
        "/** prose `@import` */\n",
        "/** @imports */\n",
        "/* @import */\n",
        "const text = '/** @import */';\n",
    );
    let diagnostics = jsdoc_parse_diag_rows("a.js", text, &options)
        .into_iter()
        .filter(|diagnostic| diagnostic.0 == 1109)
        .collect::<Vec<_>>();
    assert_eq!(
        diagnostics,
        [(
            1109,
            text.find("\"./side-effect\"").unwrap() as u32,
            1,
            "Expression expected.".to_owned(),
        )]
    );

    let ts_diagnostics = jsdoc_parse_diag_rows("a.ts", "/**\n * @import\n */\n", &options);
    assert!(
        ts_diagnostics.iter().all(|diagnostic| diagnostic.0 != 1109),
        "JSDoc parser diagnostics are JavaScript-only: {ts_diagnostics:?}"
    );
}

// ---- M7 8.1ab JSDoc type-reference recovery grammar ----

#[test]
fn jsdoc_type_reference_recovery_reports_exact_tokens() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = concat!(
        "/**\n",
        " * @template {string | number} [T=]\n",
        " * @typedef {[T]} EmptyDefault\n",
        " */\n",
        "/**\n",
        "   @typedef {{\n",
        "     foo:\n",
        "     *,\n",
        "     bar:\n",
        "     *\n",
        "   }} Broken\n",
        " */\n",
    );
    with_program_state(&[("a.js", text)], &options, |state| {
        let mut diagnostics = state
            .binder
            .source(0)
            .js_doc_diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 1110)
            .map(|diagnostic| {
                (
                    diagnostic.start.expect("TS1110 start"),
                    diagnostic.length.expect("TS1110 length"),
                    diagnostic.message_text().to_owned(),
                )
            })
            .collect::<Vec<_>>();
        diagnostics.sort_by_key(|diagnostic| diagnostic.0);
        let expected = [
            (
                text.find("[T=]").expect("empty template default") + "[T=".len(),
                1,
            ),
            (
                text.find("*,").expect("standalone star before comma") + "*".len(),
                1,
            ),
            (text.find("}} Broken").expect("closing typedef brace"), 1),
        ];
        assert_eq!(
            diagnostics,
            expected
                .map(|(start, length)| { (start as u32, length, "Type expected.".to_owned()) })
                .to_vec()
        );
    });
}

#[test]
fn jsdoc_type_reference_recovery_preserves_valid_and_non_tag_faces() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = concat!(
        "/** @template {string | number} [T=string] */\n",
        "/** @template {string | number} [T] */\n",
        "/** @templates {string | number} [T=] */\n",
        "/** prose `@template {string | number} [T=]` */\n",
        "/**\n",
        " * @typedef {{\n",
        " *   foo:\n",
        " *   *,\n",
        " *   bar:\n",
        " *   *\n",
        " * }} ValidWithStars\n",
        " */\n",
        "/**\n",
        "   @typedef {{\n",
        "     foo:\n",
        "     string,\n",
        "     bar:\n",
        "     number\n",
        "   }} ValidWithoutStars\n",
        " */\n",
        "/* @template {number} [T=] */\n",
        "const text = '/** @template {number} [T=] */';\n",
    );
    let diagnostics = jsdoc_parse_diag_rows("a.js", text, &options);
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.0 != 1110),
        "valid/non-tag type-reference faces must not produce TS1110: {diagnostics:?}"
    );

    let ts_diagnostics = jsdoc_parse_diag_rows(
        "a.ts",
        "/** @template {number} [T=] */\nconst value = 1;\n",
        &options,
    );
    assert!(
        ts_diagnostics.iter().all(|diagnostic| diagnostic.0 != 1110),
        "JSDoc parser diagnostics are JavaScript-only: {ts_diagnostics:?}"
    );
}

// ---- M7 8.1ac JSDoc expected-close-brace recovery grammar ----

#[test]
fn jsdoc_expected_close_brace_reports_exact_recovery_tokens() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = concat!(
        "/**\n",
        " * @param {number?[]} a\n",
        " * @param {...number?!} b\n",
        " * @param {...number?[]!} c\n",
        " * @typedef {C~A} C_B\n",
        " * @param {C~A} d\n",
        " */\n",
        "function f(a, b, c, d) {}\n",
    );
    with_program_state(&[("a.js", text)], &options, |state| {
        let mut diagnostics = state
            .binder
            .source(0)
            .js_doc_diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 1005)
            .map(|diagnostic| {
                (
                    diagnostic.start.expect("TS1005 start"),
                    diagnostic.length.expect("TS1005 length"),
                    diagnostic.message_text().to_owned(),
                )
            })
            .collect::<Vec<_>>();
        diagnostics.sort_by_key(|diagnostic| diagnostic.0);
        let expected = [
            (
                text.find("{number?[]}").expect("postfix nullable array") + "{number".len(),
                1,
            ),
            (
                text.find("{...number?!}")
                    .expect("nullable before non-null")
                    + "{...number".len(),
                1,
            ),
            (
                text.find("{...number?[]!}")
                    .expect("nullable before non-null array")
                    + "{...number".len(),
                1,
            ),
            (
                text.find("{C~A}").expect("typedef inner namepath") + "{C".len(),
                1,
            ),
            (
                text.rfind("{C~A}").expect("parameter inner namepath") + "{C".len(),
                1,
            ),
        ];
        assert_eq!(
            diagnostics,
            expected
                .map(|(start, length)| { (start as u32, length, "'}' expected.".to_owned()) })
                .to_vec()
        );
    });
}

#[test]
fn jsdoc_expected_close_brace_preserves_valid_and_non_tag_faces() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = concat!(
        "/**\n",
        " * @param {?number[]} a\n",
        " * @param {number?} b\n",
        " * @param {!number[]} c\n",
        " * @param {number!} d\n",
        " * @param {(number[])?} e\n",
        " * @param {[number, number?]} f\n",
        " * @param {T extends U ? [] : T} g\n",
        " * @param {Foo.Bar} h\n",
        " * @typedef {C.A} C_A\n",
        " * @params {number?[]} prose\n",
        " * prose `@param {number?[]} prose`\n",
        " */\n",
        "function valid(a, b, c, d, e, f, g, h) {}\n",
        "/* @param {number?[]} ordinary */\n",
        "const text = '/** @param {number?[]} string */';\n",
    );
    let diagnostics = jsdoc_parse_diag_rows("a.js", text, &options);
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.0 != 1005),
        "valid/non-tag close-brace faces must not produce TS1005: {diagnostics:?}"
    );

    let ts_diagnostics = jsdoc_parse_diag_rows(
        "a.ts",
        "/** @param {number?[]} value */\nfunction f(value: number) {}\n",
        &options,
    );
    assert!(
        ts_diagnostics.iter().all(|diagnostic| diagnostic.0 != 1005),
        "JSDoc parser diagnostics are JavaScript-only: {ts_diagnostics:?}"
    );
}

// ---- M7 8.1ad JSDoc template missing-equals recovery grammar ----

#[test]
fn jsdoc_template_missing_equals_reports_the_closing_bracket() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = concat!(
        "/**\n",
        " * @template {string | number} [T]\n",
        " * @typedef {[T]} MissingDefault\n",
        " */\n",
    );
    with_program_state(&[("a.js", text)], &options, |state| {
        let diagnostics = state
            .binder
            .source(0)
            .js_doc_diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 1005)
            .map(|diagnostic| {
                (
                    diagnostic.start.expect("TS1005 start"),
                    diagnostic.length.expect("TS1005 length"),
                    diagnostic.message_text().to_owned(),
                )
            })
            .collect::<Vec<_>>();
        let start = text.find("[T]").expect("bracketed template parameter") + "[T".len();
        assert_eq!(
            diagnostics,
            vec![(start as u32, 1, "'=' expected.".to_owned())]
        );
    });
}

#[test]
fn jsdoc_template_recovery_classifies_adjacent_malformed_faces() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = concat!(
        "/** @template T */\n",
        "/** @template {number} U */\n",
        "/** @template [T=string] */\n",
        "/** @template {number} [U=number] */\n",
        "/** @template [T=] */\n",
        "/** @template [] */\n",
        "/** @template [const T] */\n",
        "/** @templates [T] */\n",
        "/** prose `@template [T]` */\n",
        "/* @template [T] */\n",
        "const text = '/** @template [T] */';\n",
    );
    let diagnostics = jsdoc_parse_diag_rows("a.js", text, &options);
    assert_eq!(
        diagnostics,
        [
            (
                1110,
                (text.find("[T=]").unwrap() + "[T=".len()) as u32,
                1,
                "Type expected.".to_owned(),
            ),
            (
                1069,
                (text.find("[]").unwrap() + "[".len()) as u32,
                1,
                "Unexpected token. A type parameter name was expected without curly braces."
                    .to_owned(),
            ),
            (
                1005,
                (text.find("[const T]").unwrap() + "[const T".len()) as u32,
                1,
                "'=' expected.".to_owned(),
            ),
        ]
    );

    let ts_diagnostics = jsdoc_parse_diag_rows("a.ts", "/** @template [T] */\n", &options);
    assert!(
        ts_diagnostics.is_empty(),
        "JSDoc parser diagnostics are JavaScript-only: {ts_diagnostics:?}"
    );
}

// ---- M7 8.1s JSDoc satisfies semantics ----

#[test]
fn jsdoc_satisfies_semantics_reports_named_primitive_and_function_targets() {
    // checkJsdocSatisfiesTag15.ts is a standard-lib fixture. Keep
    // the unit program's Array target live as well: without it,
    // getTypeFromArrayOrTupleTypeNode's exact missing-global arm
    // intentionally collapses `number[]` to `{}` (pinned by the
    // no-lib sibling below).
    let lib = "interface Array<T> {}\n";
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/**\n\
                     * @typedef {Object} Required\n\
                     * @property {number} value\n\
                     */\n\
                    const object = /** @satisfies {Required} */ ({});\n\
                    /** @satisfies {string} */\n\
                    const primitive = (1);\n\
                    /**\n\
                     * @satisfies {(a: string, ...args: number[]) => void}\n\
                     * @param {string} a\n\
                     * @param {string} b\n\
                     */\n\
                    const callable = (a, b) => {};\n\
                    /**\n\
                     * @satisfies {(a: string, ...args: number[]) => void}\n\
                     * @param {string} a\n\
                     */\n\
                    const compatible = (a) => {};\n";
    with_program_state(&[("lib.d.ts", lib), ("a.js", text)], &options, |state| {
        state.check_source_file(1);
        let diagnostics = state
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 1360)
            .map(|diagnostic| {
                (
                    diagnostic.start.expect("TS1360 start"),
                    diagnostic.length.expect("TS1360 length"),
                    diagnostic.message_text().to_owned(),
                )
            })
            .collect::<Vec<_>>();
        let tags = text
            .match_indices("@satisfies")
            .map(|(start, _)| ((start + 1) as u32, "satisfies".len() as u32))
            .collect::<Vec<_>>();
        assert_eq!(tags.len(), 4, "the fourth tag is the non-firing sibling");
        assert_eq!(
                diagnostics,
                [
                    (
                        tags[0].0,
                        tags[0].1,
                        "Type '{}' does not satisfy the expected type 'Required'.".to_owned(),
                    ),
                    (
                        tags[1].0,
                        tags[1].1,
                        "Type 'number' does not satisfy the expected type 'string'.".to_owned(),
                    ),
                    (
                        tags[2].0,
                        tags[2].1,
                        "Type '(a: string, b: string) => void' does not satisfy the expected type '(a: string, ...args: number[]) => void'.".to_owned(),
                    ),
                ]
            );
    });
}

#[test]
fn jsdoc_satisfies_no_lib_rest_array_target_uses_empty_object_face() {
    // tsc-port boundary:
    // getTypeFromArrayOrTupleTypeNode @6.0.3
    // (_tsc.js:61118-61137) maps a missing global Array target
    // (`emptyGenericType`) to `emptyObjectType`; and
    // canReuseTypeNodeAnnotation (_tsc.js:50932-50955) cannot
    // recover the written ArrayType without an enclosing
    // declaration. A pure no-lib program therefore prints `{}`.
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/**\n\
                     * @satisfies {(a: string, ...args: number[]) => void}\n\
                     * @param {string} a\n\
                     * @param {string} b\n\
                     */\n\
                    const callable = (a, b) => {};\n";
    with_program_state(&[("a.js", text)], &options, |state| {
        state.check_source_file(0);
        let diagnostics = state
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 1360)
            .map(|diagnostic| diagnostic.message_text().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            diagnostics,
            [
                "Type '(a: string, b: string) => void' does not satisfy the expected type \
                     '(a: string, ...args: {}) => void'."
                    .to_owned()
            ]
        );
    });
}

#[test]
fn jsdoc_satisfies_missing_property_keeps_relation_chain_and_declaration() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/**\n\
                     * @typedef {Object} Required\n\
                     * @property {number} required\n\
                     */\n\
                    const value = /** @satisfies {Required} */ ({});\n";
    with_program_state(&[("a.js", text)], &options, |state| {
        state.check_source_file(0);
        let diagnostic = state
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code() == 1360)
            .expect("TS1360");
        fn flatten(chain: &tsc_diagnostics::MessageChain, codes: &mut Vec<u32>) {
            codes.push(chain.code);
            for child in &chain.next {
                flatten(child, codes);
            }
        }
        let mut codes = Vec::new();
        flatten(&diagnostic.message, &mut codes);
        assert_eq!(codes, [1360, 2741]);
        let related = diagnostic.related.first().expect("TS2728");
        assert_eq!(diagnostic.related.len(), 1);
        assert_eq!(related.file_name.as_deref(), Some("a.js"));
        let property_start = text.find("@property").expect("property tag");
        assert_eq!(
            (related.start, related.length),
            (
                Some(property_start as u32),
                Some(
                    text[property_start..text.find("*/").expect("JSDoc close")]
                        .encode_utf16()
                        .count() as u32
                ),
            )
        );
        assert_eq!(related.message.code, 2728);
        assert_eq!(related.message.text, "'required' is declared here.");
    });
}

#[test]
fn jsdoc_satisfies_callable_elaboration_and_nearest_decline_are_both_reported() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "const callable = () => 1;\n\
                    const didYouMean = /** @satisfies {number} */ (callable);\n\
                    const ordinary = /** @satisfies {string} */ (callable);\n";
    with_program_state(&[("a.js", text)], &options, |state| {
        state.check_source_file(0);
        let diagnostics = state
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 1360)
            .collect::<Vec<_>>();
        assert_eq!(diagnostics.len(), 2, "{:#?}", state.diagnostics);
        assert_eq!(
            diagnostics[0]
                .related
                .iter()
                .map(|related| related.message.code)
                .collect::<Vec<_>>(),
            [6212]
        );
        assert!(
            diagnostics[1]
                .related
                .iter()
                .all(|related| related.message.code != 6212),
            "a non-matching return type must decline did-you-mean elaboration: {:#?}",
            diagnostics[1]
        );
    });
}

#[test]
fn jsdoc_satisfies_semantics_preserves_contextual_object_and_non_tag_faces() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/**\n\
                     * @typedef {Object} Required\n\
                     * @property {number} value\n\
                     */\n\
                    /** @satisfies {Required} */\n\
                    const contextualMissing = {};\n\
                    const inlineValid = /** @satisfies {Required} */ ({ value: 1 });\n\
                    const inlineExcess = /** @satisfies {Required} */ ({ value: 1, extra: 2 });\n\
                    /** prose `@satisfies {string}` */\n\
                    const prose = 1;\n\
                    /** @satisfiesElse {string} */\n\
                    const boundary = 1;\n\
                    const text = \"/** @satisfies {string} */ (1)\";\n";
    let diagnostics = checked_file_diags_with("a.js", text, &options);
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.0 != 1360),
        "contextual/assignable/non-tag faces must not produce TS1360: {diagnostics:?}"
    );
}

// ---- M7 8.1r JSDoc cast type-predicate grammar ----

#[test]
fn jsdoc_cast_type_predicate_reports_invalid_return_type_position() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "let value;\n\
                    if (/** @type {value is string} */ (value)) {}\n";
    with_program_state(&[("a.js", text)], &options, |state| {
        state.check_source_file(0);
        let diagnostic = state
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code() == 1228)
            .expect("TS1228");
        let start = text.find("value is string").expect("type predicate text") as u32;
        assert_eq!(
                (
                    diagnostic.start,
                    diagnostic.length,
                    diagnostic.message_text(),
                ),
                (
                    Some(start),
                    Some("value is string".len() as u32),
                    "A type predicate is only allowed in return type position for functions and methods.",
                )
            );
    });
}

#[test]
fn jsdoc_cast_type_predicate_preserves_other_type_faces() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "let value;\n\
                    const normal = /** @type {string} */ (value);\n\
                    const boundary = /** @type {value isomorphic} */ (value);\n\
                    const object = /** @type {{ is: string }} */ ({ is: \"\" });\n\
                    const otherTag = /** @types {value is string} */ (value);\n\
                    const text = \"/** @type {value is string} */ (value)\";\n\
                    const prose = /** prose `@type {value is string}` */ (value);\n\
                    const ordinary = /* @type {value is string} */ (value);\n";
    let diagnostics = checked_file_diags_with("a.js", text, &options);
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.0 != 1228),
        "non-predicate/non-tag faces must not produce TS1228: {diagnostics:?}"
    );
}

// ---- M7 8.1f JSDoc nullable/non-nullable grammar ----

#[test]
fn jsdoc_nullable_and_non_nullable_types_report_typescript_suggestions() {
    let options = CompilerOptions {
        strict: Some(true),
        ..CompilerOptions::default()
    };
    assert_eq!(
            checked_diags_with(
                "var a: ?number;\n\
                 var b: number?;\n\
                 var c: !string;\n\
                 var d: string!;\n\
                 var e: ?void;\n",
                &options,
            ),
            [
                (
                    17020,
                    7,
                    7,
                    "'?' at the start of a type is not valid TypeScript syntax. Did you mean to write 'number | null | undefined'?"
                        .to_owned()
                ),
                (
                    17019,
                    23,
                    7,
                    "'?' at the end of a type is not valid TypeScript syntax. Did you mean to write 'number | undefined'?"
                        .to_owned()
                ),
                (
                    17020,
                    39,
                    7,
                    "'!' at the start of a type is not valid TypeScript syntax. Did you mean to write 'string'?"
                        .to_owned()
                ),
                (
                    17019,
                    55,
                    7,
                    "'!' at the end of a type is not valid TypeScript syntax. Did you mean to write 'string'?"
                        .to_owned()
                ),
                (
                    17020,
                    71,
                    5,
                    "'?' at the start of a type is not valid TypeScript syntax. Did you mean to write 'void'?"
                        .to_owned()
                ),
            ]
        );
}

// ---- M8-P12 JSDoc-only source type grammar ----

#[test]
fn jsdoc_only_source_types_report_8020_at_the_upstream_spans() {
    let text = "interface Array<T> {}\n\
                    var dotted: Array.<number>;\n\
                    var callable: function(this: number, string): string;\n\
                    var all: * = 1;\n\
                    var unknown: ? = undefined;\n\
                    var ordinary: Array<number>;\n";
    let diagnostics = checked_diags(text)
        .into_iter()
        .filter(|diagnostic| diagnostic.0 == 8020)
        .collect::<Vec<_>>();
    let callable = "function(this: number, string): string";
    assert_eq!(
        diagnostics,
        [
            (
                8020,
                text.find(".<").expect("JSDoc dot") as u32,
                1,
                "JSDoc types can only be used inside documentation comments.".to_owned(),
            ),
            (
                8020,
                text.find(callable).expect("JSDoc function type") as u32,
                callable.len() as u32,
                "JSDoc types can only be used inside documentation comments.".to_owned(),
            ),
            (
                8020,
                text.find("* =").expect("JSDoc all type") as u32,
                1,
                "JSDoc types can only be used inside documentation comments.".to_owned(),
            ),
            (
                8020,
                text.find("? =").expect("JSDoc unknown type") as u32,
                1,
                "JSDoc types can only be used inside documentation comments.".to_owned(),
            ),
        ]
    );
}

#[test]
fn jsdoc_only_source_type_8020_is_silent_in_js_files() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let diagnostics = checked_file_diags_with(
        "a.js",
        "var dotted: Array.<number>;\n\
             var callable: function(this: number, string): string;\n\
             var all: * = 1;\n\
             var unknown: ? = undefined;\n",
        &options,
    );
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.0 != 8020),
        "TS8020 is TypeScript-source-only: {diagnostics:?}"
    );
}

#[test]
fn jsdoc_accessibility_on_private_name_uses_tag_span_and_publishes_checked_js() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "\r\nclass A {\r\n    /**\r\n     * @public\r\n     */\r\n    #a = 1;\r\n}\r\n";
    with_program_state(&[("a.js", text)], &options, |state| {
        state.check_source_file(0);
        let diagnostic = state
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code() == 18010)
            .expect("TS18010");
        assert_eq!((diagnostic.start, diagnostic.length), (Some(29), Some(14)));
    });
}

#[test]
fn jsdoc_accessibility_rejects_non_attached_and_non_tag_comments() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/** @public */\n\
                    class A {\n\
                      #unattached = 1;\n\
                      /** prose `@public` */\n\
                      #prose = 1;\n\
                      /* @private */\n\
                      #ordinary = 1;\n\
                      /** @publicized */\n\
                      #boundary = 1;\n\
                      /** @protected */\n\
                      visible = 1;\n\
                      #intervening = 1;\n\
                    }\n";
    let diagnostics = checked_file_diags_with("a.js", text, &options);
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.0 != 18010),
        "negative attachment probes must not produce TS18010: {diagnostics:?}"
    );
}

#[test]
fn jsdoc_import_tag_bare_with_reports_parser_and_checker_diagnostics() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let text = "/** @import * as f from \"./foo\" with */";
    with_program_state(&[("a.js", text)], &options, |state| {
        state.check_source_file(0);
        let diagnostic = state
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code() == 1464)
            .expect("TS1464");
        assert_eq!((diagnostic.start, diagnostic.length), (Some(32), Some(4)));
        let diagnostic = state
            .binder
            .source(0)
            .js_doc_diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code() == 1005)
            .expect("TS1005");
        assert_eq!(
            (
                diagnostic.start,
                diagnostic.length,
                diagnostic.message_text()
            ),
            (Some(37), Some(0), "'{' expected.")
        );
    });
}

#[test]
fn jsdoc_import_tag_rejects_valid_attributes_prose_and_source_text() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    for text in [
        "/** @import * as f from \"./foo\" with { \"resolution-mode\": \"import\" } */",
        "/** prose `@import * as f from \"./foo\" with` */",
        "/** @imported * as f from \"./foo\" with */",
        "/** @import \"./foo\" with */",
        "/* @import * as f from \"./foo\" with */",
        "const text = '/** @import * as f from \"./foo\" with */';",
    ] {
        let diagnostics = checked_file_diags_with("a.js", text, &options);
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| !matches!(diagnostic.0, 1005 | 1464)),
            "negative JSDoc import probe must not produce TS1005/TS1464: {text:?}: {diagnostics:?}"
        );
    }
    let diagnostics = checked_file_diags_with(
        "a.ts",
        "/** @import * as f from \"./foo\" with */",
        &CompilerOptions::default(),
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| !matches!(diagnostic.0, 1005 | 1464)),
        "JSDoc import tags are checked only in JavaScript: {diagnostics:?}"
    );
}

// ---- M7 8.1c.2 declaration-file source grammar (oracle-pinned) ----

#[test]
fn declaration_file_requires_declare_or_export_on_value_declarations() {
    assert_eq!(
            checked_file_diags_with(
                "a.d.ts",
                "enum E {}\nfunction f(): void;\nclass C {}\n",
                &CompilerOptions::default(),
            ),
            [(
                1046,
                0,
                4,
                "Top-level declarations in .d.ts files must start with either a 'declare' or 'export' modifier."
                    .to_owned()
            )]
        );
}

#[test]
fn declaration_file_allows_type_declarations_and_explicit_value_modifiers() {
    assert_eq!(
            checked_file_diags_with(
                "a.d.ts",
                "interface I {}\ntype T = string;\ndeclare enum E {}\nexport class C {}\nexport default function f(): void;\n",
                &CompilerOptions::default(),
            ),
            []
        );
}

// ---- M7 8.1a modifier/decorator grammar (oracle-pinned) ----

#[test]
fn modifier_order_reports_the_oracle_span_and_message() {
    assert_eq!(
        checked_diags("abstract class C { abstract public p: string; }"),
        [(
            1029,
            28,
            6,
            "'public' modifier must precede 'abstract' modifier.".to_owned()
        )]
    );
    assert_eq!(
        checked_diags("abstract class C { public abstract p: string; }"),
        []
    );
}

#[test]
fn illegal_static_block_decorator_reports_at_the_at_token() {
    let options = CompilerOptions {
        experimental_decorators: true,
        ..CompilerOptions::default()
    };
    assert_eq!(
        checked_diags_with(
            "declare function dec(...args: any[]): any; class C { @dec static {} }",
            &options,
        ),
        [(1206, 53, 1, "Decorators are not valid here.".to_owned())]
    );
    assert_eq!(
        checked_diags_with(
            "declare function dec(...args: any[]): any; class C { static {} }",
            &options,
        ),
        []
    );
}

#[test]
fn modifier_error_suppresses_function_grammar_followers() {
    let diagnostics = checked_diags("public function f<>() {}");
    assert_eq!(
        diagnostics,
        [(
            1044,
            0,
            6,
            "'public' modifier cannot appear on a module or namespace element.".to_owned()
        )]
    );
    assert_eq!(
        checked_diags("function f<>() {}"),
        [(
            1098,
            10,
            2,
            "Type parameter list cannot be empty.".to_owned()
        )]
    );
}

#[test]
fn decorators_split_by_export_carry_related_information() {
    with_program_state(
        &[(
            "a.ts",
            "declare function dec(value: any): any; @dec export @dec class C {}",
        )],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            let diagnostic = state
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code() == 8038)
                .expect("TS8038");
            assert_eq!((diagnostic.start, diagnostic.length), (Some(51), Some(4)));
            assert_eq!(diagnostic.related.len(), 1);
            let related = &diagnostic.related[0];
            assert_eq!(related.message.code, 1486);
            assert_eq!((related.start, related.length), (Some(39), Some(4)));
        },
    );
}

// ---- M7 8.1d.3v regular-expression validator (oracle-pinned) ----

#[test]
fn regex_validator_preserves_utf16_positions_and_target_gates() {
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES5.bits()),
        ..CompilerOptions::default()
    };
    let rows = checked_diags_with("const r = /😀{/u;", &options);
    assert!(rows.iter().any(|row| {
        row.0 == 1508
            && row.1 == 13
            && row.2 == 1
            && row.3 == "Unexpected '{'. Did you mean to escape it with backslash?"
    }));
    assert!(rows.iter().any(|row| {
        row.0 == 1501
            && row.1 == 15
            && row.2 == 1
            && row.3
                == "This regular expression flag is only available when targeting 'es6' or later."
    }));
}

#[test]
fn regex_spelling_message_is_related_to_the_primary() {
    with_program_state(
        &[("a.ts", "const r = /\\p{General_Categor=Letter}/u;")],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            let primary = state
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code() == 1524)
                .expect("TS1524");
            assert_eq!((primary.start, primary.length), (Some(14), Some(15)));
            assert_eq!(primary.related.len(), 1);
            let related = &primary.related[0];
            assert_eq!(related.message.code, 1369);
            assert_eq!(related.message.text, "Did you mean 'General_Category'?");
            assert_eq!(related.file_name, None);
            assert_eq!((related.start, related.length), (Some(14), Some(15)));
        },
    );
}

#[test]
fn regex_validator_is_suppressed_by_any_file_parse_diagnostic() {
    with_program_state_allow_parse_diagnostics(
        &[("a.ts", "const broken = ; const r = /a/z;")],
        &CompilerOptions::default(),
        |state| {
            assert!(!state.binder.source(0).parse_diagnostics.is_empty());
            state.check_source_file(0);
            assert!(
                state
                    .diagnostics
                    .iter()
                    .all(|diagnostic| diagnostic.code() != 1499),
                "the unrelated parse diagnostic suppresses regex validation"
            );
        },
    );
}

#[test]
fn regex_validator_publishes_checked_javascript_diagnostics() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    with_program_state(&[("a.js", "const r = /a/z;")], &options, |state| {
        state.check_source_file(0);
        let diagnostic = state
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code() == 1499)
            .expect("TS1499");
        assert!(diagnostic.start.is_some());
        assert!(diagnostic.length.is_some());
    });
}

// ---- deferred containment (tsrs-native, 7.4 review rework) ----

fn node_of_kind(state: &CheckerState, kind: tsc_syntax::SyntaxKind) -> tsc_syntax::NodeId {
    let source = state.binder.source(0);
    source
        .arena
        .node_ids()
        .find(|&id| source.arena.node(id).kind == kind)
        .unwrap_or_else(|| panic!("no {kind:?} in fixture"))
}

#[test]
fn deferred_containment_skip_requires_the_containment_record() {
    with_program_state(
        &[(
            "a.ts",
            "declare function outer(f: (x: number) => void): void;\nouter(x => {});\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let arrow = node_of_kind(state, tsc_syntax::SyntaxKind::ArrowFunction);
            let call = node_of_kind(state, tsc_syntax::SyntaxKind::CallExpression);
            state
                .partially_checked_ranges
                .entry(0)
                .or_default()
                .push((0, u32::MAX));
            // A Vacant ancestor slot WITHOUT the containment record
            // is the benign mid-fixpoint clear (tsc 77505 `: cached`
            // on a loop-dirty fresh frame) — fully re-resolvable, so
            // the deferred check must run.
            assert!(
                !state.deferred_context_call_reverted(arrow),
                "benign Vacant must not trigger the containment skip"
            );
            state.contained_call_resolutions.insert(call);
            assert!(
                state.deferred_context_call_reverted(arrow),
                "containment-reverted Vacant triggers the skip"
            );
        },
    );
}

#[test]
fn deferred_containment_sees_jsx_children_through_the_opening_element() {
    let options = CompilerOptions {
        jsx: Some(2),
        ..CompilerOptions::default()
    };
    with_program_state(
        &[(
            "a.tsx",
            "declare var React: any;\nconst e = <div>{() => 1}</div>;\n",
        )],
        &options,
        |state| {
            let arrow = node_of_kind(state, tsc_syntax::SyntaxKind::ArrowFunction);
            let opening = node_of_kind(state, tsc_syntax::SyntaxKind::JsxOpeningElement);
            state
                .partially_checked_ranges
                .entry(0)
                .or_default()
                .push((0, u32::MAX));
            assert!(!state.deferred_context_call_reverted(arrow));
            // The resolvedSignature slot lives on the OPENING
            // element — a SIBLING subtree of the children, which an
            // ancestor walk can only reach through the JsxElement
            // hop (the pre-review walk missed it).
            state.contained_call_resolutions.insert(opening);
            assert!(
                state.deferred_context_call_reverted(arrow),
                "children resolve the slot through JsxElement.opening_element"
            );
        },
    );
}

#[test]
fn deferred_containment_sees_jsx_fragment_children_through_the_opening_fragment() {
    let options = CompilerOptions {
        jsx: Some(2),
        ..CompilerOptions::default()
    };
    with_program_state(
        &[(
            "a.tsx",
            "declare var React: any;\nconst e = <>{() => 1}</>;\n",
        )],
        &options,
        |state| {
            let arrow = node_of_kind(state, tsc_syntax::SyntaxKind::ArrowFunction);
            let opening = node_of_kind(state, tsc_syntax::SyntaxKind::JsxOpeningFragment);
            state
                .partially_checked_ranges
                .entry(0)
                .or_default()
                .push((0, u32::MAX));
            assert!(!state.deferred_context_call_reverted(arrow));
            // JsxOpeningFragment is a LEAF — the pre-review walk
            // listed it directly and could never match; the
            // JsxFragment hop is the reachable route.
            state.contained_call_resolutions.insert(opening);
            assert!(
                state.deferred_context_call_reverted(arrow),
                "fragment children resolve the slot through JsxFragment.opening_fragment"
            );
        },
    );
}

// ---- 2636 / 2637 (checkTypeParameterDeferred) — oracle-pinned ----

#[test]
fn interface_out_annotation_on_contravariant_use_reports_2636() {
    let diags = checked_diags("interface Foo<out T> { f: (x: T) => void }\n");
    assert_eq!(
        diags,
        [(
            2636,
            14,
            5,
            "Type 'Foo<sub-T>' is not assignable to type 'Foo<super-T>' as implied by \
                 variance annotation."
                .to_owned()
        )]
    );
}

// ---- tuple renderer (phase-9 9.3a) — every head oracle-probed
// (scratchpad probe-93a: noLib strict, vendored 6.0.3) ----

#[test]
fn tuple_display_labeled_members_render() {
    assert_eq!(
        checked_diags("declare const p: [a: number, b: string];\nconst q: [number] = p;\n"),
        [(
            2322,
            47,
            1,
            "Type '[a: number, b: string]' is not assignable to type '[number]'.".to_owned()
        )]
    );
}

#[test]
fn tuple_display_optional_element_parenthesizes_the_union() {
    // The stored optional element is `string | undefined` (strict,
    // eOPT off) — OptionalTypeNode's postfix parenthesizer wraps
    // it: `[(string | undefined)?]`.
    assert_eq!(
        checked_diags("declare const o: [string?];\nconst n: [number] = o;\n"),
        [(
            2322,
            34,
            1,
            "Type '[(string | undefined)?]' is not assignable to type '[number]'.".to_owned()
        )]
    );
}

#[test]
fn tuple_display_labeled_optional_member_is_unparenthesized() {
    // NamedTupleMember types never parenthesize (factory
    // 22247-22256 applies no rule): `a?: number | undefined`.
    assert_eq!(
        checked_diags("declare const p2: [a?: number];\nconst q2: [string] = p2;\n"),
        [(
            2322,
            38,
            2,
            "Type '[a?: number | undefined]' is not assignable to type '[string]'.".to_owned()
        )]
    );
}

#[test]
fn tuple_display_rest_and_variadic_elements_render() {
    assert_eq!(
        checked_diags("declare const r: [number, ...string[]];\nconst n: [boolean] = r;\n"),
        [(
            2322,
            46,
            1,
            "Type '[number, ...string[]]' is not assignable to type '[boolean]'.".to_owned()
        )]
    );
    // Rest-element unions parenthesize through the ArrayTypeNode
    // wrap: `...(string | boolean)[]`.
    assert_eq!(
        checked_diags(
            "declare const r: [number, ...(string | boolean)[]];\nconst n: [number] = r;\n"
        ),
        [(
            2322,
            58,
            1,
            "Type '[number, ...(string | boolean)[]]' is not assignable to type '[number]'."
                .to_owned()
        )]
    );
    // A generic variadic element renders bare: `...T`.
    assert_eq!(
            checked_diags(
                "function f2<T extends unknown[]>(...args: [string, ...T]) { const x: [number] = args; }\n"
            ),
            [(
                2322,
                66,
                1,
                "Type '[string, ...T]' is not assignable to type '[number]'.".to_owned()
            )]
        );
}

#[test]
fn return_satisfies_operand_elaborates_the_element() {
    // PR #55 review P1: tsc passes the EFFECTIVE check node into
    // checkTypeAssignableToAndOptionallyElaborate (84585-84587) —
    // satisfies strips off, the array literal elaborates, and the
    // element row REPLACES the outer return head.
    assert_eq!(
        checked_diags("function f(): [string] {\n  return ([1] satisfies [number]);\n}\n"),
        [(
            2322,
            36,
            1,
            "Type 'number' is not assignable to type 'string'.".to_owned()
        )]
    );
}

#[test]
fn enum_member_displays_render_qualified() {
    // PR #55 review P1: enum-member literal types print `E.A`
    // (typeToTypeNodeHelper's EnumLike arm, 51367-51399), never
    // their base literal value.
    assert_eq!(
        checked_diags("enum E { A, B }\ndeclare const x: [E.A];\nconst y: [E.B] = x;\n"),
        [(
            2322,
            46,
            1,
            "Type '[E.A]' is not assignable to type '[E.B]'.".to_owned()
        )]
    );
    assert_eq!(
        checked_diags("const enum C { X, Y }\ndeclare const x: [C.X];\nconst y: [C.Y] = x;\n"),
        [(
            2322,
            52,
            1,
            "Type '[C.X]' is not assignable to type '[C.Y]'.".to_owned()
        )]
    );
    // The 51371 single-member collapse: the member type IS the
    // declared type, so the bare enum name prints.
    assert_eq!(
        checked_diags("enum S { Only }\ndeclare const x: [S.Only];\nconst y: [string] = x;\n"),
        [(
            2322,
            49,
            1,
            "Type '[S]' is not assignable to type '[string]'.".to_owned()
        )]
    );
    // The EnumLiteral-stamped declared union prints the enum name
    // BEFORE the union walk.
    assert_eq!(
        checked_diags("enum E { A, B }\ndeclare const x: [E];\nconst y: [string] = x;\n"),
        [(
            2322,
            44,
            1,
            "Type '[E]' is not assignable to type '[string]'.".to_owned()
        )]
    );
    // Mixed unions keep interned order (string interns first).
    assert_eq!(
        checked_diags(
            "enum E { A, B }\ndeclare const x: [E.A | string];\nconst y: [boolean] = x;\n"
        ),
        [(
            2322,
            55,
            1,
            "Type '[string | E.A]' is not assignable to type '[boolean]'.".to_owned()
        )]
    );
    // A BARE enum-literal source generalizes to its base for the
    // head (reportRelationError's literal-source generalization
    // composes with the arm): 'E', not 'E.A'.
    assert_eq!(
        checked_diags("enum E { A, B }\ndeclare const x: E.A;\nconst y: [string] = x;\n"),
        [(
            2322,
            44,
            1,
            "Type 'E' is not assignable to type '[string]'.".to_owned()
        )]
    );
}

#[test]
fn relation_report_normalizes_fresh_enum_member_sources() {
    // isRelatedTo normalizes a fresh literal before handing the
    // failed pair to reportErrorResults. For a single-member enum,
    // that regular member IS the declared enum and prints bare.
    assert_eq!(
        checked_diags("enum S { Only }\ndeclare let u: undefined;\nu = S.Only;\n"),
        [(
            2322,
            42,
            1,
            "Type 'S' is not assignable to type 'undefined'.".to_owned()
        )]
    );
    // Non-firing sibling: a member of a multi-member enum remains
    // qualified because its regular twin is not the enum union.
    assert_eq!(
        checked_diags("enum E { A, B }\ndeclare let u: undefined;\nu = E.A;\n"),
        [(
            2322,
            42,
            1,
            "Type 'E.A' is not assignable to type 'undefined'.".to_owned()
        )]
    );
}

#[test]
fn tuple_display_empty_and_readonly_render() {
    assert_eq!(
        checked_diags("declare const e: [];\nconst n2: [number] = e;\n"),
        [(
            2322,
            27,
            2,
            "Type '[]' is not assignable to type '[number]'.".to_owned()
        )]
    );
    // The readonly TypeOperator wrap rides the 4104 face
    // (tryElaborateArrayLikeErrors' readonly report).
    assert_eq!(
            checked_diags(
                "declare const r: readonly [string, number];\nlet w: [string, number] = r as any;\nw = r;\n"
            ),
            [(
                4104,
                80,
                1,
                "The type 'readonly [string, number]' is 'readonly' and cannot be assigned to \
                 the mutable type '[string, number]'."
                    .to_owned()
            )]
        );
}

#[test]
fn relation_report_elaborates_read_normalized_readonly_source() {
    assert_eq!(
        checked_chain_codes(
            "function f<T extends readonly [unknown]>(source: T, target: [...T]) {\n\
                     target = source;\n\
                 }\n"
        ),
        [[2322, 4104]]
    );
}

// ---- 9.3b anonymous-object display pins (oracle-probed,
// scratchpad probe-93b-pins-final: noLib + strict + noImplicitAny
// matching the unit env) ----

#[test]
fn anonymous_object_display_basic_members_render() {
    assert_eq!(
        checked_diags("declare let a: { x: string; y: number };\na = 1;\n"),
        [(
            2322,
            41,
            1,
            "Type 'number' is not assignable to type '{ x: string; y: number; }'.".to_owned()
        )]
    );
}

#[test]
fn type_display_truncation_state_is_sticky_across_alias_arguments() {
    let short = "type Defaultize<T, D> = T & D;\n\
                     declare let target: Defaultize<{ \
                     property0: number; property1: number; property2: number; \
                     property3: number; property4: number; property5: number; \
                     }, { tail: number }>;\n\
                     target = 1;\n";
    let long = "type Defaultize<T, D> = T & D;\n\
                    declare let target: Defaultize<{ \
                    property0: number; property1: number; property2: number; \
                    property3: number; property4: number; property5: number; \
                    property6: number; property7: number; property8: number; \
                    property9: number; \
                    }, { tail: number }>;\n\
                    target = 1;\n";
    let message = |text| {
        checked_diags(text)
            .into_iter()
            .find(|row| row.0 == 2322)
            .expect("assignment diagnostic")
            .3
    };
    assert_eq!(
        message(short),
        "Type 'number' is not assignable to type \
             'Defaultize<{ property0: number; property1: number; property2: number; \
             property3: number; property4: number; property5: number; }, \
             { tail: number; }>'."
    );
    assert_eq!(
        message(long),
        "Type 'number' is not assignable to type \
             'Defaultize<{ property0: number; property1: number; property2: number; \
             property3: number; property4: number; property5: number; property6: number; \
             property7: number; property8: number; property9: number; }, { ...; }>'."
    );
    let options = CompilerOptions {
        no_error_truncation: Some(true),
        ..CompilerOptions::default()
    };
    assert_eq!(
        checked_diags_with(long, &options)
            .into_iter()
            .find(|row| row.0 == 2322)
            .expect("assignment diagnostic")
            .3,
        "Type 'number' is not assignable to type \
             'Defaultize<{ property0: number; property1: number; property2: number; \
             property3: number; property4: number; property5: number; property6: number; \
             property7: number; property8: number; property9: number; }, \
             { tail: number; }>'."
    );
}

#[test]
fn anonymous_object_display_optional_readonly_member() {
    // The optional member's declared type keeps its undefined tail
    // (strict, eOPT off): `readonly y?: number | undefined`.
    assert_eq!(
        checked_diags("declare let b: { readonly y?: number; z: string };\nb = 1;\n"),
        [(
            2322,
            51,
            1,
            "Type 'number' is not assignable to type \
                 '{ readonly y?: number | undefined; z: string; }'."
                .to_owned()
        )]
    );
}

#[test]
fn anonymous_object_display_property_name_faces() {
    // Quoted names keep their declared quote style, identifier-able
    // and numeric names print bare, non-canonical numeric strings
    // stay quoted ("1e2").
    assert_eq!(
            checked_diags(
                "declare let c: { \"a b\": string; 'c d': number; 1: boolean; \"1e2\": string };\nc = 1;\n"
            ),
            [(
                2322,
                76,
                1,
                "Type 'number' is not assignable to type \
                 '{ \"a b\": string; 'c d': number; 1: boolean; \"1e2\": string; }'."
                    .to_owned()
            )]
        );
}

#[test]
fn anonymous_object_display_index_signatures_precede_properties() {
    assert_eq!(
            checked_diags(
                "declare let d: { p: boolean; [idx: number]: unknown; [k: string]: unknown };\nd = 1;\n"
            ),
            [(
                2322,
                77,
                1,
                "Type 'number' is not assignable to type \
                 '{ [idx: number]: unknown; [k: string]: unknown; p: boolean; }'."
                    .to_owned()
            )]
        );
}

#[test]
fn concrete_mapped_source_displays_as_a_resolved_index_signature() {
    let text = "function f<K extends string>(a: { [P in K]: number }, b: { [P in string]: number }) { a = b; }\n";
    assert_eq!(
        checked_diags(text),
        [(
            2322,
            text.find("a = b").expect("assignment") as u32,
            1,
            "Type '{ [x: string]: number; }' is not assignable to type \
                 '{ [P in K]: number; }'."
                .to_owned()
        )]
    );
}

#[test]
fn error_containing_concrete_mapped_type_keeps_its_declaration_face() {
    with_program_state(
        &[("a.ts", "declare let value: { [P in string]: number };\n")],
        &CompilerOptions::default(),
        |state| {
            let mapped_node = node_of_kind(state, tsc_syntax::SyntaxKind::MappedType);
            let mapped_type = state
                .get_type_from_type_node(mapped_node)
                .expect("mapped type");
            assert!(!state
                .is_generic_mapped_type_state(mapped_type)
                .expect("genericity"));
            state
                .links
                .set_mapped_contains_error(state.speculation_depth, mapped_type);
            assert_eq!(
                state
                    .type_to_string_slice(mapped_type)
                    .expect("mapped display"),
                "{ [P in string]: number; }"
            );
        },
    );
}

#[test]
fn anonymous_object_display_nested_literal_and_union() {
    assert_eq!(
        checked_diags("declare let e: { a: { b: string | undefined } };\ne = 1;\n"),
        [(
            2322,
            49,
            1,
            "Type 'number' is not assignable to type '{ a: { b: string | undefined; }; }'."
                .to_owned()
        )]
    );
}

#[test]
fn anonymous_object_display_same_type_accessor_collapses_to_property() {
    // addPropertyToElementList's accessor fall-through: same
    // read/write type, non-class parent -> the plain property row.
    assert_eq!(
        checked_diags("declare let f: { get p(): string; set p(v: string) };\nf = 1;\n"),
        [(
            2322,
            54,
            1,
            "Type 'number' is not assignable to type '{ p: string; }'.".to_owned()
        )]
    );
}

#[test]
fn display_recursive_object_revisit_uses_alias_or_elision_and_plain_sibling() {
    with_program_state(
        &[("a.ts", "type Recursive = { next: Recursive };\n")],
        &CompilerOptions::default(),
        |state| {
            let literal = node_of_kind(state, tsc_syntax::SyntaxKind::TypeLiteral);
            let recursive = state
                .get_type_from_type_node(literal)
                .expect("recursive type literal");

            state.slice_visited_types.insert(recursive);
            assert_eq!(
                state
                    .anonymous_object_type_to_string_slice(recursive, false)
                    .expect("recursive alias revisit")
                    .0,
                "Recursive"
            );
            state.slice_visited_types.remove(&recursive);

            // Nearest non-firing sibling: the first visit walks
            // the ordinary object members.
            assert_eq!(
                state
                    .type_node_from_object_type_slice(recursive, false)
                    .expect("first structural visit")
                    .0,
                "{ next: Recursive; }"
            );

            // A symbol-less revisit takes createElidedInformationPlaceholder;
            // its corresponding first visit remains the empty type literal.
            let anonymous = state.create_resolved_empty_anonymous_type(None);
            state.slice_visited_types.insert(anonymous);
            assert_eq!(
                state
                    .anonymous_object_type_to_string_slice(anonymous, false)
                    .expect("symbol-less revisit")
                    .0,
                "..."
            );
            state.slice_visited_types.remove(&anonymous);
            assert_eq!(
                state
                    .type_node_from_object_type_slice(anonymous, false)
                    .expect("empty first visit")
                    .0,
                "{}"
            );
        },
    );
}

#[test]
fn anonymous_object_display_method_member_renders() {
    // 9.3b2 signature rung: the method face renders
    // (oracle-probed byte-exact).
    assert_eq!(
        checked_diags("declare let g: { m(): void };\ng = 1;\n"),
        [(
            2322,
            30,
            1,
            "Type 'number' is not assignable to type '{ m(): void; }'.".to_owned()
        )]
    );
}

#[test]
fn checked_js_empty_object_literal_renders_the_empty_face() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        no_implicit_any: Some(true),
        strict_null_checks: Some(true),
        ..CompilerOptions::default()
    };
    with_program_state(
        &[("a.js", "function f(a = null) { a = {}; }\n")],
        &options,
        |state| {
            state.check_source_file(0);
            assert_eq!(
                diag_rows(state),
                [(
                    2322,
                    23,
                    1,
                    "Type '{}' is not assignable to type 'null'.".to_owned()
                )]
            );
        },
    );
}

#[test]
fn jsdoc_intended_object_type_rewrites_only_with_implicit_any_off() {
    let files = [
        (
            "lib.d.ts",
            "interface Object {}\ninterface Array<T> { length: number; [n: number]: T }\n",
        ),
        (
            "a.js",
            "/** @param {Array.<Object>} values */\n\
                 const f = function(values) {};\n\
                 /** @type {string} */\n\
                 let s = f;\n",
        ),
    ];
    let rows = |no_implicit_any| {
        program_diags_with(
            &files,
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                strict: Some(false),
                no_implicit_any: Some(no_implicit_any),
                ..CompilerOptions::default()
            },
            "/",
        )
    };

    assert_eq!(
        rows(false),
        [(
            "a.js".to_owned(),
            2322,
            95,
            1,
            "Type '(values: Array<any>) => void' is not assignable to type 'string'.".to_owned()
        )]
    );
    assert_eq!(
        rows(true),
        [(
            "a.js".to_owned(),
            2322,
            95,
            1,
            "Type '(values: Array<Object>) => void' is not assignable to type 'string'.".to_owned()
        )]
    );
}

#[test]
fn checked_js_async_arrow_argument_renders_promise_signature() {
    // asyncArrowFunction_allowJs.ts's virtual file, byte-for-byte:
    // the failed callback relation must display the ordinary
    // checked-JS arrow structurally on createAnonymousTypeNode's
    // non-isJSConstructor path.
    let text = concat!(
        "\r\n",
        "// Error (good)\r\n",
        "/** @type {function(): string} */\r\n",
        "const a = () => 0\r\n",
        "\r\n",
        "// Error (good)\r\n",
        "/** @type {function(): string} */\r\n",
        "const b = async () => 0\r\n",
        "\r\n",
        "// No error (bad)\r\n",
        "/** @type {function(): string} */\r\n",
        "const c = async () => {\r\n",
        "\treturn 0\r\n",
        "}\r\n",
        "\r\n",
        "// Error (good)\r\n",
        "/** @type {function(): string} */\r\n",
        "const d = async () => {\r\n",
        "\treturn \"\"\r\n",
        "}\r\n",
        "\r\n",
        "/** @type {function(function(): string): void} */\r\n",
        "const f = (p) => {}\r\n",
        "\r\n",
        "// Error (good)\r\n",
        "f(async () => {\r\n",
        "\treturn 0\r\n",
        "})",
    );
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        no_emit: Some(true),
        target: Some(ScriptTarget::ES2017.bits()),
        ..CompilerOptions::default()
    };
    with_program_state(
        &[
            ("globals.d.ts", "interface Promise<T> {}\n"),
            ("file.js", text),
        ],
        &options,
        |state| {
            state.check_source_file(1);
            let rows = diag_rows(state)
                .into_iter()
                .filter(|row| row.0 == 2345)
                .collect::<Vec<_>>();
            assert_eq!(
                rows,
                [(
                    2345,
                    436,
                    13,
                    "Argument of type '() => Promise<number>' is not assignable to parameter \
                         of type '() => string'."
                        .to_owned(),
                )]
            );
        },
    );
}

#[test]
fn checked_js_function_type_tag_relations_render_function_signatures() {
    // checkJsdocTypeTag6.ts's virtual file, byte-for-byte. These
    // are a function expression plus all three `more` declaration
    // forms; none is a JS constructor, so all four source types
    // take the structural signature face.
    let text = concat!(
        "\n",
        "/** @type {number} */\n",
        "function f() {\n",
        "    return 1\n",
        "}\n",
        "\n",
        "/** @type {{ prop: string }} */\n",
        "var g = function (prop) {\n",
        "}\n",
        "\n",
        "/** @type {(a: number) => number} */\n",
        "function add1(a, b) { return a + b; }\n",
        "\n",
        "/** @type {(a: number, b: number) => number} */\n",
        "function add2(a, b) { return a + b; }\n",
        "\n",
        "// TODO: Should be an error since signature doesn't match.\n",
        "/** @type {(a: number, b: number, c: number) => number} */\n",
        "function add3(a, b) { return a + b; }\n",
        "\n",
        "// Confirm initializers are compatible.\n",
        "// They can't have more parameters than the type/context.\n",
        "\n",
        "/** @type {() => void} */\n",
        "function funcWithMoreParameters(more) {} // error\n",
        "\n",
        "/** @type {() => void} */\n",
        "const variableWithMoreParameters = function (more) {}; // error\n",
        "\n",
        "/** @type {() => void} */\n",
        "const arrowWithMoreParameters = (more) => {}; // error\n",
        "\n",
        "({\n",
        "  /** @type {() => void} */\n",
        "  methodWithMoreParameters(more) {}, // error\n",
        "});\n",
    );
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        no_emit: Some(true),
        strict: Some(false),
        target: Some(ScriptTarget::ES2015.bits()),
        ..CompilerOptions::default()
    };
    let rows = checked_file_diags_with("test.js", text, &options)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [
            (
                2322,
                90,
                1,
                "Type '(prop: any) => void' is not assignable to type '{ prop: string; }'."
                    .to_owned(),
            ),
            (
                2322,
                643,
                26,
                "Type '(more: any) => void' is not assignable to type '() => void'.".to_owned(),
            ),
            (
                2322,
                734,
                23,
                "Type '(more: any) => void' is not assignable to type '() => void'.".to_owned(),
            ),
            (
                2322,
                817,
                24,
                "Type '(more: any) => void' is not assignable to type '() => void'.".to_owned(),
            ),
        ]
    );
}

#[test]
fn checked_js_constructor_keeps_the_symbol_value_face() {
    // Nearest non-firing sibling: @class makes the function an
    // actual isJSConstructor. It must not fall through to `() =>
    // void`; createAnonymousTypeNode renders symbolToTypeNode
    // under Value meaning.
    let text = "/** @class */\nfunction C() {}\nlet target = \"\";\ntarget = C;\n";
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let rows = checked_file_diags_with("constructor.js", text, &options)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            text.rfind("target").expect("failing assignment") as u32,
            "target".len() as u32,
            "Type 'typeof C' is not assignable to type 'string'.".to_owned(),
        )]
    );
}

// ---- 9.3b2 signature-rung display pins (all byte-exact against
// strict-mode oracle probes; scratchpad probe-93b2-pins) ----

#[test]
fn signature_display_optional_parameter_structural() {
    // declare-let sources render structurally: the optional
    // parameter's symbol type carries `| undefined`.
    assert_eq!(
        checked_diags("declare let f: (x?: number) => void;\nlet t1: string = f;\n"),
        [(
            2322,
            41,
            2,
            "Type '(x?: number | undefined) => void' is not assignable to type 'string'."
                .to_owned()
        )]
    );
}

#[test]
fn signature_display_optional_parameter_annotation_reuse() {
    // The fn-expression twin arms the annotation-reuse channel
    // (getTypeNamesForErrorDisplay's context-sensitive enclosing):
    // the annotation `number` prints without `| undefined`.
    assert_eq!(
        checked_diags("let g = (x?: number) => {};\nlet t2: string = g;\n"),
        [(
            2322,
            32,
            2,
            "Type '(x?: number) => void' is not assignable to type 'string'.".to_owned()
        )]
    );
}

#[test]
fn signature_display_jsdoc_optional_array_annotation_reuse() {
    // classCanExtendConstructorFunction.ts's failing base-method
    // face: visitExistingNodeTreeSymbolsWorker lowers `*[]=`
    // through Optional(Array(All)) to `any[] | undefined`.
    // The `keep` assignment is the nearest non-firing sibling and
    // guards the ordinary JSDoc annotation-reuse route.
    let lib = "interface Array<T> { length: number; [n: number]: T }\n";
    let text = concat!(
        "/** @param {*[]=} supplies */\n",
        "const load = function (supplies) {};\n",
        "/** @type {string} */\n",
        "let target = load;\n",
        "/** @param {number} value */\n",
        "const keep = function (value) {};\n",
        "/** @type {(value: number) => void} */\n",
        "let compatible = keep;\n",
    );
    let rows = program_diags_with(
        &[("lib.d.ts", lib), ("source.js", text)],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(true),
            ..CompilerOptions::default()
        },
        "/",
    )
    .into_iter()
    .filter(|row| row.1 == 2322)
    .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            "source.js".to_owned(),
            2322,
            text.find("target").expect("failing declaration") as u32,
            "target".len() as u32,
            "Type '(supplies?: any[] | undefined) => void' is not assignable to type \
                 'string'."
                .to_owned(),
        )]
    );
}

#[test]
fn reused_jsdoc_type_nodes_lower_to_typescript_nodes() {
    fn render(annotation: &str) -> String {
        let text =
            format!("/** @typedef {{Object}} Box */\n/** @type {{{annotation}}} */\nlet value;\n");
        with_program_state_allow_parse_diagnostics(
            &[("source.js", &text)],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                strict: Some(true),
                ..CompilerOptions::default()
            },
            |state| {
                let expression = {
                    let source = state.binder.source(0);
                    source
                        .arena
                        .node_ids()
                        .filter(|&node| {
                            source.arena.node(node).kind
                                == tsc_syntax::SyntaxKind::JSDocTypeExpression
                        })
                        .last()
                        .expect("JSDoc type expression")
                };
                state
                    .type_annotation_text_slice(expression)
                    .expect("reused JSDoc annotation")
            },
        )
    }

    assert_eq!(render("*"), "any");
    assert_eq!(render("?"), "unknown");
    assert_eq!(render("?number"), "number | null");
    assert_eq!(render("number="), "number | undefined");
    assert_eq!(render("!number"), "number");
    assert_eq!(render("...number"), "number[]");
    assert_eq!(render(""), "any");
    assert_eq!(render("...infer U"), "(infer U)[]");
    assert_eq!(render("keyof ?Box"), "keyof (Box | null)");
    assert_eq!(
        render("(function(): number)|string"),
        "(() => number) | string"
    );
    assert_eq!(
        render("(function(): number)&{p:string}"),
        "(() => number) & { p: string; }"
    );
    assert_eq!(
        render("function(number, ...string): boolean"),
        "(arg0: number, ...args: string[]) => boolean"
    );

    let literal = concat!(
        "/**\n",
        " * @typedef {Object} Box\n",
        " * @property {number} value\n",
        " * @property {string} [label]\n",
        " */\n",
    );
    let rendered_literal = with_program_state(
        &[("source.js", literal)],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(true),
            ..CompilerOptions::default()
        },
        |state| {
            let node = {
                let source = state.binder.source(0);
                source
                    .arena
                    .node_ids()
                    .find(|&node| {
                        source.arena.node(node).kind == tsc_syntax::SyntaxKind::JSDocTypeLiteral
                    })
                    .expect("JSDoc type literal")
            };
            state
                .type_annotation_text_slice(node)
                .expect("reused JSDoc type literal")
        },
    );
    assert_eq!(
        rendered_literal,
        "{ value: number; label?: string | undefined; }"
    );
}

#[test]
fn reused_object_index_signature_is_structural_not_jsdoc_flag_gated() {
    with_program_state(
        &[("a.ts", "type Index = Object<string, number>;\n")],
        &CompilerOptions::default(),
        |state| {
            let reference = node_of_kind(state, tsc_syntax::SyntaxKind::TypeReference);
            assert_eq!(
                state
                    .type_annotation_text_slice(reference)
                    .expect("reused Object index annotation"),
                "{ [x: string]: number; }"
            );
        },
    );
}

#[test]
fn reused_dynamic_computed_member_is_removed() {
    let text = "declare const foo: any; let f = (x: { [foo()]: string }) => {}; let n: number = f;";
    let rows = checked_diags(text)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (text.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(x: {}) => void' is not assignable to type 'number'.".to_owned(),
        )]
    );
}

#[test]
fn reused_missing_member_and_parameter_types_become_any() {
    let text = "let f = (x: { p; m(a); set s(v) }) => {}; let n: number = f;";
    let rows = checked_diags(text)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (text.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(x: { p: any; m(a: any): any; set s(v: any): any; }) => void' \
                 is not assignable to type 'number'."
                .to_owned(),
        )]
    );
}

#[test]
fn reused_js_exports_entity_name_recovers_to_the_typedef() {
    let text = concat!(
        "/** @typedef {{p:string}} Foo */\n",
        "exports.Foo = {};\n",
        "/** @param {exports.Foo} x */\n",
        "const f = function(x) {};\n",
        "/** @type {number} */\n",
        "let n = f;\n",
    );
    let rows = program_diags_with(
        &[("source.js", text)],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(true),
            ..CompilerOptions::default()
        },
        "/",
    )
    .into_iter()
    .filter(|row| row.1 == 2322)
    .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            "source.js".to_owned(),
            2322,
            (text.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(x: Foo) => void' is not assignable to type 'number'.".to_owned(),
        )]
    );
}

#[test]
fn reused_this_type_query_recovers_through_its_this_container() {
    let text = "const C = class { m = (x: typeof this) => {}; }; let n: number = new C().m;";
    let rows = checked_diags(text)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (text.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(x: C) => void' is not assignable to type 'number'.".to_owned(),
        )]
    );

    let named = "class C { m = (x: typeof this) => {}; n: number = this.m; }";
    let rows = checked_diags(named)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            named.find("n: number").expect("failing declaration") as u32,
            1,
            "Type '(x: typeof this) => void' is not assignable to type 'number'.".to_owned(),
        )]
    );

    let object = "const o = { f: function(x: typeof this) {} }; let n: number = o.f;";
    let rows = checked_diags(object)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (object.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(x: { f: ...; }) => void' is not assignable to type 'number'.".to_owned(),
        )]
    );

    let explicit_this =
        "const f = function F(this: { p: string }, x: typeof this) {}; let n: number = f;";
    let rows = checked_diags(explicit_this)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (explicit_this.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(this: { p: string; }, x: { p: string; }) => void' is not assignable \
                 to type 'number'."
                .to_owned(),
        )]
    );
}

#[test]
fn reused_internal_function_name_requires_a_visible_declaration() {
    let text = "const f = function F(x: typeof F | any) {}; let n: number = f;";
    let rows = checked_diags(text)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (text.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(x: ... | any) => void' is not assignable to type 'number'.".to_owned(),
        )]
    );
}

#[test]
fn reused_nested_type_predicate_uses_the_signature_scope() {
    let text = "const f = (g: (x: unknown) => x is string) => {}; let n: number = f;";
    let rows = checked_diags(text)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (text.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(g: (x: unknown) => x is string) => void' is not assignable to type \
                 'number'."
                .to_owned(),
        )]
    );

    let conditional = "let f = (x: string extends infer U ? U : never) => {}; \
                           let n: number = f;";
    let rows = checked_diags(conditional)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (conditional.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(x: string extends infer U ? U : never) => void' is not assignable to \
                 type 'number'."
                .to_owned(),
        )]
    );

    let mapped = "let f = (x: { [K in \"a\"]: K }) => {}; let n: number = f;";
    let rows = checked_diags(mapped)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (mapped.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(x: { [K in \"a\"]: K; }) => void' is not assignable to type 'number'."
                .to_owned(),
        )]
    );

    // A tracker failure inside TypePredicate does not recover the
    // predicate itself. tsc carries hadError through the predicate
    // and rebuilds its enclosing FunctionType as one unit.
    let inaccessible = "const f = function F(g: (a: string | never) => F is string) {}; \
                            let n: number = f;";
    let rows = checked_diags(inaccessible)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (inaccessible.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(g: (a: string | never) => F is string) => void' is not assignable to \
                 type 'number'."
                .to_owned(),
        )]
    );
}

#[test]
fn deferred_conditional_target_renders_root_infer_parameters_as_declarations() {
    let text = "function f<T>() {\n  const o = { a: 1, b: 2 };\n  const o2: [T] extends [[infer U]] ? U : { b: number } = o;\n}\n";
    let rows = checked_diags(text)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            text.find("o2:").expect("failing declaration") as u32,
            2,
            "Type '{ a: number; b: number; }' is not assignable to type '[T] extends [[infer U]] ? U : { b: number; }'."
                .to_owned(),
        )]
    );
}

#[test]
fn reused_simple_type_nodes_recover_at_indexed_access_and_keyof_boundaries() {
    let indexed = "const c = class Hidden { static p = 1; \
                       m = (x: (typeof Hidden)[\"p\"] | any) => {}; }; \
                       const i = new c(); let n: number = i.m;";
    let rows = checked_diags(indexed)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (indexed.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(x: number | any) => void' is not assignable to type 'number'.".to_owned(),
        )]
    );

    let keyof = "const c = class Hidden { static p = 1; \
                     m = (x: keyof typeof Hidden | any) => {}; }; \
                     const i = new c(); let n: number = i.m;";
    let rows = checked_diags(keyof)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (keyof.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(x: (\"prototype\" | \"p\") | any) => void' is not assignable to type \
                 'number'."
                .to_owned(),
        )]
    );
}

#[test]
fn reused_simple_type_nodes_strip_only_the_special_case_parentheses() {
    let indexed = "type A = { p: string }; \
                       const f = (x: (A)[\"p\"] | any) => {}; let n: number = f;";
    let rows = checked_diags(indexed)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (indexed.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(x: A[\"p\"] | any) => void' is not assignable to type 'number'.".to_owned(),
        )]
    );

    let keyof = "type A = { p: string }; \
                     const f = (x: keyof ((A)) | any) => {}; let n: number = f;";
    let rows = checked_diags(keyof)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (keyof.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(x: keyof A | any) => void' is not assignable to type 'number'.".to_owned(),
        )]
    );
}

#[test]
fn reused_function_type_initializers_clone_expressions_and_binding_defaults() {
    let binary = "const f = (g: (x = 1 + 2) => void) => {}; let n: number = f;";
    let rows = checked_diags(binary)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (binary.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(g: (x = 1 + 2) => void) => void' is not assignable to type 'number'."
                .to_owned(),
        )]
    );

    let binding = "const f = (g: ({ a = 1 }: { a: number }) => void) => {}; \
             let n: number = f;";
    let rows = checked_diags(binding)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (binding.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(g: ({ a = 1 }: { a: number; }) => void) => void' is not assignable \
                 to type 'number'."
                .to_owned(),
        )]
    );
}

#[test]
fn reused_initializer_clone_display_covers_calls_objects_and_bodies() {
    let call = "declare function q(x: number): number; \
                    const f = (g: (x = q(1 + 2)) => void) => {}; let n: number = f;";
    let rows = checked_diags(call)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (call.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(g: (x = q(1 + 2)) => void) => void' is not assignable to type \
                 'number'."
                .to_owned(),
        )]
    );

    let numeric = "const f = (g: (x = [1..x, 1e2.x, 0x10.x]) => void) => {}; \
                       let n: number = f;";
    let rows = checked_diags(numeric)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (numeric.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(g: (x = [1..x, 100..x, 16.x]) => void) => void' is not assignable \
                 to type 'number'."
                .to_owned(),
        )]
    );

    let multiline_array = "const f = (g: (x = [\n[\n1\n]\n]) => void) => {}; let n: number = f;";
    let rows = checked_diags(multiline_array)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (multiline_array.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(g: (x = [    [        1    ]]) => void) => void' is not assignable \
                 to type 'number'."
                .to_owned(),
        )]
    );

    let object = "declare const xs: any; \
                      const f = (g: (x = { a: [1, ...xs], m() { return 1; } }) => void) => {}; \
                      let n: number = f;";
    let rows = checked_diags(object)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (object.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(g: (x = { a: [1, ...xs], m(): any { return 1; } }) => void) => void' \
                 is not assignable to type 'number'."
                .to_owned(),
        )]
    );

    let multiline_method = "const f = (g: (x = { m() {\nreturn 1\n} }) => void) => {}; \
             let n: number = f;";
    let rows = checked_diags(multiline_method)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (multiline_method
                .rfind("let n")
                .expect("failing declaration")
                + 4) as u32,
            1,
            "Type '(g: (x = { m(): any {        return 1;    } }) => void) => void' is \
                 not assignable to type 'number'."
                .to_owned(),
        )]
    );

    let class_member = "const f = (g: (x = class { p = 1 }) => void) => {}; let n: number = f;";
    let rows = checked_diags(class_member)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (class_member.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(g: (x = class {    p = 1;}) => void) => void' is not assignable to \
                 type 'number'."
                .to_owned(),
        )]
    );

    let body_declarations = "const f = (g: (x = function() { \
                                 type T = { a: string }; \
                                 interface I { p: number } \
                                 enum E { A = 1 } \
                                 return 1 \
                                 }) => void) => {}; let n: number = f;";
    let rows = checked_diags(body_declarations)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (body_declarations
                .rfind("let n")
                .expect("failing declaration")
                + 4) as u32,
            1,
            "Type '(g: (x = function (): any { type T = { a: string; }; interface I \
                 {    p: number;} enum E {    A = 1} return 1; }) => void) => void' is not \
                 assignable to type 'number'."
                .to_owned(),
        )]
    );

    let conditional = "const f = (g: (x = true ? (() => 1) : class {}) => void) => {}; \
             let n: number = f;";
    let rows = checked_diags(conditional)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (conditional.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(g: (x = true ? ((): any => 1) : class {}) => void) => void' is not \
                 assignable to type 'number'."
                .to_owned(),
        )]
    );
}

#[test]
fn reused_clone_display_preserves_decorators_prologues_and_recovery_declarations() {
    fn actual_message(source: &str) -> String {
        let mut rows = checked_diags(source)
            .into_iter()
            .filter(|row| row.0 == 2322)
            .collect::<Vec<_>>();
        assert_eq!(
            rows.len(),
            1,
            "expected one assignment diagnostic for {source:?}"
        );
        let row = rows.pop().expect("one row");
        assert_eq!(
            (row.0, row.1, row.2),
            (
                2322,
                (source.rfind("let n").expect("failing declaration") + 4) as u32,
                1,
            )
        );
        row.3
    }

    let decorated_class = "declare const dec: any; \
            const f = (g: (x = function() {\nreturn (@dec class {});\n}) => void) => {}; \
            let n: number = f;";
    let mut cases = vec![(
        decorated_class,
        "(g: (x = function (): any {    return (    @dec    class {    });}) => void) => void",
    )];

    let decorated_declaration = "declare const dec: any; \
            const f = (g: (x = function() {\n@dec class I {}\n}) => void) => {}; \
            let n: number = f;";
    cases.push((
        decorated_declaration,
        "(g: (x = function (): any {    @dec    class I {    }}) => void) => void",
    ));

    let decorated_parameter = "declare const dec: any; \
            const f = (g: (x = function() {\nreturn function(@dec value = 1) {};\n}) => void) => {}; \
            let n: number = f;";
    cases.push((
        decorated_parameter,
        "(g: (x = function (): any {    return function (    @dec    value = 1): any { };}) \
             => void) => void",
    ));

    let prologue = "const f = (g: (x = function(){\"use strict\"; return 1}) => void) => {}; \
            let n: number = f;";
    cases.push((
        prologue,
        "(g: (x = function (): any {    \"use strict\";    return 1;}) => void) => void",
    ));

    let arrow = "const f = (g: (x = <T,>(value: T) => value) => void) => {}; \
            let n: number = f;";
    cases.push((
        arrow,
        "(g: (x = <T,>(value: T): any => value) => void) => void",
    ));

    let binding = "declare function key(): PropertyKey; \
            const f = (g: ({ [key()]: value, }: { [name: string]: number }) => void) => {}; \
            let n: number = f;";
    cases.push((
        binding,
        "(g: ({ [key()]: value, }: { [name: string]: number; }) => void) => void",
    ));

    let declarations = "const f = (g: (x = function() {\n\
            namespace N {\n}\n\
            import q = require(\"m\");\n\
            export * as \"quoted\" from \"m\";\n\
            export default function() {}\n\
            }) => void) => {}; let n: number = f;";
    cases.push((
        declarations,
        "(g: (x = function (): any {    namespace N {    }    import q = require(\"m\");    \
             export * as \"quoted\" from \"m\";    export default function (): any { }}) => void) \
             => void",
    ));

    let accessor = "const f = (value: { get p() { return 1 } } | any) => {}; \
            let n: number = f;";
    cases.push((
        accessor,
        "(value: { get p(): any { return 1; } } | any) => void",
    ));

    let mut mismatches = Vec::new();
    for (source, expected_type) in cases {
        let actual = actual_message(source);
        let expected = format!("Type '{expected_type}' is not assignable to type 'number'.");
        if actual != expected {
            mismatches.push(format!(
                "source: {source:?}\nexpected: {expected}\n  actual: {actual}"
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "clone-display mismatches:\n{}",
        mismatches.join("\n\n")
    );
}

#[test]
fn reused_symbol_visibility_matches_declaration_and_alias_fallbacks() {
    let text = "import { Q } from \"./m\";\n\
                    export declare namespace Ambient { class Nested {} }\n\
                    declare module \"./m\" { export interface Q {} }\n\
                    declare const value: any;\n\
                    const {} = value;\n\
                    const local = 1;\n\
                    const { q } = value;\n";
    with_program_state(
        &[("a.ts", text), ("m.ts", "export interface Q {}\n")],
        &CompilerOptions::default(),
        |state| {
            let nodes = state.binder.source(0).arena.node_ids().collect::<Vec<_>>();
            let import_specifier = nodes
                .iter()
                .copied()
                .find(|&node| state.kind_of(node) == SyntaxKind::ImportSpecifier)
                .expect("import specifier");
            assert!(!state.reused_declaration_is_visible_slice(import_specifier));
            let import_symbol = state
                .node_symbol(import_specifier)
                .or_else(|| {
                    node_util::get_name_of_declaration(
                        state.binder.source_of_node(import_specifier),
                        import_specifier,
                    )
                    .and_then(|name| state.node_symbol(name))
                })
                .expect("import alias symbol");
            assert!(state.symbol_has_visible_declarations_slice(import_symbol));

            let nested_class = nodes
                .iter()
                .copied()
                .find(|&node| {
                    matches!(
                        state.data_of(node),
                        NodeData::ClassDeclaration(data)
                            if data.name.is_some_and(|name| {
                                state.identifier_text_of(name) == Some("Nested")
                            })
                    )
                })
                .expect("ambient nested class");
            assert!(state.reused_declaration_is_visible_slice(nested_class));

            let augmentation = nodes
                .iter()
                .copied()
                .find(|&node| {
                    matches!(
                        state.data_of(node),
                        NodeData::ModuleDeclaration(data)
                            if data.name.is_some_and(|name| {
                                matches!(
                                    state.data_of(name),
                                    NodeData::StringLiteral(data) if data.text == "./m"
                                )
                            })
                    )
                })
                .expect("external module augmentation");
            assert!(state.reused_declaration_is_visible_slice(augmentation));

            let declarations = nodes
                .iter()
                .copied()
                .filter(|&node| state.kind_of(node) == SyntaxKind::VariableDeclaration)
                .collect::<Vec<_>>();
            let empty = declarations
                .iter()
                .copied()
                .find(|&node| {
                    matches!(
                        state.data_of(node),
                        NodeData::VariableDeclaration(data)
                            if data.name.is_some_and(|name| {
                                matches!(
                                    state.data_of(name),
                                    NodeData::ObjectBindingPattern(data)
                                        if state.nodes_of(data.elements).is_empty()
                                )
                            })
                    )
                })
                .expect("empty binding declaration");
            assert!(!state.reused_declaration_is_visible_slice(empty));

            let local = declarations
                .iter()
                .copied()
                .find(|&node| {
                    matches!(
                        state.data_of(node),
                        NodeData::VariableDeclaration(data)
                            if data.name.is_some_and(|name| {
                                state.identifier_text_of(name) == Some("local")
                            })
                    )
                })
                .expect("unexported local");
            assert!(!state.reused_declaration_is_visible_slice(local));
            let local_symbol = state
                .node_symbol(local)
                .or_else(|| {
                    node_util::get_name_of_declaration(state.binder.source_of_node(local), local)
                        .and_then(|name| state.node_symbol(name))
                })
                .expect("local symbol");
            assert!(state.symbol_has_visible_declarations_slice(local_symbol));

            let binding = nodes
                .iter()
                .copied()
                .find(|&node| {
                    matches!(
                        state.data_of(node),
                        NodeData::BindingElement(data)
                            if data.name.is_some_and(|name| {
                                state.identifier_text_of(name) == Some("q")
                            })
                    )
                })
                .expect("block-scoped binding element");
            let binding_symbol = state
                .node_symbol(binding)
                .or_else(|| {
                    node_util::get_name_of_declaration(
                        state.binder.source_of_node(binding),
                        binding,
                    )
                    .and_then(|name| state.node_symbol(name))
                })
                .expect("binding symbol");
            assert!(state.symbol_has_visible_declarations_slice(binding_symbol));
        },
    );
}

#[test]
fn reused_instantiated_signature_applies_the_display_mapper() {
    let union = "class C<T> { m = (x: T | any) => {}; } \
                     const c = new C<string>(); let n: number = c.m;";
    let rows = checked_diags(union)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (union.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(x: string | any) => void' is not assignable to type 'number'.".to_owned(),
        )]
    );

    let indexed = "class C<T> { m = (x: [T, any][1]) => {}; } \
                       const c = new C<string>(); let n: number = c.m;";
    let rows = checked_diags(indexed)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (indexed.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(x: [string, any][1]) => void' is not assignable to type 'number'.".to_owned(),
        )]
    );

    let this_type = "class C<T> { m = (x: this | any) => {}; } \
                         const c = new C<string>(); let n: number = c.m;";
    let rows = checked_diags(this_type)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (this_type.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(x: C<string> | any) => void' is not assignable to type 'number'.".to_owned(),
        )]
    );

    let array = "interface Array<T> { length: number } \
                     class C<T> { m = (x: Array<T> | any) => {}; } \
                     const c = new C<string>(); let n: number = c.m;";
    let rows = checked_diags(array)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (array.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(x: string[] | any) => void' is not assignable to type 'number'.".to_owned(),
        )]
    );

    let import_type = "class C<T> { m = (x: import(\"./m\").Box<T> | any) => {}; } \
                           const c = new C<string>(); let n: number = c.m;";
    let rows = program_diags(&[
        ("m.ts", "export interface Box<T> { value: T }\n"),
        ("a.ts", import_type),
    ])
    .into_iter()
    .filter(|row| row.1 == 2322)
    .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            "a.ts".to_owned(),
            2322,
            (import_type.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(x: Box<string> | any) => void' is not assignable to type 'number'.".to_owned(),
        )]
    );
}

#[test]
fn reused_js_literal_import_type_honors_jsdoc_fallbacks() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        strict: Some(true),
        ..CompilerOptions::default()
    };
    let generic = concat!(
        "/** @param {import(\"./m\").Box} x */\n",
        "const f = function(x) {};\n",
        "/** @type {number} */\n",
        "let n = f;\n",
    );
    let rows = program_diags_with(
        &[
            ("m.d.ts", "export interface Box<T> { value: T }\n"),
            ("source.js", generic),
        ],
        &options,
        "/",
    )
    .into_iter()
    .filter(|row| row.1 == 2322)
    .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            "source.js".to_owned(),
            2322,
            (generic.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(x: Box<any>) => void' is not assignable to type 'number'.".to_owned(),
        )]
    );

    let value = concat!(
        "/** @param {import(\"./m\").V} x */\n",
        "const f = function(x) {};\n",
        "/** @type {number} */\n",
        "let n = f;\n",
    );
    let rows = program_diags_with(
        &[
            ("m.d.ts", "export const V: { p: string };\n"),
            ("source.js", value),
        ],
        &options,
        "/",
    )
    .into_iter()
    .filter(|row| row.1 == 2322)
    .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            "source.js".to_owned(),
            2322,
            (value.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(x: any) => void' is not assignable to type 'number'.".to_owned(),
        )]
    );
}

#[test]
fn reused_nested_jsdoc_references_honor_can_reuse_type_node() {
    let source = concat!(
        "/** @param {V|string} x */\n",
        "const f = function(x) {};\n",
        "/** @type {number} */\n",
        "let n = f;\n",
        "/** @param {Box|string} x */\n",
        "const g = function(x) {};\n",
        "/** @type {number} */\n",
        "let m = g;\n",
    );
    let rows = program_diags_with(
        &[
            (
                "globals.d.ts",
                "declare const V: { p: string };\ninterface Box<T> { value: T }\n",
            ),
            ("source.js", source),
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(true),
            ..CompilerOptions::default()
        },
        "/",
    )
    .into_iter()
    .filter(|row| row.1 == 2322)
    .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [
            (
                "source.js".to_owned(),
                2322,
                (source.find("let n").expect("first failing declaration") + 4) as u32,
                1,
                "Type '(x: { p: string; } | string) => void' is not assignable to type \
                     'number'."
                    .to_owned(),
            ),
            (
                "source.js".to_owned(),
                2322,
                (source.rfind("let m").expect("second failing declaration") + 4) as u32,
                1,
                "Type '(x: Box<any> | string) => void' is not assignable to type 'number'."
                    .to_owned(),
            ),
        ]
    );
}

#[test]
fn reused_type_arguments_parenthesize_a_leading_generic_function() {
    let text = "type Box<T> = { value: T }; let f = (x: Box<<T>() => T>) => {}; let n: number = f;";
    let rows = checked_diags(text)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (text.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(x: Box<(<T>() => T)>) => void' is not assignable to type 'number'.".to_owned(),
        )]
    );

    let source = concat!(
        "/** @param {Box<V>} x */\n",
        "const f = function(x) {};\n",
        "/** @type {number} */\n",
        "let n = f;\n",
    );
    let rows = program_diags_with(
        &[
            (
                "globals.d.ts",
                "declare const V: <T>() => T;\ninterface Box<T> { value: T }\n",
            ),
            ("source.js", source),
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(true),
            ..CompilerOptions::default()
        },
        "/",
    )
    .into_iter()
    .filter(|row| row.1 == 2322)
    .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            "source.js".to_owned(),
            2322,
            (source.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(x: Box<(<T>() => T)>) => void' is not assignable to type 'number'.".to_owned(),
        )]
    );
}

#[test]
fn reused_import_assert_recovers_semantically_without_reentry() {
    let source =
        "let f = (x: import(\"./m\", { assert: { type: \"json\" } }).Q) => {}; let n: number = f;";
    let rows = program_diags(&[
        ("m.ts", "export interface Q { q: string }\n"),
        ("a.ts", source),
    ])
    .into_iter()
    .filter(|row| row.1 == 2322)
    .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            "a.ts".to_owned(),
            2322,
            (source.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '(x: Q) => void' is not assignable to type 'number'.".to_owned(),
        )]
    );
}

#[test]
fn reused_shadowed_entity_recovers_semantically_without_reentry() {
    let text = "class A {}\nlet value: A;\nfunction scope() { class A {} }\n";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        let reference = node_of_kind(state, tsc_syntax::SyntaxKind::TypeReference);
        let inner_class = {
            let source = state.binder.source(0);
            source
                .arena
                .node_ids()
                .filter(|&node| {
                    source.arena.node(node).kind == tsc_syntax::SyntaxKind::ClassDeclaration
                })
                .nth(1)
                .expect("inner class")
        };
        state.slice_display_enclosing = Some(inner_class);
        assert_eq!(
            state
                .type_annotation_text_slice(reference)
                .expect("semantic recovery"),
            "globalThis.A"
        );
    });
}

#[test]
fn reused_entity_recovery_uses_the_shortest_context_name() {
    with_program_state(
        &[
            ("m.ts", "export class Q {}\n"),
            ("source.ts", "import { Q } from \"./m\"; let source: Q;\n"),
            (
                "alias.ts",
                "import { Q as Alias } from \"./m\"; let here;\n",
            ),
            ("bare.ts", "let bare;\n"),
        ],
        &CompilerOptions::default(),
        |state| {
            let reference = {
                let source = state.binder.source(1);
                source
                    .arena
                    .node_ids()
                    .find(|&node| {
                        source.arena.node(node).kind == tsc_syntax::SyntaxKind::TypeReference
                    })
                    .expect("source type reference")
            };
            let alias_enclosing = {
                let source = state.binder.source(2);
                source
                    .arena
                    .node_ids()
                    .find(|&node| {
                        source.arena.node(node).kind == tsc_syntax::SyntaxKind::VariableDeclaration
                    })
                    .expect("alias context declaration")
            };
            state.slice_display_enclosing = Some(alias_enclosing);
            assert_eq!(
                state
                    .type_annotation_text_slice(reference)
                    .expect("alias recovery"),
                "Alias"
            );

            let bare_enclosing = {
                let source = state.binder.source(3);
                source
                    .arena
                    .node_ids()
                    .find(|&node| {
                        source.arena.node(node).kind == tsc_syntax::SyntaxKind::VariableDeclaration
                    })
                    .expect("bare context declaration")
            };
            state.slice_display_enclosing = Some(bare_enclosing);
            assert_eq!(
                state
                    .type_annotation_text_slice(reference)
                    .expect("import recovery"),
                "import(\"./m\").Q"
            );
        },
    );
}

#[test]
fn mixin_class_static_side_uses_the_intersection_face() {
    let text = "function f<T extends new (...args: any) => any>(Base: T) { \
                    class C extends Base {} let n: number = C; }";
    let rows = checked_diags(text)
        .into_iter()
        .filter(|row| row.0 == 2322)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            2322,
            (text.rfind("let n").expect("failing declaration") + 4) as u32,
            1,
            "Type '{ new (...args: any): C; prototype: f<any>.C; } & T' is not assignable \
                 to type 'number'."
                .to_owned(),
        )]
    );
}

#[test]
fn symbol_expression_uses_utf16_property_access_gate() {
    let text = "declare namespace A { namespace 𐐀 { const s: unique symbol; } }\n";
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2020.bits()),
        ..CompilerOptions::default()
    };
    with_program_state(&[("a.ts", text)], &options, |state| {
        let declaration = {
            let source = state.binder.source(0);
            let name = source
                .arena
                .node_ids()
                .find(|&node| {
                    matches!(
                        &source.arena.node(node).data,
                        tsc_syntax::NodeData::Identifier(data)
                            if data.escaped_text == "s"
                    )
                })
                .expect("s identifier");
            state.parent_of(name).expect("s declaration")
        };
        let symbol = state
            .node_symbol(declaration)
            .expect("variable declaration symbol");
        assert_eq!(
            state
                .symbol_expression_face_slice(symbol, None, true)
                .expect("symbol expression"),
            "A[𐐀].s"
        );
    });
}

#[test]
fn display_leaf_reuse_renders_mapped_import_template_and_plain_sibling() {
    let text = concat!(
        "type Advanced<T> = { -readonly [K in keyof T as `x${K & string}`]+?: ",
        "import(\"./m\", { with: { \"resolution-mode\": \"import\" } }).Q<0x10n> };\n",
        "type Plain = { x: string };\n",
    );
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        let mapped = node_of_kind(state, tsc_syntax::SyntaxKind::MappedType);
        assert_eq!(
            state
                .type_annotation_text_slice(mapped)
                .expect("mapped annotation"),
            "{ -readonly [K in keyof T as `x${K & string}`]+?: \
                 import(\"./m\", { with: { \"resolution-mode\": \"import\" } }).Q<0x10n>; }"
        );

        // Nearest non-firing sibling: the pre-existing plain
        // TypeLiteral/keyword path remains byte-for-byte stable.
        let plain = node_of_kind(state, tsc_syntax::SyntaxKind::TypeLiteral);
        assert_eq!(
            state
                .type_annotation_text_slice(plain)
                .expect("plain annotation"),
            "{ x: string; }"
        );
    });
    with_program_state_allow_parse_diagnostics(
        &[(
            "recovery.ts",
            "type Recovered<T> = { [K in keyof T]; extra: string };\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let mapped = node_of_kind(state, tsc_syntax::SyntaxKind::MappedType);
            assert_eq!(
                state
                    .type_annotation_text_slice(mapped)
                    .expect("recovered mapped annotation"),
                "{ [K in keyof T]: ; extra: string;}"
            );
        },
    );

    // Synthesized node-builder names take escapeNonAsciiString;
    // ordinary ASCII is the non-firing sibling.
    assert_eq!(
        super::string_literal_name_slice("line\n😀", false).expect("escaped literal"),
        "\"line\\n\\uD83D\\uDE00\""
    );
    assert_eq!(
        super::string_literal_name_slice("plain", false).expect("plain literal"),
        "\"plain\""
    );
}

#[test]
fn signature_display_initializer_parameters_use_minimum_arity() {
    let options = CompilerOptions {
        strict: Some(false),
        ..CompilerOptions::default()
    };
    let text = "var v = <T>() => 1;\nvar v = <T>(a = 1, b = 2) => 1;\n";
    assert_eq!(
            checked_diags_with(text, &options),
            [(
                2403,
                text.rfind('v').expect("second declaration") as u32,
                1,
                "Subsequent variable declarations must have the same type.  Variable 'v' must be of type '<T>() => number', but here has type '<T>(a?: number, b?: number) => number'."
                    .to_owned()
            )]
        );
}

#[test]
fn signature_display_required_parameters_remain_required() {
    let options = CompilerOptions {
        strict: Some(false),
        ..CompilerOptions::default()
    };
    let text = "var v = <T>() => 1;\nvar v = <T>(a: number, b: number) => 1;\n";
    assert_eq!(
            checked_diags_with(text, &options),
            [(
                2403,
                text.rfind('v').expect("second declaration") as u32,
                1,
                "Subsequent variable declarations must have the same type.  Variable 'v' must be of type '<T>() => number', but here has type '<T>(a: number, b: number) => number'."
                    .to_owned()
            )]
        );
}

#[test]
fn signature_display_generic_constraint_and_default() {
    assert_eq!(
        checked_diags(
            "declare let f: <T extends string = \"a\">(x: T) => T;\nlet t3: string = f;\n"
        ),
        [(
            2322,
            69,
            1,
            "Type '<T extends string = \"a\">(x: T) => T' is not assignable to type 'string'."
                .to_owned()
        )]
    );
}

#[test]
fn signature_display_abstract_construct_shorthand() {
    assert_eq!(
            checked_diags(
                "interface D { d: number }\ndeclare let f: abstract new () => D;\nlet t4: string = f;\n"
            ),
            [(
                2322,
                67,
                2,
                "Type 'abstract new () => D' is not assignable to type 'string'.".to_owned()
            )]
        );
}

#[test]
fn display_composer_splits_abstract_constructs_from_object_members() {
    with_program_state(
        &[(
            "a.ts",
            "type Ctor = abstract new () => object;\ntype Obj = { p: string };\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let constructor = node_of_kind(state, tsc_syntax::SyntaxKind::ConstructorType);
            let constructor_type = state
                .get_type_from_type_node(constructor)
                .expect("constructor type");
            let abstract_signature = *state
                .get_signatures_of_type(
                    constructor_type,
                    crate::structural::SignatureKind::Construct,
                )
                .expect("construct signatures")
                .first()
                .expect("abstract construct signature");

            let object = node_of_kind(state, tsc_syntax::SyntaxKind::TypeLiteral);
            let object_type = state.get_type_from_type_node(object).expect("object type");
            let object_members = state
                .resolve_structured_type_members(object_type)
                .expect("object members");
            let plain_members = state.members_of(object_members).clone();

            // Nearest non-firing sibling: an ordinary member-only
            // object still emits a single TypeLiteral.
            assert_eq!(
                state
                    .type_node_from_object_type_slice(object_type, false)
                    .expect("plain object display")
                    .0,
                "{ p: string; }"
            );

            let mut mixed_members = plain_members;
            mixed_members.construct_signatures.push(abstract_signature);
            let mixed = state
                .tables
                .create_type(TypeFlags::OBJECT, TypeData::Object);
            state.tables.type_mut(mixed).object_flags = ObjectFlags::ANONYMOUS;
            let mixed_members = state.alloc_members(mixed_members);
            state
                .links
                .set_fresh_type_members(mixed, crate::links::LinkSlot::Resolved(mixed_members));
            assert_eq!(
                state
                    .type_to_string_slice(mixed)
                    .expect("abstract/member intersection display"),
                "(abstract new () => object) & { p: string; }"
            );
        },
    );
}

#[test]
fn display_composer_synthesizes_class_auto_accessor_pair_and_plain_sibling() {
    with_program_state(
        &[(
            "a.ts",
            "class C { accessor p: string = \"\"; q: number = 0; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            let class = node_of_kind(state, tsc_syntax::SyntaxKind::ClassDeclaration);
            let class_symbol = state
                .get_symbol_of_declaration(class)
                .expect("class symbol");
            let class_type = state
                .get_declared_type_of_symbol_slice(class_symbol)
                .expect("class type");

            let accessor = state
                .get_property_of_type_full(class_type, "p")
                .expect("accessor lookup")
                .expect("accessor property");
            let mut rendered = Vec::new();
            state
                .property_signature_slice(accessor, false, &mut rendered)
                .expect("auto-accessor display");
            assert_eq!(
                rendered,
                [
                    "get p(): string".to_owned(),
                    "set p(arg: string)".to_owned()
                ]
            );

            // Nearest non-firing sibling: a plain class field
            // remains one property signature.
            let plain = state
                .get_property_of_type_full(class_type, "q")
                .expect("plain lookup")
                .expect("plain property");
            rendered.clear();
            state
                .property_signature_slice(plain, false, &mut rendered)
                .expect("plain property display");
            assert_eq!(rendered, ["q: number".to_owned()]);
        },
    );
}

#[test]
fn declared_class_and_interface_targets_render_self_type_arguments() {
    // typeToTypeNodeWorker dispatches Reference before the later
    // ClassOrInterface symbol arm.  The declared target aliases its
    // own type parameters as resolved arguments, so generic targets
    // retain those arguments while their non-generic siblings stay
    // bare. Oracle-pinned vs vendored tsc 6.0.3, noLib.
    with_program_state(
        &[(
            "a.ts",
            "class Box<T, U> {}\ninterface Face<T> {}\nclass Plain {}\ninterface Empty {}\n",
        )],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            for (name, expected) in [
                ("Box", "Box<T, U>"),
                ("Face", "Face<T>"),
                ("Plain", "Plain"),
                ("Empty", "Empty"),
            ] {
                let symbol = state
                    .resolve_file_scope_name(name, SymbolFlags::TYPE)
                    .expect("declared type symbol");
                let ty = state
                    .get_declared_type_of_class_or_interface(symbol)
                    .expect("declared class/interface type");
                assert_eq!(
                    state.type_to_string_slice(ty).expect("declared display"),
                    expected
                );
            }
        },
    );
}

#[test]
fn signature_display_member_order_call_index_property() {
    // createTypeNodesFromResolvedType order: call signatures,
    // construct signatures, index signatures, properties.
    assert_eq!(
            checked_diags(
                "declare let o: { (x: string): void; [k: string]: number; p: 3 };\nlet t5: string = o;\n"
            ),
            [(
                2322,
                69,
                2,
                "Type '{ (x: string): void; [k: string]: number; p: 3; }' is not assignable to type 'string'."
                    .to_owned()
            )]
        );
}

#[test]
fn signature_display_diverging_accessor_faces() {
    assert_eq!(
        checked_diags(
            "declare let o: { get p(): number, set p(v: string) };\nlet t6: string = o;\n"
        ),
        [(
            2322,
            58,
            2,
            "Type '{ get p(): number; set p(v: string); }' is not assignable to type 'string'."
                .to_owned()
        )]
    );
}

#[test]
fn signature_display_overloaded_optional_method_members() {
    assert_eq!(
        checked_diags(
            "declare let o: { m?(): void; m?(x: 1): void; p: 2 };\nlet t7: string = o;\n"
        ),
        [(
            2322,
            57,
            2,
            "Type '{ m?(): void; m?(x: 1): void; p: 2; }' is not assignable to type 'string'."
                .to_owned()
        )]
    );
}

#[test]
fn signature_display_tuple_rest_expansion() {
    // getExpandedParameters: optional tuple members expand with
    // `?` and the strict `| undefined` element type.
    assert_eq!(
            checked_diags(
                "declare let f: (...args: [number, string?]) => void;\nlet t8: string = f;\n"
            ),
            [(
                2322,
                57,
                2,
                "Type '(args_0: number, args_1?: string | undefined) => void' is not assignable to type 'string'."
                    .to_owned()
            )]
        );
}

#[test]
fn signature_display_labeled_tuple_rest_expansion() {
    assert_eq!(
        checked_diags(
            "declare let f: (...args: [a: number, b: string]) => void;\nlet t9: string = f;\n"
        ),
        [(
            2322,
            62,
            2,
            "Type '(a: number, b: string) => void' is not assignable to type 'string'.".to_owned()
        )]
    );
}

#[test]
fn signature_display_middle_rest_keeps_declared_list() {
    // 52519-52523: a mid-list REST-flagged expanded face falls
    // back to the declared parameter list.
    assert_eq!(
            checked_diags(
                "declare let f: (...args: [number, ...string[], boolean]) => void;\nlet t23: string = f;\n"
            ),
            [(
                2322,
                70,
                3,
                "Type '(...args: [number, ...string[], boolean]) => void' is not assignable to type 'string'."
                    .to_owned()
            )]
        );
}

#[test]
fn signature_display_binding_pattern_with_annotation_reuse() {
    // Pattern name + reused parenthesized annotation compose.
    assert_eq!(
        checked_diags("let g = ({ a }: ({ a: (number) })) => {};\nlet t10: string = g;\n"),
        [(
            2322,
            46,
            3,
            "Type '({ a }: ({ a: (number); })) => void' is not assignable to type 'string'."
                .to_owned()
        )]
    );
}

#[test]
fn signature_display_asserts_predicate_return() {
    assert_eq!(
        checked_diags(
            "declare let f: (x: unknown) => asserts x is string;\nlet t11: string = f;\n"
        ),
        [(
            2322,
            56,
            3,
            "Type '(x: unknown) => asserts x is string' is not assignable to type 'string'."
                .to_owned()
        )]
    );
}

#[test]
fn signature_display_union_wraps_function_type() {
    assert_eq!(
        checked_diags("declare let f: (() => void) | null;\nlet t12: string = f;\n"),
        [(
            2322,
            40,
            3,
            "Type '(() => void) | null' is not assignable to type 'string'.".to_owned()
        )]
    );
}

#[test]
fn signature_display_optional_tuple_wraps_function_union() {
    assert_eq!(
        checked_diags("declare let f: [(() => void)?];\nlet t13: string = f;\n"),
        [(
            2322,
            36,
            3,
            "Type '[((() => void) | undefined)?]' is not assignable to type 'string'.".to_owned()
        )]
    );
}

#[test]
fn signature_display_this_parameter_unshifts() {
    assert_eq!(
            checked_diags(
                "interface W { w: number }\ndeclare let f: (this: W, x: number) => void;\nlet t14: string = f;\n"
            ),
            [(
                2322,
                75,
                3,
                "Type '(this: W, x: number) => void' is not assignable to type 'string'."
                    .to_owned()
            )]
        );
}

#[test]
fn signature_display_constraint_annotation_reuse_keeps_alias() {
    // The constraint face rides the REUSABLE-node path even
    // without an enclosing declaration (52832-52834): the alias
    // spelling survives where param/return positions resolve.
    assert_eq!(
            checked_diags(
                "type AB = \"a\" | \"b\";\ndeclare let f: <T extends AB>(x: T) => T;\nlet t15: string = f;\n"
            ),
            [(
                2322,
                81,
                1,
                "Type '<T extends AB>(x: T) => T' is not assignable to type 'string'.".to_owned()
            )]
        );
}

#[test]
fn signature_display_context_sensitive_source_stays_structural() {
    // A context-sensitive fn expression gets NO enclosing
    // (symbolValueDeclarationIsContextSensitive) — nothing to
    // reuse; the noImplicitAny 7006 rides along.
    assert_eq!(
        checked_diags("let g = (x) => x;\nlet t16: string = g;\n"),
        [
            (
                7006,
                9,
                1,
                "Parameter 'x' implicitly has an 'any' type.".to_owned()
            ),
            (
                2322,
                22,
                3,
                "Type '(x: any) => any' is not assignable to type 'string'.".to_owned()
            )
        ]
    );
}

#[test]
fn signature_display_setter_face_param_union() {
    assert_eq!(
            checked_diags(
                "declare let o: { get p(): string; set p(v: string | number) };\nlet t22: string = o;\n"
            ),
            [(
                2322,
                67,
                3,
                "Type '{ get p(): string; set p(v: string | number); }' is not assignable to type 'string'."
                    .to_owned()
            )]
        );
}

#[test]
fn signature_display_rest_tuple_expansion_beats_annotation_reuse() {
    // The expanded transient faces carry no declarations, so the
    // parenthesized rest annotation cannot reuse.
    assert_eq!(
        checked_diags("let g = (...args: ([number, string])) => {};\nlet t24: string = g;\n"),
        [(
            2322,
            49,
            3,
            "Type '(args_0: number, args_1: string) => void' is not assignable to type 'string'."
                .to_owned()
        )]
    );
}

#[test]
fn signature_display_return_annotation_reuse_keeps_parens() {
    assert_eq!(
        checked_diags(
            "let g = function (x: number): (string) { return \"s\" };\nlet t25: string = g;\n"
        ),
        [(
            2322,
            73,
            1,
            "Type '(x: number) => (string)' is not assignable to type 'string'.".to_owned()
        )]
    );
}

// ---- 9.3b2 fabrication-audit pins (shouldReportUnmatchedPropertyError,
// elaborateArrowFunction, expando suppression) ----

#[test]
fn signature_shaped_source_keeps_the_headless_relation_row() {
    // shouldReportUnmatchedPropertyError (67043): a property-less
    // callable source against a non-callable-shaped target keeps
    // the plain head — no 2741 missing-property face.
    assert_eq!(
        checked_diags(
            "interface T { f(x: number): void }\ndeclare var t: T;\nt = (x: string) => 1;\n"
        ),
        [(
            2322,
            53,
            1,
            "Type '(x: string) => number' is not assignable to type 'T'.".to_owned()
        )]
    );
}

#[test]
fn signature_shaped_source_vs_callable_target_reports_missing_property() {
    // The gate's TRUE branch: both sides callable — the missing
    // property reports.
    assert_eq!(
            checked_diags(
                "interface U { (): void; p: number }\ndeclare var src: { (): void };\ndeclare var u: U;\nu = src;\n"
            ),
            [(
                2741,
                85,
                1,
                "Property 'p' is missing in type '() => void' but required in type 'U'.".to_owned()
            )]
        );
}

#[test]
fn arrow_source_elaborates_the_return_position() {
    // elaborateArrowFunction: the row lands on the body
    // expression, not the declaration name.
    assert_eq!(
        checked_diags("var aLambda: (x: string) => number = (x) => 'a str';\n"),
        [(
            2322,
            44,
            7,
            "Type 'string' is not assignable to type 'number'.".to_owned()
        )]
    );
}

#[test]
fn member_arrow_elaborates_through_the_paren_comma_body() {
    // The member walk's inner recursion declines through
    // paren→comma→undefined, then the report anchors at the
    // arrow's return expression (the parenthesized body).
    assert_eq!(
        checked_diags(
            "type OT = { x: (p: number) => string };\nvar obj1: OT = { x: x => (x, undefined) };\n"
        ),
        [
            (
                2695,
                66,
                1,
                "Left side of comma operator is unused and has no side effects.".to_owned()
            ),
            (
                2322,
                65,
                14,
                "Type 'undefined' is not assignable to type 'string'.".to_owned()
            )
        ]
    );
}

#[test]
fn block_body_arrow_keeps_the_declaration_head() {
    assert_eq!(
        checked_diags("var aL2: (x: string) => number = (x) => { return 'a'; };\n"),
        [(
            2322,
            4,
            3,
            "Type '(x: string) => string' is not assignable to type '(x: string) => number'."
                .to_owned()
        )]
    );
}

#[test]
fn annotated_param_arrow_keeps_the_declaration_head() {
    assert_eq!(
        checked_diags("var aL3: (x: string) => number = (x: string) => 'a';\n"),
        [(
            2322,
            4,
            3,
            "Type '(x: string) => string' is not assignable to type '(x: string) => number'."
                .to_owned()
        )]
    );
}

#[test]
fn ts_expando_function_members_resolve_normally() {
    // The assignment declaration is a real export of the function
    // symbol, so both the assignment and read use the normal member
    // path without a diagnostic-side exception.
    assert_eq!(
        checked_diags("function foo() {}\nfoo.x = 1;\nvar q0: number = foo.x;\n"),
        []
    );
}

#[test]
fn class_static_assignments_still_report_2339() {
    // The control: classes are NOT expando parents — the real
    // rows keep emitting (the set-ratchet regression face).
    assert_eq!(
        checked_diags("class EC { n = 1 }\nEC.prop = 2\nvar q1 = EC.prop;\n"),
        [
            (
                2339,
                22,
                4,
                "Property 'prop' does not exist on type 'typeof EC'.".to_owned()
            ),
            (
                2339,
                43,
                4,
                "Property 'prop' does not exist on type 'typeof EC'.".to_owned()
            )
        ]
    );
}

// ---- 9.3b2 review-round pins (union best-match, IIFE effective
// args, optional missing removal) ----

#[test]
fn expando_resolution_is_name_precise() {
    // Only the assigned member resolves; other names miss in tsc
    // too — y/q report 2339, "z" reports 7053, and the expando'd
    // declaration symbol displays `typeof foo` (oracle-probed byte
    // rows).
    assert_eq!(
            checked_diags(
                "function foo() {}\nfoo.x = 1;\nfoo.y;\nfoo[\"z\"];\nconst alias = foo;\nalias.q;\nvar ok: number = foo.x;\n"
            ),
            [
                (
                    2339,
                    33,
                    1,
                    "Property 'y' does not exist on type 'typeof foo'.".to_owned()
                ),
                (
                    7053,
                    36,
                    8,
                    "Element implicitly has an 'any' type because expression of type '\"z\"' can't be used to index type 'typeof foo'."
                        .to_owned()
                ),
                (
                    2339,
                    71,
                    1,
                    "Property 'q' does not exist on type 'typeof foo'.".to_owned()
                )
            ]
        );
}

#[test]
fn expando_template_key_records_like_string_literal() {
    // Round 2: getElementOrPropertyAccessName (15134) is
    // string-literal-LIKE — a `x` no-substitution template key
    // records the member name exactly as "x" does, so the
    // .x / [`x`] / ["x"] reads resolve while .y keeps its row
    // (oracle-probed byte rows).
    assert_eq!(
        checked_diags("function foo() {}\nfoo[`x`] = 1;\nfoo.x;\nfoo[`x`];\nfoo[\"x\"];\nfoo.y;\n"),
        [(
            2339,
            63,
            1,
            "Property 'y' does not exist on type 'typeof foo'.".to_owned()
        )]
    );
}

#[test]
fn recursive_non_local_function_return_displays_as_typeof_symbol() {
    let options = CompilerOptions {
        strict_null_checks: Some(true),
        no_error_truncation: Some(true),
        target: Some(ScriptTarget::ES2015.bits()),
        ..CompilerOptions::default()
    };
    assert_eq!(
        checked_diags_with(
            concat!(
                "class C { private x = 1; }\n",
                "class D extends C {}\n",
                "function foo(x: \"hi\", item: string): typeof foo;\n",
                "function foo(x: string, item: string): typeof foo { return null; }\n",
                "var a: D = foo(\"hi\", \"\");\n",
            ),
            &options,
        ),
        [
            (
                2322,
                149,
                6,
                "Type 'null' is not assignable to type '(x: \"hi\", item: string) => typeof foo'."
                    .to_owned(),
            ),
            (
                2322,
                168,
                1,
                "Type '(x: \"hi\", item: string) => typeof foo' is not assignable to type 'D'."
                    .to_owned(),
            ),
        ]
    );
}

#[test]
fn union_target_member_elaborates_through_best_match() {
    // getBestMatchIndexedAccessTypeOrUndefined's union leg: the
    // member row lands on `m` (the head suppresses), method and
    // plain flavors alike.
    assert_eq!(
        checked_diags("let o: { m: () => string } | { x: number } = { m() { return 1 } };\n"),
        [(
            2322,
            47,
            1,
            "Type '() => number' is not assignable to type '() => string'.".to_owned()
        )]
    );
    assert_eq!(
        checked_diags("let o2: { m: string } | { x: number } = { m: 1 };\n"),
        [(
            2322,
            42,
            1,
            "Type 'number' is not assignable to type 'string'.".to_owned()
        )]
    );
}

#[test]
fn union_target_object_members_keep_the_union_head() {
    // The 65185 substitution needs a NULLABLE-shaped union — an
    // object-member union keeps the full union face (declared
    // source; the fresh-literal twin rides a pre-existing
    // discriminated-union verdict FN outside this slice).
    assert_eq!(
            checked_diags(
                "declare let src3: { kind: \"a\"; v: number };\nlet o3b: { kind: \"a\"; v: string } | { kind: \"b\"; v: number } = src3;\n"
            ),
            [(
                2322,
                48,
                3,
                "Type '{ kind: \"a\"; v: number; }' is not assignable to type '{ kind: \"a\"; v: string; } | { kind: \"b\"; v: number; }'."
                    .to_owned()
            )]
        );
}

#[test]
fn global_object_head_selection_distinguishes_members_from_signatures() {
    assert_eq!(
            checked_diags(
                "interface Object { toString(): string }\n\
                 interface I { toString(): number }\n\
                 interface Callable { (): void }\n\
                 declare let o: Object;\n\
                 declare let i: I;\n\
                 declare let c: Callable;\n\
                 i = o;\n\
                 c = o;\n"
            ),
            [
                (
                    2696,
                    173,
                    1,
                    "The 'Object' type is assignable to very few other types. Did you mean to use the 'any' type instead?"
                        .to_owned()
                ),
                (
                    2322,
                    180,
                    1,
                    "Type 'Object' is not assignable to type 'Callable'.".to_owned()
                )
            ]
        );
    assert_eq!(
        checked_chain_codes(
            "interface Object { toString(): string }\n\
                 interface I { toString(): number }\n\
                 interface Missing { x: number }\n\
                 interface Callable { (): void }\n\
                 declare let o: Object;\n\
                 declare let i: I;\n\
                 declare let m: Missing;\n\
                 declare let c: Callable;\n\
                 i = o;\n\
                 m = o;\n\
                 c = o;\n"
        ),
        [
            vec![2696, 2201, 2322],
            vec![2696, 2741],
            vec![2322, 2696, 2658],
        ]
    );
}

#[test]
fn type_variable_constraint_retry_preserves_relation_failure_frames() {
    assert_eq!(
        checked_chain_codes(
            "function f<T extends \"a\" | \"b\">(x: T) {\n\
                     let y: `${T}` = x;\n\
                 }\n"
        ),
        [vec![2322, 2322, 2322]]
    );
}

#[test]
fn iife_optional_probe_counts_effective_arguments() {
    // isOptionalParameter's IIFE arm reads
    // getEffectiveCallArguments — the spread tuple counts 2, so
    // `b` is NOT optional.
    assert_eq!(
        checked_diags("(function f(a, b) {\n    let s: string = f;\n})(...[1, \"\"] as const);\n"),
        [(
            2322,
            28,
            1,
            "Type '(a: 1, b: \"\") => void' is not assignable to type 'string'.".to_owned()
        )]
    );
}

#[test]
fn optional_target_member_reports_without_the_missing_type() {
    // The elaborateElementwise report tail strips the missing
    // type on optional targets: '() => string', not
    // '(() => string) | undefined'; shorthand rides the same
    // tail.
    assert_eq!(
        checked_diags("let o4: { m?: () => string } = { m() { return 1 } };\n"),
        [(
            2322,
            33,
            1,
            "Type '() => number' is not assignable to type '() => string'.".to_owned()
        )]
    );
    assert_eq!(
        checked_diags("declare let p: number;\nlet o6: { p?: string } = { p };\n"),
        [(
            2322,
            50,
            1,
            "Type 'number' is not assignable to type 'string'.".to_owned()
        )]
    );
}

// ---- 9.3b2 member-elaboration pins (method/accessor yields) ----

#[test]
fn method_member_elaborates_at_the_name() {
    assert_eq!(
        checked_diags("let o1: { m(): string } = { m() { return 1 } };\n"),
        [(
            2322,
            28,
            1,
            "Type '() => number' is not assignable to type '() => string'.".to_owned()
        )]
    );
}

#[test]
fn accessor_pair_double_yields_one_row_per_name() {
    // generateObjectLiteralElements yields the getter AND the
    // setter — two rows, both over the shared member's read type.
    assert_eq!(
        checked_diags("let o2: { p: string } = { get p() { return 1 }, set p(v: number) {} };\n"),
        [
            (
                2322,
                30,
                1,
                "Type 'number' is not assignable to type 'string'.".to_owned()
            ),
            (
                2322,
                52,
                1,
                "Type 'number' is not assignable to type 'string'.".to_owned()
            )
        ]
    );
}

#[test]
fn computed_method_member_keeps_the_plain_2322() {
    // Method yields carry no errorMessage — the 2418
    // computed-property swap is PropertyAssignment-only.
    assert_eq!(
        checked_diags("const k = \"m\";\nlet o3: { m(): string } = { [k]() { return 1 } };\n"),
        [(
            2322,
            43,
            3,
            "Type '() => number' is not assignable to type '() => string'.".to_owned()
        )]
    );
}

#[test]
fn accessor_members_elaborate_against_index_targets() {
    assert_eq!(
            checked_diags(
                "let o4: { [k: string]: number } = { get p() { return \"s\" }, set p(v: string) {} };\n"
            ),
            [
                (
                    2322,
                    40,
                    1,
                    "Type 'string' is not assignable to type 'number'.".to_owned()
                ),
                (
                    2322,
                    64,
                    1,
                    "Type 'string' is not assignable to type 'number'.".to_owned()
                )
            ]
        );
}

#[test]
fn method_member_elaborates_against_index_target() {
    assert_eq!(
        checked_diags("let o5: { [k: string]: number } = { m() { return \"s\" } };\n"),
        [(
            2322,
            36,
            1,
            "Type '() => string' is not assignable to type 'number'.".to_owned()
        )]
    );
}

#[test]
fn class_static_side_displays_typeof_face() {
    assert_eq!(
        checked_diags("class A3 {}\nvar v3: number = A3;\n"),
        [(
            2322,
            16,
            2,
            "Type 'typeof A3' is not assignable to type 'number'.".to_owned()
        )]
    );
}

#[test]
fn class_expression_type_queries_use_written_or_anonymous_names() {
    let messages = |text: &str, code: u32| {
        checked_diags(text)
            .into_iter()
            .filter(|row| row.0 == code)
            .map(|row| row.3)
            .collect::<Vec<_>>()
    };
    assert_eq!(
            messages(
                "function foo<T>(x = class { prop: T }): T { return undefined; }\n\
                 foo(class { static prop = \"hello\" }).length;\n"
                ,
                2345,
            ),
            [
                "Argument of type 'typeof (Anonymous class)' is not assignable to parameter of type 'typeof (Anonymous class)'.".to_owned(),
            ]
        );
    assert_eq!(
        messages(
            "var ExpandoExpr3 = class { n = 10001; };\n\
                 ExpandoExpr3.prop = 3;\n",
            2339,
        ),
        ["Property 'prop' does not exist on type 'typeof ExpandoExpr3'.".to_owned(),]
    );
}

#[test]
fn enum_object_displays_typeof_face() {
    assert_eq!(
        checked_diags("enum E3 { X }\nvar v4: number = E3;\n"),
        [(
            2322,
            18,
            2,
            "Type 'typeof E3' is not assignable to type 'number'.".to_owned()
        )]
    );
}

#[test]
fn outer_generic_reference_qualifies_changed_arguments() {
    let source = "interface Array<T> { length: number; [n: number]: T }\n\
        function mixin<T extends { new (...args: any[]): {} }>(superclass: T) {\n\
            return class extends superclass { get name() { return \"\"; } };\n\
        }\n\
        class BaseClass { set name(v: string) {} }\n\
        class MyClass extends mixin(BaseClass) { get name() { return \"\"; } }\n";
    let diagnostics = checked_diags(source);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(
        (
            diagnostics[0].0,
            diagnostics[0].2,
            diagnostics[0].3.as_str()
        ),
        (
            2611,
            4,
            "'name' is defined as a property in class \
                 'mixin<typeof BaseClass>.(Anonymous class) & BaseClass', but is overridden here \
                 in 'MyClass' as an accessor."
        )
    );
    assert_eq!(
        diagnostics[0].1,
        source.rfind("name").expect("derived accessor name") as u32
    );
}

#[test]
fn outer_generic_reference_omits_unchanged_qualification() {
    assert_eq!(
        checked_diags(
            "function make<P>() { return class { value!: P; method() { this.missing; } }; }\n"
        ),
        [(
            2339,
            63,
            7,
            "Property 'missing' does not exist on type '(Anonymous class)'.".to_owned()
        )]
    );
}

// ---- 9.3b relation-reporting pins (excess property, did-you-mean,
// elaboration extensions) ----

#[test]
fn excess_property_reports_parent_skipped_2353() {
    assert_eq!(
        checked_diags("declare let a2: { x: number };\na2 = { x: 1, y: 2 };\n"),
        [(
            2353,
            44,
            1,
            "Object literal may only specify known properties, and 'y' does not exist in \
                 type '{ x: number; }'."
                .to_owned()
        )]
    );
}

#[test]
fn excess_property_with_spelling_suggestion_reports_2561() {
    assert_eq!(
        checked_diags("declare let b2: { hello: number };\nb2 = { hallo: 1 };\n"),
        [(
            2561,
            42,
            5,
            "Object literal may only specify known properties, but 'hallo' does not exist \
                 in type '{ hello: number; }'. Did you mean to write 'hello'?"
                .to_owned()
        )]
    );
}

#[test]
fn excess_property_suggestion_uses_the_written_string_literal_name() {
    assert_eq!(
        checked_diags(
            "declare let value: { \"ns:attribute\": string };\nvalue = { attribute: \"x\" };\n",
        ),
        [(
            2561,
            57,
            9,
            "Object literal may only specify known properties, but 'attribute' does not exist \
                 in type '{ \"ns:attribute\": string; }'. Did you mean to write '\"ns:attribute\"'?"
                .to_owned()
        )]
    );
}

#[test]
fn global_augmentation_namespace_diagnostic_hides_the_internal_symbol_name() {
    let diagnostics =
        checked_diags("export {};\ndeclare global { namespace JSX { type T = JSX.Missing; } }\n");
    let missing = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.0 == 2694)
        .expect("missing global JSX member diagnostic");
    assert_eq!(
        missing.3,
        "Namespace 'global.JSX' has no exported member 'Missing'."
    );
}

#[test]
fn global_object_union_accepts_unknown_object_literal_properties() {
    assert_eq!(
        checked_diags_with(
            "interface Object {}\nconst x: Object | string = { x: 0 };\nconst y: Object | undefined = { x: 0 };\n",
            &CompilerOptions {
                strict_null_checks: Some(true),
                ..CompilerOptions::default()
            },
        ),
        []
    );
}

#[test]
fn relation_head_reports_exact_optional_property_mismatch() {
    let text = "interface Source { index?: number; groups?: { value: string } }\n\
                interface Target { index: number; groups?: { value: string } }\n\
                declare let source: Source;\n\
                let target: Target = source;\n";
    let diagnostics = checked_diags_with(
        text,
        &CompilerOptions {
            strict_null_checks: Some(true),
            exact_optional_property_types: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].0, 2375);
    assert!(diagnostics[0]
        .3
        .contains("with 'exactOptionalPropertyTypes: true'"));
}

#[test]
fn aliased_union_type_variables_keep_the_normalized_intersection_verdict() {
    // oracle (vendored 6.0.3, strict, noLib, 2026-07-23): clean.
    // The origin's instantiable constituents hide inside the named
    // union member `N<T, U>`. The relation must decide this through
    // getEffectiveConstraintOfIntersection; display is not allowed
    // to act as a verdict shield.
    assert_eq!(
        checked_diags(
            "type A = 1 | 2;\ntype B = 2 | 3;\ntype N<T, U> = (T & U) | 4;\n\nfunction f<T \
                 extends A, U extends B>(\n  ab: T & U\n): N<T, U> & (A | B) {\n  return ab;\n}\n"
        ),
        []
    );
}

#[test]
fn unique_symbol_missing_property_prints_qualified_computed_faces() {
    // oracle (vendored 6.0.3, strict, noLib, 2026-07-23):
    // Property '[B.sym]' is missing in type '{ [A.sym]: number; }'
    // but required in type '{ [B.sym]: number; }'. @4:5 len 1,
    // related 2728 '[B.sym]' is declared here. The namespace-
    // nested symbols qualify through the property-declaration
    // enclosing (addPropertyToElementList 52265-52267) on the
    // PLAIN pass — no FQ retry involved; the propName rides
    // WriteComputedProps' name-node reprint.
    assert_eq!(
        checked_diags(
            "declare namespace A { const sym: unique symbol }\ndeclare namespace B { const \
                 sym: unique symbol }\ndeclare const a: { [A.sym]: number };\nlet b: { [B.sym]: \
                 number } = a;\n"
        ),
        [(
            2741,
            140,
            1,
            "Property '[B.sym]' is missing in type '{ [A.sym]: number; }' but required in \
                 type '{ [B.sym]: number; }'."
                .to_owned()
        )]
    );
}

#[test]
fn unique_symbol_member_uses_the_value_with_the_matching_declared_type() {
    // getContainersOfSymbol's firstVariableMatch: the unique
    // symbol member belongs to the TYPE-only SymbolConstructor,
    // but its Value expression qualifies through the in-scope
    // `Symbol` value whose type is exactly SymbolConstructor.
    let messages = checked_diags(
        "interface SymbolConstructor { readonly iterator: unique symbol; }\n\
             declare var Symbol: SymbolConstructor;\n\
             declare var source: { [Symbol.iterator]?(): string };\n\
             let target: number = source;\n",
    )
    .into_iter()
    .filter(|row| row.0 == 2322)
    .map(|row| row.3)
    .collect::<Vec<_>>();
    assert_eq!(
        messages,
        ["Type '{ [Symbol.iterator]?(): string; }' is not assignable to type 'number'."]
    );
}

#[test]
fn quoted_missing_property_preserves_its_written_name() {
    let messages =
        checked_diags("declare let source: {};\nlet target: { '1.0': string } = source;\n")
            .into_iter()
            .filter(|row| row.0 == 2741)
            .map(|row| row.3)
            .collect::<Vec<_>>();

    assert_eq!(
        messages,
        ["Property ''1.0'' is missing in type '{}' but required in type '{ '1.0': string; }'."]
    );
}

#[test]
fn top_level_unique_symbol_member_face_stays_bare() {
    // oracle (vendored 6.0.3, strict, noLib, 2026-07-23):
    // Property '[s]' is missing in type '{}' but required in type
    // '{ [s]: number; }'. A global-script symbol is accessible
    // bare from the property declaration, so no qualifier prints.
    assert_eq!(
        checked_diags(
            "declare const s: unique symbol;\ndeclare const a4: {};\nlet b4: { [s]: number } \
                 = a4;\n"
        ),
        [(
            2741,
            58,
            2,
            "Property '[s]' is missing in type '{}' but required in type '{ [s]: number; }'."
                .to_owned()
        )]
    );
}

#[test]
fn umd_global_alias_is_excluded_inside_external_modules() {
    // oracle probe A (vendored 6.0.3, strict, driver.mjs
    // 2026-07-24): ONE 2741 @a.ts:34 len 1 — Property '[U.s]' is
    // missing in type '{}' but required in type
    // '{ [s]: number; }'. — the WriteComputedProps head keeps the
    // written '[U.s]'; the target member face drops the UMD
    // global-alias route (trySymbolTable 50341, enclosing is the
    // external-module property declaration) AND its module parent
    // (52996-52998, yieldModuleSymbol falsy on the
    // symbolToExpression path), leaving the bare '[s]'. related
    // 2728 @a.ts:61 len 5 '[U.s]' is declared here. The raw
    // checker stream also contains 2686 at the `U` reference
    // @a.ts:62 len 1; the public program layer consumes it through
    // @ts-ignore. Suggestions such as 6133 stay outside this sink.
    with_program_state(
        &[
            (
                "umd.d.ts",
                "export as namespace U;\nexport const s: unique symbol;\n",
            ),
            (
                "a.ts",
                "export {};\ndeclare let a: {};\nlet b: {\n    // @ts-ignore\n    [U.s]: \
                     number\n} = a;\n",
            ),
        ],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(1);
            assert_eq!(
                diag_rows(state),
                [
                    (
                        2686,
                        62,
                        1,
                        "'U' refers to a UMD global, but the current file is a module. \
                             Consider adding an import instead."
                            .to_owned()
                    ),
                    (
                        2741,
                        34,
                        1,
                        "Property '[U.s]' is missing in type '{}' but required in type '{ \
                             [s]: number; }'."
                            .to_owned()
                    )
                ]
            );
        },
    );
}

#[test]
fn self_import_export_value_local_wins_over_the_alias_scan() {
    // oracle probe C (vendored 6.0.3, strict, driver.mjs
    // 2026-07-24): 2741 @c.ts:91 len 1 — Property '[s]' is missing
    // in type '{}' but required in type '{ [s]: number; }'. — NOT
    // '[Self.s]': the exportSymbol arm (50348-50357) fires on the
    // "s" EXPORT_VALUE local BEFORE the later "Self" entry's alias
    // leg inside tsc's single per-entry forEachEntry pass. related
    // 2728 @c.ts:96 len 3 '[s]' is declared here; the 6133
    // suggestions stay outside the sink.
    with_program_state(
        &[(
            "c.ts",
            "export declare const s: unique symbol;\nimport * as Self from \
                 \"./c\";\ndeclare let a: {};\nlet b: { [s]: number } = a;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            assert_eq!(
                diag_rows(state),
                [(
                    2741,
                    91,
                    1,
                    "Property '[s]' is missing in type '{}' but required in type '{ [s]: \
                         number; }'."
                        .to_owned()
                )]
            );
        },
    );
}

#[test]
fn script_global_member_face_ignores_module_local_shadowing() {
    // oracle probe D (vendored 6.0.3, strict, driver.mjs
    // 2026-07-24): 2741 @a.ts:66 len 1 — Property '[s]' is missing
    // in type '{}' but required in type '{ [s]: number; }'.
    // related 2728 at the SCRIPT declaration (global.d.ts:49 len
    // 3). The member face re-encloses at the property declaration
    // in the script file, where the globals direct hit precedes
    // both the alias scan and the globals-tail globalThis probe
    // (50359) — the module-local shadowing `s` never enters. Pins
    // the globals-tail omission's re-justification
    // (try_symbol_table_slice header).
    with_program_state(
        &[
            (
                "global.d.ts",
                "declare const s: unique symbol;\ndeclare let g: { [s]: number };\n",
            ),
            (
                "a.ts",
                "export {};\ndeclare const s: unique symbol;\ndeclare let a: {};\nlet b: \
                     typeof g = a;\n",
            ),
        ],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(1);
            assert_eq!(
                diag_rows(state),
                [(
                    2741,
                    66,
                    1,
                    "Property '[s]' is missing in type '{}' but required in type '{ [s]: \
                         number; }'."
                        .to_owned()
                )]
            );
        },
    );
}

#[test]
fn alias_typed_computed_member_splits_prop_name_and_face() {
    // oracle (vendored 6.0.3, strict, noLib, 2026-07-23):
    // Property '[k]' is missing in type '{}' but required in type
    // '{ [B.sym]: number; }'. The propName re-prints the written
    // name node (`[k]`); the member face renders the NAMETYPE
    // symbol's chain (`[B.sym]`).
    assert_eq!(
        checked_diags(
            "declare namespace B { const sym: unique symbol }\ndeclare const k: typeof \
                 B.sym;\ndeclare const a2: {};\nlet b2: { [k]: number } = a2;\n"
        ),
        [(
            2741,
            106,
            2,
            "Property '[k]' is missing in type '{}' but required in type '{ [B.sym]: number; \
                 }'."
            .to_owned()
        )]
    );
}

#[test]
fn early_bound_computed_string_name_reprints_the_bracket_face() {
    // oracle (vendored 6.0.3, strict, noLib, 2026-07-23):
    // Property '["ab"]' is missing in type '{}' but required in
    // type '{ ab: number; }'. The single-quoted, space-padded
    // source name normalizes through the printer: double quotes,
    // no padding — while the TYPE face keeps the identifier form.
    assert_eq!(
        checked_diags("declare const a5: {};\nlet b5: { [ 'ab' ]: number } = a5;\n"),
        [(
            2741,
            26,
            2,
            "Property '[\"ab\"]' is missing in type '{}' but required in type '{ ab: number; \
                 }'."
            .to_owned()
        )]
    );
}

#[test]
fn late_bound_multi_missing_list_prints_source_verbatim() {
    // oracle (vendored 6.0.3, strict, noLib, 2026-07-23) — the
    // multi-property 2739 rides plain symbolToString: the
    // late-bound name prints its declaration SOURCE text verbatim
    // (`[ B . sym ]`, spaces kept) while the TYPE face qualifies
    // through the property enclosing (`[B.sym]`) and sorts the
    // late-bound member after the early ones.
    assert_eq!(
        checked_diags(
            "declare namespace B { const sym: unique symbol }\ndeclare const a8: {};\nlet \
                 b8: { [ B . sym ]: number; other: string } = a8;\n"
        ),
        [(
            2739,
            75,
            2,
            "Type '{}' is missing the following properties from type '{ other: string; \
                 [B.sym]: number; }': other, [ B . sym ]"
                .to_owned()
        )]
    );
}

#[test]
fn five_missing_properties_are_all_named_but_six_use_the_summary_form() {
    let text = "interface Five { a: 1; b: 2; c: 3; d: 4; e: 5 }\n\
                interface Six { a: 1; b: 2; c: 3; d: 4; e: 5; f: 6 }\n\
                declare const source: {};\n\
                const five: Five = source;\n\
                const six: Six = source;\n";
    let rows = checked_diags(text)
        .into_iter()
        .filter(|row| matches!(row.0, 2739 | 2740))
        .map(|row| (row.0, row.3))
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [
            (
                2739,
                "Type '{}' is missing the following properties from type 'Five': a, b, c, d, e"
                    .to_owned(),
            ),
            (
                2740,
                "Type '{}' is missing the following properties from type 'Six': a, b, c, d, and 2 more."
                    .to_owned(),
            ),
        ]
    );
}

#[test]
fn multi_missing_source_uses_the_structural_relation_face() {
    let text = "interface Number {}\n\
                    interface Obj { hello: string; world: number }\n\
                    interface NumberTo<T> { [x: number]: T }\n\
                    type NumberToNumber = NumberTo<number>;\n\
                    declare const n: NumberToNumber;\n\
                    const a: Obj = n;\n\
                    type Brand<T> = number & { __brand: T };\n\
                    declare const b: Brand<{ view: number; styleMedia: string }>;\n\
                    const c: Obj = b;\n";
    let rows: Vec<_> = checked_diags(text)
        .into_iter()
        .filter(|row| row.0 == 2739)
        .collect();
    assert_eq!(
            rows.iter().map(|row| row.3.as_str()).collect::<Vec<_>>(),
            [
                "Type 'NumberTo<number>' is missing the following properties from type 'Obj': hello, world",
                "Type 'Number & { __brand: { view: number; styleMedia: string; }; }' is missing the following properties from type 'Obj': hello, world",
            ]
        );
}

#[test]
fn did_you_mean_new_reports_at_the_member_value() {
    // elaborateDidYouMeanToCallOrConstruct re-anchors the member
    // relation at the VALUE (`A2`, not the property name) and the
    // missing-property override renders the class-static typeof
    // face.
    assert_eq!(
        checked_diags(
            "class A2 { foo(): string { return '' } }\nvar c2: { [x: string]: A2 } = { a: A2 };\n"
        ),
        [(
            2741,
            76,
            2,
            "Property 'foo' is missing in type 'typeof A2' but required in type 'A2'.".to_owned()
        )]
    );
}

#[test]
fn shorthand_member_supports_missing_property_head() {
    // The shorthand walk feeds the literal's members; the head is
    // the parent-skipped missing-'b' face at the declaration.
    assert_eq!(
        checked_diags("var id: number = 1;\nvar person: { b: string; id: number } = { id };\n"),
        [(
            2741,
            24,
            6,
            "Property 'b' is missing in type '{ id: number; }' but required in type \
                 '{ b: string; id: number; }'."
                .to_owned()
        )]
    );
}

#[test]
fn shorthand_member_row_replaces_the_return_head() {
    // generateObjectLiteralElements yields shorthand members with
    // no inner expression — the member row anchors at the NAME.
    assert_eq!(
        checked_diags(
            "var name2: string = 'x';\nfunction foo(): { name2: number } { return { name2 }; }\n"
        ),
        [(
            2322,
            70,
            5,
            "Type 'string' is not assignable to type 'number'.".to_owned()
        )]
    );
}

#[test]
fn index_signature_target_elaborates_member_rows() {
    // elaborateElementwise's targetPropType is an indexed access:
    // a property miss falls through to the applicable index
    // signature's value type.
    assert_eq!(
        checked_diags("var d2: { [x: number]: string } = { 1: 1 };\n"),
        [(
            2322,
            36,
            1,
            "Type 'number' is not assignable to type 'string'.".to_owned()
        )]
    );
}

#[test]
fn constructor_return_elaborates_and_reports_2409() {
    let rows = checked_diags("class F { x: string = ''; constructor() { return { x: 1 }; } }\n");
    assert_eq!(
        rows,
        [
            (
                2322,
                51,
                1,
                "Type 'number' is not assignable to type 'string'.".to_owned()
            ),
            (
                2409,
                42,
                6,
                "Return type of constructor signature must be assignable to the instance \
                     type of the class."
                    .to_owned()
            )
        ]
    );
}

#[test]
fn merged_declaration_initializer_elaborates_member_rows() {
    assert_eq!(
            checked_diags(
                "var p: { x: number; y: number };\nvar p: { x: number; y: number } = { x: 0, y: '' };\n"
            ),
            [(
                2322,
                75,
                1,
                "Type 'string' is not assignable to type 'number'.".to_owned()
            )]
        );
}

#[test]
fn non_primitive_source_walks_as_the_empty_object_face() {
    // structuredTypeRelatedTo apparent-izes `object` in place —
    // the missing-property face renders '{}'.
    assert_eq!(
        checked_diags("var y2 = { foo: 'bar' };\ndeclare var o: object;\ny2 = o;\n"),
        [(
            2741,
            48,
            2,
            "Property 'foo' is missing in type '{}' but required in type \
                 '{ foo: string; }'."
                .to_owned()
        )]
    );
}

#[test]
fn template_literal_index_key_admits_matching_property_names() {
    // isKnownProperty probes applicability through the faithful
    // isApplicableIndexType — `sfoo` fits `[k: \`s${string}\`]`,
    // so the literal is clean (the flag-shortcut fabricated an
    // excess verdict here).
    assert_eq!(
            checked_diags(
                "type F2 = { [k: `s${string}`]: (x: string) => void };\ndeclare let f3: F2;\nf3 = { sfoo: (x) => {} };\n"
            ),
            []
        );
}

#[test]
fn case_clause_excess_property_reports_2353() {
    // The comparable relation runs the same excess arm — the 2678
    // head never lands.
    assert_eq!(
            checked_diags(
                "class C3 { id: number = 1 }\nswitch (new C3()) {\n    case { id: 12, name3: '' }:\n}\n"
            ),
            [(
                2353,
                67,
                5,
                "Object literal may only specify known properties, and 'name3' does not exist \
                 in type 'C3'."
                    .to_owned()
            )]
        );
}

#[test]
fn non_finite_numeric_keys_resolve_by_canonical_name() {
    // Members declared with numeric keys that stringify to
    // non-finite/huge canonical names ("Infinity",
    // "9.671406556917009e+24") resolve through both the string
    // and numeric element-access faces (binaryIntegerLiteral's
    // clean rows — the 7053 face fabricated here while the object
    // display curtained the report).
    assert_eq!(
        checked_diags("var o = { 1e999: true };\no[\"Infinity\"];\n"),
        []
    );
    assert_eq!(
            checked_diags(
                "var o2 = { 9.671406556917009e+24: true };\no2[9.671406556917009e+24];\no2[\"9.671406556917009e+24\"];\n"
            ),
            []
        );
    assert_eq!(checked_diags("var o3 = { 1e999: true };\no3[1e999];\n"), []);
}

#[test]
fn interface_in_annotation_on_covariant_use_reports_2636() {
    let diags = checked_diags("interface Foo<in T> { f: () => T }\n");
    assert_eq!(
        diags,
        [(
            2636,
            14,
            4,
            "Type 'Foo<super-T>' is not assignable to type 'Foo<sub-T>' as implied by \
                 variance annotation."
                .to_owned()
        )]
    );
}

#[test]
fn correct_variance_annotations_are_silent() {
    assert_eq!(checked_diags("interface Foo<out T> { f: () => T }\n"), []);
    // in out together: tsc skips the marker probe (modifiers must
    // be exactly In or exactly Out).
    assert_eq!(
        checked_diags("interface Foo<in out T> { f: (x: T) => void }\n"),
        []
    );
}

#[test]
fn alias_out_annotation_reports_2636_with_alias_display() {
    let diags = checked_diags("type F<out T> = (x: T) => void;\n");
    assert_eq!(
        diags,
        [(
            2636,
            7,
            5,
            "Type 'F<sub-T>' is not assignable to type 'F<super-T>' as implied by \
                 variance annotation."
                .to_owned()
        )]
    );
}

#[test]
fn alias_annotation_on_non_object_rhs_reports_2637() {
    let diags = checked_diags("type F<in T> = T[];\ninterface Array<T> { length: number }\n");
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2637, 7, 4));
}

#[test]
fn class_property_out_annotation_reports_2636() {
    // Oracle pair: 2564 (checkPropertyInitialization's
    // no-constructor face, live since 5.8c) + the variance 2636.
    let diags = checked_diags("class C<out T> { f: (x: T) => void; }\n");
    assert_eq!(
        diags,
        [
            (
                2564,
                17,
                1,
                "Property 'f' has no initializer and is not definitely assigned in the \
                     constructor."
                    .to_owned()
            ),
            (
                2636,
                8,
                5,
                "Type 'C<sub-T>' is not assignable to type 'C<super-T>' as implied by \
                     variance annotation."
                    .to_owned()
            )
        ]
    );
}

#[test]
fn class_method_parameters_stay_bivariant_and_silent() {
    assert_eq!(checked_diags("class C<out T> { f(x: T): void {} }\n"), []);
}

#[test]
fn multi_parameter_marker_display_names_other_parameters() {
    let diags = checked_diags("interface P<A, out B> { f: (x: B) => A }\n");
    assert_eq!(
        diags,
        [(
            2636,
            15,
            5,
            "Type 'P<A, sub-B>' is not assignable to type 'P<A, super-B>' as implied \
                 by variance annotation."
                .to_owned()
        )]
    );
}

#[test]
fn block_nested_interfaces_are_checked_via_check_block() {
    let diags = checked_diags("{ interface J<out T> { g: (x: T) => void } }\n");
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2636, 14, 5));
}

// ---- checkTypeParameters family — oracle-pinned ----

#[test]
fn self_referential_default_reports_2744_not_2716() {
    let diags = checked_diags("interface I<T = T> { x: T }\n");
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2744, 16, 1));
}

#[test]
fn forward_referencing_default_reports_2744() {
    let diags = checked_diags("interface I<T = U, U = string> { x: T }\n");
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2744, 16, 1));
}

#[test]
fn required_parameter_after_optional_reports_2706() {
    let diags = checked_diags("interface I<T = string, U> { x: T }\n");
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2706, 24, 1));
}

#[test]
fn cross_generic_default_cycle_reports_2716() {
    let diags = checked_diags("interface P<T = Q> { x: T }\ninterface Q<U = P> { y: U }\n");
    assert_eq!(
        diags,
        [(
            2716,
            44,
            1,
            "Type parameter 'U' has a circular default.".to_owned()
        )]
    );
}

#[test]
fn default_not_satisfying_constraint_reports_2344() {
    let diags = checked_diags("interface I<T extends string = number> { x: T }\n");
    assert_eq!(
        diags,
        [(
            2344,
            31,
            6,
            "Type 'number' does not satisfy the constraint 'string'.".to_owned()
        )]
    );
}

#[test]
fn circular_constraint_reports_2313_through_the_driver() {
    let diags = checked_diags("interface I<T extends T> { x: T }\n");
    assert_eq!(
        diags,
        [(
            2313,
            22,
            1,
            "Type parameter 'T' has a circular constraint.".to_owned()
        )]
    );
}

#[test]
fn reserved_names_report_2368_2457_2427() {
    let diags = checked_diags("interface I<undefined> { x: number }\n");
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2368, 12, 9));

    let diags = checked_diags("type undefined = string;\n");
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2457, 5, 9));

    let diags = checked_diags("interface any { x: number }\n");
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2427, 10, 3));
}

#[test]
fn intrinsic_keyword_validity_reports_2795() {
    let diags = checked_diags("type Foo<T> = intrinsic;\n");
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2795, 14, 9));

    assert_eq!(
        checked_diags("type Uppercase<S extends string> = intrinsic;\n"),
        []
    );
}

#[test]
fn libless_missing_lib_names_report_the_2583_family() {
    // With lib loading landed (conformance programs always carry
    // their lib set), the 5.4-era lib_globals gate is retired: a
    // LIBLESS program reports missing default-lib names exactly
    // like tsc under noLib (oracle-pinned), with the suggested-lib
    // argument from the static feature table.
    let diags = checked_diags("interface I<T extends Map> { x: T }\n");
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2583, 22, 3));
    assert!(diags[0].3.ends_with("'es2015' or later."), "{}", diags[0].3);
    let diags = checked_diags("interface I<T extends console> { x: T }\n");
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2584, 22, 7));
}

#[test]
fn unresolved_names_in_constraints_and_defaults_flow_2304() {
    let diags = checked_diags("interface I<T extends Missing> { x: T }\n");
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2304, 22, 7));

    let diags = checked_diags("interface I<T = Missing> { x: T }\n");
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2304, 16, 7));
}

// ---- checkTypeArgumentConstraints — oracle-pinned ----

#[test]
fn explicit_type_arguments_check_their_constraints() {
    let diags = checked_diags("interface I<T extends string> { x: T }\ntype X = I<number>;\n");
    assert_eq!(
        diags,
        [(
            2344,
            50,
            6,
            "Type 'number' does not satisfy the constraint 'string'.".to_owned()
        )]
    );
    assert_eq!(
        checked_diags("interface I<T extends string> { x: T }\ntype X = I<\"a\">;\n"),
        []
    );
    // Defaults fill through fillMissingTypeArguments before the
    // constraint instantiates.
    assert_eq!(
        checked_diags(
            "interface I<T extends string, U extends T = T> { x: T }\ntype X = I<\"a\">;\n"
        ),
        []
    );
    // `result = result && checkTypeAssignableTo(...)` is
    // observable: after the first failing constraint, tsc 6.0.3
    // does not publish a second 2344 for the same reference.
    assert_eq!(
        checked_diags(
            "interface Pair<T extends string, U extends number> { t: T; u: U }\n\
                 type Bad = Pair<boolean, boolean>;\n"
        ),
        [(
            2344,
            82,
            7,
            "Type 'boolean' does not satisfy the constraint 'string'.".to_owned()
        )]
    );
}

#[test]
fn alias_type_arguments_check_their_constraints() {
    let diags = checked_diags(
            "type A<T extends number> = T[];\ninterface Array<T> { length: number }\ntype X = A<string>;\n",
        );
    assert_eq!(
        diags,
        [(
            2344,
            81,
            6,
            "Type 'string' does not satisfy the constraint 'number'.".to_owned()
        )]
    );
}

// ---- driver bookkeeping ----

#[test]
fn rechecking_a_type_checked_file_is_idempotent() {
    with_program_state(
        &[("a.ts", "interface Foo<out T> { f: (x: T) => void }\n")],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            let first = diag_rows(state);
            assert_eq!(first.len(), 1);
            state.check_source_file(0);
            assert_eq!(diag_rows(state), first, "TypeChecked gate must hold");
            assert!(
                state.deferred_nodes.is_empty(),
                "deferred set drains and clears"
            );
        },
    )
}

// ---- 9.3b3 symbol/value/module head pins (all rows oracle-
// probed byte-exact against vendored 6.0.3, noLib + strict;
// multi-file pins use the unit env's extension-less quoted module
// names — the corpus harness roots names at "/", so goldens show
// `import("/b")` where these pins show `import("b")`, the same
// binder naming rule over a different fileName input) ----

/// Program-driving helper for the multi-file pins: (file, code,
/// start, length, message) rows in checker sink order.
fn program_diags(files: &[(&str, &str)]) -> Vec<(String, u32, u32, u32, String)> {
    program_diags_with(files, &CompilerOptions::default(), "/")
}

/// The options/cwd-carrying twin: `cwd` mirrors the harness
/// ProgramJson `cwd` the driver threads through
/// check_program_with_libs_at.
fn program_diags_with(
    files: &[(&str, &str)],
    options: &CompilerOptions,
    cwd: &str,
) -> Vec<(String, u32, u32, u32, String)> {
    with_program_state(files, options, |state| {
        state.host_current_directory = cwd.to_owned();
        for index in 0..state.binder.files().count() {
            state.check_source_file(index);
        }
        state
            .diagnostics
            .iter()
            .filter(|diag| {
                diag.file_name.is_some()
                    && diag.category() == tsc_diagnostics::DiagnosticCategory::Error
            })
            .map(|diag| {
                (
                    diag.file_name.clone().unwrap(),
                    diag.code(),
                    diag.start.unwrap_or(u32::MAX),
                    diag.length.unwrap_or(u32::MAX),
                    diag.message_text().to_owned(),
                )
            })
            .collect()
    })
}

#[test]
fn namespace_value_faces_print_typeof_unqualified() {
    // lookupSymbolChainWorker 52950-52952: no enclosingDeclaration
    // -> chain=[symbol] -> the NESTED namespace face prints
    // `typeof Inner`, NOT `typeof Outer.Inner`.
    assert_eq!(
            checked_diags(
                "namespace Outer {\n    export namespace Inner {\n        export const x = 1;\n    }\n}\nOuter.NoSuch;\nOuter.Inner.NoSuch;\nlet n: number = Outer.Inner;\n"
            ),
            [
                (
                    2339,
                    89,
                    6,
                    "Property 'NoSuch' does not exist on type 'typeof Outer'.".to_owned()
                ),
                (
                    2339,
                    109,
                    6,
                    "Property 'NoSuch' does not exist on type 'typeof Inner'.".to_owned()
                ),
                (
                    2322,
                    121,
                    1,
                    "Type 'typeof Inner' is not assignable to type 'number'.".to_owned()
                )
            ]
        );
}

#[test]
fn merged_interface_namespace_value_prints_typeof() {
    // The upstream named-object arm's VALUE_MODULE disjunct: the
    // merged value side prints `typeof X` (createAnonymousTypeNode
    // 51779) while the TYPE position keeps the plain `X` face.
    assert_eq!(
            checked_diags(
                "interface X { i: number }\nnamespace X { export const a = 1 }\nlet n: number = X;\nlet t: X = { i: 1, extra: 2 };\n"
            ),
            [
                (
                    2322,
                    65,
                    1,
                    "Type 'typeof X' is not assignable to type 'number'.".to_owned()
                ),
                (
                    2353,
                    99,
                    5,
                    "Object literal may only specify known properties, and 'extra' does not exist in type 'X'.".to_owned()
                )
            ]
        );
}

#[test]
fn merged_class_and_enum_namespace_values_keep_typeof() {
    // Upstream-arm regression control: class+ns / enum+ns merges
    // keep the class-static/enum typeof split.
    assert_eq!(
            checked_diags(
                "class C {}\nnamespace C { export const a = 1 }\nenum E { A }\nnamespace E { export const b = 1 }\nlet n: number = C;\nlet m: number = E;\n"
            ),
            [
                (
                    2322,
                    98,
                    1,
                    "Type 'typeof C' is not assignable to type 'number'.".to_owned()
                ),
                (
                    2322,
                    117,
                    1,
                    "Type 'typeof E' is not assignable to type 'number'.".to_owned()
                )
            ]
        );
}

#[test]
fn global_this_value_prints_typeof_global_this() {
    assert_eq!(
        checked_diags("let n: number = globalThis;\n"),
        [(
            2322,
            4,
            1,
            "Type 'typeof globalThis' is not assignable to type 'number'.".to_owned()
        )]
    );
}

#[test]
fn function_namespace_merge_value_prints_typeof() {
    // The VALUE_MODULE arm runs before the FUNCTION admission at
    // the anonymous gate (tsc's 51779 disjunct order): the merged
    // fn+ns value prints `typeof f`, not a structural signature.
    assert_eq!(
        checked_diags(
            "function f() { return 1 }\nnamespace f { export const q = 1 }\nlet n: number = f;\n"
        ),
        [(
            2322,
            77,
            1,
            "Type 'typeof f' is not assignable to type 'number'.".to_owned()
        )]
    );
}

#[test]
fn private_base_constructor_uses_the_fully_qualified_class_name() {
    let rows = checked_diags(
        "namespace N { export class Base { private constructor() {} } }\n\
             class Derived extends N.Base {}\n",
    );
    assert_eq!(
        rows.into_iter()
            .filter(|row| row.0 == 2675)
            .map(|row| row.3)
            .collect::<Vec<_>>(),
        ["Cannot extend a class 'N.Base'. Class constructor is marked as private.".to_owned()]
    );
}

#[test]
fn ambient_module_value_prints_import_face() {
    // hasNonGlobalAugmentationExternalModuleSymbol admits the
    // string-literal ModuleDeclaration; the specifier is the
    // unquoted symbol name (getSpecifierForModuleSymbol 53077).
    assert_eq!(
        program_diags(&[
            (
                "g.d.ts",
                "declare module \"amb\" {\n    export const v: number;\n}\n"
            ),
            ("a.ts", "import * as A from \"amb\";\nA.nope;\n"),
        ]),
        [(
            "a.ts".to_owned(),
            2339,
            28,
            4,
            "Property 'nope' does not exist on type 'typeof import(\"amb\")'.".to_owned()
        )]
    );
}

#[test]
fn source_file_module_value_prints_import_face() {
    // The specifier is the binder's quoted module name minus
    // quotes — extension-free because
    // bindSourceFileAsExternalModule strips it at naming time —
    // rendered through the host's absolute normalized form (the
    // oracle host roots every fileName, so tsc binds and prints
    // `import("/b")` for this fixture; oracle-probed).
    assert_eq!(
        program_diags(&[
            ("b.ts", "export const bee = 1;\n"),
            ("a.ts", "import * as b from \"./b\";\nb.nope;\n"),
        ]),
        [(
            "a.ts".to_owned(),
            2339,
            28,
            4,
            "Property 'nope' does not exist on type 'typeof import(\"/b\")'.".to_owned()
        )]
    );
}

#[test]
fn empty_ambient_module_specifier_falls_back_to_file_name() {
    // getSpecifierForModuleSymbol's fileName fallback (53080):
    // `declare module ""` binds `""`, which fails
    // ambientModuleSymbolRegex, so the specifier reads
    // getNonAugmentationDeclaration's rooted file name, extension
    // intact (oracle-probed: `typeof import("/g.d.ts")`).
    assert_eq!(
        program_diags(&[
            (
                "g.d.ts",
                "declare module \"\" { export const x: number; }\n"
            ),
            ("main.ts", "import * as ns from \"\";\nns.y;\n"),
        ]),
        [(
            "main.ts".to_owned(),
            2339,
            27,
            1,
            "Property 'y' does not exist on type 'typeof import(\"/g.d.ts\")'.".to_owned()
        )]
    );
}

#[test]
fn fully_qualified_namespace_under_module_prints_import_qualifier() {
    // UseFullyQualifiedType roots the symbol chain at the external
    // module (getSymbolChain's container walk), so the 53117 gate
    // fires on chain[0] and the namespace rides as the
    // ImportTypeNode's qualifier — NOT the quoted-name entity face
    // (oracle-probed: `typeof import("/b").N` vs
    // `typeof import("/a").N`).
    assert_eq!(
            program_diags(&[
                ("a.ts", "export namespace N { export const x = 1; }\n"),
                ("b.ts", "export namespace N { export const x = \"s\"; }\n"),
                (
                    "c.ts",
                    "import { N as NA } from \"./a\";\nimport { N as NB } from \"./b\";\nlet v: typeof NA;\nv = NB;\n"
                ),
            ]),
            [(
                "c.ts".to_owned(),
                2322,
                80,
                1,
                "Type 'typeof import(\"/b\").N' is not assignable to type 'typeof import(\"/a\").N'."
                    .to_owned()
            )]
        );
}

#[test]
fn fully_qualified_interface_under_module_prints_import_type_qualifier() {
    // The Type-meaning twin of the namespace Value face:
    // symbolToTypeNode roots the chain at the external module and
    // emits an ImportTypeNode without `typeof`.
    assert_eq!(
            program_diags(&[
                (
                    "a.ts",
                    "export namespace dom { export namespace JSX { export interface Element { a: string } } }\n"
                ),
                (
                    "b.ts",
                    "export namespace dom { export namespace JSX { export interface Element { b: string } } }\n"
                ),
                (
                    "c.ts",
                    "import { dom as A } from \"./a\";\nimport { dom as B } from \"./b\";\ndeclare let source: B.JSX.Element;\nlet target: A.JSX.Element = source;\n"
                ),
            ]),
            [(
                "c.ts".to_owned(),
                2741,
                103,
                6,
                "Property 'a' is missing in type 'import(\"/b\").dom.JSX.Element' but required in \
                 type 'import(\"/a\").dom.JSX.Element'."
                    .to_owned()
            )]
        );
}

#[test]
fn fully_qualified_nested_namespace_joins_import_qualifier() {
    // The below-root links join as the qualifier spine
    // (createAccessFromSymbolChain with stopper 1; oracle-probed:
    // `typeof import("/b").A.B`).
    assert_eq!(
            program_diags(&[
                (
                    "a.ts",
                    "export namespace A { export namespace B { export const x = 1; } }\n"
                ),
                (
                    "b.ts",
                    "export namespace A { export namespace B { export const x = \"s\"; } }\n"
                ),
                (
                    "c.ts",
                    "import { A as XA } from \"./a\";\nimport { A as XB } from \"./b\";\nlet v: typeof XA.B;\nv = XB.B;\n"
                ),
            ]),
            [(
                "c.ts".to_owned(),
                2322,
                82,
                1,
                "Type 'typeof import(\"/b\").A.B' is not assignable to type 'typeof import(\"/a\").A.B'."
                    .to_owned()
            )]
        );
}

#[test]
fn fully_qualified_alias_reexport_names_the_export_entry() {
    // getContainersOfSymbol's candidates leg (49994-50001): a
    // parentless namespace re-exported via `export { N as M }`
    // roots at the module (getAliasForSymbolInContainer admits the
    // container), and createAccessFromSymbolChain names the link
    // from the export-table entry (oracle-probed:
    // `typeof import("/b").M`, not `typeof N`).
    assert_eq!(
            program_diags(&[
                ("a.ts", "namespace N { export const x = 1; }\nexport { N as M };\n"),
                (
                    "b.ts",
                    "namespace N { export const x = \"s\"; }\nexport { N as M };\n"
                ),
                (
                    "c.ts",
                    "import { M as MA } from \"./a\";\nimport { M as MB } from \"./b\";\nlet v: typeof MA;\nv = MB;\n"
                ),
            ]),
            [(
                "c.ts".to_owned(),
                2322,
                80,
                1,
                "Type 'typeof import(\"/b\").M' is not assignable to type 'typeof import(\"/a\").M'."
                    .to_owned()
            )]
        );
}

#[test]
fn export_table_order_names_the_qualifier() {
    // createAccessFromSymbolChain (53210-53217): the FIRST
    // resolved-export entry that same-references the link names it
    // — regardless of the symbol's own name or the import path
    // (oracle-probed both orders).
    let face = |first: &str, second: &str, import_name: &str, expected: &str| {
        let a = format!("namespace N {{ export const x = 1; }}\n{first}\n{second}\n");
        let b = format!("namespace N {{ export const x = \"s\"; }}\n{first}\n{second}\n");
        let c = format!(
                "import {{ {import_name} as NA }} from \"./a\";\nimport {{ {import_name} as NB }} from \"./b\";\nlet v: typeof NA;\nv = NB;\n"
            );
        assert_eq!(
                program_diags(&[("a.ts", &a), ("b.ts", &b), ("c.ts", &c)]),
                [(
                    "c.ts".to_owned(),
                    2322,
                    80,
                    1,
                    format!(
                        "Type 'typeof import(\"/b\").{expected}' is not assignable to type 'typeof import(\"/a\").{expected}'."
                    )
                )]
            );
    };
    face("export { N as M };", "export { N };", "N", "M");
    face("export { N };", "export { N as M };", "M", "N");
}

#[test]
fn fully_qualified_nested_namespace_under_alias_reexport() {
    // The chain recursion applies the export-table naming at every
    // below-root link: the aliased root child renders `M`, the
    // parent-fast-path child renders `P` (oracle-probed:
    // `typeof import("/b").M.P`).
    assert_eq!(
            program_diags(&[
                (
                    "a.ts",
                    "namespace N { export namespace P { export const x = 1; } }\nexport { N as M };\n"
                ),
                (
                    "b.ts",
                    "namespace N { export namespace P { export const x = \"s\"; } }\nexport { N as M };\n"
                ),
                (
                    "c.ts",
                    "import { M as MA } from \"./a\";\nimport { M as MB } from \"./b\";\nlet v: typeof MA.P;\nv = MB.P;\n"
                ),
            ]),
            [(
                "c.ts".to_owned(),
                2322,
                82,
                1,
                "Type 'typeof import(\"/b\").M.P' is not assignable to type 'typeof import(\"/a\").M.P'."
                    .to_owned()
            )]
        );
}

#[test]
fn default_exported_namespace_names_the_default_entry() {
    // The below-root naming scan skips only export= and late-bound
    // keys — `default` is an admissible qualifier name
    // (oracle-probed: `typeof import("/b").default`).
    assert_eq!(
            program_diags(&[
                ("a.ts", "namespace N { export const x = 1; }\nexport default N;\n"),
                (
                    "b.ts",
                    "namespace N { export const x = \"s\"; }\nexport default N;\n"
                ),
                (
                    "c.ts",
                    "import MA from \"./a\";\nimport MB from \"./b\";\nlet v: typeof MA;\nv = MB;\n"
                ),
            ]),
            [(
                "c.ts".to_owned(),
                2322,
                62,
                1,
                "Type 'typeof import(\"/b\").default' is not assignable to type 'typeof import(\"/a\").default'."
                    .to_owned()
            )]
        );
}

#[test]
fn named_default_class_uses_its_written_declaration_name() {
    assert_eq!(
        program_diags(&[
            ("a.ts", "export default class Foo { p = 1 }\n"),
            ("b.ts", "import D from \"./a\"; let s: string = new D();\n"),
        ]),
        [(
            "b.ts".to_owned(),
            2322,
            25,
            1,
            "Type 'Foo' is not assignable to type 'string'.".to_owned()
        )]
    );
}

#[test]
fn anonymous_default_class_keeps_the_default_symbol_name() {
    assert_eq!(
        program_diags(&[
            ("a.ts", "export default class { p = 1 }\n"),
            ("b.ts", "import D from \"./a\"; let s: string = new D();\n"),
        ]),
        [(
            "b.ts".to_owned(),
            2322,
            25,
            1,
            "Type 'default' is not assignable to type 'string'.".to_owned()
        )]
    );
}

#[test]
fn export_equals_namespace_member_renders_import_qualifier() {
    // getWithAlternativeContainers' additionalContainers (50024):
    // the file whose export= IS the member's parent container
    // roots the chain; the export-table naming scan skips the
    // export= entry and falls to the symbol name (oracle-probed
    // under @module: commonjs: `typeof import("/b").Q`).
    let options = CompilerOptions {
        module: Some(1),
        ..CompilerOptions::default()
    };
    assert_eq!(
            program_diags_with(
                &[
                    (
                        "a.ts",
                        "namespace P { export namespace Q { export const x = 1; } }\nexport = P;\n"
                    ),
                    (
                        "b.ts",
                        "namespace P { export namespace Q { export const x = \"s\"; } }\nexport = P;\n"
                    ),
                    (
                        "c.ts",
                        "import PA = require(\"./a\");\nimport PB = require(\"./b\");\nlet v: typeof PA.Q;\nv = PB.Q;\n"
                    ),
                ],
                &options,
                "/"
            ),
            [(
                "c.ts".to_owned(),
                2322,
                76,
                1,
                "Type 'typeof import(\"/b\").Q' is not assignable to type 'typeof import(\"/a\").Q'."
                    .to_owned()
            )]
        );
}

#[test]
fn ambient_export_equals_member_prints_bare_import_face() {
    // getSymbolChain's export= short-circuit (52978-52981): the
    // ambient module (candidates ModuleBlock arm, 49999-50001)
    // whose export= target IS the symbol renders as the bare
    // parent chain — a length-1 import face (oracle-probed under
    // @module: commonjs: `typeof import("amba")`).
    let options = CompilerOptions {
        module: Some(1),
        ..CompilerOptions::default()
    };
    assert_eq!(
            program_diags_with(
                &[
                    (
                        "g.d.ts",
                        "declare module \"amba\" { namespace Q { const x: number; } export = Q; }\ndeclare module \"ambb\" { namespace Q { const x: string; } export = Q; }\n"
                    ),
                    (
                        "a.ts",
                        "import A = require(\"amba\");\nimport B = require(\"ambb\");\nlet v: typeof A;\nv = B;\n"
                    ),
                ],
                &options,
                "/"
            ),
            [(
                "a.ts".to_owned(),
                2322,
                73,
                1,
                "Type 'typeof import(\"ambb\")' is not assignable to type 'typeof import(\"amba\")'."
                    .to_owned()
            )]
        );
}

#[test]
fn script_alias_chain_prints_alias_qualified_face() {
    // getAccessibleSymbolChain's globals alias scan with the
    // candidate-table recursion (50328-50411): a script-file
    // `import M = A` reaches the nested namespace as [M, B], and
    // the alias parent's EMPTY unresolved export table falls the
    // link name back to getNameOfSymbolAsWritten (oracle-probed:
    // `typeof M.B` vs `typeof import("/m").A.B`).
    assert_eq!(
        program_diags(&[
            (
                "s.ts",
                "namespace A { export namespace B { export const x = 1; } }\nimport M = A;\n"
            ),
            (
                "m.ts",
                "namespace A { export namespace B { export const x = \"s\"; } }\nexport { A };\n"
            ),
            (
                "c.ts",
                "import { A as XA } from \"./m\";\nlet v: typeof XA.B;\nv = A.B;\n"
            ),
        ]),
        [(
            "c.ts".to_owned(),
            2322,
            51,
            1,
            "Type 'typeof M.B' is not assignable to type 'typeof import(\"/m\").A.B'.".to_owned()
        )]
    );
}

#[test]
fn script_global_direct_hit_beats_the_alias_scan() {
    // trySymbolTable's direct hit (50321-50327) precedes the alias
    // scan: the global namespace renders its bare name while the
    // module side names the export entry (oracle-probed:
    // `typeof N` vs `typeof import("/m").O`).
    assert_eq!(
        program_diags(&[
            (
                "s.ts",
                "namespace N { export const x = 1; }\nimport M = N;\n"
            ),
            (
                "m.ts",
                "namespace N { export const x = \"s\"; }\nexport { N as O };\n"
            ),
            (
                "c.ts",
                "import { O } from \"./m\";\nlet v: typeof O;\nv = N;\n"
            ),
        ]),
        [(
            "c.ts".to_owned(),
            2322,
            42,
            1,
            "Type 'typeof N' is not assignable to type 'typeof import(\"/m\").O'.".to_owned()
        )]
    );
}

#[test]
fn same_name_unexported_namespaces_take_the_2719_face() {
    // A namespace that is neither exported nor re-exported has no
    // qualifying container (the candidates filter, 50014), so both
    // faces stay `typeof N` after the fully-qualified re-render —
    // reportRelationError swaps the generic head to 2719
    // (65097-65098; oracle-probed).
    assert_eq!(
            program_diags(&[
                (
                    "a.ts",
                    "namespace N { export const x = 1; }\nexport const val = N;\n"
                ),
                (
                    "b.ts",
                    "namespace N { export const x = \"s\"; }\nexport const val = N;\n"
                ),
                (
                    "c.ts",
                    "import { val as va } from \"./a\";\nimport { val as vb } from \"./b\";\nlet v: typeof va;\nv = vb;\n"
                ),
            ]),
            [(
                "c.ts".to_owned(),
                2719,
                84,
                1,
                "Type 'typeof N' is not assignable to type 'typeof N'. Two different types with this name exist, but they are unrelated."
                    .to_owned()
            )]
        );
}

#[test]
fn type_parameter_name_collision_takes_the_2719_face() {
    // Type parameters never chain (lookupSymbolChainWorker 52946:
    // isTypeParameter forces [symbol]), so shadowed same-name
    // parameters stay `T` under the re-render and the head swaps
    // to 2719 (oracle-probed).
    assert_eq!(
            checked_diags(
                "function f<T>(a: T) {\n    return function g<T>(b: T): T {\n        return a;\n    };\n}\n"
            ),
            [(
                2719,
                66,
                6,
                "Type 'T' is not assignable to type 'T'. Two different types with this name exist, but they are unrelated."
                    .to_owned()
            )]
        );
}

#[test]
fn inherited_signature_type_parameter_collision_keeps_the_2719_chain() {
    fn flatten(
        chain: &tsc_diagnostics::MessageChain,
        codes: &mut Vec<u32>,
        texts: &mut Vec<String>,
    ) {
        codes.push(chain.code);
        texts.push(chain.text.clone());
        for child in &chain.next {
            flatten(child, codes, texts);
        }
    }

    // The source `T` belongs to I while the target `T` belongs to
    // A's generic call signature. UseFullyQualifiedType must not
    // turn the former into `I.T`: tsc's type-parameter short
    // circuit keeps both names bare, selects 2719, then appends
    // the target-parameter constraint reason (5082).
    let (codes, texts) = with_program_state(
        &[(
            "a.ts",
            "interface A { a: <T>(x: T) => void; }\n\
                 interface I<T> extends A { a: (x: T) => void; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            let diagnostic = state
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code() == 2430)
                .expect("inheritance relation reports 2430");
            let mut codes = Vec::new();
            let mut texts = Vec::new();
            flatten(&diagnostic.message, &mut codes, &mut texts);
            (codes, texts)
        },
    );
    assert_eq!(codes, [2430, 2326, 2322, 2328, 2719, 5082]);
    assert_eq!(
            texts,
            [
                "Interface 'I<T>' incorrectly extends interface 'A'.",
                "Types of property 'a' are incompatible.",
                "Type '(x: T) => void' is not assignable to type '<T>(x: T) => void'.",
                "Types of parameters 'x' and 'x' are incompatible.",
                "Type 'T' is not assignable to type 'T'. Two different types with this name exist, but they are unrelated.",
                "'T' could be instantiated with an arbitrary type which could be unrelated to 'T'.",
            ]
        );
}

#[test]
fn recursive_generic_interface_error_reaches_the_first_non_recursive_property() {
    fn flatten(chain: &tsc_diagnostics::MessageChain, texts: &mut Vec<String>) {
        texts.push(chain.text.clone());
        for child in &chain.next {
            flatten(child, texts);
        }
    }

    let options = CompilerOptions {
        strict_null_checks: Some(true),
        ..CompilerOptions::default()
    };
    let texts = with_program_state(
        &[(
            "a.ts",
            "interface Collection<K, V> {\n\
                 map<M>(mapper: (value: V, key: K, iter: this) => M): Collection<K, M>;\n\
                 filter<F extends V>(predicate: (value: V, key: K, iter: this) => value is F): Collection<K, F>;\n\
                 filter(predicate: (value: V, key: K, iter: this) => any): this;\n\
                 readonly size: number;\n\
             }\n\
             interface Seq<K, V> extends Collection<K, V> {\n\
                 readonly size: number | undefined;\n\
                 map<M>(mapper: (value: V, key: K, iter: this) => M): Seq<K, M>;\n\
                 filter<F extends V>(predicate: (value: V, key: K, iter: this) => value is F): Seq<K, F>;\n\
                 filter(predicate: (value: V, key: K, iter: this) => any): this;\n\
             }\n",
        )],
        &options,
        |state| {
            state.check_source_file(0);
            let diagnostic = state
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code() == 2430)
                .expect("recursive interface inheritance reports 2430");
            let mut texts = Vec::new();
            flatten(&diagnostic.message, &mut texts);
            texts
        },
    );

    assert_eq!(
        texts,
        [
            "Interface 'Seq<K, V>' incorrectly extends interface 'Collection<K, V>'.",
            "The types of 'map(...).size' are incompatible between these types.",
            "Type 'number | undefined' is not assignable to type 'number'.",
            "Type 'undefined' is not assignable to type 'number'.",
        ]
    );
}

#[test]
fn incompatible_constructor_return_path_parenthesizes_before_property_access() {
    fn flatten(chain: &tsc_diagnostics::MessageChain, texts: &mut Vec<String>) {
        texts.push(chain.text.clone());
        for child in &chain.next {
            flatten(child, texts);
        }
    }

    let texts = with_program_state(
        &[(
            "a.ts",
            concat!(
                "class A { g!: string; }\n",
                "class B { g!: number; }\n",
                "declare let x: { f: typeof A };\n",
                "declare let y: { f: typeof B };\n",
                "x = y;\n",
            ),
        )],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            let diagnostic = state
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code() == 2322)
                .expect("constructor-return incompatibility");
            let mut texts = Vec::new();
            flatten(&diagnostic.message, &mut texts);
            texts
        },
    );

    assert!(
        texts
            .iter()
            .any(|text| text == "The types of '(new f()).g' are incompatible between these types."),
        "{texts:?}",
    );
}

#[test]
fn function_type_initializer_renders_parameter_as_optional() {
    let diagnostics = checked_diags("var y = <(a: string = \"\") => any>(undefined);\n");
    let message = diagnostics
        .iter()
        .find_map(|(code, _, _, message)| (*code == 2352).then_some(message))
        .expect("invalid conversion diagnostic");

    assert!(message.contains("type '(a?: string) => any'"), "{message}");
    assert!(!message.contains("(a: string | undefined)"), "{message}");
}

#[test]
fn source_file_specifier_roots_at_the_program_cwd() {
    // The oracle host absolutizes every fileName against the
    // ProgramJson cwd (program-host.mjs absoluteProgramFileName),
    // so the extension-free source-file specifier renders
    // cwd-rooted (oracle-probed under @currentDirectory: /src:
    // `typeof import("/src/b")`).
    assert_eq!(
        program_diags_with(
            &[
                ("b.ts", "export const bee = 1;\n"),
                ("a.ts", "import * as b from \"./b\";\nb.nope;\n"),
            ],
            &CompilerOptions::default(),
            "/src"
        ),
        [(
            "a.ts".to_owned(),
            2339,
            28,
            4,
            "Property 'nope' does not exist on type 'typeof import(\"/src/b\")'.".to_owned()
        )]
    );
}

#[test]
fn fully_qualified_specifier_roots_at_the_program_cwd() {
    // The cwd rooting rides the chain faces too (oracle-probed:
    // `typeof import("/src/b").N` vs `typeof import("/src/a").N`).
    assert_eq!(
            program_diags_with(
                &[
                    ("a.ts", "export namespace N { export const x = 1; }\n"),
                    ("b.ts", "export namespace N { export const x = \"s\"; }\n"),
                    (
                        "c.ts",
                        "import { N as NA } from \"./a\";\nimport { N as NB } from \"./b\";\nlet v: typeof NA;\nv = NB;\n"
                    ),
                ],
                &CompilerOptions::default(),
                "/src"
            ),
            [(
                "c.ts".to_owned(),
                2322,
                80,
                1,
                "Type 'typeof import(\"/src/b\").N' is not assignable to type 'typeof import(\"/src/a\").N'."
                    .to_owned()
            )]
        );
}

#[test]
fn module_specifier_needing_escapes_prints_the_synthesized_literal() {
    // The nodeBuilder-created module specifier has no
    // NoAsciiEscaping/source-text exemption: the printer applies
    // escapeString and emits the exact `typeof import("a\"b")`
    // face in the 2339.
    assert_eq!(
        program_diags(&[
            (
                "d.d.ts",
                "declare module \"a\\\"b\" { export const x: number; }\n"
            ),
            ("main.ts", "import * as m from \"a\\\"b\";\nm.y;\n"),
        ]),
        [(
            "main.ts".to_owned(),
            2339,
            29,
            1,
            "Property 'y' does not exist on type 'typeof import(\"a\\\"b\")'.".to_owned()
        )]
    );
}

#[test]
fn module_export_alias_over_merged_local_is_a_known_value_property() {
    // The NEW_FP family this slice fixed at source: `export { A }`
    // over a local that merges a type-only import alias with a
    // const is a VALUE property of the module face — both
    // isKnownProperty (via getPropertyOfObjectType) and
    // getNamedMembers gate through the alias-FOLLOWING
    // symbolIsValue (50092-50094), so the object literal below
    // reports NO 2353 (tsc emits only a 6133 unused-suggestion
    // here; that band's absence is a pre-existing suggestion-side
    // FN, not part of this pin).
    assert_eq!(
            program_diags(&[
                ("z.ts", "interface A {}\nexport type { A };\n"),
                (
                    "a.ts",
                    "import { A } from './z';\nconst A = 0;\nexport { A };\nexport class B {};\n"
                ),
                (
                    "b.ts",
                    "import * as types from './a';\nlet t: typeof types = {\n  A: undefined as any,\n  B: undefined as any,\n};\n"
                ),
            ]),
            []
        );
    // The properties view itself carries the alias export.
    with_program_state(
        &[
            ("z.ts", "interface A {}\nexport type { A };\n"),
            (
                "a.ts",
                "import { A } from './z';\nconst A = 0;\nexport { A };\nexport class B {};\n",
            ),
        ],
        &CompilerOptions::default(),
        |state| {
            let root = state.binder.source(1).root;
            let module_symbol = state.binder.node_symbol(root).expect("module symbol");
            let module_type = state
                .get_type_of_symbol(module_symbol)
                .expect("module type");
            let names: Vec<String> = state
                .get_properties_of_object_type_owned(module_type)
                .expect("properties")
                .into_iter()
                .map(|p| state.symbol_display_name(p))
                .collect();
            assert_eq!(names, ["A", "B"]);
        },
    );
}

#[test]
fn expando_namespace_cross_file_merge_keeps_name_precision() {
    // The amalgamated-duplicates merge clones per-file symbols
    // into fresh program symbols; the stage-3.4c expando-record
    // consults follow the merge sources, so assigned members
    // (p1) suppress, namespace exports (p2) resolve, and an
    // unassigned name still reports with the merged `typeof EM`
    // face. The cross-file fn+ns merge itself is tsc error 2433.
    assert_eq!(
            program_diags(&[
                (
                    "expando.ts",
                    "function EM(n: number) { return n }\nEM.p1 = 111;\nvar r1 = EM.p1;\nvar r2 = EM.p2;\nEM.zzz;\n"
                ),
                ("ns.ts", "namespace EM { export var p2 = 222 }\n"),
            ]),
            [
                (
                    "expando.ts".to_owned(),
                    2339,
                    84,
                    3,
                    "Property 'zzz' does not exist on type 'typeof EM'.".to_owned()
                ),
                (
                    "ns.ts".to_owned(),
                    2433,
                    10,
                    2,
                    "A namespace declaration cannot be in a different file from a class or function with which it is merged.".to_owned()
                )
            ]
        );
}

// ---- 9.3b4 type-operator display pins (all rows oracle-probed
// byte-exact against vendored 6.0.3, noLib; strict unless noted;
// target-position annotations because source-position operator
// types generalize to their constraints in reportRelationError) ----

#[test]
fn keyof_faces_render_the_type_operator_arm() {
    // f2: keyof (keyof T) resolves through the apparent type
    // (never under noLib) — nesting is display-covered via the
    // g4 indexed-access object below. f3: keyof (T & U)
    // distributes into a union whose TypeOperator members join
    // bare. f4: the nullable-candidate substitution (65185)
    // reports against the stripped `keyof T`. f5: TypeOperator
    // joins an intersection bare.
    assert_eq!(
            checked_diags(
                "\nfunction f1<T>(x: number) { const y: keyof T = x; }\nfunction f2<T>(x: number) { const y: keyof keyof T = x; }\nfunction f3<T, U>(x: number) { const y: keyof (T & U) = x; }\nfunction f4<T>(x: number) { const y: keyof T | null = x; }\nfunction f5<T, U>(x: number) { const y: keyof T & U = x; }\n"
            ),
            [
                (
                    2322,
                    35,
                    1,
                    "Type 'number' is not assignable to type 'keyof T'.".to_owned()
                ),
                (
                    2322,
                    87,
                    1,
                    "Type 'number' is not assignable to type 'never'.".to_owned()
                ),
                (
                    2322,
                    148,
                    1,
                    "Type 'number' is not assignable to type 'keyof T | keyof U'.".to_owned()
                ),
                (
                    2322,
                    206,
                    1,
                    "Type 'number' is not assignable to type 'keyof T'.".to_owned()
                ),
                (
                    2322,
                    268,
                    1,
                    "Type 'number' is not assignable to type 'keyof T & U'.".to_owned()
                ),
            ]
        );
}

// ---- 9.3b5 display special tail (all oracle-probed byte-exact;
// probe-f/probe-b batches in the session scratchpad) ----

#[test]
fn operator_error_retries_identical_names_fully_qualified_and_keeps_them() {
    // getTypeNamesForErrorDisplay 50751-50754: equal renders retry
    // through getTypeNameForErrorDisplay and the retried texts are
    // used EVEN IF STILL EQUAL — same-type operands print
    // `'symbol' and 'symbol'`; tsc has no third fallback.
    assert_eq!(
        checked_diags("declare const s: symbol;\nvar r = s + s;\n"),
        [(
            2365,
            33,
            5,
            "Operator '+' cannot be applied to types 'symbol' and 'symbol'.".to_owned()
        )]
    );
}

#[test]
fn class_extends_heritage_flows_2454_and_reports_2507_empty_face() {
    // The extends expression of a CLASS is expression context
    // (isExpressionWithTypeArgumentsInClassExtendsClause) — its
    // identifier flow-stamps, so the unassigned `x` reports 2454;
    // the 2507 face renders the canonical emptyTypeLiteralType as
    // `{}` and the errorType continuation replaces the old
    // curtain unwind.
    assert_eq!(
        checked_diags("var x: {};\nclass C6 extends x { }\n"),
        [
            (
                2454,
                28,
                1,
                "Variable 'x' is used before being assigned.".to_owned()
            ),
            (
                2507,
                28,
                1,
                "Type '{}' is not a constructor function type.".to_owned()
            ),
        ]
    );
}

#[test]
fn extends_interface_reports_2689_before_the_reprobe_gate() {
    // checkAndReportErrorForExtendingInterface is SECOND in the
    // 48114 resolveName failure chain — ahead of the port's
    // all-meanings re-probe gate, which used to swallow the report
    // because I resolves under the Interface meaning.
    assert_eq!(
        checked_diags("interface I {\n    foo: string;\n}\nclass C extends I { }\n"),
        [(
            2689,
            49,
            1,
            "Cannot extend an interface 'I'. Did you mean 'implements'?".to_owned()
        )]
    );
}

#[test]
fn type_parameter_base_reports_2507_with_did_you_mean_related() {
    // 57172-57183: a TypeParameter base constructor adds the 2735
    // related info anchored at declarations[0], with the
    // constraint's construct return (unknownType fallback).
    with_program_state(
        &[(
            "a.ts",
            "function f<T>(ctor: T) { class C extends ctor { } return C; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            let row = state
                .diagnostics
                .iter()
                .find(|diag| diag.code() == 2507)
                .expect("2507 emitted");
            assert_eq!(
                row.message_text(),
                "Type 'T' is not a constructor function type."
            );
            assert_eq!(row.start, Some(41));
            assert_eq!(row.related.len(), 1);
            assert_eq!(
                row.related[0].message.text,
                "Did you mean for 'T' to be constrained to type 'new (...args: any[]) => unknown'?"
            );
            assert_eq!(row.related[0].start, Some(11));
        },
    );
}

#[test]
fn invalid_base_constructor_return_reports_2509_and_continues() {
    // 57277-57286: the 2509 head renders through the display slice
    // and resolution continues with the emptyArray sentinel.
    assert_eq!(
            checked_diags("declare const x: new () => number;\nclass C extends x { }\n"),
            [(
                2509,
                51,
                1,
                "Base constructor return type 'number' is not an object type or intersection of object types with statically known members."
                    .to_owned()
            )]
        );
}

#[test]
fn origin_intersection_of_unions_renders_the_syntactic_face() {
    // 51542-51544: the denormalized union substitutes its ORIGIN
    // wholesale — `(A | B) & (C | D)` prints the syntactic shape
    // with union members parenthesized by the intersection rule.
    // (2454 lands first in sink order: checkIdentifier runs before
    // the assignment relation.)
    assert_eq!(
            checked_diags(
                "interface A { a: string }\ninterface B { b: string }\ninterface C { c: string }\ninterface D { d: string }\nvar y: (A | B) & (C | D);\nvar x: A & B;\ny = x;\n"
            ),
            [
                (
                    2454,
                    148,
                    1,
                    "Variable 'x' is used before being assigned.".to_owned()
                ),
                (
                    2322,
                    144,
                    1,
                    "Type 'A & B' is not assignable to type '(A | B) & (C | D)'.".to_owned()
                ),
            ]
        );
}

#[test]
fn origin_with_instantiable_members_uses_the_relation_verdict() {
    // `T & U ⊆ (A | B) & T & U` holds in tsc through the
    // normalized-intersection constraint path. This canary must
    // stay clean without using typeToString as a verdict shield.
    assert_eq!(
            checked_diags(
                "type A = 1 | 2;\ntype B = 2 | 3;\nfunction f2<T extends A, U extends B>(ab: T & U): (A | B) & T & U { return ab; }\n"
            ),
            []
        );
}

#[test]
fn all_consumed_object_rest_renders_the_empty_face() {
    // getRestType results are BORN resolved
    // (make_resolved_anonymous_type) — an all-consumed rest is a
    // REAL `{}` and the 2741 single-missing face renders it.
    assert_eq!(
            checked_diags(
                "declare const s: { a: number };\nconst { a, ...r } = s;\nconst q: { b: string } = r;\n"
            ),
            [(
                2741,
                61,
                1,
                "Property 'b' is missing in type '{}' but required in type '{ b: string; }'."
                    .to_owned()
            )]
        );
}

#[test]
fn unique_symbol_relation_faces_take_the_fq_typeof_chain() {
    // reportRelationError's GENERALIZED render is
    // getTypeNameForErrorDisplay (UseFullyQualifiedType) and
    // getBaseTypeOfLiteralType passes unique symbols through
    // unchanged — the namespace chain qualifies.
    assert_eq!(
        checked_diags(
            "declare namespace NS { const tp: unique symbol; }\nvar z: object = NS.tp;\n"
        ),
        [(
            2322,
            54,
            1,
            "Type 'typeof NS.tp' is not assignable to type 'object'.".to_owned()
        )]
    );
}

#[test]
fn unique_symbol_plain_face_is_the_operator_keyword() {
    // typeToString's DEFAULT flags include AllowUniqueESSymbolType
    // (50717) — with generalization skipped (singleton-capable
    // target) the plain render is the `unique symbol` operator.
    assert_eq!(
        checked_diags("declare const local: unique symbol;\nvar z: \"a\" | \"b\" = local;\n"),
        [(
            2322,
            40,
            1,
            "Type 'unique symbol' is not assignable to type '\"a\" | \"b\"'.".to_owned()
        )]
    );
}

#[test]
fn string_literal_faces_spell_escapes_but_not_non_ascii() {
    // 51401-51403: NoAsciiEscaping — escapeString('"') only.
    assert_eq!(
        checked_diags("var x: \"AB\\r\\nC\" = \"AB\\nC\";\n"),
        [(
            2322,
            4,
            1,
            "Type '\"AB\\nC\"' is not assignable to type '\"AB\\r\\nC\"'.".to_owned()
        )]
    );
}

#[test]
fn unique_symbol_member_name_renders_the_computed_face() {
    // 53427-53429: nameType UniqueESSymbol →
    // createComputedPropertyName(symbolToExpression(symbol, Value))
    // — the [symbol]-chain face `[sym]`.
    assert_eq!(
            checked_diags(
                "declare const sym: unique symbol;\nconst o = { [sym]: 0 };\nconst t: { [key: symbol]: string } = o;\n"
            ),
            [(
                2322,
                64,
                1,
                "Type '{ [sym]: number; }' is not assignable to type '{ [key: symbol]: string; }'."
                    .to_owned()
            )]
        );
}

#[test]
fn instantiation_expression_type_renders_structurally() {
    // 51755-51770: 2635 renders the original expression type, not
    // the filtered InstantiationExpressionType result. It therefore
    // keeps the ordinary structural face.
    assert_eq!(
            checked_diags(
                "declare const f: { (): number; g<U>(): U; };\nconst h = f<number>;\n"
            ),
            [(
                2635,
                57,
                6,
                "Type '{ (): number; g<U>(): U; }' has no signatures for which the type argument list is applicable."
                    .to_owned()
            )]
        );
}

#[test]
fn failed_type_query_instantiation_reuses_its_exact_syntax_in_constraint_errors() {
    let text = "type Return<T extends (...args: any) => any> = T;\n\
                declare function f<A, B>(value: A): B;\n\
                type Result<Q> = Return<typeof f<Q>>;\n";
    let diagnostics = checked_diags(text);
    let constraint = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.0 == 2344)
        .expect("constraint diagnostic");
    assert!(
        constraint.3.starts_with(
            "Type 'typeof f<Q>' does not satisfy the constraint '(...args: any) => any'."
        ),
        "{}",
        constraint.3,
    );
}

#[test]
fn json_declaration_twin_precedes_the_json_resolution() {
    // A present <stem>.d.json.ts twin wins the TYPES probe. The
    // false option reports getResolutionDiagnostic's 6263 without
    // loading the declaration; true loads its string default.
    // Without the twin the JSON literal shape resolves and relates.
    let base_options = CompilerOptions {
        resolve_json_module: Some(true),
        // ModuleKind.CommonJS
        module: Some(1),
        ..CompilerOptions::default()
    };
    let run = |files: &[(&str, &str)], options: &CompilerOptions| -> Vec<(u32, u32, u32, String)> {
        let names: Vec<String> = files.iter().map(|(name, _)| (*name).to_owned()).collect();
        with_program_state(files, options, |state| {
            // The unit harness has no ProgramJson host.
            state.host_file_paths = names.iter().cloned().collect();
            state.check_source_file(0);
            diag_rows(state)
        })
    };
    let with_twin = [
        (
            "/main.ts",
            "import data from \"./data.json\";\nlet x: string = data;\n",
        ),
        ("/data.json", "{}"),
        (
            "/data.d.json.ts",
            "declare var val: string;\nexport default val;\n",
        ),
    ];
    assert_eq!(
            run(
                &with_twin,
                &CompilerOptions {
                    allow_arbitrary_extensions: Some(false),
                    ..base_options.clone()
                },
            ),
            [(
                6263,
                17,
                13,
                "Module './data.json' was resolved to '/data.d.json.ts', but '--allowArbitraryExtensions' is not set.".to_owned(),
            )]
        );
    assert_eq!(
        run(
            &with_twin,
            &CompilerOptions {
                allow_arbitrary_extensions: Some(true),
                ..base_options.clone()
            },
        ),
        []
    );
    let without_twin = run(
        &[
            (
                "/main.ts",
                "import data from \"./data.json\";\nlet x: string = data;\n",
            ),
            ("/data.json", "{}"),
        ],
        &base_options,
    );
    assert_eq!(
        without_twin,
        [(
            2322,
            36,
            1,
            "Type '{}' is not assignable to type 'string'.".to_owned()
        )]
    );
}

#[test]
fn indexed_access_faces_parenthesize_the_object_side_only() {
    // g2: chained accesses join bare (the kind is listed in no
    // parenthesizer rule); g3/g4: union and TypeOperator OBJECT
    // sides wrap (parenthesizeNonArrayTypeOfPostfixType); g5: a
    // literal index over a template resolves through the apparent
    // type (2339 on `{}` under noLib); g7: the INDEX side joins
    // bare.
    assert_eq!(
            checked_diags(
                "\nfunction g1<T, K extends keyof T>(x: number) { const y: T[K] = x; }\nfunction g2<T, K extends keyof T, K2 extends keyof T[K]>(x: number) { const y: T[K][K2] = x; }\nfunction g3<T, U, K extends keyof (T | U)>(x: number) { const y: (T | U)[K] = x; }\nfunction g4<T, K extends keyof keyof T>(x: number) { const y: (keyof T)[K] = x; }\nfunction g5<T extends string>(x: number) { const y: `a${T}`[\"x\"] = x; }\nfunction g6<T, K extends keyof T>(x: number) { const y: T[K] | null = x; }\nfunction g7<T, K extends keyof T>(x: number) { const y: T[keyof T] = x; }\n"
            ),
            [
                (
                    2322,
                    54,
                    1,
                    "Type 'number' is not assignable to type 'T[K]'.".to_owned()
                ),
                (
                    2322,
                    145,
                    1,
                    "Type 'number' is not assignable to type 'T[K][K2]'.".to_owned()
                ),
                (
                    2322,
                    226,
                    1,
                    "Type 'number' is not assignable to type '(T | U)[K]'.".to_owned()
                ),
                (
                    2322,
                    306,
                    1,
                    "Type 'number' is not assignable to type '(keyof T)[K]'.".to_owned()
                ),
                (
                    2339,
                    389,
                    3,
                    "Property 'x' does not exist on type '{}'.".to_owned()
                ),
                (
                    2322,
                    454,
                    1,
                    "Type 'number' is not assignable to type 'T[K]'.".to_owned()
                ),
                (
                    2322,
                    529,
                    1,
                    "Type 'number' is not assignable to type 'T[keyof T]'.".to_owned()
                ),
            ]
        );
}

#[test]
fn template_literal_faces_render_head_spans_and_tail() {
    // h3: a union span distributes at construction — the display
    // renders the resulting union of templates, members bare;
    // h4: nullable-candidate substitution strips to the bare
    // template; h5: adjacent spans share an empty middle text.
    assert_eq!(
            checked_diags(
                "\nfunction h1<T extends string>(x: number) { const y: `a${T}b` = x; }\nfunction h2<T extends string>(x: number) { const y: `${T}` = x; }\nfunction h3<T extends string, U extends string>(x: number) { const y: `a${T | U}b` = x; }\nfunction h4<T extends string>(x: number) { const y: `a${T}` | null = x; }\nfunction h5<T extends string, U extends string>(x: number) { const y: `${T}${U}` = x; }\n"
            ),
            [
                (
                    2322,
                    50,
                    1,
                    "Type 'number' is not assignable to type '`a${T}b`'.".to_owned()
                ),
                (
                    2322,
                    118,
                    1,
                    "Type 'number' is not assignable to type '`${T}`'.".to_owned()
                ),
                (
                    2322,
                    202,
                    1,
                    "Type 'number' is not assignable to type '`a${T}b` | `a${U}b`'.".to_owned()
                ),
                (
                    2322,
                    274,
                    1,
                    "Type 'number' is not assignable to type '`a${T}`'.".to_owned()
                ),
                (
                    2322,
                    366,
                    1,
                    "Type 'number' is not assignable to type '`${T}${U}`'.".to_owned()
                ),
            ]
        );
}

#[test]
fn template_literal_texts_reescape_like_the_printer() {
    // Cooked texts re-escape through template_text_raw: CRLF is
    // the map's pair entry, a null before a digit prints `\x00`
    // (getReplacement's lookahead), unmapped controls and
    // non-ASCII take `\uXXXX` (astral = two surrogate escapes),
    // and `$`/`{` are identity when not forming `${`.
    assert_eq!(
            checked_diags(
                "\nfunction e1<T extends string>(x: number) { const y: `a\\r\\nb${T}` = x; }\nfunction e2<T extends string>(x: number) { const y: `a\\u0000b${T}` = x; }\nfunction e3<T extends string>(x: number) { const y: `a\\u00001${T}` = x; }\nfunction e4<T extends string>(x: number) { const y: `a\\u0001b${T}` = x; }\nfunction e5<T extends string>(x: number) { const y: `あ${T}` = x; }\nfunction e6<T extends string>(x: number) { const y: `😀${T}` = x; }\nfunction e7<T extends string>(x: number) { const y: `a\\rb${T}` = x; }\nfunction e8<T extends string>(x: number) { const y: `a$b{c${T}` = x; }\n"
            ),
            [
                (
                    2322,
                    50,
                    1,
                    "Type 'number' is not assignable to type '`a\\r\\nb${T}`'.".to_owned()
                ),
                (
                    2322,
                    122,
                    1,
                    "Type 'number' is not assignable to type '`a\\0b${T}`'.".to_owned()
                ),
                (
                    2322,
                    196,
                    1,
                    "Type 'number' is not assignable to type '`a\\x001${T}`'.".to_owned()
                ),
                (
                    2322,
                    270,
                    1,
                    "Type 'number' is not assignable to type '`a\\u0001b${T}`'.".to_owned()
                ),
                (
                    2322,
                    344,
                    1,
                    "Type 'number' is not assignable to type '`\\u3042${T}`'.".to_owned()
                ),
                (
                    2322,
                    411,
                    1,
                    "Type 'number' is not assignable to type '`\\uD83D\\uDE00${T}`'.".to_owned()
                ),
                (
                    2322,
                    479,
                    1,
                    "Type 'number' is not assignable to type '`a\\rb${T}`'.".to_owned()
                ),
                (
                    2322,
                    549,
                    1,
                    "Type 'number' is not assignable to type '`a$b{c${T}`'.".to_owned()
                ),
            ]
        );
    assert_eq!(
        checked_diags("function s<T extends string>(x: number) { const y: `\\uD800${T}` = x; }"),
        [(
            2322,
            48,
            1,
            "Type 'number' is not assignable to type '`\\uD800${T}`'.".to_owned()
        )]
    );
}

#[test]
fn string_mapping_faces_render_the_intrinsic_reference() {
    // Local intrinsic aliases stand in for the lib set (same
    // symbol-name route). m4: keyof over a string mapping
    // resolves through the apparent type (never under noLib);
    // m5: a mapping nests bare inside a template span.
    assert_eq!(
            checked_diags(
                "\ntype Uppercase<S extends string> = intrinsic;\ntype Lowercase<S extends string> = intrinsic;\ntype Capitalize<S extends string> = intrinsic;\n\nfunction m1<T extends string>(x: number) { const y: Uppercase<T> = x; }\nfunction m2<T extends string>(x: number) { const y: Lowercase<Uppercase<T>> = x; }\nfunction m3<T extends string>(x: number) { const y: Uppercase<T> | null = x; }\nfunction m4<T extends string>(x: number) { const y: keyof Uppercase<T> = x; }\nfunction m5<T extends string>(x: number) { const y: `a${Uppercase<T>}b` = x; }\n"
            ),
            [
                (
                    2322,
                    190,
                    1,
                    "Type 'number' is not assignable to type 'Uppercase<T>'.".to_owned()
                ),
                (
                    2322,
                    262,
                    1,
                    "Type 'number' is not assignable to type 'Lowercase<Uppercase<T>>'.".to_owned()
                ),
                (
                    2322,
                    345,
                    1,
                    "Type 'number' is not assignable to type 'Uppercase<T>'.".to_owned()
                ),
                (
                    2322,
                    424,
                    1,
                    "Type 'number' is not assignable to type 'never'.".to_owned()
                ),
                (
                    2322,
                    502,
                    1,
                    "Type 'number' is not assignable to type '`a${Uppercase<T>}b`'.".to_owned()
                ),
            ]
        );
    assert_eq!(
            checked_diags(
                "type Uppercase<S extends string> = intrinsic;\nfunction s<T extends string>(x: number) { const y: Uppercase<`\\uD800a${T}`> = x; }"
            ),
            [(
                2322,
                94,
                1,
                "Type 'number' is not assignable to type '`\\uD800A${Uppercase<T>}`'.".to_owned()
            )]
        );
}

#[test]
fn any_intrinsics_hide_internal_names_in_type_display() {
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let error = state.tables.intrinsics.error;
        let unresolved = state.tables.intrinsics.unresolved;
        let any = state.tables.intrinsics.any;
        let intrinsic_marker = state.tables.intrinsics.intrinsic_marker;
        let unknown = state.tables.intrinsics.unknown;

        assert_eq!(state.type_to_string_slice(error).unwrap(), "any");
        assert_eq!(state.type_to_string_slice(unresolved).unwrap(), "any");
        assert_eq!(state.type_to_string_slice(any).unwrap(), "any");
        assert_eq!(
            state.type_to_string_slice(intrinsic_marker).unwrap(),
            "intrinsic"
        );
        assert_eq!(state.type_to_string_slice(unknown).unwrap(), "unknown");
    });
}

#[test]
fn operator_faces_in_array_positions_follow_the_postfix_rule() {
    // Local Array/ReadonlyArray interfaces supply the display
    // sugar targets. TypeOperator elements wrap ((keyof T)[],
    // and again under the readonly face); indexed-access,
    // template, and reference elements join bare.
    assert_eq!(
            checked_diags(
                "\ninterface Array<T> { length: number; }\ninterface ReadonlyArray<T> { length: number; }\n\ntype Uppercase<S extends string> = intrinsic;\ntype Lowercase<S extends string> = intrinsic;\ntype Capitalize<S extends string> = intrinsic;\n\nfunction a1<T>(x: number) { const y: (keyof T)[] = x; }\nfunction a2<T, K extends keyof T>(x: number) { const y: T[K][] = x; }\nfunction a3<T extends string>(x: number) { const y: `a${T}`[] = x; }\nfunction a4<T extends string>(x: number) { const y: Uppercase<T>[] = x; }\nfunction a5<T>(x: number) { const y: readonly (keyof T)[] = x; }\n"
            ),
            [
                (
                    2322,
                    262,
                    1,
                    "Type 'number' is not assignable to type '(keyof T)[]'.".to_owned()
                ),
                (
                    2322,
                    337,
                    1,
                    "Type 'number' is not assignable to type 'T[K][]'.".to_owned()
                ),
                (
                    2322,
                    403,
                    1,
                    "Type 'number' is not assignable to type '`a${T}`[]'.".to_owned()
                ),
                (
                    2322,
                    472,
                    1,
                    "Type 'number' is not assignable to type 'Uppercase<T>[]'.".to_owned()
                ),
                (
                    2322,
                    531,
                    1,
                    "Type 'number' is not assignable to type 'readonly (keyof T)[]'.".to_owned()
                ),
            ]
        );
}

#[test]
fn signature_display_iterable_protocol_elides_only_trailing_default_arguments() {
    assert_eq!(
        checked_diags(
            "\
interface Iterable<T, TReturn = any, TNext = any> {}
interface IterableIterator<T, TReturn = any, TNext = any> {}
interface AsyncIterable<T, TReturn = any, TNext = any> {}
interface AsyncIterableIterator<T, TReturn = any, TNext = any> {}
interface Generator<T, TReturn = any, TNext = any> {}
interface Other<T, U = any> {}
declare let a: Iterable<string, any, any>;
declare let b: IterableIterator<string, void, any>;
declare let c: AsyncIterable<string, any, any>;
declare let d: AsyncIterableIterator<string, void, any>;
declare let e: Generator<string, any, any>;
declare let f: Other<string, any>;
const aa: number = a;
const bb: number = b;
const cc: number = c;
const dd: number = d;
const ee: number = e;
const ff: number = f;
"
        ),
        [
            (
                2322,
                608,
                2,
                "Type 'Iterable<string>' is not assignable to type 'number'.".to_owned()
            ),
            (
                2322,
                630,
                2,
                "Type 'IterableIterator<string, void>' is not assignable to type 'number'."
                    .to_owned()
            ),
            (
                2322,
                652,
                2,
                "Type 'AsyncIterable<string>' is not assignable to type 'number'.".to_owned()
            ),
            (
                2322,
                674,
                2,
                "Type 'AsyncIterableIterator<string, void>' is not assignable to type 'number'."
                    .to_owned()
            ),
            (
                2322,
                696,
                2,
                "Type 'Generator<string, any, any>' is not assignable to type 'number'.".to_owned()
            ),
            (
                2322,
                718,
                2,
                "Type 'Other<string, any>' is not assignable to type 'number'.".to_owned()
            ),
        ]
    );
}

#[test]
fn jsdoc_signature_display_instantiates_parameter_annotations() {
    fn flatten(chain: &tsc_diagnostics::MessageChain, out: &mut Vec<String>) {
        out.push(chain.text.clone());
        for child in &chain.next {
            flatten(child, out);
        }
    }

    fn relation_texts(text: &str) -> Vec<String> {
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(true),
            target: Some(ScriptTarget::ES2015.bits()),
            ..CompilerOptions::default()
        };
        with_program_state(&[("a.js", text)], &options, |state| {
            state.check_source_file(0);
            let mut texts = Vec::new();
            for diagnostic in state
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 2322)
            {
                flatten(&diagnostic.message, &mut texts);
            }
            texts
        })
    }

    let instantiated = relation_texts(
        "/**\n\
             * @template in out T\n\
             * @typedef {Object} Invariant\n\
             * @property {(x: T) => T} f\n\
             */\n\
             /** @type {Invariant<unknown>} */\n\
             let target = { f: (x) => x };\n\
             /** @type {Invariant<string>} */\n\
             let source = { f: (x) => x };\n\
             target = source;\n",
    );
    assert!(
            instantiated.iter().any(|text| {
                text == "Type '(x: string) => string' is not assignable to type '(x: unknown) => unknown'."
            }),
            "instantiated JSDoc signature must render mapped parameter and return types: {instantiated:?}"
        );
    assert!(
        instantiated.iter().all(|text| !text.contains("(x: T)")),
        "the source annotation must not bypass its signature mapper: {instantiated:?}"
    );

    let generic_sibling = relation_texts(
        "/**\n\
             * @template in out T\n\
             * @typedef {Object} Invariant\n\
             * @property {(x: T) => T} f\n\
             */\n\
             /**\n\
             * @template T\n\
             * @param {Invariant<T>} source\n\
             * @param {Invariant<unknown>} target\n\
             */\n\
             function keep(source, target) { target = source; }\n",
    );
    assert!(
        generic_sibling
            .iter()
            .any(|text| text.contains("Type '(x: T) => T'")),
        "a mapper whose target is still T must retain the generic face: {generic_sibling:?}"
    );
}

#[test]
fn global_array_reference_with_appended_this_keeps_array_type_sugar() {
    assert_eq!(
        checked_diags(
            "\
interface Array<T> { length: number; }
type T3 = number[];
interface I3 extends T3 { length: string }
"
        ),
        [(
            2430,
            69,
            2,
            "Interface 'I3' incorrectly extends interface 'number[]'.".to_owned()
        )]
    );
}

#[test]
fn operator_faces_in_optional_tuple_positions_split_by_kind() {
    // strict:false keeps optional elements bare (no `| undefined`
    // widening), exposing parenthesizeTypeOfOptionalType per
    // kind: TypeOperator wraps, indexed-access and template
    // faces join bare.
    let options = CompilerOptions {
        strict: Some(false),
        ..CompilerOptions::default()
    };
    let diags = with_program_state(
            &[(
                "a.ts",
                "\nfunction o1<T>(x: number) { const y: [(keyof T)?] = x; }\nfunction o2<T, K extends keyof T>(x: number) { const y: [T[K]?] = x; }\nfunction o3<T extends string>(x: number) { const y: [`a${T}`?] = x; }\n",
            )],
            &options,
            |state| {
                state.check_source_file(0);
                diag_rows(state)
            },
        );
    assert_eq!(
        diags,
        [
            (
                2322,
                35,
                1,
                "Type 'number' is not assignable to type '[(keyof T)?]'.".to_owned()
            ),
            (
                2322,
                111,
                1,
                "Type 'number' is not assignable to type '[T[K]?]'.".to_owned()
            ),
            (
                2322,
                178,
                1,
                "Type 'number' is not assignable to type '[`a${T}`?]'.".to_owned()
            ),
        ]
    );
}

#[test]
fn mapped_name_type_and_readonly_index_write_are_checked() {
    let rows = checked_diags(
        "type Bad<T extends string> = { [K in T as {}]: T };\n\
             function write<T, K extends keyof T>(\n\
               target: { readonly [P in keyof T]: T[P] }, key: K, value: T[K]\n\
             ) { target[key] = value; }\n",
    );
    let codes: Vec<u32> = rows.iter().map(|row| row.0).collect();
    assert!(codes.contains(&2322), "{rows:?}");
    assert!(codes.contains(&2542), "{rows:?}");
}

#[test]
fn template_number_pattern_admits_the_tonumber_coercion_forms() {
    // Audit pin (oracle-probed byte-exact): `${number}` placeholder
    // validity rides the FULL JS ToNumber — radix forms 0b/0o/0x
    // and exponent forms admit; "other" and the JS-rejected "inf"
    // spelling refuse. The M4-era local coercion slice dropped
    // 0b/0o, and the 9.3b4 template display unmasked the stale
    // verdicts as templateLiteralTypesPatterns 2345 fabrications
    // (the reporting Err had contained them).
    assert_eq!(
            checked_diags(
                "declare function numbers(x: `${number}`): void;\nnumbers(\"1\");\nnumbers(\"-1\");\nnumbers(\"0\");\nnumbers(\"0b1\");\nnumbers(\"0o1\");\nnumbers(\"0x1\");\nnumbers(\"1e21\");\nnumbers(\"other\");\nnumbers(\"inf\");\nnumbers(\"0x100000000000000000000000000000000\");\nnumbers(\"0b111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111\");\nnumbers(\"0o77777777777777777777777777777777777777777777777777\");\n",
            ),
            [
                (
                    2345,
                    164,
                    7,
                    "Argument of type '\"other\"' is not assignable to parameter of type '`${number}`'.".to_owned()
                ),
                (
                    2345,
                    182,
                    5,
                    "Argument of type '\"inf\"' is not assignable to parameter of type '`${number}`'.".to_owned()
                ),
            ]
        );
}

#[test]
fn template_text_escape_tables_cover_the_map() {
    // Spec twins for cooked texts a .ts fixture cannot spell
    // directly (the scanner normalizes raw CR/CRLF to LF and the
    // source-expressible escapes ride the probe pins above):
    // the vendored tables at _tsc.js:16275-16295 — mapped
    // entries, the CRLF pair, LF identity, the null lookahead
    // against a non-digit, and per-unit surrogate escapes.
    assert_eq!(super::template_text_raw("a\r\nb"), "a\\r\\nb");
    assert_eq!(super::template_text_raw("a\rb"), "a\\rb");
    assert_eq!(super::template_text_raw("a\nb"), "a\nb");
    assert_eq!(
        super::template_text_raw("a\tb\u{8}\u{B}\u{C}"),
        "a\\tb\\b\\v\\f"
    );
    assert_eq!(super::template_text_raw("a\0b"), "a\\0b");
    assert_eq!(super::template_text_raw("a\u{0}1"), "a\\x001");
    assert_eq!(super::template_text_raw("a\0あ"), "a\\0\\u3042");
    assert_eq!(
        super::template_text_raw("\u{2028}\u{2029}\u{85}"),
        "\\u2028\\u2029\\u0085"
    );
    assert_eq!(super::template_text_raw("\u{1}\u{1F}"), "\\u0001\\u001F");
    assert_eq!(super::template_text_raw("\u{7F}"), "\u{7F}");
    assert_eq!(super::template_text_raw("😀"), "\\uD83D\\uDE00");
    assert_eq!(super::template_text_raw("a`b\\c"), "a\\`b\\\\c");
    assert_eq!(super::template_text_raw("${x}$y{z"), "\\${x}$y{z");
    assert_eq!(super::template_text_raw("$${"), "$\\${");
    assert_eq!(
        super::template_text_utf16_raw(&[0xD800, b'a' as u16, 0xDC00]),
        "\\uD800a\\uDC00"
    );
}

use tsc_types::CompilerOptions;

use crate::state::test_support::with_program_state;
use crate::{check_program, InputFile};

/// Driver-level fixture check (operators.rs idiom): oracle-pinned
/// rows (tsc 6.0.3, noLib, options {}) — scratchpad p.ts probes,
/// 2026-07-13. Suggestion-band 6133 and 80007 rows coexist with
/// grammar/error rows where tsc reports both; null-span global 2318
/// rows are file-less and filtered by the harness.
#[test]
fn setter_return_annotation_feeds_the_bare_return_7030() {
    // getEffectiveReturnTypeNode reads a set accessor's parsed
    // (grammatically-illegal, 1095) annotation generically
    // (16768) — the bare-return face still consults it (6.6
    // review D2; oracle-pinned vs vendored tsc 6.0.3 noLib).
    let options = CompilerOptions {
        no_implicit_returns: Some(true),
        strict_null_checks: Some(false),
        ..CompilerOptions::default()
    };
    assert_eq!(
        checked_rows_with(
            "class C { set p(v: number): number { if (v) { return; } } }\n",
            &options
        ),
        [(1095, 14, 1), (7030, 46, 6)]
    );
}

fn checked_rows(text: &str) -> Vec<(u32, u32, u32)> {
    checked_rows_with(text, &CompilerOptions::default())
}

fn checked_rows_with(text: &str, options: &CompilerOptions) -> Vec<(u32, u32, u32)> {
    with_program_state(&[("a.ts", text)], options, |state| {
        state.check_source_file(0);
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
    })
}

fn checked_program_rows_with_file(
    file_name: &str,
    text: &str,
    options: &CompilerOptions,
) -> Vec<(u32, u32, u32)> {
    check_program(
        &[InputFile::new(file_name.to_owned(), text.to_owned())],
        options,
    )
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

#[test]
fn mts_cts_generic_arrow_requires_disambiguating_type_parameter_syntax() {
    let diagnostic = |file_name: &str, source: &str| {
        checked_program_rows_with_file(file_name, source, &CompilerOptions::default())
            .into_iter()
            .filter(|(code, _, _)| *code == 7060)
            .collect::<Vec<_>>()
    };
    let ambiguous = "const x = <T>() => 0;\n";
    assert_eq!(diagnostic("a.mts", ambiguous), [(7060, 11, 1)]);
    assert_eq!(diagnostic("a.cts", ambiguous), [(7060, 11, 1)]);
    assert!(diagnostic("a.ts", ambiguous).is_empty());
    assert!(diagnostic("a.mts", "const x = <T,>() => 0;\n").is_empty());
    assert!(diagnostic("a.mts", "const x = <T extends unknown>() => 0;\n").is_empty());
    assert!(diagnostic("a.mts", "const x = <T, U>() => 0;\n").is_empty());
}

#[test]
fn signature_parameter_grammar_reads_each_nodes_own_parameter_array() {
    // tsc 6.0.3, noLib: grammar 1015 precedes the ordinary
    // signature-only initializer diagnostic for every declaration.
    assert_eq!(
        checked_rows(
            "interface I { (x? = 1): void; m(x? = 1): void; }\n\
                 type T = { (x? = 1): void; m(x? = 1): void; };\n"
        ),
        [
            (1015, 15, 1),
            (2371, 15, 6),
            (1015, 32, 1),
            (2371, 32, 6),
            (1015, 61, 1),
            (2371, 61, 6),
            (1015, 78, 1),
            (2371, 78, 6),
        ]
    );
}

#[test]
fn unrelated_jsdoc_does_not_hide_checked_js_parameter_assignments() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        no_implicit_any: Some(true),
        strict_null_checks: Some(true),
        ..CompilerOptions::default()
    };
    assert_eq!(
        checked_program_rows_with_file(
            "a.js",
            "/** @type {number} */\n\
                 var documented = 1;\n\
                 function f(a = null) {\n\
                     a = undefined;\n\
                     a = null;\n\
                     a = 1;\n\
                     a = true;\n\
                     a = 'ok';\n\
                     void a;\n\
                 }\n",
            &options,
        ),
        [(2322, 65, 1), (2322, 90, 1), (2322, 97, 1), (2322, 107, 1),]
    );
}

#[test]
fn checked_js_module_overload_tags_participate_in_implementation_compatibility() {
    let text = "/**\n\
                    * @overload\n\
                    * @param {number} a\n\
                    * @param {number} b\n\
                    * @returns {number}\n\
                    *\n\
                    * @overload\n\
                    * @param {string} a\n\
                    * @param {boolean} b\n\
                    * @returns {string}\n\
                    *\n\
                    * @param {string | number} a\n\
                    * @param {string | number} b\n\
                    * @returns {string | number}\n\
                    */\n\
                    export function overloaded(a, b) {\n\
                        return a;\n\
                    }\n";
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        strict: Some(false),
        ..CompilerOptions::default()
    };
    let rows: Vec<_> = checked_program_rows_with_file("a.js", text, &options)
        .into_iter()
        .filter(|row| matches!(row.0, 2394 | 7009 | 7012))
        .collect();
    let second_overload = text
        .match_indices("@overload")
        .nth(1)
        .expect("second overload tag")
        .0 as u32
        + 1;
    assert_eq!(rows, [(2394, second_overload, 8)]);
}

#[test]
fn checked_js_global_script_does_not_check_jsdoc_overloads_as_local_symbol_overloads() {
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
    let rows: Vec<_> = checked_program_rows_with_file("templateInsideCallback.js", text, &options)
        .into_iter()
        .filter(|row| matches!(row.0, 2394 | 7012))
        .collect();
    assert!(rows.is_empty(), "{rows:?}");
}

#[test]
fn checked_js_constructor_overloads_keep_constructor_semantics() {
    let text = "class Foo {\n\
                    #a = true ? 1 : \"1\"\n\
                    #b\n\
                    /**\n\
                     * @constructor\n\
                     * @overload\n\
                     * @param {string} a\n\
                     * @param {number} b\n\
                     */\n\
                    /**\n\
                     * @constructor\n\
                     * @overload\n\
                     * @param {number} a\n\
                     */\n\
                    /**\n\
                     * @constructor\n\
                     * @overload\n\
                     * @param {string} a\n\
                     */\n\
                    /**\n\
                     * @constructor\n\
                     * @param {number | string} a\n\
                     */\n\
                    constructor(a, b) {\n\
                        this.#a = a;\n\
                        this.#b = b;\n\
                    }\n\
                }\n\
                new Foo();\n\
                new Foo(\"str\");\n\
                new Foo(2);\n\
                new Foo(\"str\", 2);\n";
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        strict: Some(true),
        target: Some(tsc_types::ScriptTarget::ES_NEXT.bits()),
        ..CompilerOptions::default()
    };
    let rows: Vec<_> = checked_program_rows_with_file("a.js", text, &options)
        .into_iter()
        .filter(|row| matches!(row.0, 2394 | 7009 | 7012))
        .collect();
    let second_overload = text
        .match_indices("@overload")
        .nth(1)
        .expect("second overload tag")
        .0 as u32
        + 1;
    assert_eq!(rows, [(2394, second_overload, 8)]);
}

#[test]
fn recovered_missing_body_is_not_an_absent_implementation() {
    let result = check_program(
        &[InputFile::new(
            "a.ts".to_owned(),
            "function f() => 4;\n".to_owned(),
        )],
        &CompilerOptions::default(),
    );
    assert_eq!(
        result
            .syntactic_diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [1144]
    );
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| !matches!(diagnostic.code(), 2389 | 2391)),
        "{:?}",
        result.diagnostics
    );
    assert_eq!(
        checked_rows("function f(): void;\nconst x = 1;\n"),
        [(2391, 9, 1)]
    );
}

// ---- checkTypePredicate tail (M5 close; rows oracle-pinned vs
// vendored tsc 6.0.3 noLib per shape, 2026-07-19) ----

#[test]
fn type_predicate_type_must_be_assignable_to_its_parameter() {
    // 2677 at node.type.
    assert_eq!(
        checked_rows("function f(x: string): x is number {\n    void x;\n    return true;\n}\n"),
        [(2677, 28, 6)]
    );
    // The containingMessageChain wrap (64890-64896) folds the
    // no-common-properties face under the SAME 2677 head — never
    // a bare 2559.
    assert_eq!(
        checked_rows("declare function w(x: { a(): void }): x is { b?: number };\n"),
        [(2677, 43, 14)]
    );
    // Width subtyping runs predicate→parameter: extra predicate
    // members are fine.
    assert_eq!(
        checked_rows("declare function m(x: { a: number }): x is { a: number, b: number };\n"),
        []
    );
    assert_eq!(
        checked_rows("declare function ok(x: number | string): x is string;\n"),
        []
    );
    // asserts-identifier predicates take the same tail; a bare
    // asserts (no type) checks nothing.
    assert_eq!(
        checked_rows("declare function a1(x: string): asserts x is number;\n"),
        [(2677, 45, 6)]
    );
    assert_eq!(
        checked_rows("declare function a2(x: string): asserts x;\n"),
        []
    );
    // This/AssertsThis kinds skip the identifier tail entirely.
    assert_eq!(
        checked_rows("class C { m(): this is C { return true; } }\n"),
        []
    );
    // MethodSignature and FunctionType parents reach the same
    // check (getTypePredicateParent kinds).
    assert_eq!(
        checked_rows("interface I { p(x: string): x is number; }\n"),
        [(2677, 33, 6)]
    );
    assert_eq!(
        checked_rows("let ft: (x: string) => x is number;\n"),
        [(2677, 28, 6)]
    );
}

#[test]
fn type_predicate_assignability_keeps_relation_chain_and_related_info() {
    fn flatten_codes(chain: &tsc_diagnostics::MessageChain, codes: &mut Vec<u32>) {
        codes.push(chain.code);
        for child in &chain.next {
            flatten_codes(child, codes);
        }
    }

    let diagnostics = crate::state::test_support::with_program_state(
        &[(
            "a.ts",
            "interface A { propA: string; }\n\
                 interface B { propB: string; }\n\
                 declare function missing(x: A): x is B;\n\
                 declare function primitive(x: string): x is number;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            state
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 2677)
                .map(|diagnostic| (diagnostic.message.clone(), diagnostic.related.clone()))
                .collect::<Vec<_>>()
        },
    );
    assert_eq!(diagnostics.len(), 2);
    let chains = diagnostics
        .iter()
        .map(|(message, _)| {
            let mut codes = Vec::new();
            flatten_codes(message, &mut codes);
            codes
        })
        .collect::<Vec<_>>();
    assert_eq!(chains, [vec![2677, 2741], vec![2677, 2322]]);
    assert_eq!(diagnostics[0].1.len(), 1);
    assert_eq!(diagnostics[0].1[0].message.code, 2728);
    assert_eq!(
        diagnostics[0].1[0].message.text,
        "'propA' is declared here."
    );
    assert!(diagnostics[1].1.is_empty());
}

#[test]
fn type_predicate_parameter_reference_errors() {
    // 1229: the predicate references the rest parameter itself.
    assert_eq!(
        checked_rows("declare function b4(...a: any[]): a is number;\n"),
        [(1229, 34, 1)]
    );
    // A rest parameter elsewhere in the list doesn't gate the
    // named parameter's assignability face.
    assert_eq!(
        checked_rows("declare function r(x: string, ...rest: any[]): x is number;\n"),
        [(2677, 52, 6)]
    );
    // 1225: no parameter of that name.
    assert_eq!(
        checked_rows("declare function h(y: string): x is number;\n"),
        [(1225, 31, 1)]
    );
    // 1230: the name lives inside a binding pattern (object,
    // nested, and the no-match fallback to 1225).
    assert_eq!(
        checked_rows("declare function b5({ a, b, p1 }: any, p2: any): p1 is number;\n"),
        [(1230, 49, 2)]
    );
    assert_eq!(
        checked_rows("declare function b7({ a, c: { p1 } }: any, p2: any): p1 is number;\n"),
        [(1230, 53, 2)]
    );
    assert_eq!(
        checked_rows("declare function b8({ a, b }: any, p2: any): q is number;\n"),
        [(1225, 45, 1)]
    );
}

// ---- implicit returns (6.6c; rows oracle-pinned vs vendored
// tsc 6.0.3 noLib per shape, 2026-07-19) ----

#[test]
fn reachable_end_in_non_void_function_reports_the_ladder() {
    // 2355: declared non-void, no explicit return, end reachable.
    assert_eq!(checked_rows("function f(): number { }\n"), [(2355, 14, 6)]);
    // 2534: declared never with a reachable end point.
    assert_eq!(checked_rows("function f(): never { }\n"), [(2534, 14, 5)]);
    // 2366: strictNullChecks (TS6 default-on) + explicit return
    // present but end still reachable.
    assert_eq!(
        checked_rows("function f(x: boolean): number { if (x) return 1; }\n"),
        [(2366, 24, 6)]
    );
    // A throw-terminated body has an unreachable end — clean.
    assert_eq!(checked_rows("function f(): number { throw 1; }\n"), []);
}

#[test]
fn jsdoc_return_types_anchor_2355_at_the_effective_type_node() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        strict: Some(true),
        ..CompilerOptions::default()
    };
    let rows = |source: &str| {
        checked_program_rows_with_file("a.js", source, &options)
            .into_iter()
            .filter(|(code, _, _)| *code == 2355)
            .collect::<Vec<_>>()
    };

    let function_type = "/** @type {function(): number} */\nfunction f() {}\n";
    assert_eq!(rows(function_type), [(2355, 23, 6)]);

    let return_tag = "/** @return {T} */\n\
                          const dedupingMixin = function(mixin) {};\n\
                          /** @template T */\n\
                          const PropertyAccessors = dedupingMixin(() => {});\n";
    assert_eq!(rows(return_tag), [(2355, 13, 1)]);
}

#[test]
fn checked_js_constructor_self_return_does_not_add_undefined() {
    let text = "/** @param {number} x */\n\
                    function A(x) {\n\
                        if (!(this instanceof A)) return new A(x);\n\
                        this.x = x;\n\
                    }\n\
                    var instance = A(1);\n\
                    instance.x;\n\
                    /** @param {boolean} flag */\n\
                    function ordinary(flag) {\n\
                        if (flag) return { x: 1 };\n\
                    }\n\
                    var maybe = ordinary(true);\n\
                    maybe.x;\n";
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        strict: Some(true),
        no_implicit_this: Some(false),
        target: Some(tsc_types::ScriptTarget::ES2015.bits()),
        ..CompilerOptions::default()
    };
    let rows = checked_program_rows_with_file("a.js", text, &options)
        .into_iter()
        .filter(|row| row.0 == 18048)
        .collect::<Vec<_>>();
    let ordinary_use = text.rfind("maybe.x").expect("ordinary optional return") as u32;
    assert_eq!(rows, [(18048, ordinary_use, "maybe".len() as u32)]);
}

#[test]
fn reachability_refinements_suppress_implicit_return_reports() {
    // The three checker-side refinements the bind-time flag could
    // not see (the retired [FLOW M5] switch/call gate's faces).
    // Never-returning call (getEffectsSignature):
    assert_eq!(
        checked_rows("declare function fail(): never;\nfunction f(): number { fail(); }\n"),
        []
    );
    // Exhaustive switch (SwitchClause clauseStart==clauseEnd +
    // isExhaustiveSwitchStatement):
    assert_eq!(
        checked_rows(
            "function f(x: 1 | 2): number { switch (x) { case 1: return 1; case 2: return 2; } }\n"
        ),
        []
    );
    // Non-exhaustive control: the suppression must NOT over-fire.
    assert_eq!(
        checked_rows("function f(x: 1 | 2): number { switch (x) { case 1: return 1; } }\n"),
        [(2366, 22, 6)]
    );
    // asserts-false argument (isFalseExpression):
    assert_eq!(
            checked_rows(
                "declare function assert(v: boolean): asserts v;\nfunction f(): number { assert(false); }\n"
            ),
            []
        );
}

#[test]
fn no_implicit_returns_arms_report_7030() {
    let options = CompilerOptions {
        no_implicit_returns: Some(true),
        ..CompilerOptions::default()
    };
    // Annotation-less: inferred return type is non-void with an
    // explicit return elsewhere (79096's !type block).
    assert_eq!(
        checked_rows_with("function f(x: boolean) { if (x) return 1; }\n", &options),
        [(7030, 9, 1)]
    );
    // Declared undefined-including type still reaches the trailing
    // arm (the else-if LADDER: the snc arm's condition fails —
    // undefined IS assignable — and falls through, 79087-79096).
    assert_eq!(
        checked_rows_with(
            "function f(x: boolean): number | undefined { if (x) return 1; }\n",
            &options
        ),
        [(7030, 24, 18)]
    );
    // checkReturnStatement's bare-`return;` face (84546) — only
    // reachable with strictNullChecks off.
    let snc_off = CompilerOptions {
        no_implicit_returns: Some(true),
        strict_null_checks: Some(false),
        ..CompilerOptions::default()
    };
    assert_eq!(
        checked_rows_with(
            "function f(x: boolean): number { if (x) { return; } return 1; }\n",
            &snc_off
        ),
        [(7030, 42, 6)]
    );
}

#[test]
fn never_typed_callable_parameter_suppresses_implicit_return() {
    // The effects consult must see the FunctionTYPE declaration's
    // never annotation (getEffectiveReturnTypeNode reads `.type`
    // on every signature-declaration kind — the FunctionType arm
    // was the 6.6c FP face, neverReturningFunctions1 f12).
    assert_eq!(
            checked_rows(
                "function f12(x: number, fail: (message?: string) => never): number {\n    if (x >= 0) return x;\n    fail(\"negative number\");\n    x;\n}\n"
            ),
            // The implicit-return 2366 remains suppressed; the final
            // expression is independently unreachable and carries
            // checkSourceElementUnreachable's default suggestion.
            [(7027, 128, 2)]
        );
}

// ---- fn-expression bodies (deferred pass) ----

#[test]
fn function_expression_block_bodies_check_deferred() {
    assert_eq!(
        checked_rows("(function () { \"x\".foo; });\n"),
        [(2339, 19, 3)]
    );
}

#[test]
fn function_declaration_signature_infers_from_body() {
    // h : () => string via getSignatureFromDeclaration +
    // getReturnTypeFromBody — unlocks the operator band on
    // function declarations (5.5e FN row).
    assert_eq!(
        checked_rows("function h() { return \"s\"; }\nh * 2;\n"),
        [(2362, 29, 1)]
    );
}

#[test]
fn contextual_signature_types_unannotated_parameters() {
    assert_eq!(
        checked_rows("declare let cb: (n: number) => void;\ncb = (x) => { x.foo; };\n"),
        [(2339, 53, 3)]
    );
}

#[test]
fn getter_bodies_infer_through_get_type_of_accessors() {
    // The 3417-band un-escape: "s" widens to string (unit-type
    // contextual widening, no contextual signature).
    assert_eq!(
        checked_rows("({ get g() { return \"s\" } }).g.bad;\n"),
        [(2339, 31, 3)]
    );
}

// ---- parameter-list grammar ----

#[test]
fn required_after_optional_reports_1016() {
    assert_eq!(
        checked_rows("(function (a?: number, b: string) {});\n"),
        [(1016, 23, 1), (6133, 11, 1), (6133, 23, 1)]
    );
}

#[test]
fn jsdoc_optional_binding_pattern_reports_2463() {
    let text = "/** @param {{ a: string }} [options] */\n\
                    function optional({ a }) {}\n\
                    /** @param {{ a: string }} options */\n\
                    function required({ a }) {}\n\
                    /** @param {{ a: string }} [options] */\n\
                    function initialized({ a } = { a: '' }) {}\n";
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let rows = checked_program_rows_with_file("a.js", text, &options)
        .into_iter()
        .filter(|row| row.0 == 2463)
        .collect::<Vec<_>>();
    let parameter = "{ a }";
    let start = text.find(parameter).expect("optional binding parameter") as u32;
    assert_eq!(rows, [(2463, start, parameter.len() as u32)]);
}

#[test]
fn optional_rest_reports_1047() {
    // Both oracle rows since the A3 wiring: the grammar 1047 and
    // checkSignatureDeclaration's 2370 (`number[] | undefined`
    // fails the readonly-array relation), plus the function-
    // expression-owned unused suggestion.
    assert_eq!(
        checked_rows("(function (...rest?: number[]) {});\n"),
        [(1047, 18, 1), (2370, 11, 18), (6133, 14, 4)]
    );
}

#[test]
fn use_strict_with_non_simple_parameters_reports_1346_1347() {
    assert_eq!(
        checked_rows("(function (a = 2) { \"use strict\"; });\n"),
        [(1346, 11, 5), (1347, 20, 13), (6133, 11, 1)]
    );
}

// ---- await / yield grammar ----

#[test]
fn top_level_await_in_non_module_reports_1375() {
    assert_eq!(checked_rows("await 1;\n"), [(1375, 0, 5), (80007, 0, 7)]);
}

#[test]
fn await_inside_plain_function_expression_reports_1308() {
    // related 1356 (mark the function async) rides on the 1308 row.
    assert_eq!(
        checked_rows("(function f2() { return await 2; });\n"),
        [(1308, 24, 5), (80007, 24, 7)]
    );
}

#[test]
fn top_level_await_in_node_commonjs_reports_1309() {
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
                checked_program_rows_with_file(
                    &format!("a.{extension}"),
                    "const x = await 1;\nexport { x };\n",
                    &options
                ),
                [(1309, 10, 5), (80007, 10, 7)]
            );
        }
    }
}

#[test]
fn top_level_await_in_node_esm_remains_allowed() {
    for (extension, allow_js, check_js) in [("mts", false, None), ("mjs", true, Some(true))] {
        let options = CompilerOptions {
            module: Some(100),
            target: Some(tsc_types::ScriptTarget::ES2022.bits()),
            allow_js,
            check_js,
            ..CompilerOptions::default()
        };
        assert_eq!(
            checked_program_rows_with_file(
                &format!("a.{extension}"),
                "const x = await 1;\nexport { x };\n",
                &options
            ),
            [(80007, 10, 7)]
        );
    }
}

#[test]
fn no_effect_await_suggestion_excludes_changed_error_any_and_unknown_types() {
    let text = "declare const n: number;
declare const p: { then(cb: (value: number) => void): void };
declare const a: any;
declare const u: unknown;
(async () => {
    await n;
    await p;
    await a;
    await u;
    await missing;
})();
";
    let rows = with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        state
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 80007)
            .map(|diagnostic| {
                (
                    diagnostic.code(),
                    diagnostic.category(),
                    diagnostic.start,
                    diagnostic.length,
                    diagnostic.message.text.clone(),
                    diagnostic.related.len(),
                )
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(
        rows,
        [(
            80007,
            tsc_diagnostics::DiagnosticCategory::Suggestion,
            Some(text.find("await n").expect("positive await") as u32),
            Some(7),
            "'await' has no effect on the type of this expression.".to_owned(),
            0,
        )]
    );

    let js = "(async () => { await 1; })();\n";
    let js_rows = check_program(
        &[InputFile::new("a.js".to_owned(), js.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    )
    .diagnostics
    .iter()
    .filter(|diagnostic| diagnostic.code() == 80007)
    .map(|diagnostic| {
        (
            diagnostic.category(),
            diagnostic.start,
            diagnostic.length,
            diagnostic.message.text.clone(),
        )
    })
    .collect::<Vec<_>>();
    assert_eq!(
        js_rows,
        [(
            tsc_diagnostics::DiagnosticCategory::Suggestion,
            Some(js.find("await 1").expect("checked-JS await") as u32),
            Some(7),
            "'await' has no effect on the type of this expression.".to_owned(),
        )]
    );
}

#[test]
fn yield_outside_generator_reports_1163() {
    assert_eq!(
        checked_rows("(function () { yield 5; });\n"),
        [(1163, 15, 5)]
    );
}

#[test]
fn checked_js_publishes_implicit_any_yield_only_without_return_context() {
    let text = "function* f() { let o; while (true) { o = yield o; } }\n";
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        no_implicit_any: Some(true),
        target: Some(tsc_types::ScriptTarget::ES2015.bits()),
        ..CompilerOptions::default()
    };
    let rows = checked_program_rows_with_file("a.js", text, &options)
        .into_iter()
        .filter(|(code, _, _)| *code == 7057)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            7057,
            text.find("yield").expect("yield expression") as u32,
            5
        )]
    );

    let unchecked = CompilerOptions {
        check_js: Some(false),
        ..options.clone()
    };
    assert!(
        checked_program_rows_with_file("unchecked.js", text, &unchecked)
            .into_iter()
            .all(|(code, _, _)| code != 7057)
    );
    assert!(checked_program_rows_with_file(
        "typed.ts",
        "function* f(): any { const value = yield 0; }\n",
        &CompilerOptions {
            no_implicit_any: Some(true),
            target: Some(tsc_types::ScriptTarget::ES2015.bits()),
            ..CompilerOptions::default()
        },
    )
    .into_iter()
    .all(|(code, _, _)| code != 7057));
}

// ---- await family error paths ----

#[test]
fn non_callable_then_callback_reports_1320() {
    // { then(cb: number): void } is thenable but its callback is
    // not callable — getAwaitedTypeNoAlias' thenable tail.
    assert_eq!(
        checked_rows("declare const r: { then(cb: number): void };\n(async () => { await r; });\n"),
        [(2697, 46, 24), (1320, 60, 7)]
    );
}

#[test]
fn self_referential_thenable_reports_1062() {
    assert_eq!(
            checked_rows(
                "type T = { then(cb: (v: T) => void): void };\ndeclare const s: T;\n(async () => { await s; });\n"
            ),
            [(2697, 66, 24), (1062, 80, 7)]
        );
}

#[test]
fn union_self_referential_thenable_reports_1062() {
    assert_eq!(
            checked_rows(
                "type U = number | { then(cb: (v: U) => void): void };\ndeclare const u: U;\n(async () => { await u; });\n"
            ),
            [(2697, 75, 24), (1062, 89, 7)]
        );
}

#[test]
fn custom_thenable_awaits_to_its_promised_type() {
    // Oracle rows exactly: 2697 (untyped thenable await needs a
    // declared Promise) + 2339 @94 (x.bad → number). The 2339 row
    // recovered when getQuickTypeOfExpression's await arm went
    // live (the initializer used to contain the whole element).
    assert_eq!(
            checked_rows(
                "declare const p: { then(cb: (v: number) => void): void };\n(async () => { const x = await p; x.bad; });\n"
            ),
            [(2697, 59, 41), (2339, 94, 3)]
        );
}

#[test]
fn async_block_body_without_promise_reports_2697() {
    assert_eq!(
            checked_rows(
                "declare const th: { then: number };\ndeclare let r: () => void;\nr = async () => { await th; };\n"
            ),
            [(2697, 67, 25), (80007, 81, 8)]
        );
}

#[test]
fn method_modifier_error_suppresses_type_parameter_grammar() {
    // m4-review S7 (oracle: vendored tsc 6.0.3, noLib, strict,
    // 2026-07-19): tsc 1031 @10 + 1183 @30 — the live declare
    // verdict heads the `||` ladder and
    // suppresses the empty-type-parameter-list 1098 the port
    // reported pre-fix.
    assert_eq!(
        checked_rows("class C { declare m<>(): void {} }\n"),
        [(1031, 10, 7), (1183, 30, 1)]
    );
}

// ---- m4-review A3: checkSignatureDeclaration on expression
// forms (oracle: vendored tsc 6.0.3, noLib, strict, 2026-07-19).
// The contextual once-path used to end in a no-op stub, so every
// signature-declaration row was FN for fn-exprs/arrows/obj-methods.

#[test]
fn arrow_type_predicate_unassignable_reports_2677() {
    assert_eq!(
        checked_rows("const p = (x: number): x is string => typeof x === \"string\";\n"),
        [(2677, 28, 6)]
    );
}

#[test]
fn generator_function_expression_void_annotation_reports_2505() {
    assert_eq!(
        checked_rows("const g = function* (): void {};\n"),
        [(2505, 24, 4)]
    );
}

#[test]
fn async_arrow_non_promise_annotation_reports_1064() {
    assert_eq!(
            checked_rows(
                "interface Promise<T> { p: T }\ndeclare const a: any;\nconst h = async (): number => a;\n"
            ),
            [(1064, 72, 6)]
        );
}

#[test]
fn async_unresolved_alias_return_stays_on_the_error_type_bailout() {
    let options = CompilerOptions {
        target: Some(tsc_types::ScriptTarget::ES2015.bits()),
        ..CompilerOptions::default()
    };
    let rows = checked_rows_with(
        "interface Promise<T> {}\nasync function f(): Missing<void> {}\n",
        &options,
    );
    assert!(
        rows.iter().all(|row| !matches!(row.0, 1055 | 1064)),
        "an alias-bearing error type must not trigger async return validation: {rows:?}"
    );
}

#[test]
fn checked_js_async_function_type_tag_checks_the_contextual_return() {
    let text = "/** @type {function(): string} */\nconst value = async () => 0;\n";
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        target: Some(2),
        ..CompilerOptions::default()
    };
    assert_eq!(
        checked_program_rows_with_file("a.js", text, &options),
        [(2322, text.rfind('0').expect("return expression") as u32, 1)]
    );
}

#[test]
fn checked_js_function_type_tag_reports_8030_at_the_type_node() {
    let text = "/** @type {number} */\n\
                    function f() {}\n\
                    /** @type {(a: number) => number} */\n\
                    function add1(a, b) {}\n\
                    /** @type {() => void} */\n\
                    function more(value) {}\n\
                    /** @typedef {{(s: string): 0 | 1; (b: boolean): 2 | 3}} G */\n\
                    /** @type {G} */\n\
                    function overloaded(value) {}\n\
                    /** @type {Self} */\n\
                    function Self() {}\n";
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        strict: Some(false),
        ..CompilerOptions::default()
    };
    let rows: Vec<_> = checked_program_rows_with_file("a.js", text, &options)
        .into_iter()
        .filter(|row| row.0 == 8030)
        .collect();
    assert_eq!(
        rows,
        [
            (8030, text.find("number").unwrap() as u32, 6),
            (8030, text.find("(a: number) => number").unwrap() as u32, 21),
            (8030, text.find("() => void").unwrap() as u32, 10),
            (8030, text.rfind("{G}").unwrap() as u32 + 1, 1),
        ]
    );
}

#[test]
fn checked_js_function_type_tag_keeps_callable_siblings_non_emitting() {
    let text = "/** @type {(a: number) => number} */\n\
                    function exact(a) {}\n\
                    /** @type {(a: number, b: number, c: number) => number} */\n\
                    function wider(a, b) {}\n\
                    /** @type {{(a: number): string}} */\n\
                    function objectCall(a) {}\n\
                    /** @typedef {(x: number) => string} Alias */\n\
                    /** @type {Alias} */\n\
                    function aliased(x) {}\n\
                    /** @type {Object<string, *>} */\n\
                    const namespaceSibling = {};\n\
                    function noTypeTag(input) { return input; }\n";
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    assert!(checked_program_rows_with_file("a.js", text, &options)
        .into_iter()
        .all(|row| row.0 != 8030));
}

#[test]
fn checked_js_async_callback_alias_checks_the_contextual_return() {
    let text = "/**\n\
                    * @callback FunctionReturningNever\n\
                    * @returns {never}\n\
                    */\n\
                    /** @type {FunctionReturningNever} */\n\
                    async function value() { return 1; }\n";
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        target: Some(2),
        ..CompilerOptions::default()
    };
    let result = check_program(
        &[InputFile::new("a.js".to_owned(), text.to_owned())],
        &options,
    );
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code() == 2322)
        .expect("the callback alias contextually checks the return");
    assert_eq!(
        diagnostic.start,
        Some(text.rfind("return").expect("return statement") as u32)
    );
    assert_eq!(diagnostic.length, Some("return".len() as u32));
    assert!(diagnostic.related.is_empty());
    assert!(result
        .diagnostics
        .iter()
        .all(|diagnostic| diagnostic.code() != 1065));
}

#[test]
fn checked_js_es5_thenable_alias_preserves_relation_chain() {
    let lib = "declare type PromiseConstructorLike = new <T>(executor: (resolve: (value: T | PromiseLike<T>) => void, reject: (reason?: any) => void) => void) => PromiseLike<T>;\n\
                   interface PromiseLike<T> {\n\
                     then<TResult1 = T, TResult2 = never>(onfulfilled?: (value: T) => TResult1 | PromiseLike<TResult1>, onrejected?: (reason: any) => TResult2 | PromiseLike<TResult2>): PromiseLike<TResult1 | TResult2>;\n\
                   }\n";
    let js = "/**\n\
                  * @callback T3\n\
                  * @returns {Thenable}\n\
                  */\n\
                  /** @type {T3} */\n\
                  const value = async () => 1;\n";
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        target: Some(1),
        ..CompilerOptions::default()
    };
    let result = check_program(
        &[
            InputFile::new("/lib.d.ts".to_owned(), lib.to_owned()),
            InputFile::new(
                "/types.d.ts".to_owned(),
                "declare class Thenable { then(): void; }\n".to_owned(),
            ),
            InputFile::new("/a.js".to_owned(), js.to_owned()),
        ],
        &options,
    );
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code() == 1065)
        .expect("the invalid ES5 constructor relation is reported");
    assert_eq!(diagnostic.start, Some(js.rfind("T3").unwrap() as u32));
    assert!(diagnostic.related.is_empty());
    assert_eq!(diagnostic.message.next[0].code, 1055);
    assert_eq!(diagnostic.message.next[0].next[0].code, 2203);
    assert_eq!(diagnostic.message.next[0].next[0].next[0].code, 2201);
    assert_eq!(
        diagnostic.message.next[0].next[0].next[0].next[0].code,
        2322
    );
}

#[test]
fn es5_async_constructor_relation_uses_the_tsc_compatibility_pyramid() {
    let lib = "declare type PromiseConstructorLike = new <T>(executor: (resolve: (value: T | PromiseLike<T>) => void, reject: (reason?: any) => void) => void) => PromiseLike<T>;\n\
                   interface PromiseLike<T> {\n\
                     then<TResult1 = T, TResult2 = never>(onfulfilled?: (value: T) => TResult1 | PromiseLike<TResult1>, onrejected?: (reason: any) => TResult2 | PromiseLike<TResult2>): PromiseLike<TResult1 | TResult2>;\n\
                   }\n";
    let source = "declare class Thenable { then(): void; }\nasync function value(): Thenable {}\n";
    let result = check_program(
        &[
            InputFile::new("/lib.d.ts".to_owned(), lib.to_owned()),
            InputFile::new("/a.ts".to_owned(), source.to_owned()),
        ],
        &CompilerOptions {
            target: Some(tsc_types::ScriptTarget::ES5.bits()),
            ..CompilerOptions::default()
        },
    );
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code() == 1055)
        .expect("the invalid ES5 promise constructor is reported");
    assert_eq!(diagnostic.message.next[0].code, 2203);
    assert_eq!(diagnostic.message.next[0].next[0].code, 2201);
    assert_eq!(diagnostic.message.next[0].next[0].next[0].code, 2322);
}

#[test]
fn arrow_non_array_rest_parameter_reports_2370() {
    assert_eq!(
            checked_rows(
                "interface Array<T> { length: number }\ninterface ReadonlyArray<T> { length: number }\ninterface ConcatArray<T> { length: number }\nconst f = (...r: number) => r;\n"
            ),
            [
                (2370, 139, 12),
                (6133, 15, 3),
                (6133, 61, 3),
                (6133, 105, 3),
            ]
        );
}

// ---- m4-review A2: obj-literal accessors defer to the whole
// checkAccessorDeclaration (oracle: vendored tsc 6.0.3, noLib,
// strict, 2026-07-19). The subset route checked signature +
// accessor types but never entered the body.

#[test]
fn obj_literal_getter_body_is_checked() {
    assert_eq!(
            checked_rows(
                "const o = {\n    get x() {\n        let a: number = \"s\";\n        return 1;\n    },\n};\n"
            ),
            [(2322, 38, 1), (6133, 38, 1)]
        );
}

#[test]
fn obj_literal_accessor_grammar_and_setter_body_rows() {
    assert_eq!(
            checked_rows(
                "const o = {\n    get x(this: void, extra: number) {\n        return 1;\n    },\n    set y(_: string) {\n        let b: string = 123;\n        b;\n    },\n};\n"
            ),
            [
                (1054, 20, 1),
                (2784, 22, 10),
                (2322, 111, 1),
                (6133, 34, 5),
            ]
        );
}

#[test]
fn checked_js_inherited_jsdoc_getter_type_does_not_form_a_derived_accessor_cycle() {
    let lib = "interface String {}\n";
    let source = "export class Element {\n\
                      /** @returns {String} */\n\
                      get textContent() { return ''; }\n\
                      set textContent(value) {}\n\
                      }\n\
                      export class HTMLElement extends Element {}\n\
                      export class TextElement extends HTMLElement {\n\
                      get innerHTML() { return this.textContent; }\n\
                      set innerHTML(html) { this.textContent = html; }\n\
                      }\n";
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        strict: Some(true),
        target: Some(tsc_types::ScriptTarget::ES2015.bits()),
        ..CompilerOptions::default()
    };
    let result = check_program(
        &[
            InputFile::new("/lib.d.ts".to_owned(), lib.to_owned()),
            InputFile::new("/a.js".to_owned(), source.to_owned()),
        ],
        &options,
    );
    let circular: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|diagnostic| matches!(diagnostic.code(), 7022 | 7023))
        .map(|diagnostic| diagnostic.code())
        .collect();
    assert!(
        circular.is_empty(),
        "the inherited JSDoc getter type must anchor derived accessor inference: {circular:?}"
    );
}

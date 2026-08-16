use tsc_types::{CompilerOptions, TypeFlags};

use crate::relpin::find_probe_annotation;
use crate::state::test_support::with_program_state;
use crate::{check_program, InputFile};

#[test]
fn non_generic_type_alias_resolves_to_aliased_type() {
    with_program_state(
        &[("a.ts", "type A = string | number;\ndeclare var v: A;\n")],
        &CompilerOptions::default(),
        |state| {
            let annotation =
                find_probe_annotation(state.binder.source(0), "v").expect("annotation");
            let resolved = state
                .get_type_from_type_node(annotation)
                .expect("alias resolves");
            assert!(state.tables.flags_of(resolved).intersects(TypeFlags::UNION));
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn circular_type_alias_reports_2456_and_yields_error_type() {
    with_program_state(
        &[("a.ts", "type A = A;\ndeclare var v: A;\n")],
        &CompilerOptions::default(),
        |state| {
            let annotation =
                find_probe_annotation(state.binder.source(0), "v").expect("annotation");
            let resolved = state
                .get_type_from_type_node(annotation)
                .expect("circular alias resolves to errorType");
            assert!(state.tables.is_error_type(resolved));
            let codes: Vec<u32> = state.diagnostics.iter().map(|d| d.code()).collect();
            assert_eq!(codes, [2456]);
        },
    );
}

#[test]
fn jsdoc_enum_circular_alias_reports_2456_on_the_enum_type() {
    let source = "\n/** @enum {E} */\nconst E = { x: 0 };\n";
    let result = check_program(
        &[InputFile::new("a.js".to_owned(), source.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    let rows = result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code() == 2456)
        .map(|diagnostic| {
            (
                diagnostic.file_name.as_deref(),
                diagnostic.start,
                diagnostic.length,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(rows, [(Some("a.js"), Some(12), Some(1))]);
}

#[test]
fn typeof_annotated_var_resolves_to_declared_type() {
    with_program_state(
        &[(
            "a.ts",
            "declare var w: \"lit\";\ndeclare var v: typeof w;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let annotation =
                find_probe_annotation(state.binder.source(0), "v").expect("annotation");
            let resolved = state
                .get_type_from_type_node(annotation)
                .expect("typeof resolves");
            // Regular (non-fresh) literal type, like tsc's
            // getRegularTypeOfLiteralType tail.
            assert!(state
                .tables
                .flags_of(resolved)
                .intersects(TypeFlags::STRING_LITERAL));
            assert_eq!(
                state.tables.get_regular_type_of_literal_type(resolved),
                resolved
            );
        },
    );
}

#[test]
fn typeof_namespace_member_resolves_through_exports() {
    with_program_state(
        &[(
            "a.ts",
            "namespace N { export const K: number = 1; }\ndeclare var v: typeof N.K;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let annotation =
                find_probe_annotation(state.binder.source(0), "v").expect("annotation");
            let resolved = state
                .get_type_from_type_node(annotation)
                .expect("qualified typeof resolves");
            assert_eq!(resolved, state.tables.intrinsics.number);
        },
    );
}

#[test]
fn typeof_query_uses_the_flow_type_at_its_query_location() {
    assert_eq!(
        checked_rows(concat!(
            "declare let c: string | number;\n",
            "if (typeof c === \"string\") {\n",
            "    type C = { [key: string]: typeof c };\n",
            "    const bad: C = { bar: 1 };\n",
            "    void bad;\n",
            "}\n",
        )),
        [(2322, 124, 3)],
    );
}

fn checked_rows(text: &str) -> Vec<(u32, u32, u32)> {
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
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

// m6 7.6: `typeof f<...>` instantiation expressions — the
// TypeQuery face of checkExpressionWithTypeArguments (60602 →
// 77963) over the 7.4-live getInstantiationExpressionType.
// tsc-probed rows, vendored 6.0.3 noLib.

#[test]
fn typeof_with_type_arguments_instantiates_the_signature() {
    // r is `(x: number) => number`; the call result feeds the
    // 2322 (primitives render in-slice).
    assert_eq!(
        checked_rows(
            "declare function f<T>(x: T): T;\ntype R = typeof f<number>;\ndeclare const r: R;\nconst n: string = r(1);\n"
        ),
        [(2322, 85, 1)]
    );
}

#[test]
fn typeof_with_empty_type_argument_list_reports_1099() {
    // checkGrammarExpressionWithTypeArguments runs on the
    // TypeQuery face (89562) — the list itself anchors the row.
    assert_eq!(
        checked_rows("declare function f<T>(x: T): T;\ntype R = typeof f<>;\n"),
        [(1099, 49, 2)]
    );
}

#[test]
fn typeof_with_type_arguments_instantiates_object_returning_signature() {
    // Direct (non-alias) face: b is `(value: string) => { value: string }`.
    assert_eq!(
        checked_rows(
            "declare function makeBox<T>(value: T): { value: T };\ntype B = typeof makeBox<string>;\ndeclare const b: B;\nconst w: number = b(\"a\").value;\n"
        ),
        [(2322, 112, 1)]
    );
}

#[test]
fn typeof_instantiation_expression_survives_outer_alias_instantiation() {
    // BoxFunc<string> re-instantiates the InstantiationExpressionType
    // through instantiateAnonymousType's node-carrying copy
    // (63649-63651): the outer alias parameter substitutes into
    // the instantiated signature, so `.value.v` is string.
    // (Array-free on purpose — the pin harness is noLib.)
    assert_eq!(
        checked_rows(
            "declare function makeBox<T>(value: T): { value: T };\ntype BoxFunc<T> = typeof makeBox<{ v: T }>;\ntype B = BoxFunc<string>;\ndeclare const b: B;\nconst w: number = b({ v: \"a\" }).value.v;\n"
        ),
        [(2322, 149, 1)]
    );
}

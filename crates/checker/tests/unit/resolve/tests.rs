use tsc_diagnostics::DiagnosticCategory;
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{CompilerOptions, SymbolFlags, TypeFlags};

use crate::state::test_support::with_program_state;
use crate::state::CheckerState;
use crate::{check_program, check_program_with_libs, InputFile};

/// First Identifier node whose text is `text`, in allocation order.
fn identifier_named(state: &CheckerState, text: &str) -> NodeId {
    let source = state.binder.source(0);
    source
        .arena
        .node_ids()
        .find(|&id| {
            matches!(
                &source.arena.node(id).data,
                NodeData::Identifier(data) if data.escaped_text == text
            )
        })
        .expect("identifier present")
}

fn annotation_of_var(state: &CheckerState, name: &str) -> NodeId {
    crate::relpin::find_probe_annotation(state.binder.source(0), name)
        .expect("declared var with annotation")
}

#[test]
fn qualified_name_resolves_through_namespace_exports() {
    with_program_state(
        &[(
            "a.ts",
            "namespace N { export interface I { a: number } }\ndeclare var v: N.I;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let annotation = annotation_of_var(state, "v");
            let resolved = state
                .get_type_from_type_node(annotation)
                .expect("qualified interface reference resolves");
            assert!(state
                .tables
                .flags_of(resolved)
                .intersects(TypeFlags::OBJECT));
            let symbol = state
                .tables
                .type_of(resolved)
                .symbol
                .expect("interface symbol");
            assert_eq!(state.binder.symbol(symbol).escaped_name, "I");
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn inner_scope_shadows_outer() {
    with_program_state(
        &[(
            "a.ts",
            "interface I { a: number }\nfunction f() { interface I { b: string } var v: I; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            // Resolve "I" from the annotation inside f: the inner
            // interface wins.
            let annotation = annotation_of_var(state, "v");
            let symbol = state
                .resolve_name(Some(annotation), "I", SymbolFlags::TYPE, None, false, false)
                .expect("resolve_name")
                .expect("inner interface resolves");
            let declaration = state.binder.symbol(symbol).declarations[0];
            let outer = state
                .resolve_name(
                    Some(state.binder.source(0).root),
                    "I",
                    SymbolFlags::TYPE,
                    None,
                    false,
                    false,
                )
                .expect("resolve_name")
                .expect("outer interface resolves");
            assert_ne!(symbol, outer);
            // The inner declaration sits inside f's body.
            assert!(state
                .find_ancestor_of_kind(declaration, SyntaxKind::FunctionDeclaration)
                .is_some());
        },
    );
}

#[test]
fn arguments_resolves_inside_functions_only() {
    with_program_state(
        &[(
            "a.ts",
            "function f() { var n: number; }\nvar outer: string;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let inner = identifier_named(state, "n");
            let resolved = state
                .resolve_name(
                    Some(inner),
                    "arguments",
                    SymbolFlags::VARIABLE,
                    None,
                    false,
                    false,
                )
                .expect("resolve_name")
                .expect("arguments resolves inside f");
            assert_eq!(resolved, state.arguments_symbol);
            let outer = identifier_named(state, "outer");
            assert_eq!(
                state
                    .resolve_name(
                        Some(outer),
                        "arguments",
                        SymbolFlags::VARIABLE,
                        None,
                        false,
                        false,
                    )
                    .expect("resolve_name"),
                None
            );
        },
    );
}

#[test]
fn class_type_parameter_resolves_in_members() {
    with_program_state(
        &[("a.ts", "class C<T> { m(v: T): void {} }\n")],
        &CompilerOptions::default(),
        |state| {
            // From the parameter annotation inside m, T resolves to
            // the class's type parameter through the class-members
            // arm.
            let v = identifier_named(state, "v");
            let symbol = state
                .resolve_name(
                    Some(v),
                    "T",
                    SymbolFlags::TYPE_PARAMETER,
                    None,
                    false,
                    false,
                )
                .expect("resolve_name")
                .expect("class type parameter resolves");
            assert_eq!(
                state.kind_of(state.binder.symbol(symbol).declarations[0]),
                SyntaxKind::TypeParameter
            );
        },
    );
}

#[test]
fn const_resolution_inside_const_assertion_returns_none() {
    with_program_state(
        &[("a.ts", "var v = 1 as const;\n")],
        &CompilerOptions::default(),
        |state| {
            let source = state.binder.source(0);
            let as_expression = source
                .arena
                .node_ids()
                .find(|&id| source.arena.node(id).kind == SyntaxKind::AsExpression)
                .expect("as expression");
            assert_eq!(
                state
                    .resolve_name(
                        Some(as_expression),
                        "const",
                        SymbolFlags::TYPE,
                        None,
                        false,
                        false
                    )
                    .expect("resolve_name"),
                None
            );
        },
    );
}

#[test]
fn missing_name_with_message_emits_plain_2304() {
    with_program_state(
        &[("a.ts", "var v: number;\n")],
        &CompilerOptions::default(),
        |state| {
            let v = identifier_named(state, "v");
            let message = state.cannot_find_name_diagnostic_for_name(v);
            let resolved = state
                .resolve_name(
                    Some(v),
                    "nope",
                    SymbolFlags::VALUE,
                    Some(message),
                    true,
                    false,
                )
                .expect("resolve_name");
            assert_eq!(resolved, None);
            let codes: Vec<u32> = state.diagnostics.iter().map(|d| d.code()).collect();
            assert_eq!(codes, [2304]);
        },
    );
}

#[test]
fn unchecked_js_spelling_rows_publish_as_suggestions() {
    let lib_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../vendor/typescript-6.0.3/lib/lib.es5.d.ts"
    );
    let result = check_program_with_libs(
        &[InputFile::new(
            "lib.es5.d.ts".to_owned(),
            std::fs::read_to_string(lib_path).expect("vendored lib.es5.d.ts"),
        )],
        &[InputFile::new(
            "a.js".to_owned(),
            "export var inModule = 1;\n\
                   inmodule.toFixed();\n\
                   var object = { spaaace: 3 };\n\
                   object.spaace;\n"
                .to_owned(),
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
            .map(|diagnostic| (
                diagnostic.code(),
                diagnostic.category(),
                diagnostic.related.len(),
            ))
            .collect::<Vec<_>>(),
        [
            (2570, DiagnosticCategory::Suggestion, 0),
            (2568, DiagnosticCategory::Suggestion, 1),
        ]
    );
}

#[test]
fn value_only_symbol_reports_2749_in_type_position() {
    let result = check_program(
        &[InputFile::new(
            "a.ts".to_owned(),
            "const value = 1;\ntype Bad = value;\nclass Both {}\ntype Good = Both;\n".to_owned(),
        )],
        &CompilerOptions::default(),
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
        [(2749, 28, 5)]
    );
}

#[test]
fn type_only_symbol_reports_the_type_as_value_alternate() {
    let result = check_program(
        &[InputFile::new(
            "a.ts".to_owned(),
            "interface Only {}\nOnly;\nclass Both {}\nBoth;\ninterface Promise {}\nPromise;\n"
                .to_owned(),
        )],
        &CompilerOptions::default(),
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [2693, 2585]
    );
}

#[test]
fn class_and_interface_import_alias_targets_report_2702() {
    let result = check_program(
        &[InputFile::new(
            "a.ts".to_owned(),
            "class C {}\nimport c = C;\ninterface I {}\nimport i = I;\n".to_owned(),
        )],
        &CompilerOptions::default(),
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [2702, 2702]
    );
}

#[test]
fn value_only_namespace_roots_fall_through_to_2503() {
    let result = check_program(
        &[InputFile::new(
            "a.ts".to_owned(),
            "var V = 1;\n\
                   import v = V;\n\
                   declare namespace lf {\n\
                     interface Transaction {\n\
                       attach(query: query.Builder): void;\n\
                     }\n\
                   }\n"
            .to_owned(),
        )],
        &CompilerOptions::default(),
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [2503, 2503]
    );
}

#[test]
fn empty_internal_namespaces_resolve_through_cross_file_alias_collisions() {
    let result = check_program(
        &[
            InputFile::new(
                "a.ts".to_owned(),
                "namespace P { }\nimport p = P;\nvar q;\n".to_owned(),
            ),
            InputFile::new(
                "b.ts".to_owned(),
                "namespace Q { }\nimport q = Q;\nvar p;\n".to_owned(),
            ),
        ],
        &CompilerOptions::default(),
    );
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
}

#[test]
fn empty_internal_namespace_resolves_for_a_same_file_alias() {
    let result = check_program(
        &[InputFile::new(
            "a.ts".to_owned(),
            "namespace P { }\nimport p = P;\n".to_owned(),
        )],
        &CompilerOptions::default(),
    );
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
}

#[test]
fn internal_import_equals_qualified_name_uses_namespace_meaning_without_a_value_error() {
    let source = concat!(
        "namespace x {\n",
        "    interface c {\n",
        "    }\n",
        "}\n",
        "declare export import a = x.c;\n",
        "var b: a;\n",
    );
    let result = check_program(
        &[InputFile::new("/main.ts".to_owned(), source.to_owned())],
        &CompilerOptions::default(),
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| matches!(diagnostic.code(), 1029 | 2694 | 2708))
            .map(|diagnostic| (
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
            ))
            .collect::<Vec<_>>(),
        [(1029, 48, 6), (2694, 68, 1)]
    );
}

#[test]
fn namespace_used_directly_reports_value_and_type_alternates() {
    let result = check_program(
        &[InputFile::new(
            "a.ts".to_owned(),
            "namespace N { export interface I {} }\nN;\ntype T = N;\n".to_owned(),
        )],
        &CompilerOptions::default(),
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [2708, 2709]
    );
}

#[test]
fn external_module_umd_value_reference_is_error_or_suggestion() {
    let files = [
        InputFile::new(
            "umd.d.ts".to_owned(),
            "export as namespace Foo;\nexport const value: number;\n".to_owned(),
        ),
        InputFile::new(
            "a.ts".to_owned(),
            "export {};\nconst value = Foo;\n".to_owned(),
        ),
    ];
    let categories = |options: &CompilerOptions| {
        check_program(&files, options)
            .diagnostics
            .into_iter()
            .filter(|diagnostic| diagnostic.code() == 2686)
            .map(|diagnostic| diagnostic.category())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        categories(&CompilerOptions::default()),
        [DiagnosticCategory::Error]
    );
    assert_eq!(
        categories(&CompilerOptions {
            allow_umd_global_access: Some(true),
            ..CompilerOptions::default()
        }),
        [DiagnosticCategory::Suggestion]
    );
    let suppressed = check_program(
        &[
            InputFile::new(
                "umd.d.ts".to_owned(),
                "export as namespace Foo;\nexport const value: number;\n".to_owned(),
            ),
            InputFile::new(
                "a.ts".to_owned(),
                "export {};\n// @ts-ignore\nconst value = Foo;\n".to_owned(),
            ),
        ],
        &CompilerOptions::default(),
    );
    assert!(!suppressed
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code() == 2686));
}

#[test]
fn type_only_alias_value_uses_report_origin_and_preserve_type_sites() {
    let result = check_program(
        &[
            InputFile::new("/a.ts".to_owned(), "export class A {}\n".to_owned()),
            InputFile::new(
                "/b.ts".to_owned(),
                "import type { A } from './a';\n\
                       new A();\n\
                       const shorthand = { A };\n\
                       type T = A;\n\
                       type Q = typeof A;\n\
                       interface I extends A {}\n\
                       class C implements A {}\n"
                    .to_owned(),
            ),
        ],
        &CompilerOptions::default(),
    );
    let diagnostics = result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code() == 1361)
        .collect::<Vec<_>>();
    assert_eq!(diagnostics.len(), 2);
    assert!(diagnostics.iter().all(|diagnostic| {
        diagnostic.file_name.as_deref() == Some("/b.ts")
            && diagnostic.message_text()
                == "'A' cannot be used as a value because it was imported using 'import type'."
            && diagnostic.related.len() == 1
            && diagnostic.related[0].file_name.as_deref() == Some("/b.ts")
            && diagnostic.related[0].message.code == 1376
    }));
}

#[test]
fn checked_js_type_only_export_value_use_is_explicitly_published() {
    let result = check_program(
        &[
            InputFile::new("/a.js".to_owned(), "export class A {}\n".to_owned()),
            InputFile::new("/b.js".to_owned(), "export type * from './a';\n".to_owned()),
            InputFile::new(
                "/c.js".to_owned(),
                "import { A } from './b';\nA;\n".to_owned(),
            ),
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            module: Some(1),
            target: Some(2),
            ..CompilerOptions::default()
        },
    );
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code() == 1362)
        .expect("checked-JS TS1362");
    assert_eq!(
        (
            diagnostic.file_name.as_deref(),
            diagnostic.message_text(),
            diagnostic.related.len(),
            diagnostic.related[0].file_name.as_deref(),
            diagnostic.related[0].message.code,
        ),
        (
            Some("/c.js"),
            "'A' cannot be used as a value because it was exported using 'export type'.",
            1,
            Some("/b.js"),
            1377,
        )
    );
}

#[test]
fn qualified_value_only_symbol_reports_2749_for_the_full_name() {
    let result = check_program(
        &[InputFile::new("a.ts".to_owned(), "interface Object {}\nnamespace N { export const value = 1; export interface Both {} }\ntype Bad = N.value;\ntype Good = N.Both;\n"
                .to_owned())],
        &CompilerOptions::default(),
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
        [(2749, 96, 7)]
    );
}

#[test]
fn qualified_enum_value_property_reports_2749_via_its_value_type() {
    let text = "interface Object { toString(): string }\n\
                enum Color { Red }\n\
                type Bad = Color.Red.toString;\n\
                type Good = typeof Color.Red.toString;\n";
    let result = check_program(
        &[InputFile::new("a.ts".to_owned(), text.to_owned())],
        &CompilerOptions::default(),
    );
    let diagnostics = result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code() == 2749)
        .map(|diagnostic| {
            (
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
                diagnostic.message_text().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    let start = text.find("Color.Red.toString;").expect("bad type name") as u32;
    assert_eq!(
        diagnostics,
        [(
            start,
            "Color.Red.toString".len() as u32,
            "'Color.Red.toString' refers to a value, but is being used as a type here. Did you mean 'typeof Color.Red.toString'?".to_owned(),
        )]
    );
}

#[test]
fn mixed_checked_js_keeps_value_misses_but_shields_jsdoc_type_misses() {
    let result = check_program(
        &[
            InputFile::new("a.js".to_owned(), "missingValue;\n/** @typedef {{ nested: { value: number } }} Hidden */\nCtor.prototype = {};\n"
                    .to_owned()),
            InputFile::new("b.ts".to_owned(), "type T = Hidden;\nmissingTs;\n".to_owned()),
        ],
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
                diagnostic.file_name.as_deref().unwrap_or_default(),
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
            ))
            .collect::<Vec<_>>(),
        [("a.js", 2304, 0), ("b.ts", 2304, 17)]
    );
}

#[test]
fn diagnostics_preserve_escaped_identifier_spelling() {
    let result = check_program(
        &[InputFile::new(
            "a.ts".to_owned(),
            "let \\u0078x: number;\n\\u0078x;\n\\u005F01234;\n".to_owned(),
        )],
        &CompilerOptions {
            strict_null_checks: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| matches!(diagnostic.code(), 2304 | 2454))
            .map(|diagnostic| (diagnostic.code(), diagnostic.message_text()))
            .collect::<Vec<_>>(),
        [
            (2454, "Variable '\\u0078x' is used before being assigned."),
            (2304, "Cannot find name '\\u005F01234'."),
        ]
    );
}

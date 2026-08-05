use crate::state::test_support::with_program_state;
use crate::{check_program, check_program_with_libs_at, CompilerOptions, InputFile};
use tsc_diagnostics::{gen as diagnostics, DiagnosticCategory, MessageChain};
use tsc_syntax::{NodeData, SyntaxKind};

fn internal_import_reference_state(tail: &str, options: &CompilerOptions) -> (bool, bool) {
    let text =
        format!("namespace N {{ export const x = 1; }}\nimport A = N;\nexport {{}};\n{tail}\n");
    with_program_state(&[("a.ts", &text)], options, |state| {
        state.check_source_file(0);
        let root = state.binder.source(0).root;
        let statements = match state.data_of(root) {
            NodeData::SourceFile(data) => data.statements,
            _ => unreachable!("root is a source file"),
        };
        let declaration = state
            .nodes_of(statements)
            .into_iter()
            .find(|&node| state.kind_of(node) == SyntaxKind::ImportEqualsDeclaration)
            .expect("internal import declaration");
        let symbol = state
            .get_symbol_of_declaration(declaration)
            .expect("internal import symbol");
        let links = state.links.symbol(symbol);
        (links.alias_referenced, !links.is_referenced.is_empty())
    })
}

#[test]
fn alias_reference_marks_are_separate_and_follow_use_sites() {
    let options = CompilerOptions::default();
    assert_eq!(
        internal_import_reference_state("A.x;", &options),
        (true, true),
        "a property access marks both alias accessibility and ordinary symbol use"
    );
    assert_eq!(
        internal_import_reference_state("A;", &options),
        (true, true),
        "a direct identifier use marks both reference channels"
    );
    assert_eq!(
        internal_import_reference_state("", &options),
        (false, false),
        "a declaration alone marks neither channel"
    );
    assert_eq!(
        internal_import_reference_state(
            "A.x;",
            &CompilerOptions {
                verbatim_module_syntax: Some(true),
                ..CompilerOptions::default()
            },
        ),
        (false, true),
        "verbatimModuleSyntax disables alias accessibility collection only"
    );
}

#[test]
fn exported_import_equals_marks_alias_accessibility_without_a_use() {
    let text = "namespace N { export const x = 1; }\nexport import A = N;\n";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        let root = state.binder.source(0).root;
        let statements = match state.data_of(root) {
            NodeData::SourceFile(data) => data.statements,
            _ => unreachable!("root is a source file"),
        };
        let declaration = state
            .nodes_of(statements)
            .into_iter()
            .find(|&node| state.kind_of(node) == SyntaxKind::ImportEqualsDeclaration)
            .expect("exported internal import");
        let symbol = state
            .get_symbol_of_declaration(declaration)
            .expect("exported import symbol");
        let links = state.links.symbol(symbol);
        assert!(links.alias_referenced);
        assert!(links.is_referenced.is_empty());
    });
}

/// Driver-level multi-file rows: (file, code, start, length) for
/// located diagnostics (noLib artifacts are locationless and drop
/// with the filter — the calls.rs checked_rows discipline).
fn program_rows(files: &[(&str, &str)], options: &CompilerOptions) -> Vec<(String, u32, u32, u32)> {
    let inputs: Vec<InputFile> = files
        .iter()
        .map(|(name, text)| InputFile {
            name: (*name).to_owned(),
            text: (*text).to_owned(),
        })
        .collect();
    let result = check_program(&inputs, options);
    result
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.file_name.is_some()
                && diagnostic.category() == tsc_diagnostics::DiagnosticCategory::Error
        })
        .map(|diagnostic| {
            (
                diagnostic.file_name.clone().expect("filtered"),
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
            )
        })
        .collect()
}

fn rows(files: &[(&str, &str)]) -> Vec<(String, u32, u32, u32)> {
    program_rows(files, &CompilerOptions::default())
}

#[test]
fn deferred_import_grammar_reports_shape_before_module_mode() {
    let cases = [
        ("import defer foo from \"./a\";\n", 99, (18058, 7, 9)),
        ("import defer { foo } from \"./a\";\n", 99, (18059, 7, 13)),
        ("import defer * as ns from \"./a\";\n", 1, (18060, 7, 13)),
    ];
    for (source, module, expected) in cases {
        let rows = program_rows(
            &[("a.ts", source)],
            &CompilerOptions {
                module: Some(module),
                ..CompilerOptions::default()
            },
        );
        let actual = rows
            .into_iter()
            .filter_map(|(_, code, start, length)| {
                matches!(code, 18058..=18060).then_some((code, start, length))
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, [expected], "source={source:?}, module={module}");
    }

    for module in [99, 200] {
        let rows = program_rows(
            &[("a.ts", "import defer * as ns from \"./a\";\n")],
            &CompilerOptions {
                module: Some(module),
                ..CompilerOptions::default()
            },
        );
        assert!(
            rows.iter()
                .all(|(_, code, _, _)| !matches!(code, 18058..=18060)),
            "module={module}: {rows:?}"
        );
    }
}

fn targeted_rows(
    result: &crate::CheckResult,
    codes: &[u32],
) -> Vec<(String, u32, u32, u32, String)> {
    let mut rows = result
        .diagnostics
        .iter()
        .filter(|diagnostic| codes.contains(&diagnostic.code()))
        .map(|diagnostic| {
            (
                diagnostic
                    .file_name
                    .clone()
                    .expect("targeted row is located"),
                diagnostic.code(),
                diagnostic.start.expect("targeted row has a start"),
                diagnostic.length.expect("targeted row has a length"),
                diagnostic.message_text().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    rows.sort();
    rows
}

#[test]
fn import_specifiers_report_direct_and_intermediate_deprecated_aliases() {
    let main = "import { old as via } from \"./b\";\n\
                    import { old as direct } from \"./direct\";\n";
    let result = check_program(
        &[
            InputFile {
                name: "/a.js".to_owned(),
                text: "export const current = 1;\n".to_owned(),
            },
            InputFile {
                name: "/b.js".to_owned(),
                text: "export { /** @deprecated use current */ current as old } from \"./a\";\n"
                    .to_owned(),
            },
            InputFile {
                name: "/direct.js".to_owned(),
                text: "/** @deprecated use current */\nexport const old = 1;\n".to_owned(),
            },
            InputFile {
                name: "/main.js".to_owned(),
                text: main.to_owned(),
            },
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    let rows = result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code() == 6385)
        .map(|diagnostic| {
            (
                diagnostic.file_name.as_deref(),
                diagnostic.start,
                diagnostic.length,
                diagnostic.category(),
                diagnostic.message_text(),
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
                Some("/main.js"),
                Some(main.find("old as via").expect("intermediate alias") as u32),
                Some("old as via".len() as u32),
                DiagnosticCategory::Suggestion,
                "'old' is deprecated.",
                vec![2798],
            ),
            (
                Some("/main.js"),
                Some(main.find("old as direct").expect("direct alias") as u32),
                Some("old as direct".len() as u32),
                DiagnosticCategory::Suggestion,
                "'old' is deprecated.",
                vec![2798],
            ),
        ]
    );
}

#[test]
fn unresolved_side_effect_import_reports_2882_unless_explicitly_disabled() {
    let files = [("a.ts", "import \"x\";\n")];
    assert_eq!(rows(&files), [("a.ts".to_owned(), 2882, 7, 3)]);
    assert!(program_rows(
        &files,
        &CompilerOptions {
            no_unchecked_side_effect_imports: Some(false),
            ..CompilerOptions::default()
        },
    )
    .is_empty());
}

#[test]
fn import_and_export_declaration_modifiers_report_dedicated_grammar_rows() {
    let source = "export import 'fs'\nexport export { C }\n";
    assert_eq!(
        program_rows(
            &[("a.js", source)],
            &CompilerOptions {
                allow_js: true,
                ..CompilerOptions::default()
            },
        ),
        [
            ("a.js".to_owned(), 1191, 0, 6),
            (
                "a.js".to_owned(),
                1193,
                source.find("export export").expect("export modifier") as u32,
                6,
            ),
        ]
    );
}

#[test]
fn specific_modifier_error_suppresses_import_and_export_follower_rows() {
    let source = "async import 'assert'\nasync export { C }\n";
    assert_eq!(
        program_rows(
            &[("a.js", source)],
            &CompilerOptions {
                allow_js: true,
                ..CompilerOptions::default()
            },
        ),
        [
            ("a.js".to_owned(), 1042, 0, 5),
            (
                "a.js".to_owned(),
                1042,
                source.find("async export").expect("export modifier") as u32,
                5,
            ),
        ]
    );
}

#[test]
fn unmodified_import_and_export_declarations_do_not_report_modifier_rows() {
    let diagnostics = program_rows(
        &[("a.js", "import 'present';\nexport {};\n")],
        &CompilerOptions {
            allow_js: true,
            ..CompilerOptions::default()
        },
    );
    assert!(
        diagnostics
            .iter()
            .all(|(_, code, _, _)| !matches!(code, 1191 | 1193)),
        "{diagnostics:?}"
    );
}

fn node16_options() -> CompilerOptions {
    CompilerOptions {
        module: Some(100),
        target: Some(9),
        ..CompilerOptions::default()
    }
}

#[test]
fn package_exports_conditions_match_tsc_condition_sets() {
    let files = [
        (
            "/node_modules/conditions/package.json",
            r#"{ "name": "conditions", "type": "module", "exports": {
                    ".": { "node": "./node.js", "default": "./web.js" }
                } }"#,
        ),
        (
            "/node_modules/conditions/node.d.ts",
            "export const node: number;\n",
        ),
        (
            "/node_modules/conditions/web.d.ts",
            "export const web: number;\n",
        ),
        (
            "/node_modules/versioned/package.json",
            r#"{ "name": "versioned", "exports": {
                    ".": {
                        "types@>=10000": "./future.d.ts",
                        "types@>=1": "./current.d.ts",
                        "types": "./old.d.ts"
                    }
                } }"#,
        ),
        (
            "/node_modules/versioned/future.d.ts",
            "export const future: number;\n",
        ),
        (
            "/node_modules/versioned/current.d.ts",
            "export const current: number;\n",
        ),
        (
            "/node_modules/versioned/old.d.ts",
            "export const old: number;\n",
        ),
        (
            "/node_modules/custom/package.json",
            r#"{ "name": "custom", "exports": {
                    ".": { "browser": "./browser.d.ts", "default": "./default.d.ts" }
                } }"#,
        ),
        (
            "/node_modules/custom/browser.d.ts",
            "export const browser: number;\n",
        ),
        (
            "/node_modules/custom/default.d.ts",
            "export const fallback: number;\n",
        ),
        (
            "/main.mts",
            "import { web } from \"conditions\";\n\
                 import { current } from \"versioned\";\n\
                 import { browser } from \"custom\";\n\
                 web; current; browser;\n",
        ),
    ];
    let bundler = program_rows(
        &files,
        &CompilerOptions {
            module: Some(99),
            module_resolution: Some(100),
            custom_conditions: Some(vec!["browser".to_owned()]),
            ..CompilerOptions::default()
        },
    );
    assert!(
        bundler
            .iter()
            .all(|(_, code, _, _)| !matches!(code, 2305 | 2339 | 2551)),
        "{bundler:?}"
    );

    let node = program_rows(&files, &node16_options());
    assert!(
        node.iter()
            .any(|(file, code, _, _)| file == "/main.mts" && *code == 2305),
        "Node conditions include `node`, unlike Bundler: {node:?}"
    );
}

#[test]
fn checked_cjs_default_import_reports_non_default_export_in_node_modes() {
    let files = [
        ("/1.cjs", "module.exports = {};\n"),
        ("/2.cjs", "exports.foo = 0;\n"),
        ("/3.cjs", "import \"foo\";\nexports.foo = {};\n"),
        ("/4.cjs", ";\n"),
        (
            "/5.cjs",
            "import two from \"./2.cjs\";   // ok\n\
                 import three from \"./3.cjs\"; // error\n\
                 two.foo;\n\
                 three.foo;\n",
        ),
    ];
    for module in [101, 102, 199] {
        let inputs: Vec<InputFile> = files
            .iter()
            .map(|(name, text)| InputFile {
                name: (*name).to_owned(),
                text: (*text).to_owned(),
            })
            .collect();
        let result = check_program(
            &inputs,
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                target: Some(9),
                module: Some(module),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            targeted_rows(&result, &[1192]),
            [(
                "/5.cjs".to_owned(),
                1192,
                42,
                5,
                "Module '\"/3\"' has no default export.".to_owned(),
            )],
            "module={module}"
        );
    }
}

/// Oracle pins (tsc 6.0.3, scratchpad probe58d/pins, 2026-07-15).
#[test]
fn not_a_module_and_missing_member_report_2306_and_2305() {
    let files = [
            ("script.ts", "var g = 1;\n"),
            ("other.ts", "export const yes = 1;\n"),
            (
                "amb.d.ts",
                "declare module \"amb\" { export const a: number; }\n",
            ),
            (
                "main.ts",
                "import { a } from \"amb\";\nimport * as s from \"./script\";\nimport { nope } from \"./other\";\na; s; nope;\n",
            ),
        ];
    assert_eq!(
        rows(&files),
        [
            ("main.ts".to_owned(), 2306, 44, 10),
            ("main.ts".to_owned(), 2305, 65, 4),
        ]
    );
}

#[test]
fn missing_module_member_diagnostics_use_written_specifiers_and_names() {
    let missing = check_program(
        &[
            InputFile {
                name: "/mod.ts".to_owned(),
                text: "export const present = 1;\n".to_owned(),
            },
            InputFile {
                name: "/main.ts".to_owned(),
                text: "import { \"missing\" as x, absent } from \"./mod.js\";\n".to_owned(),
            },
        ],
        &CompilerOptions::default(),
    );
    let messages = targeted_rows(&missing, &[2305])
        .into_iter()
        .map(|row| row.4)
        .collect::<Vec<_>>();
    assert_eq!(
        messages,
        [
            "Module '\"./mod.js\"' has no exported member '\"missing\"'.".to_owned(),
            "Module '\"./mod.js\"' has no exported member 'absent'.".to_owned(),
        ]
    );

    let default_only = check_program(
        &[
            InputFile {
                name: "/default.ts".to_owned(),
                text: "export default 1;\n".to_owned(),
            },
            InputFile {
                name: "/main.ts".to_owned(),
                text: "import { Oops } from \"./default.js\";\n".to_owned(),
            },
        ],
        &CompilerOptions::default(),
    );
    assert_eq!(
            targeted_rows(&default_only, &[2614])
                .into_iter()
                .map(|row| row.4)
                .collect::<Vec<_>>(),
            [
                "Module '\"./default.js\"' has no exported member 'Oops'. Did you mean to use 'import Oops from \"./default.js\"' instead?".to_owned(),
            ]
        );

    let pattern_ambient = check_program(
        &[
            InputFile {
                name: "/ambient.d.ts".to_owned(),
                text: "declare module \"*.foo\" { export const present: number; }\n".to_owned(),
            },
            InputFile {
                name: "/main.ts".to_owned(),
                text: "import { absent } from \"b.foo\";\n".to_owned(),
            },
        ],
        &CompilerOptions::default(),
    );
    assert_eq!(
        targeted_rows(&pattern_ambient, &[2305])
            .into_iter()
            .map(|row| row.4)
            .collect::<Vec<_>>(),
        ["Module '\"*.foo\"' has no exported member 'absent'.".to_owned()]
    );
}

#[test]
fn source_file_module_symbols_use_normalized_host_names() {
    let result = check_program(
        &[
            InputFile {
                name: "t4.ts".to_owned(),
                text: "export const value = 1;\n".to_owned(),
            },
            InputFile {
                name: "foo.ts".to_owned(),
                text: "export interface Present {}\n".to_owned(),
            },
            InputFile {
                name: "export-equals.ts".to_owned(),
                text: "declare const value: number;\nexport = value;\n".to_owned(),
            },
            InputFile {
                name: "main.ts".to_owned(),
                text: "import missingDefault from \"./t4\";\n\
                           import foo = require(\"./foo\");\n\
                           export * from \"./export-equals\";\n\
                           let value: foo.Missing;\n"
                    .to_owned(),
            },
        ],
        &CompilerOptions::default(),
    );
    assert_eq!(
        targeted_rows(&result, &[1192, 2498, 2694])
            .into_iter()
            .map(|row| (row.1, row.4))
            .collect::<Vec<_>>(),
        [
            (1192, "Module '\"/t4\"' has no default export.".to_owned(),),
            (
                2498,
                "Module '\"/export-equals\"' uses 'export =' and cannot be used with 'export *'."
                    .to_owned(),
            ),
            (
                2694,
                "Namespace '\"/foo\"' has no exported member 'Missing'.".to_owned(),
            ),
        ]
    );
}

#[test]
fn js_binding_pattern_checks_require_after_optional_container_symbol_probe() {
    let source = "const { SomeClass } = require('./missing');\n";
    assert_eq!(
        program_rows(
            &[("/main.js", source)],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        ),
        [(
            "/main.js".to_owned(),
            2307,
            source.find("'./missing'").expect("module specifier") as u32,
            "'./missing'".len() as u32,
        )]
    );
}

#[test]
fn checked_js_publishes_not_a_module_from_default_lib_collision() {
    let source = "const { SomeClass } = require('./lib');\n";
    let libs = [InputFile {
        name: "lib.d.ts".to_owned(),
        text: "interface DefaultLibraryFace {}\n".to_owned(),
    }];
    let files = [
        InputFile {
            name: "lib.js".to_owned(),
            text: "class SomeClass {}\nmodule.exports = { SomeClass };\n".to_owned(),
        },
        InputFile {
            name: "main.js".to_owned(),
            text: source.to_owned(),
        },
    ];
    let result = check_program_with_libs_at(
        &libs,
        &files,
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
        "/",
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .find(|diagnostic| {
                diagnostic.category() == tsc_diagnostics::DiagnosticCategory::Error
            })
            .expect("semantic module diagnostic")
            .message_text(),
        "File '/lib.d.ts' is not a module."
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.file_name.is_some()
                    && diagnostic.category() == tsc_diagnostics::DiagnosticCategory::Error
            })
            .map(|diagnostic| (
                diagnostic.file_name.clone().expect("filtered"),
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
            ))
            .collect::<Vec<_>>(),
        [(
            "main.js".to_owned(),
            2306,
            source.find("'./lib'").expect("module specifier") as u32,
            "'./lib'".len() as u32,
        )]
    );
}

#[test]
fn checked_js_commonjs_require_uses_module_members_and_readonly_exports() {
    let importer = "const mod = require('./mod1');\nmod.missing;\nmod.readonly = 1;\n";
    let files = [
            (
                "/globals.d.ts",
                "declare var Object: { defineProperty(target: any, name: string, descriptor: any): any };\n",
            ),
            (
                "/mod1.js",
                "Object.defineProperty(exports, \"readonly\", { value: 1 });\n\
/** @type {string} */\n\
let unrelated = \"\";\n",
            ),
            ("/importer.js", importer),
        ];
    let result = check_program(
        &files
            .into_iter()
            .map(|(name, text)| InputFile {
                name: name.to_owned(),
                text: text.to_owned(),
            })
            .collect::<Vec<_>>(),
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(true),
            target: Some(2),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.category() == tsc_diagnostics::DiagnosticCategory::Error
            })
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
                Some("/importer.js"),
                2339,
                importer.find("missing").expect("missing access") as u32,
                "missing".len() as u32,
                "Property 'missing' does not exist on type 'typeof import(\"/mod1\")'.",
            ),
            (
                Some("/importer.js"),
                2540,
                importer.find("readonly =").expect("readonly assignment") as u32,
                "readonly".len() as u32,
                "Cannot assign to 'readonly' because it is a read-only property.",
            ),
        ]
    );
}

#[test]
fn checked_js_commonjs_require_property_alias_publishes_nested_object_miss() {
    let importer = "const x = require('./ch').x;\n\
                        x;\n\
                        x.grey;\n\
                        x.x.grey;\n";
    let result = check_program(
        &[
            InputFile {
                name: "/ch.js".to_owned(),
                text: "const x = { grey: {} };\nexport { x };\n".to_owned(),
            },
            InputFile {
                name: "/main.js".to_owned(),
                text: importer.to_owned(),
            },
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(true),
            module: Some(1),
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
            Some("/main.js"),
            2339,
            importer.find("x.x").expect("nested missing access") as u32 + 2,
            1,
            "Property 'x' does not exist on type '{ grey: {}; }'.",
        )]
    );
}

#[test]
fn plain_js_nested_object_is_closed_for_typescript_consumers() {
    let consumer = "obj.property.a = 1;\n";
    let result = check_program(
        &[
            InputFile {
                name: "/a.js".to_owned(),
                text: "var obj = { property: {} };\nobj.property.a = 0;\n".to_owned(),
            },
            InputFile {
                name: "/b.ts".to_owned(),
                text: consumer.to_owned(),
            },
        ],
        &CompilerOptions {
            allow_js: true,
            target: Some(2),
            ..CompilerOptions::default()
        },
    );
    // TypeScript 6.0.3 keeps the nested object-literal member
    // closed across the JS-to-TS boundary under its implicit
    // strict defaults; the earlier no-diagnostic expectation came
    // from the removed local memberless-JS admission heuristic.
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
            Some("/b.ts"),
            2339,
            consumer.find('a').expect("closed nested member access") as u32,
            1,
            "Property 'a' does not exist on type '{}'.",
        )]
    );
}

#[test]
fn checked_js_local_require_function_is_not_commonjs_resolution() {
    let source = "function require() { return {}; }\nrequire('./missing-module-for-local-call');\n";
    assert!(program_rows(
        &[("/main.js", source)],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    )
    .iter()
    .all(|(_, code, _, _)| *code != 2307));
}

#[test]
fn checked_js_require_contains_cross_file_duplicate_export_flow() {
    let files = [
            (
                "/mod.js",
                "exports.apply = undefined;\nexports.apply = function apply() {};\nexports.apply = 1;\n",
            ),
            (
                "/main.js",
                "const { apply } = require('./mod');\napply.toFixed();\n",
            ),
        ];
    assert!(program_rows(
        &files,
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(true),
            ..CompilerOptions::default()
        },
    )
    .iter()
    .all(|(_, code, _, _)| *code != 18048));
}

#[test]
fn checked_js_duplicate_commonjs_export_alias_uses_exporting_file_end_flow() {
    let files = [
        (
            "/lib.d.ts",
            "interface Number { toFixed(): string; }\n\
                 interface String { toUpperCase(): string; }\n",
        ),
        (
            "/mod.js",
            "exports.apply = undefined;\n\
                 exports.apply = undefined;\n\
                 function a() {}\n\
                 exports.apply = a;\n\
                 exports.apply();\n\
                 exports.apply = 'ok';\n\
                 var OK = exports.apply.toUpperCase();\n\
                 exports.apply = 1;\n",
        ),
        (
            "/main.js",
            "const { apply } = require('./mod');\n\
                 const result = apply.toFixed();\n",
        ),
    ];
    assert_eq!(
        program_rows(
            &files,
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                strict: Some(true),
                target: Some(2),
                ..CompilerOptions::default()
            },
        ),
        [],
        "the importing alias sees the final number assignment, not the export's union"
    );
}

#[test]
fn checked_js_duplicate_commonjs_export_alias_keeps_undefined_final_assignment() {
    let consumer = "const { apply } = require('./mod');\napply.toFixed();\n";
    let rows = program_rows(
        &[
            ("/lib.d.ts", "interface Number { toFixed(): string; }\n"),
            (
                "/mod.js",
                "exports.apply = 1;\nexports.apply = undefined;\n",
            ),
            ("/main.js", consumer),
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(true),
            target: Some(2),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        rows,
        [(
            "/main.js".to_owned(),
            18048,
            consumer.rfind("apply").expect("consumer use") as u32,
            "apply".len() as u32,
        )],
        "the end-flow branch must not degrade duplicated exports to any"
    );
}

#[test]
fn export_assignment_rows_1203_2309_1202() {
    let files = [
        ("m.ts", "const x = 1;\nexport = x;\nexport const y = 2;\n"),
        ("main.ts", "import m = require(\"./m\");\nm;\n"),
    ];
    assert_eq!(
        rows(&files),
        [
            ("m.ts".to_owned(), 1203, 13, 11),
            ("m.ts".to_owned(), 2309, 13, 11),
            ("main.ts".to_owned(), 1202, 0, 26),
        ]
    );
}

#[test]
fn verbatim_export_assignments_distinguish_type_and_type_only_values() {
    let common_js_files = [
        ("/a.ts", "interface I {}\nexport = I;\n"),
        (
            "/c.ts",
            "interface I {}\nnamespace I { export const x = 1; }\nexport = I;\n",
        ),
        (
            "/d.ts",
            "import I = require(\"./c\");\nimport type J = require(\"./c\");\nexport = J;\n",
        ),
    ];
    assert_eq!(
        program_rows(
            &common_js_files,
            &CompilerOptions {
                module: Some(1),
                target: Some(99),
                module_resolution: Some(100),
                verbatim_module_syntax: Some(true),
                ..CompilerOptions::default()
            },
        ),
        [
            ("/a.ts".to_owned(), 1282, 24, 1),
            ("/d.ts".to_owned(), 1283, 68, 1),
        ]
    );

    let es_module_files = [
        ("/main5.ts", "export default class C {}\n"),
        ("/main6.ts", "interface I {}\nexport default I;\n"),
        (
            "/main7.ts",
            "import type C from \"./main5\";\nexport default C;\n",
        ),
    ];
    assert_eq!(
        program_rows(
            &es_module_files,
            &CompilerOptions {
                module: Some(99),
                target: Some(2),
                module_resolution: Some(100),
                verbatim_module_syntax: Some(true),
                ..CompilerOptions::default()
            },
        ),
        [
            ("/main6.ts".to_owned(), 1284, 30, 1),
            ("/main7.ts".to_owned(), 1285, 45, 1),
        ]
    );
}

#[test]
fn verbatim_commonjs_export_default_reports_the_whole_assignment() {
    let source = "interface I {}\nexport default I;\n";
    let result = check_program(
        &[InputFile {
            name: "/main.ts".to_owned(),
            text: source.to_owned(),
        }],
        &CompilerOptions {
            module: Some(1),
            target: Some(99),
            module_resolution: Some(100),
            verbatim_module_syntax: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
            targeted_rows(&result, &[1284, 1295]),
            [(
                "/main.ts".to_owned(),
                1295,
                source.find("export default").expect("export default") as u32,
                "export default I;".len() as u32,
                "ECMAScript imports and exports cannot be written in a CommonJS file under 'verbatimModuleSyntax'. Adjust the 'type' field in the nearest 'package.json' to make this file an ECMAScript module, or adjust your 'verbatimModuleSyntax', 'module', and 'moduleResolution' settings in TypeScript.".to_owned(),
            )]
        );
}

#[test]
fn verbatim_commonjs_only_reports_instantiated_namespace_export() {
    let source = "export namespace JustTypes {\n    export type T = number;\n}\n\
                      export namespace Values {\n    export const x = 1;\n}\n";
    let commonjs = CompilerOptions {
        module: Some(1),
        target: Some(99),
        module_resolution: Some(100),
        verbatim_module_syntax: Some(true),
        ..CompilerOptions::default()
    };
    let result = check_program(
        &[InputFile {
            name: "/main.ts".to_owned(),
            text: source.to_owned(),
        }],
        &commonjs,
    );
    assert_eq!(
            targeted_rows(&result, &[1287]),
            [(
                "/main.ts".to_owned(),
                1287,
                source.rfind("export namespace").expect("value namespace") as u32,
                "export".len() as u32,
                "A top-level 'export' modifier cannot be used on value declarations in a CommonJS module when 'verbatimModuleSyntax' is enabled.".to_owned(),
            )]
        );

    let esm = CompilerOptions {
        module: Some(99),
        ..commonjs
    };
    let esm_result = check_program(
        &[InputFile {
            name: "/main.ts".to_owned(),
            text: source.to_owned(),
        }],
        &esm,
    );
    assert!(targeted_rows(&esm_result, &[1287]).is_empty());
}

#[test]
fn isolated_export_assignment_reports_external_type_only_origin() {
    let inputs = [
        InputFile {
            name: "/a.ts".to_owned(),
            text: "class A {}\nexport type { A };\n".to_owned(),
        },
        InputFile {
            name: "/d.ts".to_owned(),
            text: "import { A } from \"./a\";\nexport = A;\n".to_owned(),
        },
    ];
    let result = check_program(
        &inputs,
        &CompilerOptions {
            module: Some(1),
            target: Some(2),
            isolated_modules: Some(true),
            ..CompilerOptions::default()
        },
    );
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code() == 1289)
        .expect("TS1289");
    assert_eq!(
            (
                diagnostic.file_name.as_deref(),
                diagnostic.start,
                diagnostic.length,
                diagnostic.message_text(),
            ),
            (
                Some("/d.ts"),
                Some(34),
                Some(1),
                "'A' resolves to a type-only declaration and must be marked type-only in this file before re-exporting when 'isolatedModules' is enabled. Consider using 'import type' where 'A' is imported.",
            )
        );
    assert_eq!(diagnostic.related.len(), 1);
    assert_eq!(
        (
            diagnostic.related[0].file_name.as_deref(),
            diagnostic.related[0].start,
            diagnostic.related[0].length,
            diagnostic.related[0].message.code,
        ),
        (Some("/a.ts"), Some(25), Some(1), 1377)
    );
}

#[test]
fn isolated_alias_exports_distinguish_types_from_type_only_values() {
    let a = "export type A = {};\n";
    let b = "class B {}\nexport type { B };\n";
    let d = "export { A as AA } from \"./a\";\nexport { B as BB } from \"./b\";\n";
    let inputs = [
        InputFile {
            name: "/a.ts".to_owned(),
            text: a.to_owned(),
        },
        InputFile {
            name: "/b.ts".to_owned(),
            text: b.to_owned(),
        },
        InputFile {
            name: "/d.ts".to_owned(),
            text: d.to_owned(),
        },
    ];
    let result = check_program(
        &inputs,
        &CompilerOptions {
            module: Some(99),
            target: Some(2),
            isolated_modules: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
            targeted_rows(&result, &[1205, 1448]),
            [
                (
                    "/d.ts".to_owned(),
                    1205,
                    d.find("A as AA").expect("type export") as u32,
                    "A as AA".len() as u32,
                    "Re-exporting a type when 'isolatedModules' is enabled requires using 'export type'.".to_owned(),
                ),
                (
                    "/d.ts".to_owned(),
                    1448,
                    d.find("B as BB").expect("type-only value export") as u32,
                    "B as BB".len() as u32,
                    "'B' resolves to a type-only declaration and must be re-exported using a type-only re-export when 'isolatedModules' is enabled.".to_owned(),
                ),
            ]
        );
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code() == 1448)
        .expect("TS1448");
    assert_eq!(
        diagnostic
            .related
            .iter()
            .map(|related| (
                related.file_name.as_deref(),
                related.start,
                related.length,
                related.message.code,
                related.message.text.as_str(),
            ))
            .collect::<Vec<_>>(),
        [(
            Some("/b.ts"),
            Some(b.rfind('B').expect("type-only export name") as u32),
            Some(1),
            1377,
            "'B' was exported here.",
        )]
    );
}

#[test]
fn checked_js_type_alias_imports_report_18042_on_the_imported_name() {
    let declaration = "export interface TypeOnly {}\nexport const value = 1;\n";
    let source = "import { TypeOnly, TypeOnly as Alias, value } from \"./types\";\n";
    let result = check_program(
        &[
            InputFile {
                name: "/types.d.ts".to_owned(),
                text: declaration.to_owned(),
            },
            InputFile {
                name: "/main.js".to_owned(),
                text: source.to_owned(),
            },
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            module: Some(1),
            target: Some(2),
            ..CompilerOptions::default()
        },
    );
    let first = source.find("TypeOnly").expect("first type import") as u32;
    let second = source[first as usize + 1..]
        .find("TypeOnly")
        .map(|offset| offset as u32 + first + 1)
        .expect("aliased type import");
    assert_eq!(
            targeted_rows(&result, &[18042]),
            [
                (
                    "/main.js".to_owned(),
                    18042,
                    first,
                    "TypeOnly".len() as u32,
                    "'TypeOnly' is a type and cannot be imported in JavaScript files. Use 'import(\"./types\").TypeOnly' in a JSDoc type annotation.".to_owned(),
                ),
                (
                    "/main.js".to_owned(),
                    18042,
                    second,
                    "TypeOnly".len() as u32,
                    "'TypeOnly' is a type and cannot be imported in JavaScript files. Use 'import(\"./types\").TypeOnly' in a JSDoc type annotation.".to_owned(),
                ),
            ]
        );
}

#[test]
fn jsdoc_import_aliases_resolve_named_namespace_and_default_targets() {
    let declaration = "export type Named = string;\n\
                           export type Nested = string;\n\
                           export default interface DefaultType { value: string }\n";
    let source = "/** @import { Named } from './types.d.ts' */\n\
                      /** @import * as types from './types.d.ts' */\n\
                      /** @import DefaultType from './types.d.ts' */\n\
                      /** @type {Named} */\n\
                      const named = 1;\n\
                      /** @type {types.Nested} */\n\
                      const namespaced = 2;\n\
                      /** @type {DefaultType} */\n\
                      const defaulted = { value: 3 };\n\
                      /** @returns {Named} */\n\
                      function namedReturn() { return 4; }\n";
    let result = check_program(
        &[
            InputFile {
                name: "/types.d.ts".to_owned(),
                text: declaration.to_owned(),
            },
            InputFile {
                name: "/a.js".to_owned(),
                text: source.to_owned(),
            },
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            module: Some(200),
            module_resolution: Some(100),
            target: Some(2),
            no_emit: Some(true),
            allow_importing_ts_extensions: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        targeted_rows(&result, &[2322]),
        [
            (
                "/a.js".to_owned(),
                2322,
                source.find("named =").expect("named declaration") as u32,
                "named".len() as u32,
                "Type 'number' is not assignable to type 'string'.".to_owned(),
            ),
            (
                "/a.js".to_owned(),
                2322,
                source.find("namespaced =").expect("namespace declaration") as u32,
                "namespaced".len() as u32,
                "Type 'number' is not assignable to type 'string'.".to_owned(),
            ),
            (
                "/a.js".to_owned(),
                2322,
                source.rfind("value").expect("default member") as u32,
                "value".len() as u32,
                "Type 'number' is not assignable to type 'string'.".to_owned(),
            ),
            (
                "/a.js".to_owned(),
                2322,
                source.rfind("return 4").expect("JSDoc return statement") as u32,
                "return".len() as u32,
                "Type 'number' is not assignable to type 'string'.".to_owned(),
            ),
        ]
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.category() == tsc_diagnostics::DiagnosticCategory::Error
            })
            .count(),
        4,
        "{:#?}",
        result.diagnostics
    );
}

#[test]
fn checked_js_imported_jsdoc_namespace_resolves_to_its_type_only_face() {
    let declaration = "/**\n\
                           * @namespace myTypes\n\
                           * @global\n\
                           * @type {Object<string, *>}\n\
                           */\n\
                           const myTypes = {};\n\
                           /** @typedef {string} myTypes.typeA */\n\
                           export { myTypes };\n";
    let source = "import { myTypes } from \"./types.js\";\n";
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        module: Some(1),
        target: Some(2),
        ..CompilerOptions::default()
    };
    with_program_state(
        &[("/types.js", declaration), ("/main.js", source)],
        &options,
        |state| {
            let file = state
                .node_symbol(state.binder.source(0).root)
                .expect("external module symbol");
            let exported = state.binder.symbol(file).exports["myTypes"];
            assert!(state
                .binder
                .symbol(exported)
                .flags
                .intersects(tsc_types::SymbolFlags::NAMESPACE_MODULE));
            let import = state
                .binder
                .source(1)
                .arena
                .node_ids()
                .find(|&node| state.kind_of(node) == SyntaxKind::ImportSpecifier)
                .expect("import specifier");
            let alias = state
                .get_symbol_of_declaration(import)
                .expect("import alias symbol");
            let target = state.resolve_alias(alias).expect("import target");
            assert_eq!(target, exported);
        },
    );
    let result = check_program(
        &[
            InputFile {
                name: "/types.js".to_owned(),
                text: declaration.to_owned(),
            },
            InputFile {
                name: "/main.js".to_owned(),
                text: source.to_owned(),
            },
        ],
        &options,
    );
    assert_eq!(
            targeted_rows(&result, &[18042]),
            [(
                "/main.js".to_owned(),
                18042,
                source.find("myTypes").expect("imported namespace") as u32,
                "myTypes".len() as u32,
                "'myTypes' is a type and cannot be imported in JavaScript files. Use 'import(\"./types.js\").myTypes' in a JSDoc type annotation.".to_owned(),
            )]
        );
}

#[test]
fn checked_js_type_exports_report_18043_with_automatic_export_context() {
    let source = "/** @typedef {{ x: number }} JSDocType */\n\
                      export { JSDocType };\n\
                      export { JSDocType as Alias };\n";
    let result = check_program(
        &[InputFile {
            name: "/main.js".to_owned(),
            text: source.to_owned(),
        }],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            module: Some(1),
            target: Some(2),
            ..CompilerOptions::default()
        },
    );
    let first = source.find("JSDocType };").expect("first type export") as u32;
    let second = source.find("JSDocType as").expect("aliased type export") as u32;
    assert_eq!(
        targeted_rows(&result, &[18043]),
        [
            (
                "/main.js".to_owned(),
                18043,
                first,
                "JSDocType".len() as u32,
                "Types cannot appear in export declarations in JavaScript files.".to_owned(),
            ),
            (
                "/main.js".to_owned(),
                18043,
                second,
                "JSDocType".len() as u32,
                "Types cannot appear in export declarations in JavaScript files.".to_owned(),
            ),
        ]
    );
    for diagnostic in result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code() == 18043)
    {
        assert_eq!(diagnostic.related.len(), 1);
        assert_eq!(
            (
                diagnostic.related[0].file_name.as_deref(),
                diagnostic.related[0].start,
                diagnostic.related[0].length,
                diagnostic.related[0].message.code,
                diagnostic.related[0].message.text.as_str(),
            ),
            (
                Some("/main.js"),
                Some(source.find("@typedef").expect("typedef tag") as u32),
                Some("@typedef {{ x: number }} JSDocType".len() as u32),
                18044,
                "'JSDocType' is automatically exported here.",
            )
        );
    }
}

#[test]
fn checked_js_alias_guard_does_not_fire_for_values_or_typescript() {
    let declaration = "export interface TypeOnly {}\nexport const value = 1;\n";
    let import = "import { TypeOnly, value } from \"./types\";\n";
    let result = check_program(
        &[
            InputFile {
                name: "/types.ts".to_owned(),
                text: declaration.to_owned(),
            },
            InputFile {
                name: "/main.ts".to_owned(),
                text: import.to_owned(),
            },
            InputFile {
                name: "/value.js".to_owned(),
                text: "import { value } from \"./types\";\nvalue;\n".to_owned(),
            },
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            module: Some(1),
            target: Some(2),
            ..CompilerOptions::default()
        },
    );
    assert!(
        targeted_rows(&result, &[18042, 18043]).is_empty(),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn verbatim_alias_imports_distinguish_type_only_origins() {
    let a = "export type A = {};\nexport class C {}\n";
    let b = "import { A } from \"./a\";\nexport type { C } from \"./a\";\n";
    let c = "import { C } from \"./b\";\n";
    let internal = "export {};\nnamespace Foo { export type T = any; }\nimport f = Foo.T;\n";
    let inputs = [
        InputFile {
            name: "/a.ts".to_owned(),
            text: a.to_owned(),
        },
        InputFile {
            name: "/b.ts".to_owned(),
            text: b.to_owned(),
        },
        InputFile {
            name: "/c.ts".to_owned(),
            text: c.to_owned(),
        },
        InputFile {
            name: "/internal.ts".to_owned(),
            text: internal.to_owned(),
        },
    ];
    let result = check_program(
        &inputs,
        &CompilerOptions {
            module: Some(99),
            target: Some(2),
            module_resolution: Some(100),
            verbatim_module_syntax: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
            targeted_rows(&result, &[1288, 1484, 1485]),
            [
                (
                    "/b.ts".to_owned(),
                    1484,
                    b.find('A').expect("type import") as u32,
                    1,
                    "'A' is a type and must be imported using a type-only import when 'verbatimModuleSyntax' is enabled.".to_owned(),
                ),
                (
                    "/c.ts".to_owned(),
                    1485,
                    c.find('C').expect("type-only value import") as u32,
                    1,
                    "'C' resolves to a type-only declaration and must be imported using a type-only import when 'verbatimModuleSyntax' is enabled.".to_owned(),
                ),
                (
                    "/internal.ts".to_owned(),
                    1288,
                    internal.find("import f").expect("internal import equals") as u32,
                    "import f = Foo.T;".len() as u32,
                    "An import alias cannot resolve to a type or type-only declaration when 'verbatimModuleSyntax' is enabled.".to_owned(),
                ),
            ]
        );
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code() == 1485)
        .expect("TS1485");
    assert_eq!(
        diagnostic
            .related
            .iter()
            .map(|related| (
                related.file_name.as_deref(),
                related.start,
                related.length,
                related.message.code,
            ))
            .collect::<Vec<_>>(),
        [(
            Some("/b.ts"),
            Some(b.find("C }").expect("type-only export name") as u32),
            Some(1),
            1377,
        )]
    );
}

#[test]
fn verbatim_ambient_const_enum_aliases_report_binding_and_keep_global_access() {
    let declaration = "export declare const enum E { A, B, C }\n\
                           declare global { const enum F { A, B, C } }\n";
    let importing = "import { E } from \"./pkg\";\n\
                         import type { E as _E } from \"./pkg\";\n\
                         E.A;\n\
                         F.A;\n";
    let exporting = "export { E } from \"./pkg\";\n\
                         export type { E as _E } from \"./pkg\";\n";
    let result = check_program(
        &[
            InputFile {
                name: "/pkg.d.ts".to_owned(),
                text: declaration.to_owned(),
            },
            InputFile {
                name: "/a.ts".to_owned(),
                text: importing.to_owned(),
            },
            InputFile {
                name: "/b.ts".to_owned(),
                text: exporting.to_owned(),
            },
        ],
        &CompilerOptions {
            module: Some(200),
            target: Some(99),
            verbatim_module_syntax: Some(true),
            ..CompilerOptions::default()
        },
    );
    let message = "Cannot access ambient const enums when 'verbatimModuleSyntax' is enabled.";
    assert_eq!(
        targeted_rows(&result, &[2748]),
        [
            (
                "/a.ts".to_owned(),
                2748,
                importing.find("E }").expect("import binding") as u32,
                1,
                message.to_owned(),
            ),
            (
                "/a.ts".to_owned(),
                2748,
                importing.find("F.A").expect("global access") as u32,
                1,
                message.to_owned(),
            ),
            (
                "/b.ts".to_owned(),
                2748,
                exporting.find("E }").expect("re-export binding") as u32,
                1,
                message.to_owned(),
            ),
        ]
    );
}

#[test]
fn verbatim_regular_const_enum_aliases_do_not_report_2748() {
    let result = check_program(
        &[
            InputFile {
                name: "/enum.ts".to_owned(),
                text: "export const enum E { A }\n".to_owned(),
            },
            InputFile {
                name: "/import.ts".to_owned(),
                text: "import { E } from \"./enum\";\nE.A;\n".to_owned(),
            },
            InputFile {
                name: "/export.ts".to_owned(),
                text: "export { E } from \"./enum\";\n".to_owned(),
            },
        ],
        &CompilerOptions {
            module: Some(200),
            target: Some(99),
            verbatim_module_syntax: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert!(targeted_rows(&result, &[2748]).is_empty());
}

#[test]
fn verbatim_commonjs_aliases_select_the_extension_specific_message() {
    let declaration = "export default function f() {}\nexport function named() {}\n";
    let main = "import f from \"./decl\";\nimport * as ns from \"./decl\";\nimport { named as g } from \"./decl\";\n";
    let options = CompilerOptions {
        module: Some(1),
        target: Some(99),
        module_resolution: Some(100),
        verbatim_module_syntax: Some(true),
        ..CompilerOptions::default()
    };
    let result = check_program(
        &[
            InputFile {
                name: "/decl.ts".to_owned(),
                text: declaration.to_owned(),
            },
            InputFile {
                name: "/main.ts".to_owned(),
                text: main.to_owned(),
            },
        ],
        &options,
    );
    let message = "ECMAScript imports and exports cannot be written in a CommonJS file under 'verbatimModuleSyntax'. Adjust the 'type' field in the nearest 'package.json' to make this file an ECMAScript module, or adjust your 'verbatimModuleSyntax', 'module', and 'moduleResolution' settings in TypeScript.";
    assert_eq!(
        targeted_rows(&result, &[1295]),
        [
            (
                "/main.ts".to_owned(),
                1295,
                main.find("f from").expect("default import") as u32,
                1,
                message.to_owned(),
            ),
            (
                "/main.ts".to_owned(),
                1295,
                main.find("ns from").expect("namespace import") as u32,
                2,
                message.to_owned(),
            ),
            (
                "/main.ts".to_owned(),
                1295,
                main.find("named as g").expect("named import") as u32,
                "named as g".len() as u32,
                message.to_owned(),
            ),
        ]
    );

    let cts_result = check_program(
        &[
            InputFile {
                name: "/decl.ts".to_owned(),
                text: declaration.to_owned(),
            },
            InputFile {
                name: "/main.cts".to_owned(),
                text: "import f from \"./decl\";\n".to_owned(),
            },
        ],
        &options,
    );
    assert_eq!(
            targeted_rows(&cts_result, &[1286, 1295]),
            [(
                "/main.cts".to_owned(),
                1286,
                7,
                1,
                "ECMAScript imports and exports cannot be written in a CommonJS file under 'verbatimModuleSyntax'.".to_owned(),
            )]
        );
}

#[test]
fn checked_js_export_assignment_uses_package_emit_format() {
    let source = "const a = {};\nexport = a;\n";
    assert_eq!(
        program_rows(
            &[
                ("/index.js", source),
                (
                    "/package.json",
                    "{ \"name\": \"package\", \"private\": true, \"type\": \"module\" }\n",
                ),
            ],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                module: Some(100),
                target: Some(9),
                ..CompilerOptions::default()
            },
        ),
        [
            (
                "/index.js".to_owned(),
                1203,
                source.find("export = a").expect("export assignment") as u32,
                "export = a;".len() as u32,
            ),
            (
                "/index.js".to_owned(),
                8003,
                source.find("export = a").expect("export assignment") as u32,
                "export = a;".len() as u32,
            ),
        ]
    );
}

#[test]
fn module_keyword_and_quoted_name_rows_1540_1035() {
    let files = [(
        "a.ts",
        "module M { export const x = 1; }\nmodule \"bad\" {}\nexport {};\n",
    )];
    assert_eq!(
        rows(&files),
        [
            ("a.ts".to_owned(), 1540, 7, 1),
            ("a.ts".to_owned(), 1035, 40, 5),
        ]
    );
}

#[test]
fn circular_import_alias_reports_2303() {
    let files = [("c.ts", "import A = B;\nimport B = A;\nA; B;\nexport {};\n")];
    assert_eq!(rows(&files), [("c.ts".to_owned(), 2303, 0, 13)]);
}

/// Static and dynamic imports in an .mts file use Node's ESM
/// resolution mode and therefore never extension-probe.
#[test]
fn static_mts_imports_require_explicit_extensions_under_node16() {
    let files = [
        ("/src/foo.mts", "export function foo() { return \"\"; }\n"),
        (
            "/src/bar.mts",
            "import { foo } from \"./foo\";\nimport { baz } from \"./baz\";\n",
        ),
    ];
    assert_eq!(
        program_rows(&files, &node16_options()),
        [
            ("/src/bar.mts".to_owned(), 2835, 20, 7),
            ("/src/bar.mts".to_owned(), 2834, 49, 7),
        ]
    );
}

/// Static imports and import() share the ESM resolution mode in an
/// .mts file. The noLib Promise 2711 rides the import call.
#[test]
fn static_and_dynamic_mts_imports_share_node_esm_resolution() {
    let files = [
        ("foo.ts", "export const x = 1;\n"),
        (
            "buzz.mts",
            "import(\"./foo\");\nimport { x } from \"./foo\";\nx;\n",
        ),
    ];
    assert_eq!(
        program_rows(&files, &node16_options()),
        [
            ("buzz.mts".to_owned(), 2711, 0, 15),
            ("buzz.mts".to_owned(), 2835, 7, 7),
            ("buzz.mts".to_owned(), 2835, 35, 7),
        ]
    );
}

#[test]
fn dynamic_import_in_plain_ts_uses_node_esm_resolution() {
    let files = [
        ("foo.ts", "export const x = 1;\n"),
        ("main.ts", "import(\"./foo\");\n"),
    ];
    assert_eq!(
        program_rows(&files, &node16_options()),
        [
            ("main.ts".to_owned(), 2711, 0, 15),
            ("main.ts".to_owned(), 2835, 7, 7),
        ]
    );
}

#[test]
fn import_equals_in_mts_uses_commonjs_resolution() {
    let files = [("main.mts", "import foo = require(\"./foo\");\n")];
    assert_eq!(
        program_rows(&files, &node16_options()),
        [("main.mts".to_owned(), 2307, 21, 7)]
    );
}

#[test]
fn type_only_resolution_mode_override_controls_node_resolution() {
    let files = [
        ("foo.ts", "export type X = number;\n"),
        ("package.json", "{}\n"),
        (
            "main.mts",
            "import type { Missing } from \"./foo\" with { \"resolution-mode\": \"require\" };\n",
        ),
        (
            "main.cts",
            "import type { X } from \"./foo\" with { \"resolution-mode\": \"import\" };\n",
        ),
    ];
    assert_eq!(
        program_rows(&files, &node16_options()),
        [
            ("main.cts".to_owned(), 2835, 23, 7),
            ("main.mts".to_owned(), 2305, 14, 7),
        ]
    );
}

#[test]
fn recovered_bare_import_type_and_dynamic_import_use_package_meaning() {
    let bad = "type Bad =\n\
                   & import(\"pkg\", {\"resolution-mode\": \"require\"}).RequireInterface\n\
                   & import(\"pkg\", {\"resolution-mode\": \"import\"}).ImportInterface;\n";
    let good = "type Good = import(\"pkg\", { with: {\"resolution-mode\": \"require\"} }).RequireInterface;\n";
    let files = [
            ("/globals.d.ts", "interface Promise<T> {}\n"),
            (
                "/node_modules/pkg/package.json",
                "{ \"name\": \"pkg\", \"exports\": { \"import\": \"./import.js\", \"require\": \"./require.js\" } }\n",
            ),
            (
                "/node_modules/pkg/import.d.ts",
                "export interface ImportInterface {}\n",
            ),
            (
                "/node_modules/pkg/require.d.ts",
                "export interface RequireInterface {}\n",
            ),
            ("/bad.ts", bad),
            ("/dynamic.ts", "import(\"pkg\").ImportInterface;\n"),
            ("/good.ts", good),
        ];
    let rows = program_rows(&files, &node16_options())
        .into_iter()
        .filter(|(_, code, _, _)| *code == 1340)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            "/bad.ts".to_owned(),
            1340,
            bad.find("import").expect("recovered import type") as u32,
            "import(\"pkg\", {".len() as u32,
        )]
    );

    let inputs = files
        .into_iter()
        .map(|(name, text)| InputFile {
            name: name.to_owned(),
            text: text.to_owned(),
        })
        .collect::<Vec<_>>();
    let messages = check_program(&inputs, &node16_options())
        .diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.code() == 2339)
        .map(|diagnostic| {
            let message = diagnostic.message_text().to_owned();
            (
                diagnostic.file_name.expect("located property miss"),
                message,
            )
        })
        .collect::<Vec<_>>();
    let expected_message = "Property 'ImportInterface' does not exist on type \
                                'Promise<{ default: typeof import(\"/node_modules/pkg/import\"); }>'."
            .to_owned();
    assert_eq!(
        messages,
        [
            ("/bad.ts".to_owned(), expected_message.clone()),
            ("/dynamic.ts".to_owned(), expected_message),
        ],
        "the recovered dynamic import resolves the ESM package condition; \
             the valid require-mode ImportType is the non-firing sibling"
    );
}

#[test]
fn implicit_any_module_uses_node10_alternate_result_chain() {
    let source = "import { pkg } from \"pkg\";\n";
    let result = check_program(
        &[
            InputFile {
                name: "/node_modules/pkg/package.json".to_owned(),
                text: r#"{
                        "name": "pkg",
                        "version": "1.0.0",
                        "main": "./untyped.js",
                        "exports": { ".": "./definitely-not-index.js" }
                    }"#
                .to_owned(),
            },
            InputFile {
                name: "/node_modules/pkg/untyped.js".to_owned(),
                text: "export {};\n".to_owned(),
            },
            InputFile {
                name: "/node_modules/pkg/definitely-not-index.d.ts".to_owned(),
                text: "export {};\n".to_owned(),
            },
            InputFile {
                name: "/index.ts".to_owned(),
                text: source.to_owned(),
            },
        ],
        &CompilerOptions {
            module_resolution: Some(2),
            no_implicit_any: Some(true),
            ..CompilerOptions::default()
        },
    );
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code() == 7016)
        .expect("implicit-any module diagnostic");
    assert_eq!(diagnostic.category(), DiagnosticCategory::Error);
    assert_eq!(
            (
                diagnostic.file_name.as_deref(),
                diagnostic.start,
                diagnostic.length,
                diagnostic.message_text(),
            ),
            (
                Some("/index.ts"),
                Some(source.find("\"pkg\"").expect("specifier") as u32),
                Some("\"pkg\"".len() as u32),
                "Could not find a declaration file for module 'pkg'. '/node_modules/pkg/untyped.js' implicitly has an 'any' type.",
            )
        );
    assert_eq!(
            diagnostic.message.next,
            [MessageChain::new(
                &diagnostics::There_are_types_at_0_but_this_result_could_not_be_resolved_under_your_current_moduleResolution_setting_Consider_updating_to_node16_nodenext_or_bundler,
                &["/node_modules/pkg/definitely-not-index.d.ts".to_owned()],
            )]
        );
}

#[test]
fn implicit_any_module_suggestion_is_published_from_checked_js() {
    let source = "const u = require('untyped');\nu.assignment.nested = true;\n";
    let result = check_program(
        &[
            InputFile {
                name: "/node_modules/untyped/index.js".to_owned(),
                text: "module.exports = {};\n".to_owned(),
            },
            InputFile {
                name: "/main.js".to_owned(),
                text: source.to_owned(),
            },
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(true),
            ..CompilerOptions::default()
        },
    );
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code() == 7016)
        .expect("checked-JS implicit-any suggestion");
    assert_eq!(diagnostic.category(), DiagnosticCategory::Suggestion);
    assert_eq!(diagnostic.file_name.as_deref(), Some("/main.js"));
    assert_eq!(
        diagnostic.start,
        Some(source.find("'untyped'").expect("specifier") as u32)
    );
    assert_eq!(diagnostic.length, Some("'untyped'".len() as u32));
    assert_eq!(
            diagnostic.message_text(),
            "Could not find a declaration file for module 'untyped'. '/node_modules/untyped/index.js' implicitly has an 'any' type."
        );
    assert!(diagnostic.message.next.is_empty());
}

#[test]
fn implicit_any_module_prefers_later_typed_exports_condition() {
    let result = check_program(
        &[
            InputFile {
                name: "/node_modules/dep/package.json".to_owned(),
                text: r#"{
                        "name": "dep",
                        "version": "1.0.0",
                        "exports": {
                            ".": {
                                "import": "./dist/index.mjs",
                                "require": "./dist/index.js",
                                "types": "./dist/index.d.ts"
                            }
                        }
                    }"#
                .to_owned(),
            },
            InputFile {
                name: "/node_modules/dep/dist/index.d.ts".to_owned(),
                text: "export {};\n".to_owned(),
            },
            InputFile {
                name: "/node_modules/dep/dist/index.mjs".to_owned(),
                text: "export {};\n".to_owned(),
            },
            InputFile {
                name: "/index.mts".to_owned(),
                text: "import {} from \"dep\";\n".to_owned(),
            },
        ],
        &CompilerOptions {
            module: Some(100),
            allow_js: true,
            no_implicit_any: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code() != 7016),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn implicit_any_module_prefers_visible_at_types_package() {
    let result = check_program(
        &[
            InputFile {
                name: "/node_modules/@types/react/index.d.ts".to_owned(),
                text: "declare const React: any;\nexport = React;\n".to_owned(),
            },
            InputFile {
                name: "/packages/a/node_modules/react/index.js".to_owned(),
                text: "module.exports = {};\n".to_owned(),
            },
            InputFile {
                name: "/packages/a/index.ts".to_owned(),
                text: "import React from \"react\";\nReact;\n".to_owned(),
            },
        ],
        &CompilerOptions {
            module: Some(100),
            no_implicit_any: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code() != 7016),
        "{:?}",
        result.diagnostics
    );
}

/// Oracle pin (tsc 6.0.3, plainJSGrammarErrors.ts, 2026-07-26):
/// the same nested import declaration selects TS1473 in JavaScript
/// and TS1232 in TypeScript.
#[test]
fn nested_import_declaration_selects_javascript_context_diagnostic() {
    let source = "function f() {\n  import \"x\";\n}\n";
    assert_eq!(
        program_rows(
            &[("/a.js", source), ("/b.ts", source)],
            &CompilerOptions {
                allow_js: true,
                module: Some(99),
                target: Some(99),
                ..CompilerOptions::default()
            },
        )
        .into_iter()
        .filter(|(_, code, _, _)| matches!(*code, 1232 | 1473))
        .collect::<Vec<_>>(),
        [
            (
                "/a.js".to_owned(),
                1473,
                source.find("import").expect("nested import") as u32,
                "import".len() as u32,
            ),
            (
                "/b.ts".to_owned(),
                1232,
                source.find("import").expect("nested import") as u32,
                "import".len() as u32,
            ),
        ]
    );
}

/// Oracle pin (tsc 6.0.3, plainJSGrammarErrors.ts, 2026-07-26):
/// the same nested export declaration selects TS1474 in JavaScript
/// and TS1233 in TypeScript.
#[test]
fn nested_export_declaration_selects_javascript_context_diagnostic() {
    let source = "function f() {\n  export { };\n}\n";
    assert_eq!(
        program_rows(
            &[("/a.js", source), ("/b.ts", source)],
            &CompilerOptions {
                allow_js: true,
                module: Some(99),
                target: Some(99),
                ..CompilerOptions::default()
            },
        )
        .into_iter()
        .filter(|(_, code, _, _)| matches!(*code, 1233 | 1474))
        .collect::<Vec<_>>(),
        [
            (
                "/a.js".to_owned(),
                1474,
                source.find("export").expect("nested export") as u32,
                "export".len() as u32,
            ),
            (
                "/b.ts".to_owned(),
                1233,
                source.find("export").expect("nested export") as u32,
                "export".len() as u32,
            ),
        ]
    );
}

/// Oracle pins (tsc 6.0.3, nodeModulesJson.ts, 2026-07-26).
#[test]
fn node18_json_attribute_predicate_projects_suppressed_package_targets() {
    let source = "import root from \"pkg\";\n\
                      import typed from \"pkg/typed\";\n\
                      import attributed from \"pkg\" with { type: \"json\" };\n\
                      import relative from \"./config.json\";\n\
                      root; typed; attributed; relative;\n";
    let files = [
            (
                "/node_modules/pkg/package.json",
                "{ \"name\": \"pkg\", \"type\": \"module\", \"exports\": { \".\": \"./index.json\", \"./typed\": \"./typed.d.json.ts\" } }\n",
            ),
            ("/node_modules/pkg/index.json", "{}\n"),
            (
                "/node_modules/pkg/typed.d.json.ts",
                "declare const value: {};\nexport default value;\n",
            ),
            ("/config.json", "{}\n"),
            ("/main.mts", source),
        ];
    let node18 = CompilerOptions {
        module: Some(101),
        target: Some(9),
        resolve_json_module: Some(true),
        ..CompilerOptions::default()
    };
    assert_eq!(
        program_rows(&files, &node18)
            .into_iter()
            .filter(|(_, code, _, _)| *code == 1543)
            .collect::<Vec<_>>(),
        [
            (
                "/main.mts".to_owned(),
                1543,
                source.find("\"pkg\"").expect("root package specifier") as u32,
                "\"pkg\"".len() as u32,
            ),
            (
                "/main.mts".to_owned(),
                1543,
                source
                    .find("\"pkg/typed\"")
                    .expect("typed package specifier") as u32,
                "\"pkg/typed\"".len() as u32,
            ),
            (
                "/main.mts".to_owned(),
                1543,
                source
                    .find("\"./config.json\"")
                    .expect("relative JSON specifier") as u32,
                "\"./config.json\"".len() as u32,
            ),
        ]
    );

    assert!(program_rows(
        &files,
        &CompilerOptions {
            module: Some(100),
            target: Some(9),
            resolve_json_module: Some(true),
            ..CompilerOptions::default()
        },
    )
    .into_iter()
    .all(|(_, code, _, _)| code != 1543));
}

#[test]
fn explicit_non_ts_extension_reports_plain_2307_under_node16() {
    let files = [("/src/main.mts", "export * from \"./missing.css\";\n")];
    assert_eq!(
        program_rows(&files, &node16_options()),
        [("/src/main.mts".to_owned(), 2307, 14, 15)]
    );
}

#[test]
fn arbitrary_extension_declaration_twin_reports_exact_6263() {
    let source = "import * as mod from \"./component.html\";\nmod.value;\n";
    let files = [
        ("/component.d.html.ts", "export const value: number;\n"),
        ("/file.ts", source),
    ];
    let inputs: Vec<InputFile> = files
        .iter()
        .map(|(name, text)| InputFile {
            name: (*name).to_owned(),
            text: (*text).to_owned(),
        })
        .collect();
    let result = check_program(
        &inputs,
        &CompilerOptions {
            allow_arbitrary_extensions: Some(false),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
            targeted_rows(&result, &[6263]),
            [(
                "/file.ts".to_owned(),
                6263,
                21,
                18,
                "Module './component.html' was resolved to '/component.d.html.ts', but '--allowArbitraryExtensions' is not set.".to_owned(),
            )]
        );
    assert_eq!(
        program_rows(
            &files,
            &CompilerOptions {
                allow_arbitrary_extensions: Some(true),
                ..CompilerOptions::default()
            },
        ),
        []
    );
}

#[test]
fn arbitrary_extension_twin_option_gate_uses_the_importing_file() {
    let files = [
        (
            "/dir/native.d.node.ts",
            "export function doNativeThing(flag: string): unknown;\n",
        ),
        ("/main.d.ts", "export * from \"./dir/native.node\";\n"),
    ];
    assert_eq!(
        program_rows(
            &files,
            &CompilerOptions {
                allow_arbitrary_extensions: Some(false),
                ..CompilerOptions::default()
            },
        ),
        [],
        "a declaration-file importer suppresses needAllowArbitraryExtensions"
    );

    let source = "import mod = require(\"./dir/native.node\");\nmod.doNativeThing(\"good\");\n";
    let files = [
        (
            "/dir/native.d.node.ts",
            "export function doNativeThing(flag: string): unknown;\n",
        ),
        ("/main.ts", source),
    ];
    let inputs: Vec<InputFile> = files
        .iter()
        .map(|(name, text)| InputFile {
            name: (*name).to_owned(),
            text: (*text).to_owned(),
        })
        .collect();
    let result = check_program(
        &inputs,
        &CompilerOptions {
            allow_arbitrary_extensions: Some(false),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
            targeted_rows(&result, &[6263]),
            [(
                "/main.ts".to_owned(),
                6263,
                21,
                19,
                "Module './dir/native.node' was resolved to '/dir/native.d.node.ts', but '--allowArbitraryExtensions' is not set.".to_owned(),
            )]
        );
}

/// Recognized extensions use their fixed substitution groups.
/// Arbitrary declaration twins such as file.d.js.ts must not make
/// those authoritative misses host-dependent.
#[test]
fn recognized_extension_misses_ignore_arbitrary_declaration_twins() {
    let files = [
            (
                "/main.ts",
                "import d1 from \"./file.js\";\nimport d2 from \"./file.jsx\";\nimport d3 from \"./file.ts\";\nimport d4 from \"./file.tsx\";\nimport d5 from \"./file.mjs\";\nimport d6 from \"./file.cjs\";\nimport d7 from \"./file.mts\";\nimport d8 from \"./file.cts\";\nimport d9 from \"./file.d.ts\";\nimport d10 from \"./file.d.cts\";\nimport d11 from \"./file.d.mts\";\nimport d12 from \"./file.d.json.ts\";\nd1; d2; d3; d4; d5; d6; d7; d8; d9; d10; d11; d12;\n",
            ),
            ("/file.d.js.ts", "export {};\n"),
            ("/file.d.jsx.ts", "export {};\n"),
            ("/file.d.ts.ts", "export {};\n"),
            ("/file.d.tsx.ts", "export {};\n"),
            ("/file.d.mjs.ts", "export {};\n"),
            ("/file.d.cjs.ts", "export {};\n"),
            ("/file.d.mts.ts", "export {};\n"),
            ("/file.d.cts.ts", "export {};\n"),
            ("/file.d.d.ts.ts", "export {};\n"),
            ("/file.d.d.cts.ts", "export {};\n"),
            ("/file.d.d.mts.ts", "export {};\n"),
            ("/file.d.d.json.ts", "export {};\n"),
        ];
    let diagnostics = program_rows(&files, &node16_options());
    assert_eq!(diagnostics.len(), 12, "{diagnostics:?}");
    assert!(diagnostics
        .iter()
        .all(|(file, code, _, _)| file == "/main.ts" && *code == 2307));
}

#[test]
fn checked_js_require_literal_publishes_definite_module_miss() {
    let files = [(
        "/a.js",
        "require(\"\" + \"./foo.ts\");\nrequire(\"./foo.ts\");\n",
    )];
    assert_eq!(
        program_rows(
            &files,
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            }
        ),
        [("/a.js".to_owned(), 2307, 34, 10)]
    );
}

#[test]
fn commonjs_property_immediate_target_skips_deprecated_reexport_alias() {
    // getTargetOfImportEqualsDeclaration's CommonJS-property arm calls
    // resolveSymbol without forwarding dontRecursivelyResolve. Thus the
    // immediate target of the require variable is `original`, not the
    // deprecated `foo` re-export alias, and the use has no 6385.
    let files = [
        InputFile {
            name: "/base.ts".to_owned(),
            text: "export function original() {}".to_owned(),
        },
        InputFile {
            name: "/dep.ts".to_owned(),
            text: "export { /** @deprecated use original */ original as foo } from \"./base\";"
                .to_owned(),
        },
        InputFile {
            name: "/consumer.js".to_owned(),
            text: "const foo = require(\"./dep\").foo; foo();".to_owned(),
        },
    ];
    let result = check_program(
        &files,
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            module: Some(1),
            module_resolution: Some(2),
            ..CompilerOptions::default()
        },
    );
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code() != 6385),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn checked_js_destructured_require_aliases_preserve_bare_class_and_accessed_value_faces() {
    // getTargetOfAliasDeclaration 49071-49108 routes BindingElement
    // through getTargetOfImportSpecifier 48959-48983. A bare require
    // resolves the exported class symbol, so its JSDoc value reference
    // sees the instance face. The accessed-require second stage returns
    // the object-literal Property symbol verbatim;
    // getTypeFromJSDocValueReference consequently keeps its constructor
    // value face, on which both instance-member reads miss.
    let files = [
        (
            "/mod.js",
            "class K { values() {} }\nexports.K = K;\nexports.box = { K };\n",
        ),
        (
            "/main.js",
            "const { K } = require('./mod');\n\
                 /** @param {K} value */\n\
                 function use(value) { value.values(); value.missing; }\n",
        ),
        (
            "/accessed.js",
            "const { K: NestedK } = require('./mod').box;\n\
                 /** @param {NestedK} value */\n\
                 function use(value) { value.values(); value.missing; }\n",
        ),
    ];
    let rows = program_rows(
        &files,
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        rows.into_iter()
            .filter(|(_, code, _, _)| *code == 2339)
            .map(|(file, code, _, _)| (file, code))
            .collect::<Vec<_>>(),
        [
            ("/accessed.js".to_owned(), 2339),
            ("/accessed.js".to_owned(), 2339),
            ("/main.js".to_owned(), 2339),
        ]
    );
}

#[test]
fn destructured_require_sees_named_members_merged_onto_commonjs_export_equals() {
    // getCommonJsExportEquals 49691-49714 merges the file's named
    // exports onto the resolved export= target before the binding
    // element selects its module member.
    let files = [
        (
            "/commonJSAliasedExport.js",
            "const donkey = ast => ast;\n\
                 function funky(declaration) { return false; }\n\
                 module.exports = donkey;\n\
                 module.exports.funky = funky;\n",
        ),
        (
            "/bug43713.js",
            "const { funky } = require('./commonJSAliasedExport');\n\
                 /** @type {boolean} */\n\
                 var diddy;\n\
                 var diddy = funky(1);\n",
        ),
    ];
    let rows = program_rows(
        &files,
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        rows.into_iter()
            .filter(|(_, code, _, _)| *code == 2339)
            .map(|(file, code, _, _)| (file, code))
            .collect::<Vec<_>>(),
        []
    );
}

#[test]
fn allow_js_resolves_in_program_js_after_ts_substitution_candidates() {
    let files = [
            (
                "/mod.js",
                "const present = 1;\n/** @typedef {() => number} buz */\nmodule.exports = { present };\n",
            ),
            (
                "/main.ts",
                "type T = import(\"./mod\").Missing;\ntype U = import(\"./mod\").buz;\n",
            ),
        ];
    assert_eq!(
        program_rows(
            &files,
            &CompilerOptions {
                allow_js: true,
                ..CompilerOptions::default()
            }
        ),
        [("/main.ts".to_owned(), 2694, 25, 7)]
    );

    let files = [
        ("/foo.ts", "export const value: number = 1;\n"),
        ("/foo.js", "exports.value = \"js\";\n"),
        (
            "/main.ts",
            "import { value } from \"./foo.js\";\nconst n: number = value;\n",
        ),
    ];
    assert_eq!(
        program_rows(
            &files,
            &CompilerOptions {
                allow_js: true,
                ..CompilerOptions::default()
            }
        ),
        []
    );

    let files = [
        ("/foo.cjs", "exports.foo = \"foo\";\n"),
        ("/bar.ts", "import foo from \"./foo.cjs\";\nfoo.foo;\n"),
    ];
    assert_eq!(
        program_rows(
            &files,
            &CompilerOptions {
                allow_js: true,
                module: Some(100),
                target: Some(9),
                ..CompilerOptions::default()
            }
        ),
        []
    );
}

#[test]
fn default_import_chases_module_exports_alias_without_adopting_expandos() {
    let files = [
        (
            "/mod1.js",
            "class Alias { bar() { return 1 } }\nmodule.exports = Alias;\n",
        ),
        (
            "/main.js",
            "import A from './mod1';\n\
                 A.prototype.foo = 0;\n\
                 A.prototype.func = function() { this._func = 0; };\n\
                 new A().bar;\n\
                 new A().foo;\n\
                 new A().func();\n\
                 new A().def;\n",
        ),
    ];
    assert_eq!(
        program_rows(
            &files,
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                no_emit: Some(true),
                es_module_interop: Some(true),
                target: Some(2),
                ..CompilerOptions::default()
            }
        ),
        [
            ("/main.js".to_owned(), 2339, 36, 3),
            ("/main.js".to_owned(), 2339, 57, 4),
            ("/main.js".to_owned(), 2339, 82, 5),
            ("/main.js".to_owned(), 2339, 117, 3),
            ("/main.js".to_owned(), 2339, 130, 4),
            ("/main.js".to_owned(), 2339, 146, 3),
        ]
    );
}

#[test]
fn explicit_mts_cts_extensions_report_or_suggest_the_full_extension() {
    let files = [
            ("/main.ts", "import {} from \"./foo.d.mts\";\nimport {} from \"./bar.d.cts\";\nimport {} from \"./baz.mts\";\nimport {} from \"./qux.cts\";\n"),
            ("/foo.d.mts", "export {};\n"),
            ("/bar.d.cts", "export {};\n"),
            ("/baz.mts", "export {};\n"),
            ("/qux.cts", "export {};\n"),
        ];
    let inputs: Vec<InputFile> = files
        .iter()
        .map(|(name, text)| InputFile {
            name: (*name).to_owned(),
            text: (*text).to_owned(),
        })
        .collect();
    let diagnostics = check_program(&inputs, &CompilerOptions::default()).diagnostics;
    let pins: Vec<(u32, u32, String)> = diagnostics
        .iter()
        .filter_map(|diagnostic| {
            diagnostic.start.map(|start| {
                (
                    diagnostic.code(),
                    start,
                    diagnostic.message_text().to_owned(),
                )
            })
        })
        .collect();
    assert_eq!(
            pins,
            [
                (
                    2846,
                    15,
                    "A declaration file cannot be imported without 'import type'. Did you mean to import an implementation file './foo.mjs' instead?".to_owned(),
                ),
                (
                    2846,
                    45,
                    "A declaration file cannot be imported without 'import type'. Did you mean to import an implementation file './bar.js' instead?".to_owned(),
                ),
                (
                    5097,
                    75,
                    "An import path can only end with a '.mts' extension when 'allowImportingTsExtensions' is enabled.".to_owned(),
                ),
                (
                    5097,
                    103,
                    "An import path can only end with a '.cts' extension when 'allowImportingTsExtensions' is enabled.".to_owned(),
                ),
            ]
        );
}

#[test]
fn declaration_extension_substitution_preserves_ts_extension_provenance() {
    let files = [
        (
            "/types.d.ts",
            "import {} from \"./a.d.ts\";\nimport type {} from \"./a.d.ts\";\n",
        ),
        ("/a.ts", "export {};\n"),
    ];
    assert_eq!(
        program_rows(
            &files,
            &CompilerOptions {
                allow_importing_ts_extensions: Some(true),
                ..CompilerOptions::default()
            },
        ),
        [("/types.d.ts".to_owned(), 2846, 15, 10)]
    );
}

#[test]
fn jsdoc_declaration_file_import_suppresses_2846_without_suppressing_value_imports() {
    let jsdoc = "/** @import { T } from \"./types.d.ts\" */\n\
                     /** @type {T} */\n\
                     export const jsdocValue = \"ok\";\n";
    let value_import = "import {} from \"./types.d.ts\";\nexport const valueImport = \"ok\";\n";
    let files = [
        ("/types.d.ts", "export type T = string;\n"),
        ("/jsdoc.js", jsdoc),
        ("/value.ts", value_import),
    ];
    let rows = program_rows(
        &files,
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            no_emit: Some(true),
            module: Some(200),
            module_resolution: Some(100),
            target: Some(9),
            ..CompilerOptions::default()
        },
    )
    .into_iter()
    .filter(|(_, code, _, _)| *code == 2846)
    .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            "/value.ts".to_owned(),
            2846,
            value_import
                .find("\"./types.d.ts\"")
                .expect("value import specifier") as u32,
            "\"./types.d.ts\"".len() as u32,
        )]
    );
}

#[test]
fn jsdoc_ts_extension_import_reports_5097_but_import_type_does_not() {
    let jsdoc = "/** @import { T } from \"./types.ts\" */\n\
                     /** @type {T} */\n\
                     export const jsdocValue = \"ok\";\n";
    let import_type = "import type { T } from \"./types.ts\";\nexport type Imported = T;\n";
    let files = [
        ("/types.ts", "export type T = string;\n"),
        ("/jsdoc.js", jsdoc),
        ("/type-only.ts", import_type),
    ];
    let rows = program_rows(
        &files,
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            no_emit: Some(true),
            module: Some(200),
            module_resolution: Some(100),
            target: Some(9),
            ..CompilerOptions::default()
        },
    )
    .into_iter()
    .filter(|(_, code, _, _)| *code == 5097)
    .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            "/jsdoc.js".to_owned(),
            5097,
            jsdoc
                .find("\"./types.ts\"")
                .expect("JSDoc import specifier") as u32,
            "\"./types.ts\"".len() as u32,
        )]
    );
}

#[test]
fn rewrite_relative_import_reports_file_looking_directory_resolution() {
    let files = [
        ("/foo.ts/index.ts", "export = {};\n"),
        (
            "/index.ts",
            "import foo = require(\"./foo.ts\");\n\
                 import type only = require(\"./foo.ts\");\n",
        ),
    ];
    assert_eq!(
        program_rows(
            &files,
            &CompilerOptions {
                target: Some(9),
                module: Some(102),
                verbatim_module_syntax: Some(true),
                rewrite_relative_import_extensions: Some(true),
                ..CompilerOptions::default()
            },
        ),
        [("/index.ts".to_owned(), 2876, 21, 10)]
    );
}

#[test]
fn rewrite_relative_import_checks_jsdoc_import_but_not_literal_import_type() {
    let jsdoc = "/** @import { T } from \"./foo.ts\" */\n\
                     /** @type {T} */\n\
                     export const jsdocValue = {};\n";
    let import_type = "export type Imported = import(\"./foo.ts\");\n";
    let files = [
        ("/foo.ts/index.ts", "export interface T {}\n"),
        ("/jsdoc.js", jsdoc),
        ("/type-only.ts", import_type),
    ];
    let rows = program_rows(
        &files,
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            target: Some(9),
            module: Some(102),
            verbatim_module_syntax: Some(true),
            rewrite_relative_import_extensions: Some(true),
            ..CompilerOptions::default()
        },
    )
    .into_iter()
    .filter(|(_, code, _, _)| *code == 2876)
    .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            "/jsdoc.js".to_owned(),
            2876,
            jsdoc.find("\"./foo.ts\"").expect("JSDoc import specifier") as u32,
            "\"./foo.ts\"".len() as u32,
        )]
    );
}

#[test]
fn declaration_import_suggestion_uses_usage_emit_mode() {
    let files = [
        ("/main.ts", "import(\"./foo.d.mts\");\n"),
        ("/foo.d.mts", "export {};\n"),
    ];
    let inputs: Vec<InputFile> = files
        .iter()
        .map(|(name, text)| InputFile {
            name: (*name).to_owned(),
            text: (*text).to_owned(),
        })
        .collect();
    let diagnostics = check_program(
        &inputs,
        &CompilerOptions {
            module: Some(1),
            target: Some(9),
            module_resolution: Some(100),
            ..CompilerOptions::default()
        },
    )
    .diagnostics;
    let pins: Vec<(u32, u32, String)> = diagnostics
        .iter()
        .filter_map(|diagnostic| {
            diagnostic.start.map(|start| {
                (
                    diagnostic.code(),
                    start,
                    diagnostic.message_text().to_owned(),
                )
            })
        })
        .collect();
    assert_eq!(
            pins,
            [
                (
                    2711,
                    0,
                    "A dynamic import call returns a 'Promise'. Make sure you have a declaration for 'Promise' or include 'ES2015' in your '--lib' option.".to_owned(),
                ),
                (
                    2846,
                    7,
                    "A declaration file cannot be imported without 'import type'. Did you mean to import an implementation file './foo' instead?".to_owned(),
                ),
            ]
        );

    let files = [
        ("/main.ts", "import {} from \"./foo.d.mts\";\n"),
        ("/foo.d.mts", "export {};\n"),
    ];
    let inputs: Vec<InputFile> = files
        .iter()
        .map(|(name, text)| InputFile {
            name: (*name).to_owned(),
            text: (*text).to_owned(),
        })
        .collect();
    let diagnostics = check_program(
        &inputs,
        &CompilerOptions {
            module: Some(200),
            target: Some(9),
            module_resolution: Some(100),
            ..CompilerOptions::default()
        },
    )
    .diagnostics;
    let pins: Vec<(u32, u32, String)> = diagnostics
        .iter()
        .filter_map(|diagnostic| {
            diagnostic.start.map(|start| {
                (
                    diagnostic.code(),
                    start,
                    diagnostic.message_text().to_owned(),
                )
            })
        })
        .collect();
    assert_eq!(
            pins,
            [(
                2846,
                15,
                "A declaration file cannot be imported without 'import type'. Did you mean to import an implementation file './foo.mjs' instead?".to_owned(),
            )]
        );
}

#[test]
fn checked_js_mixed_common_js_exports_publish_redeclarations() {
    let source = "module.exports.bothBefore = 'string';\n\
                      A.justExport = 4;\n\
                      A.bothBefore = 2;\n\
                      A.bothAfter = 3;\n\
                      module.exports = A;\n\
                      function A() { this.p = 1; }\n\
                      module.exports.bothAfter = 'string';\n\
                      module.exports.justProperty = 'string';\n";
    let files = [
        (
            "/requires.d.ts",
            "declare var module: { exports: any };\n\
                 declare var exports: any;\n",
        ),
        ("/mod1.js", source),
    ];
    let offset = |needle: &str| source.find(needle).expect("fixture needle") as u32;
    assert_eq!(
        program_rows(
            &files,
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                target: Some(2),
                ..CompilerOptions::default()
            },
        ),
        [
            (
                "/mod1.js".to_owned(),
                2323,
                offset("module.exports.bothBefore"),
                "module.exports.bothBefore".len() as u32,
            ),
            (
                "/mod1.js".to_owned(),
                2323,
                offset("A.bothBefore"),
                "A.bothBefore".len() as u32,
            ),
            (
                "/mod1.js".to_owned(),
                2323,
                offset("A.bothAfter"),
                "A.bothAfter".len() as u32,
            ),
            (
                "/mod1.js".to_owned(),
                2323,
                offset("module.exports.bothAfter"),
                "module.exports.bothAfter".len() as u32,
            ),
        ]
    );
}

#[test]
fn checked_js_common_js_export_flow_reports_use_before_assignment() {
    let source = "module.exports.a = module.exports.b;\nmodule.exports.b = function b() {}\n";
    let b_read = source.find("module.exports.b").expect("fixture read") as u32
        + "module.exports.".len() as u32;
    assert_eq!(
        program_rows(
            &[("/mod.js", source)],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                strict: Some(true),
                ..CompilerOptions::default()
            },
        ),
        [("/mod.js".to_owned(), 2565, b_read, 1)]
    );
}

#[test]
fn export_equals_keeps_broader_common_js_source_type_contained() {
    assert_eq!(
        program_rows(
            &[(
                "/mod.js",
                "module.exports = function x() {}\nmodule.exports()\n",
            )],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                strict: Some(true),
                ..CompilerOptions::default()
            },
        ),
        []
    );
}

#[test]
fn duplicated_common_js_export_alias_uses_assignment_flow() {
    let source = "exports.apply = undefined;\n\
                      function a() {}\n\
                      exports.apply()\n\
                      exports.apply = a;\n\
                      exports.apply()\n";
    let first_call = source.find("exports.apply()").expect("fixture call") as u32;
    assert_eq!(
        program_rows(
            &[("/mod.js", source)],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                strict: Some(true),
                ..CompilerOptions::default()
            },
        ),
        [(
            "/mod.js".to_owned(),
            2722,
            first_call,
            "exports.apply".len() as u32,
        )]
    );
}

#[test]
fn duplicated_pure_common_js_exports_do_not_redeclare() {
    let files = [
        (
            "/requires.d.ts",
            "declare var module: { exports: any };\n\
                 declare var exports: any;\n",
        ),
        ("/mod.js", "exports.same = 1;\nmodule.exports.same = 2;\n"),
    ];
    assert!(program_rows(
        &files,
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    )
    .is_empty());
}

#[test]
fn legacy_module_call_import_equals_reports_node_global_hint() {
    let rows = program_rows(
        &[(
            "/main.ts",
            "import rect = module(\"rect\"); var bar = new rect.Rect();",
        )],
        &CompilerOptions {
            target: Some(2),
            ..CompilerOptions::default()
        },
    );
    assert!(rows.iter().any(|row| row.1 == 2591 && row.2 == 14));
}

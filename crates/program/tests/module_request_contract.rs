use std::path::Path;

use tsc_program::{
    plan_module_requests, plan_source_requests, plan_static_module_requests, CompilerOptions,
    PreparedSourceFile, ProgramPath, ResolutionError, ResolutionMode,
};
use tsc_types::ScriptTarget;

fn path(display: &str) -> ProgramPath {
    ProgramPath::from_trusted_parts(display, display).expect("trusted test path")
}

fn node_options() -> CompilerOptions {
    CompilerOptions {
        module: Some(102),
        ..CompilerOptions::default()
    }
}

fn source(text: &str, mode: ResolutionMode) -> PreparedSourceFile {
    source_at("/index.mts", text, Some(mode))
}

fn source_at(file_name: &str, text: &str, mode: Option<ResolutionMode>) -> PreparedSourceFile {
    let source = PreparedSourceFile::new(path(file_name), text);
    match mode {
        Some(mode) => source.with_implied_node_format(mode),
        None => source,
    }
}

#[test]
fn static_imports_produce_stable_exact_deduplicated_keys() {
    let source = source(
        concat!(
            "import \"inner/first\";\n",
            "import * as second from \"inner/second\";\n",
            "import \"inner/first\";\n",
            "export { second };\n",
        ),
        ResolutionMode::EsNext,
    );

    let requests =
        plan_static_module_requests(&source, &node_options()).expect("plan static imports");
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].source().as_path(), Path::new("/index.mts"));
    assert_eq!(requests[0].specifier(), "inner/first");
    assert_eq!(requests[0].mode(), ResolutionMode::EsNext);
    assert_eq!(requests[1].specifier(), "inner/second");
}

#[test]
fn authoritative_source_format_is_the_request_mode() {
    let source = source("import \"inner/cjs/index\";\n", ResolutionMode::CommonJs);
    let requests =
        plan_static_module_requests(&source, &node_options()).expect("plan CommonJS import");
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].mode(), ResolutionMode::CommonJs);
}

#[test]
fn expanded_plan_includes_export_from_and_literal_dynamic_imports() {
    let source = source(
        concat!(
            "export { x } from \"./other.js\";\n",
            "const loaded = import(\"inner\");\n",
            "import \"inner/static\";\n",
        ),
        ResolutionMode::CommonJs,
    );
    let requests = plan_module_requests(&source, &node_options()).expect("plan H0.2c requests");
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].specifier(), "./other.js");
    assert_eq!(requests[0].mode(), ResolutionMode::CommonJs);
    assert_eq!(requests[1].specifier(), "inner/static");
    assert_eq!(requests[1].mode(), ResolutionMode::CommonJs);
    assert_eq!(requests[2].specifier(), "inner");
    assert_eq!(requests[2].mode(), ResolutionMode::EsNext);
}

#[test]
fn expanded_plan_includes_external_import_equals_as_common_js() {
    let source = source(
        concat!(
            "import alias = require(\"inner/required\");\n",
            "import * as imported from \"inner/imported\";\n",
            "const loaded = import(\"inner/dynamic\");\n",
        ),
        ResolutionMode::EsNext,
    );
    let requests = plan_module_requests(&source, &node_options()).expect("plan import equals");
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].specifier(), "inner/required");
    assert_eq!(requests[0].mode(), ResolutionMode::CommonJs);
    assert_eq!(requests[1].specifier(), "inner/imported");
    assert_eq!(requests[1].mode(), ResolutionMode::EsNext);
    assert_eq!(requests[2].specifier(), "inner/dynamic");
    assert_eq!(requests[2].mode(), ResolutionMode::EsNext);
}

#[test]
fn empty_static_specifiers_are_ignored_while_dynamic_occurrences_are_retained() {
    let static_source = source(
        concat!(
            "import \"\";\n",
            "export * from \"\";\n",
            "import alias = require(\"\");\n",
        ),
        ResolutionMode::EsNext,
    );
    let static_plan =
        plan_source_requests(&static_source, &node_options()).expect("plan empty static controls");
    assert!(static_plan.module_requests().is_empty());
    assert_eq!(static_plan.observed_request_occurrence_count(), 0);

    let dynamic_source = source(
        concat!("type Imported = import(\"\").Value;\n", "import(\"\");\n"),
        ResolutionMode::EsNext,
    );
    let dynamic_plan = plan_source_requests(&dynamic_source, &node_options())
        .expect("plan empty dynamic requests");
    assert_eq!(dynamic_plan.module_requests().len(), 1);
    assert_eq!(dynamic_plan.module_requests()[0].specifier(), "");
    assert_eq!(dynamic_plan.observed_request_occurrence_count(), 2);

    let jsdoc_source = source_at(
        "/a.js",
        "/** @import { Empty } from '' */\nconst value = 0;\n",
        Some(ResolutionMode::EsNext),
    );
    let jsdoc_plan =
        plan_source_requests(&jsdoc_source, &node_options()).expect("plan empty JSDoc control");
    assert!(jsdoc_plan.module_requests().is_empty());
    assert_eq!(jsdoc_plan.observed_request_occurrence_count(), 0);

    let require_source = source_at(
        "/a.js",
        "const loaded = require(\"\");\n",
        Some(ResolutionMode::CommonJs),
    );
    let require_plan = plan_source_requests(&require_source, &node_options())
        .expect("plan empty JavaScript require");
    assert_eq!(require_plan.module_requests().len(), 1);
    assert_eq!(require_plan.module_requests()[0].specifier(), "");
}

#[test]
fn jsdoc_imports_retain_source_order_and_exact_resolution_mode_keys() {
    let source = source_at(
        "/a.js",
        concat!(
            "/** @import { Import } from 'foo' with { 'resolution-mode': 'import' } */\n",
            "import \"after-jsdoc\";\n",
            "/** @import { Require } from 'foo' with { 'resolution-mode': 'require' } */\n",
            "const requireUse = 0;\n",
            "/** @import { Fallback } from 'fallback' */\n",
            "const fallbackUse = 0;\n",
            "/** @import { Duplicate } from 'foo' with { 'resolution-mode': 'import' } */\n",
            "const duplicateUse = 0;\n",
        ),
        Some(ResolutionMode::CommonJs),
    );

    let requests = plan_module_requests(&source, &node_options()).expect("plan JSDoc imports");
    assert_eq!(requests.len(), 4);
    assert_eq!(
        requests
            .iter()
            .map(|request| (request.specifier(), request.mode()))
            .collect::<Vec<_>>(),
        [
            ("after-jsdoc", ResolutionMode::CommonJs),
            ("foo", ResolutionMode::EsNext),
            ("foo", ResolutionMode::CommonJs),
            ("fallback", ResolutionMode::CommonJs),
        ]
    );
}

#[test]
fn source_plan_reuses_the_parse_for_exact_type_reference_directives() {
    let text = concat!(
        "/// <reference types=\"JqUeRy\" />\n",
        "/// <reference types='@scope/pkg' resolution-mode='import'/>\n",
        "/// <reference types=\"JqUeRy\" />\n",
        "/// <reference types=\"@scope/pkg\" resolution-mode=\"require\"/>\n",
        "import \"module-request\";\n",
    );
    let source = source_at("/index.ts", text, None)
        .with_implied_node_formats(Some(ResolutionMode::CommonJs), None);
    let options = CompilerOptions {
        module: Some(99),
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };

    let plan = plan_source_requests(&source, &options).expect("plan source requests once");
    assert_eq!(plan.module_requests().len(), 1);
    assert_eq!(plan.module_requests()[0].specifier(), "module-request");
    assert_eq!(plan.observed_request_occurrence_count(), 5);

    let directives = plan.type_reference_directives();
    assert_eq!(directives.len(), 4);
    assert_eq!(directives[0].key().specifier(), "JqUeRy");
    assert_eq!(directives[0].key().mode(), ResolutionMode::Unspecified);
    assert_eq!(directives[0].span(), 22..28);
    assert_eq!(directives[0].pos(), 22);
    assert_eq!(directives[0].end(), 28);
    assert_eq!(directives[0].length(), 6);
    assert_eq!(utf16_text_at(text, directives[0].span()), "JqUeRy");

    assert_eq!(directives[1].key().specifier(), "@scope/pkg");
    assert_eq!(directives[1].key().mode(), ResolutionMode::EsNext);
    assert_eq!(
        utf16_text_at(text, directives[1].span()),
        directives[1].key().specifier()
    );

    assert_eq!(directives[2].key(), directives[0].key());
    assert_ne!(directives[2].span(), directives[0].span());
    assert_eq!(
        utf16_text_at(text, directives[2].span()),
        directives[2].key().specifier()
    );

    assert_eq!(directives[3].key().specifier(), "@scope/pkg");
    assert_eq!(directives[3].key().mode(), ResolutionMode::CommonJs);
    assert_eq!(
        utf16_text_at(text, directives[3].span()),
        directives[3].key().specifier()
    );

    let containing_mode_source = source_at(
        "/mode.cts",
        "/// <reference types=\"fallback\" />\n",
        Some(ResolutionMode::CommonJs),
    );
    let containing_mode_plan = plan_source_requests(&containing_mode_source, &node_options())
        .expect("plan fallback type-reference mode");
    assert_eq!(
        containing_mode_plan.type_reference_directives()[0]
            .key()
            .mode(),
        ResolutionMode::CommonJs
    );
}

#[test]
fn source_plan_projects_path_and_lib_references_from_the_same_parse() {
    let text = concat!(
        "/// <reference path=\"./dependency.ts\" preserve=\"true\" />\n",
        "/// <reference lib='es2023' preserve='true' />\n",
        "/// <reference types=\"pkg\" preserve=\"true\" />\n",
        "import \"module-request\";\n",
    );
    let source = source_at("/index.ts", text, Some(ResolutionMode::EsNext));
    let plan = plan_source_requests(&source, &node_options()).expect("plan all source requests");

    assert_eq!(plan.path_references().len(), 1);
    let path = &plan.path_references()[0];
    assert_eq!(path.file_name(), "./dependency.ts");
    assert_eq!(
        path.length(),
        "./dependency.ts".encode_utf16().count() as u32
    );
    assert_eq!(utf16_text_at(text, path.span()), path.file_name());
    assert!(path.preserve());

    assert_eq!(plan.lib_reference_directives().len(), 1);
    let lib = &plan.lib_reference_directives()[0];
    assert_eq!(lib.file_name(), "es2023");
    assert_eq!(lib.length(), 6);
    assert_eq!(utf16_text_at(text, lib.span()), lib.file_name());
    assert!(lib.preserve());

    assert_eq!(plan.type_reference_directives().len(), 1);
    assert!(plan.type_reference_directives()[0].preserve());
    assert_eq!(plan.module_requests().len(), 1);
    assert_eq!(plan.observed_request_occurrence_count(), 4);
}

#[test]
fn recoverable_parse_diagnostics_do_not_hide_source_requests() {
    let source = source_at(
        "/index.ts",
        concat!(
            "/// <reference resolution-mode=\"import\" />\n",
            "const broken = ;\n",
            "import \"dependency\";\n",
        ),
        Some(ResolutionMode::EsNext),
    );

    let plan = plan_source_requests(&source, &node_options())
        .expect("tsc still discovers requests from a recovered source file");
    assert!(plan.path_references().is_empty());
    assert_eq!(plan.module_requests().len(), 1);
    assert_eq!(plan.module_requests()[0].specifier(), "dependency");
}

#[test]
fn source_plan_accepts_the_es2015_module_default_used_by_typings_fixtures() {
    let source = source_at(
        "/a.ts",
        "/// <reference types=\"jquery\" />\nimport \"module-request\";\n",
        None,
    );
    let options = CompilerOptions {
        module: Some(5),
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };

    let plan = plan_source_requests(&source, &options).expect("plan ES2015 source requests");
    assert_eq!(plan.module_requests().len(), 1);
    assert_eq!(plan.module_requests()[0].specifier(), "module-request");
    assert_eq!(plan.module_requests()[0].mode(), ResolutionMode::EsNext);
    assert_eq!(plan.type_reference_directives().len(), 1);
    assert_eq!(
        plan.type_reference_directives()[0].key().specifier(),
        "jquery"
    );
}

#[test]
fn import_helpers_prepends_the_exact_synthetic_tslib_request() {
    let source = source_at(
        "/main.ts",
        "export class C { #value = 1; }\nimport \"tslib\";\nimport \"after\";\n",
        None,
    );
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        import_helpers: Some(true),
        isolated_modules: Some(true),
        ..CompilerOptions::default()
    };

    let plan = plan_source_requests(&source, &options).expect("plan synthetic tslib request");
    assert_eq!(
        plan.module_requests()
            .iter()
            .map(|request| (request.specifier(), request.mode()))
            .collect::<Vec<_>>(),
        [
            ("tslib", ResolutionMode::EsNext),
            ("after", ResolutionMode::EsNext),
        ]
    );
    assert!(plan
        .module_requests()
        .iter()
        .all(|request| request.source().as_path() == Path::new("/main.ts")));
}

#[test]
fn synthetic_tslib_request_obeys_the_upstream_source_boundary() {
    let options = CompilerOptions {
        module: Some(99),
        module_resolution: Some(100),
        import_helpers: Some(true),
        ..CompilerOptions::default()
    };
    for (file_name, text) in [
        ("/global.ts", "const value = 1;\n"),
        ("/external.d.ts", "export declare const value: number;\n"),
    ] {
        let plan = plan_source_requests(&source_at(file_name, text, None), &options)
            .expect("plan non-synthetic control");
        assert!(plan.module_requests().is_empty(), "{file_name}");
    }

    let isolated = CompilerOptions {
        isolated_modules: Some(true),
        ..options.clone()
    };
    let plan = plan_source_requests(
        &source_at("/isolated.ts", "const value = 1;\n", None),
        &isolated,
    )
    .expect("plan isolated synthetic request");
    assert_eq!(plan.module_requests().len(), 1);
    assert_eq!(plan.module_requests()[0].specifier(), "tslib");
    assert_eq!(plan.module_requests()[0].mode(), ResolutionMode::EsNext);

    let forced = CompilerOptions {
        module_detection: Some(3),
        ..options.clone()
    };
    let plan = plan_source_requests(
        &source_at("/forced.ts", "const value = 1;\n", None),
        &forced,
    )
    .expect("plan force-detected synthetic request");
    assert_eq!(plan.module_requests().len(), 1);
    assert_eq!(plan.module_requests()[0].specifier(), "tslib");

    let package_esm = source_at(
        "/package-source.ts",
        "const value = 1;\n",
        Some(ResolutionMode::EsNext),
    );
    let plan = plan_source_requests(&package_esm, &options)
        .expect("plan package-format synthetic request");
    assert_eq!(plan.module_requests().len(), 1);
    assert_eq!(plan.module_requests()[0].specifier(), "tslib");

    let jsx = CompilerOptions {
        jsx: Some(4),
        ..options.clone()
    };
    assert!(matches!(
        plan_source_requests(&source_at("/automatic.tsx", "<div />;\n", None), &jsx),
        Err(ResolutionError::Unsupported { .. })
    ));

    let jsx_import_source = CompilerOptions {
        jsx_import_source: Some("preact".to_owned()),
        ..options.clone()
    };
    assert!(matches!(
        plan_source_requests(
            &source_at("/option.tsx", "const value = 1;\n", None),
            &jsx_import_source,
        ),
        Err(ResolutionError::Unsupported { .. })
    ));

    assert!(matches!(
        plan_source_requests(
            &source_at(
                "/pragma.tsx",
                "/** @jsxRuntime automatic */\nconst value = 1;\n",
                None,
            ),
            &options,
        ),
        Err(ResolutionError::Unsupported { .. })
    ));
}

#[test]
fn request_modes_follow_node_bundler_and_emit_module_semantics() {
    let requests = concat!(
        "import alias = require(\"required\");\n",
        "import {} from \"static\";\n",
        "import(\"dynamic\");\n",
    );
    let cases = [
        (
            "node CommonJS source",
            CompilerOptions {
                module: Some(102),
                module_resolution: Some(3),
                ..CompilerOptions::default()
            },
            source_at("/index.cts", requests, Some(ResolutionMode::CommonJs)),
            [
                ResolutionMode::CommonJs,
                ResolutionMode::CommonJs,
                ResolutionMode::EsNext,
            ],
        ),
        (
            "NodeNext ESM source",
            CompilerOptions {
                module: Some(199),
                module_resolution: Some(99),
                ..CompilerOptions::default()
            },
            source_at("/index.mts", requests, Some(ResolutionMode::EsNext)),
            [
                ResolutionMode::CommonJs,
                ResolutionMode::EsNext,
                ResolutionMode::EsNext,
            ],
        ),
        (
            "Bundler CommonJS source",
            CompilerOptions {
                module: Some(1),
                module_resolution: Some(100),
                ..CompilerOptions::default()
            },
            source_at("/index.ts", requests, None),
            [
                ResolutionMode::CommonJs,
                ResolutionMode::CommonJs,
                ResolutionMode::CommonJs,
            ],
        ),
        (
            "Bundler explicit ESM source under CommonJS",
            CompilerOptions {
                module: Some(1),
                module_resolution: Some(100),
                ..CompilerOptions::default()
            },
            source_at("/index.mts", requests, Some(ResolutionMode::EsNext)),
            [
                ResolutionMode::CommonJs,
                ResolutionMode::EsNext,
                ResolutionMode::EsNext,
            ],
        ),
        (
            "Bundler ESNext source",
            CompilerOptions {
                module: Some(99),
                module_resolution: Some(100),
                ..CompilerOptions::default()
            },
            source_at("/index.ts", requests, None),
            [
                ResolutionMode::CommonJs,
                ResolutionMode::EsNext,
                ResolutionMode::EsNext,
            ],
        ),
        (
            "Bundler Preserve CommonJS source",
            CompilerOptions {
                module: Some(200),
                module_resolution: Some(100),
                ..CompilerOptions::default()
            },
            source_at("/index.cts", requests, Some(ResolutionMode::CommonJs)),
            [
                ResolutionMode::CommonJs,
                ResolutionMode::CommonJs,
                ResolutionMode::EsNext,
            ],
        ),
        (
            "Bundler Preserve default source",
            CompilerOptions {
                module: Some(200),
                module_resolution: Some(100),
                ..CompilerOptions::default()
            },
            source_at("/index.ts", requests, None),
            [
                ResolutionMode::CommonJs,
                ResolutionMode::EsNext,
                ResolutionMode::EsNext,
            ],
        ),
    ];

    for (label, options, source, expected_modes) in cases {
        let planned = plan_module_requests(&source, &options)
            .unwrap_or_else(|error| panic!("{label}: request planning failed: {error}"));
        assert_eq!(planned.len(), 3, "{label}");
        assert_eq!(planned[0].specifier(), "required", "{label}");
        assert_eq!(planned[1].specifier(), "static", "{label}");
        assert_eq!(planned[2].specifier(), "dynamic", "{label}");
        assert_eq!(
            planned.iter().map(|key| key.mode()).collect::<Vec<_>>(),
            expected_modes,
            "{label}"
        );
    }
}

#[test]
fn expanded_javascript_require_calls_use_the_effective_commonjs_mode() {
    let text = concat!(
        "const untyped = require(\"untyped\");\n",
        "const templated = require(`templated`);\n",
        "import \"static\";\n",
    );
    for (label, module_resolution, expected_modes) in [
        (
            "Bundler",
            100,
            [
                ResolutionMode::CommonJs,
                ResolutionMode::CommonJs,
                ResolutionMode::EsNext,
            ],
        ),
        (
            "Node16",
            3,
            [
                ResolutionMode::CommonJs,
                ResolutionMode::CommonJs,
                ResolutionMode::EsNext,
            ],
        ),
        (
            "Classic",
            1,
            [
                ResolutionMode::Unspecified,
                ResolutionMode::Unspecified,
                ResolutionMode::Unspecified,
            ],
        ),
        (
            "Node10",
            2,
            [
                ResolutionMode::Unspecified,
                ResolutionMode::Unspecified,
                ResolutionMode::Unspecified,
            ],
        ),
    ] {
        let options = CompilerOptions {
            module: Some(99),
            module_resolution: Some(module_resolution),
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let requests = plan_module_requests(&source_at("/bug40140.js", text, None), &options)
            .unwrap_or_else(|error| panic!("{label}: request planning failed: {error}"));
        assert_eq!(
            requests
                .iter()
                .map(|request| (request.specifier(), request.mode()))
                .collect::<Vec<_>>(),
            [
                ("static", expected_modes[2]),
                ("untyped", expected_modes[0]),
                ("templated", expected_modes[1]),
            ],
            "{label}"
        );
    }
}

#[test]
fn module_augmentation_uses_static_mode_and_deduplicates_its_import() {
    let source = source_at(
        "/a.ts",
        concat!(
            "declare module \"foo\" { export const x: number; }\n",
            "import { x } from \"foo\";\n",
        ),
        None,
    );
    let options = CompilerOptions {
        module: Some(1),
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };

    let plan = plan_source_requests(&source, &options).expect("plan module augmentation");
    let requests = plan.module_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].specifier(), "foo");
    assert_eq!(requests[0].mode(), ResolutionMode::CommonJs);
    assert_eq!(plan.module_request_loads_source(&requests[0]), Some(true));
}

#[test]
fn external_module_augmentation_resolves_without_loading_its_target() {
    let source = source_at(
        "/a.ts",
        "export {};\ndeclare module \"foo\" { export const x: number; }\n",
        None,
    );
    let options = CompilerOptions {
        module: Some(1),
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };

    let plan = plan_source_requests(&source, &options).expect("plan resolution-only augmentation");
    assert_eq!(plan.module_requests().len(), 1);
    assert_eq!(
        plan.module_request_loads_source(&plan.module_requests()[0]),
        Some(false)
    );
}

#[test]
fn imports_precede_augmentations_and_a_loadable_duplicate_wins() {
    let source = source_at(
        "/a.ts",
        concat!(
            "export {};\n",
            "declare module \"foo\" { export const x: number; }\n",
            "import \"bar\";\n",
            "import \"foo\";\n",
        ),
        None,
    );
    let options = CompilerOptions {
        module: Some(1),
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };

    let plan = plan_source_requests(&source, &options).expect("plan imports before augmentations");
    assert_eq!(
        plan.module_requests()
            .iter()
            .map(|request| request.specifier())
            .collect::<Vec<_>>(),
        ["bar", "foo"]
    );
    assert!(plan
        .module_requests()
        .iter()
        .all(|request| plan.module_request_loads_source(request) == Some(true)));
}

#[test]
fn jsdoc_imports_are_planned_only_for_javascript_files() {
    let text = "/** @import { Value } from 'dependency' */\nconst value = 0;\n";
    let options = CompilerOptions {
        module: Some(1),
        module_resolution: Some(100),
        allow_js: true,
        ..CompilerOptions::default()
    };

    let typescript = plan_source_requests(&source_at("/a.ts", text, None), &options)
        .expect("TypeScript JSDoc control");
    assert!(typescript.module_requests().is_empty());

    let javascript = plan_source_requests(&source_at("/a.js", text, None), &options)
        .expect("JavaScript JSDoc request");
    assert_eq!(javascript.module_requests().len(), 1);
    assert_eq!(javascript.module_requests()[0].specifier(), "dependency");
    assert_eq!(
        javascript.module_request_loads_source(&javascript.module_requests()[0]),
        Some(true)
    );
}

#[test]
fn top_level_script_ambient_module_has_no_resolution_request() {
    let options = CompilerOptions {
        module: Some(1),
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };

    for file_name in ["/declarations.d.ts", "/ambient.ts"] {
        let source = source_at(
            file_name,
            "declare module \"foo\" { export const x: number; }\n",
            None,
        );
        let requests = plan_module_requests(&source, &options)
            .unwrap_or_else(|error| panic!("{file_name}: ambient planning failed: {error}"));
        assert!(requests.is_empty(), "{file_name}");
    }
}

#[test]
fn bare_string_named_module_in_external_source_is_not_an_augmentation_request() {
    let source = source_at(
        "/bare.ts",
        concat!("export {};\n", "module \"foo\" { export const x = 1; }\n",),
        None,
    );
    let options = CompilerOptions {
        module: Some(1),
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };

    let requests = plan_module_requests(&source, &options).expect("plan bare module control");
    assert!(requests.is_empty());
}

#[test]
fn module_body_requests_fail_closed_instead_of_leaking_into_the_source_plan() {
    let options = CompilerOptions {
        module: Some(1),
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };
    for (label, file_name, text) in [
        (
            "external bare module",
            "/bare.ts",
            "export {}; module \"foo\" { import \"bar\"; }\n",
        ),
        (
            "external augmentation",
            "/augmentation.ts",
            "export {}; declare module \"foo\" { import \"bar\"; }\n",
        ),
        (
            "script ambient non-relative import",
            "/ambient.d.ts",
            "declare module \"foo\" { import \"bar\"; }\n",
        ),
        (
            "script ambient relative import",
            "/ambient-relative.d.ts",
            "declare module \"foo\" { import \"./bar\"; }\n",
        ),
    ] {
        let result = plan_module_requests(&source_at(file_name, text, None), &options);
        assert!(
            matches!(result, Err(ResolutionError::Unsupported { .. })),
            "{label}: {result:?}"
        );
    }
}

#[test]
fn nested_string_named_module_fails_closed_without_ambient_context_tracking() {
    let source = source_at(
        "/nested.ts",
        concat!(
            "export {};\n",
            "declare namespace outer { module \"foo\" { export const x: number; } }\n",
        ),
        None,
    );
    let options = CompilerOptions {
        module: Some(1),
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };

    assert!(matches!(
        plan_module_requests(&source, &options),
        Err(ResolutionError::Unsupported { .. })
    ));
}

#[test]
fn classic_import_types_retain_explicit_modes_and_use_unspecified_fallback() {
    let source = source_at(
        "/app.ts",
        concat!(
            "type Default = typeof import(\"foo\").x;\n",
            "type Import = typeof import(\"foo\", { with: { \"resolution-mode\": \"import\" } }).x;\n",
            "type Require = typeof import(\"foo\", { with: { \"resolution-mode\": \"require\" } }).x;\n",
            "type ImportRelative = typeof import(\"./other\", { with: { \"resolution-mode\": \"import\" } }).x;\n",
            "type RequireRelative = typeof import(\"./other\", { with: { \"resolution-mode\": \"require\" } }).x;\n",
        ),
        None,
    );
    let classic = CompilerOptions {
        module: Some(99),
        module_resolution: Some(1),
        ..CompilerOptions::default()
    };

    let requests = plan_module_requests(&source, &classic).expect("plan Classic import types");
    assert_eq!(
        requests
            .iter()
            .map(|request| (request.specifier(), request.mode()))
            .collect::<Vec<_>>(),
        [
            ("foo", ResolutionMode::Unspecified),
            ("foo", ResolutionMode::EsNext),
            ("foo", ResolutionMode::CommonJs),
            ("./other", ResolutionMode::EsNext),
            ("./other", ResolutionMode::CommonJs),
        ]
    );

    let bundler = CompilerOptions {
        module_resolution: Some(100),
        ..classic
    };
    let requests = plan_module_requests(&source, &bundler).expect("plan Bundler import types");
    assert_eq!(
        requests
            .iter()
            .map(|request| (request.specifier(), request.mode()))
            .collect::<Vec<_>>(),
        [
            ("foo", ResolutionMode::EsNext),
            ("foo", ResolutionMode::CommonJs),
            ("./other", ResolutionMode::EsNext),
            ("./other", ResolutionMode::CommonJs),
        ],
        "the default and explicit-import requests share one Bundler cache key"
    );
}

#[test]
fn classic_type_only_imports_retain_explicit_modes_and_use_unspecified_fallback() {
    let source = source_at(
        "/app.ts",
        concat!(
            "import type { x as Default } from \"foo\";\n",
            "import type { x as Import } from \"foo\" with { \"resolution-mode\": \"import\" };\n",
            "import type { x as Require } from \"foo\" with { \"resolution-mode\": \"require\" };\n",
            "import type { x as ImportRelative } from \"./other\" with { \"resolution-mode\": \"import\" };\n",
            "import type { x as RequireRelative } from \"./other\" with { \"resolution-mode\": \"require\" };\n",
        ),
        None,
    );
    let classic = CompilerOptions {
        module: Some(99),
        module_resolution: Some(1),
        ..CompilerOptions::default()
    };

    let requests = plan_module_requests(&source, &classic).expect("plan Classic type imports");
    assert_eq!(
        requests
            .iter()
            .map(|request| (request.specifier(), request.mode()))
            .collect::<Vec<_>>(),
        [
            ("foo", ResolutionMode::Unspecified),
            ("foo", ResolutionMode::EsNext),
            ("foo", ResolutionMode::CommonJs),
            ("./other", ResolutionMode::EsNext),
            ("./other", ResolutionMode::CommonJs),
        ]
    );

    let bundler = CompilerOptions {
        module_resolution: Some(100),
        ..classic
    };
    let requests = plan_module_requests(&source, &bundler).expect("plan Bundler type imports");
    assert_eq!(
        requests
            .iter()
            .map(|request| (request.specifier(), request.mode()))
            .collect::<Vec<_>>(),
        [
            ("foo", ResolutionMode::EsNext),
            ("foo", ResolutionMode::CommonJs),
            ("./other", ResolutionMode::EsNext),
            ("./other", ResolutionMode::CommonJs),
        ],
        "the default and explicit-import requests share one Bundler cache key"
    );
}

#[test]
fn node10_static_import_uses_the_unspecified_resolution_mode() {
    let source = source_at("/index.ts", "import { pkg } from \"pkg\";\n", None);
    let options = CompilerOptions {
        module: Some(99),
        module_resolution: Some(2),
        ..CompilerOptions::default()
    };

    let requests = plan_module_requests(&source, &options).expect("plan Node10 static import");
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].specifier(), "pkg");
    assert_eq!(requests[0].mode(), ResolutionMode::Unspecified);
}

#[test]
fn node10_amd_projects_plan_static_imports_and_javascript_requires() {
    let sources = [
        ("/root.ts", "import * as m1 from \"m1\";\n", "m1"),
        (
            "/node_modules/m1/index.js",
            "var m2 = require(\"m2\");\n",
            "m2",
        ),
    ];
    for module in [1, 2] {
        let options = CompilerOptions {
            allow_js: true,
            module: Some(module),
            module_resolution: Some(2),
            ..CompilerOptions::default()
        };
        for (file_name, text, specifier) in sources {
            let plan = plan_source_requests(&source_at(file_name, text, None), &options)
                .expect("plan CommonJS/AMD Node10 project requests");
            assert_eq!(plan.module_requests().len(), 1);
            assert_eq!(plan.module_requests()[0].specifier(), specifier);
            assert_eq!(
                plan.module_requests()[0].mode(),
                ResolutionMode::Unspecified
            );
        }
    }
}

#[test]
fn resolution_mode_attributes_outside_the_owned_type_only_shape_fail_closed() {
    let options = CompilerOptions {
        module: Some(99),
        module_resolution: Some(1),
        ..CompilerOptions::default()
    };
    for text in [
        "import { x } from \"foo\" with { \"resolution-mode\": \"import\" };\n",
        "import type { x } from \"foo\" with { \"type\": \"json\" };\n",
        "type Imported = import(\"foo\", { with: { \"type\": \"json\" } });\n",
    ] {
        assert!(matches!(
            plan_module_requests(&source_at("/index.ts", text, None), &options),
            Err(ResolutionError::Unsupported { .. })
        ));
    }
}

#[test]
fn raw_default_commonjs_can_fall_back_to_the_emit_module_kind() {
    let source = PreparedSourceFile::new(
        path("/index.ts"),
        "import {} from \"static\";\nimport(\"dynamic\");\n",
    )
    .with_implied_node_formats(Some(ResolutionMode::CommonJs), None);
    assert_eq!(source.implied_node_format(), Some(ResolutionMode::CommonJs));
    assert_eq!(source.implied_node_format_for_emit(), None);

    let options = CompilerOptions {
        module: Some(99),
        module_resolution: Some(99),
        ..CompilerOptions::default()
    };
    let planned = plan_module_requests(&source, &options).expect("plan effective ESM requests");
    assert_eq!(planned.len(), 2);
    assert!(planned
        .iter()
        .all(|request| request.mode() == ResolutionMode::EsNext));
}

#[test]
fn internal_import_equals_does_not_publish_a_resolution_request() {
    let source = source("import alias = namespace.value;\n", ResolutionMode::EsNext);
    let requests =
        plan_module_requests(&source, &node_options()).expect("plan internal import equals");
    assert!(requests.is_empty());
}

#[test]
fn expanded_plan_still_fails_closed_for_unowned_request_syntax() {
    for text in [
        "type Imported = import(123).Value;\n",
        "const required = require(\"inner/call\");\n",
        "import(getName());\n",
    ] {
        assert!(matches!(
            plan_module_requests(&source(text, ResolutionMode::EsNext), &node_options()),
            Err(ResolutionError::Unsupported { .. })
        ));
    }
}

#[test]
fn incomplete_static_plans_fail_closed() {
    for text in [
        "export * from \"inner/exported\";\n",
        "import alias = require(\"inner/required\");\n",
        "type Imported = import(\"inner/typed\").Value;\n",
        "import(\"inner/dynamic\");\n",
        "const required = require(\"inner/call\");\n",
    ] {
        let error =
            plan_static_module_requests(&source(text, ResolutionMode::EsNext), &node_options())
                .expect_err("unplanned module-bearing syntax must fail closed");
        let ResolutionError::Unsupported { feature, detail } = error else {
            panic!("expected typed unsupported request plan, got {error:?}");
        };
        assert_eq!(feature, "static-module-request-plan");
        assert!(!detail.is_empty());
    }

    let missing_mode = PreparedSourceFile::new(path("/index.ts"), "import \"inner/pkg\";\n");
    assert!(matches!(
        plan_static_module_requests(&missing_mode, &node_options()),
        Err(ResolutionError::Unsupported { .. })
    ));

    let jsdoc_import = source_at(
        "/a.js",
        "/** @import { Value } from 'inner/jsdoc' */\nconst value = 0;\n",
        Some(ResolutionMode::EsNext),
    );
    assert!(matches!(
        plan_static_module_requests(&jsdoc_import, &node_options()),
        Err(ResolutionError::Unsupported { .. })
    ));

    let checked_js_require = source_at(
        "/index.js",
        "const required = require(\"inner/call\");\n",
        Some(ResolutionMode::CommonJs),
    );
    let checked_js_options = CompilerOptions {
        check_js: Some(true),
        ..node_options()
    };
    assert!(matches!(
        plan_static_module_requests(&checked_js_require, &checked_js_options),
        Err(ResolutionError::Unsupported { .. })
    ));
}

fn utf16_text_at(text: &str, span: std::ops::Range<u32>) -> String {
    let utf16 = text.encode_utf16().collect::<Vec<_>>();
    String::from_utf16(&utf16[span.start as usize..span.end as usize])
        .expect("directive span contains valid UTF-16")
}

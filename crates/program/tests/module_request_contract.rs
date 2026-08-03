use std::path::Path;

use tsc_program::{
    plan_module_requests, plan_source_requests, plan_static_module_requests, CompilerOptions,
    PreparedSourceFile, ProgramPath, ResolutionError, ResolutionMode,
};

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
    assert_eq!(requests[1].specifier(), "inner");
    assert_eq!(requests[1].mode(), ResolutionMode::EsNext);
    assert_eq!(requests[2].specifier(), "inner/static");
    assert_eq!(requests[2].mode(), ResolutionMode::CommonJs);
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
            ("foo", ResolutionMode::EsNext),
            ("after-jsdoc", ResolutionMode::CommonJs),
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
        "type Imported = import(\"inner/typed\").Value;\n",
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
}

fn utf16_text_at(text: &str, span: std::ops::Range<u32>) -> String {
    let utf16 = text.encode_utf16().collect::<Vec<_>>();
    String::from_utf16(&utf16[span.start as usize..span.end as usize])
        .expect("directive span contains valid UTF-16")
}

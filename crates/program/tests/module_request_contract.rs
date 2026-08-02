use std::path::Path;

use tsc_program::{
    plan_module_requests, plan_static_module_requests, CompilerOptions, PreparedSourceFile,
    ProgramPath, ResolutionError, ResolutionMode,
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
}

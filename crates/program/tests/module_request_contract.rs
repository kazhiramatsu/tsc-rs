use std::path::Path;

use tsc_program::{
    plan_static_module_requests, CompilerOptions, PreparedSourceFile, ProgramPath, ResolutionError,
    ResolutionMode,
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
    PreparedSourceFile::new(path("/index.mts"), text).with_implied_node_format(mode)
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

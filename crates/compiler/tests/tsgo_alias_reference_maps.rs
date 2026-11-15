//! Synthetic class aliases keep tsgo source-map and lexical boundaries.
use std::path::PathBuf;

use tsc_compiler::{MemoryOutputSink, ProgramSession};
use tsc_host::MemoryCompilerHost;
use tsc_program::{load_emitting_program, LibraryCatalog, ProgramLoadLimits, ProgramOptions};
use tsc_types::CompilerOptions;

#[allow(dead_code)]
#[path = "support/witness_libraries.rs"]
mod witness_libraries;

fn assert_maps(stem: &str, source: &str, declaration_only: bool, expected: &[(&str, &str)]) {
    let mut builder = MemoryCompilerHost::builder("/project")
        .case_sensitive(true)
        .file(format!("/project/{stem}.ts"), source.as_bytes())
        .file("/project/package.json", b"{\"type\":\"module\"}")
        .file(
            "/project/dep.ts",
            include_bytes!("fixtures/tsgo-alias-reference-maps/dep.ts"),
        );
    for (path, bytes) in witness_libraries::files() {
        builder = builder.file(path.as_str(), bytes.as_slice());
    }
    let host = builder.build().expect("memory host");
    let options = CompilerOptions {
        target: Some(11),            // ES2024
        module: Some(199),           // NodeNext
        module_resolution: Some(99), // NodeNext
        strict: Some(stem != "t-alias-boundaries"),
        experimental_decorators: true,
        use_define_for_class_fields: Some(false),
        source_map: Some(true),
        declaration: Some(true),
        declaration_map: Some(true),
        emit_declaration_only: Some(declaration_only),
        out_dir: Some("/project/out".into()),
        ..CompilerOptions::default()
    };
    let prepared = load_emitting_program(
        &host,
        &[PathBuf::from(format!("/project/{stem}.ts"))],
        options,
        ProgramOptions::default(),
        &LibraryCatalog::typescript_7_1("/lib"),
        ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024),
    )
    .expect("emitting program");
    let mut sink = MemoryOutputSink::new();
    ProgramSession::new(prepared).emit(&mut sink).expect("emit");
    for &(extension, pinned) in expected {
        let path = format!("/project/out/{stem}.{extension}");
        let artifact = sink
            .writes()
            .iter()
            .find(|artifact| artifact.path().to_string_lossy() == path)
            .expect("expected output");
        assert_eq!(
            artifact.materialized_bytes().as_ref(),
            pinned.as_bytes(),
            "{path}, declaration_only={declaration_only}"
        );
    }
}

#[test]
fn alias_reference_maps_match_pinned_tsgo() {
    // Expected maps and generated text are the pinned tsgo binary's exact output.
    macro_rules! case {
        ($stem:literal) => {
            assert_maps(
                $stem,
                include_str!(concat!("fixtures/tsgo-alias-reference-maps/", $stem, ".ts")),
                false,
                &[
                    (
                        "js",
                        include_str!(concat!("fixtures/tsgo-alias-reference-maps/", $stem, ".js")),
                    ),
                    (
                        "js.map",
                        include_str!(concat!(
                            "fixtures/tsgo-alias-reference-maps/",
                            $stem,
                            ".js.map"
                        )),
                    ),
                    (
                        "d.ts",
                        include_str!(concat!(
                            "fixtures/tsgo-alias-reference-maps/",
                            $stem,
                            ".d.ts"
                        )),
                    ),
                    (
                        "d.ts.map",
                        include_str!(concat!(
                            "fixtures/tsgo-alias-reference-maps/",
                            $stem,
                            ".d.ts.map"
                        )),
                    ),
                ],
            );
        };
    }
    case!("q-alias-method-maps");
    case!("r-undecorated-maps");
    case!("t-alias-boundaries");
    case!("u-unreferenced-method-maps");
    case!("w-alias-member-name-maps");
}

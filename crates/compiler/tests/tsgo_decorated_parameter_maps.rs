//! Shared-name map flags from the legacy decorator pass reach declaration emit.
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
            include_bytes!("fixtures/tsgo-decorated-parameter-maps/dep.ts"),
        );
    for (path, bytes) in witness_libraries::files() {
        builder = builder.file(path.as_str(), bytes.as_slice());
    }
    let host = builder.build().expect("memory host");
    let options = CompilerOptions {
        target: Some(11),            // ES2024
        module: Some(199),           // NodeNext
        module_resolution: Some(99), // NodeNext
        strict: Some(true),
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
fn decorated_parameter_maps_match_pinned_tsgo_in_both_emit_modes() {
    // Every expected byte is the pinned tsgo binary's output. In particular,
    // full emit and declaration-only emit intentionally differ for P and S.
    macro_rules! case {
        ($stem:literal, $js_map:literal) => {{
            assert_maps(
                $stem,
                include_str!(concat!(
                    "fixtures/tsgo-decorated-parameter-maps/",
                    $stem,
                    ".ts"
                )),
                false,
                &[
                    (
                        "js",
                        include_str!(concat!(
                            "fixtures/tsgo-decorated-parameter-maps/",
                            $stem,
                            ".js"
                        )),
                    ),
                    (
                        "d.ts",
                        include_str!(concat!(
                            "fixtures/tsgo-decorated-parameter-maps/",
                            $stem,
                            ".d.ts"
                        )),
                    ),
                    (
                        "d.ts.map",
                        include_str!(concat!(
                            "fixtures/tsgo-decorated-parameter-maps/",
                            $stem,
                            ".d.ts.map"
                        )),
                    ),
                ],
            );
            if $js_map {
                assert_maps(
                    $stem,
                    include_str!(concat!(
                        "fixtures/tsgo-decorated-parameter-maps/",
                        $stem,
                        ".ts"
                    )),
                    false,
                    &[(
                        "js.map",
                        include_str!(concat!(
                            "fixtures/tsgo-decorated-parameter-maps/",
                            $stem,
                            ".js.map"
                        )),
                    )],
                );
            }
            assert_maps(
                $stem,
                include_str!(concat!(
                    "fixtures/tsgo-decorated-parameter-maps/",
                    $stem,
                    ".ts"
                )),
                true,
                &[(
                    "d.ts.map",
                    include_str!(concat!(
                        "fixtures/tsgo-decorated-parameter-maps/",
                        $stem,
                        ".declaration-only.d.ts.map"
                    )),
                )],
            );
        }};
    }
    case!("p-decorated-parameter-maps", true);
    case!("r-undecorated-maps", true);
    case!("s-parameter-phase-maps", false);
    case!("v-updated-binding-name-maps", true);
}

#[test]
fn parameter_property_name_maps_match_pinned_tsgo_in_both_field_modes() {
    macro_rules! case {
        ($stem:literal, $mode:literal, $define:literal) => {
            (
                $stem,
                include_str!(concat!(
                    "fixtures/tsgo-decorated-parameter-maps/",
                    $stem,
                    ".ts"
                )),
                $define,
                [
                    (
                        "js",
                        include_str!(concat!(
                            "fixtures/tsgo-decorated-parameter-maps/",
                            $mode,
                            "/",
                            $stem,
                            ".js"
                        )),
                    ),
                    (
                        "js.map",
                        include_str!(concat!(
                            "fixtures/tsgo-decorated-parameter-maps/",
                            $mode,
                            "/",
                            $stem,
                            ".js.map"
                        )),
                    ),
                    (
                        "d.ts",
                        include_str!(concat!(
                            "fixtures/tsgo-decorated-parameter-maps/",
                            $mode,
                            "/",
                            $stem,
                            ".d.ts"
                        )),
                    ),
                    (
                        "d.ts.map",
                        include_str!(concat!(
                            "fixtures/tsgo-decorated-parameter-maps/",
                            $mode,
                            "/",
                            $stem,
                            ".d.ts.map"
                        )),
                    ),
                ],
            )
        };
    }
    for (stem, source, define, expected) in [
        case!("w-defaulted-property", "assign", false),
        case!("x-no-field", "assign", false),
        case!("y-plain-parameter", "assign", false),
        case!("z-no-default", "assign", false),
        case!("w-defaulted-property", "define", true),
        case!("x-no-field", "define", true),
        case!("y-plain-parameter", "define", true),
        case!("z-no-default", "define", true),
    ] {
        let host = MemoryCompilerHost::builder("/project")
            .case_sensitive(true)
            .file(format!("/project/{stem}.ts"), source.as_bytes())
            .file("/project/package.json", b"{\"type\":\"commonjs\"}")
            .build()
            .expect("memory host");
        let prepared = load_emitting_program(
            &host,
            &[PathBuf::from(format!("/project/{stem}.ts"))],
            CompilerOptions {
                target: Some(11),
                module: Some(199),
                module_resolution: Some(99),
                no_resolve: Some(true),
                experimental_decorators: true,
                use_define_for_class_fields: Some(define),
                source_map: Some(true),
                declaration: Some(true),
                declaration_map: Some(true),
                out_dir: Some("/project/out".into()),
                ..CompilerOptions::default()
            },
            ProgramOptions::default()
                .with_no_lib(true)
                .with_types(Vec::new()),
            &LibraryCatalog::typescript_7_1("/lib"),
            ProgramLoadLimits::new(64, 128, 16, 1 << 20, 1 << 22),
        )
        .expect("emitting program");
        let mut sink = MemoryOutputSink::new();
        ProgramSession::new(prepared).emit(&mut sink).expect("emit");
        assert_eq!(sink.writes().len(), expected.len());
        for (extension, pinned) in expected {
            let path = format!("/project/out/{stem}.{extension}");
            let artifact = sink
                .writes()
                .iter()
                .find(|artifact| artifact.path().to_string_lossy() == path)
                .expect("expected output");
            assert_eq!(
                artifact.materialized_bytes().as_ref(),
                pinned.as_bytes(),
                "{path}, useDefineForClassFields={define}"
            );
        }
    }
}

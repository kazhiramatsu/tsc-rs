//! Isolated declaration inference through the production emitter, without a harness option floor.

#[test]
fn isolated_inference_matches_complete_typescript_observations() {
    let artifact: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../fixtures/isolated-declaration-inference.json"
    ))
    .expect("frozen TypeScript observations");
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    assert_eq!(artifact["cases"].as_array().unwrap().len(), 12);
    super::h2_7c_declaration_blocking::assert_cases(&artifact);
}

#[test]
fn specialized_isolated_declaration_seams_remain_typed_before_writes() {
    use std::path::{Path, PathBuf};
    use tsc_compiler::{DriverError, MemoryOutputSink, ProgramSession};
    use tsc_emitter::{EmitFailure, TransformError, UnsupportedEmitFeature};
    use tsc_host::MemoryCompilerHost;
    use tsc_program::{
        load_emitting_program, CompilerOptions, LibraryCatalog, ProgramLoadLimits, ProgramOptions,
    };

    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cases = [
        (
            "accessor",
            "export class C { get value() { return 1 + 1; } }",
        ),
        (
            "computed name",
            "const name = 'value'; export class C { [name]: number = 1; }",
        ),
        ("enum initializer", "export enum E { Value = 1 }"),
        ("expando", "export function f(): void {} f.value = 1;"),
    ];
    for (name, text) in cases {
        let root = PathBuf::from("/project/source.ts");
        let mut builder = MemoryCompilerHost::builder("/project").file(&root, text.as_bytes());
        for entry in std::fs::read_dir(workspace.join("vendor/typescript-6.0.3/lib")).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("lib.") && name.ends_with(".d.ts") {
                builder =
                    builder.file(format!("/lib/{name}"), std::fs::read(entry.path()).unwrap());
            }
        }
        let prepared = load_emitting_program(
            &builder.build().unwrap(),
            &[root],
            CompilerOptions {
                target: Some(99),
                module: Some(1),
                declaration: Some(true),
                isolated_declarations: Some(true),
                emit_declaration_only: Some(true),
                strict: Some(true),
                skip_default_lib_check: Some(true),
                ..CompilerOptions::default()
            },
            ProgramOptions::default(),
            &LibraryCatalog::typescript_6_0_3("/lib"),
            ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024),
        )
        .unwrap();
        let mut sink = MemoryOutputSink::new();
        let error = ProgramSession::new(prepared)
            .emit(&mut sink)
            .expect_err(name);
        assert!(
            matches!(error, DriverError::Emit(EmitFailure::Transform(error))
            if matches!(*error, TransformError::Unsupported(UnsupportedEmitFeature::IsolatedDeclarations))),
            "{name}"
        );
        assert!(sink.writes().is_empty(), "{name}");
    }
}

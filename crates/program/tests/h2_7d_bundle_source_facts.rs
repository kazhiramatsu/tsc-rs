//! Program-owned module facts retained from discovery, before checker borrowing.
use serde_json::Value;
use std::path::{Path, PathBuf};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_emitting_program, CompilerOptions, LibraryCatalog, ProgramLoadLimits, ProgramOptions,
};

fn options(value: &Value) -> CompilerOptions {
    let mut options = CompilerOptions::default();
    for (key, value) in value.as_object().unwrap() {
        match key.as_str() {
            "target" => options.target = Some(value.as_i64().unwrap() as i32),
            "module" => options.module = Some(value.as_i64().unwrap() as i32),
            "moduleResolution" => options.module_resolution = Some(value.as_i64().unwrap() as i32),
            "newLine" => options.new_line = Some(value.as_i64().unwrap() as i32),
            "outFile" => options.out_file = value.as_str().map(str::to_owned),
            "outDir" => options.out_dir = value.as_str().map(str::to_owned),
            "declarationDir" => options.declaration_dir = value.as_str().map(str::to_owned),
            "declaration" => options.declaration = value.as_bool(),
            "emitDeclarationOnly" => options.emit_declaration_only = value.as_bool(),
            "sourceMap" => options.source_map = value.as_bool(),
            "inlineSourceMap" => options.inline_source_map = value.as_bool(),
            "declarationMap" => options.declaration_map = value.as_bool(),
            "noEmitOnError" => options.no_emit_on_error = value.as_bool(),
            "noEmit" => options.no_emit = value.as_bool(),
            "noResolve" => options.no_resolve = value.as_bool(),
            "strict" => options.strict = value.as_bool(),
            "skipDefaultLibCheck" => options.skip_default_lib_check = value.as_bool(),
            "noErrorTruncation" => options.no_error_truncation = value.as_bool(),
            "allowJs" => options.allow_js = value.as_bool().unwrap(),
            "noEmitForJsFiles" => options.no_emit_for_js_files = value.as_bool(),
            "resolveJsonModule" => options.resolve_json_module = value.as_bool(),
            "emitBOM" => options.emit_bom = value.as_bool(),
            "listEmittedFiles" => options.list_emitted_files = value.as_bool(),
            other => panic!("unprojected compiler option {other}"),
        }
    }
    options
}

#[test]
fn h2_7d_discovery_retains_exact_bundle_source_facts_and_input_order() {
    let artifact: Value = serde_json::from_slice(include_bytes!(
        "../../emitter/tests/fixtures/bundle-plan.json"
    ))
    .unwrap();
    let library_directory =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/typescript-6.0.3/lib");
    let libraries = std::fs::read_dir(library_directory)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_string_lossy().into_owned(),
                entry.path(),
            )
        })
        .filter(|(name, _)| name.starts_with("lib.") && name.ends_with(".d.ts"))
        .map(|(name, path)| (name, std::fs::read(path).unwrap()))
        .collect::<Vec<_>>();
    for case in artifact["cases"].as_array().unwrap() {
        let mut builder = MemoryCompilerHost::builder(case["current_directory"].as_str().unwrap())
            .case_sensitive(case["use_case_sensitive_file_names"].as_bool().unwrap());
        let files = case["files"].as_array().unwrap();
        for file in files {
            builder = builder.file(
                file["path"].as_str().unwrap(),
                file["text"].as_str().unwrap().as_bytes(),
            );
        }
        for (name, bytes) in &libraries {
            builder = builder.file(format!("/lib/{name}"), bytes.clone());
        }
        let host = builder.build().unwrap();
        let roots = case["roots"]
            .as_array()
            .map(|roots| {
                roots
                    .iter()
                    .map(|value| PathBuf::from(value.as_str().unwrap()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| {
                files
                    .iter()
                    .map(|file| PathBuf::from(file["path"].as_str().unwrap()))
                    .collect()
            });
        let options = options(&case["options"]);
        let expected_sources = case["program_sources"].as_array().unwrap();
        for _ in 0..2 {
            let loaded = load_emitting_program(
                &host,
                &roots,
                options.clone(),
                ProgramOptions::default(),
                &LibraryCatalog::typescript_6_0_3("/lib"),
                ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024),
            );
            if options.no_emit == Some(true) {
                assert!(matches!(
                    loaded,
                    Err(tsc_program::ProgramLoadError::InvalidInput {
                        operation: tsc_program::ProgramLoadOperation::ValidateOptions,
                        detail,
                        path: None,
                    }) if detail == "emitting program rejects effective compilerOptions.noEmit=true"
                ));
                continue;
            }
            let prepared = loaded.unwrap_or_else(|error| panic!("{}: {error}", case["case_id"]));
            for source in prepared.source_files() {
                let name = source.path().display().to_string_lossy();
                let expected = expected_sources
                    .iter()
                    .find(|source| source["path"] == name.as_ref())
                    .unwrap_or_else(|| panic!("{} unexpected source {name}", case["case_id"]));
                assert_eq!(
                    source.is_external_module(),
                    expected["is_external_module"].as_bool(),
                    "{} {name}",
                    case["case_id"]
                );
            }
            assert_eq!(
                prepared.source_files().len(),
                expected_sources.len(),
                "{} source count",
                case["case_id"]
            );
            let is_input = |name: &str| files.iter().any(|file| file["path"] == name);
            let actual_order = prepared
                .source_files()
                .iter()
                .map(|source| source.path().display().to_string_lossy().into_owned())
                .filter(|name| is_input(name))
                .collect::<Vec<_>>();
            let expected_order = expected_sources
                .iter()
                .map(|source| source["path"].as_str().unwrap().to_owned())
                .filter(|name| is_input(name))
                .collect::<Vec<_>>();
            assert_eq!(
                actual_order, expected_order,
                "{} input order",
                case["case_id"]
            );
        }
    }
}

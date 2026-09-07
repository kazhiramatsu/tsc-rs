//! Original H2.7c corpus inputs with unmodified effective options and root selection.
use serde_json::Value;
use std::path::{Path, PathBuf};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_emitting_program, parse_config_root_plan, CompilerConfigHost, CompilerOptions,
    ConfigRootPlanRequest, LibraryCatalog, ProgramLoadLimits, ProgramOptions,
};

#[test]
fn h2_7c_corpus_matches_original_typescript_observations() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let artifact = serde_json::from_slice(
        &std::fs::read(workspace.join("ratchets/h2-7c-qualification.v1.json")).unwrap(),
    )
    .unwrap();
    assert_corpus(&artifact);
}

pub(super) fn assert_corpus(artifact: &Value) {
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    let all_cases = artifact["cases"].as_array().unwrap();
    assert_eq!(all_cases.len(), 42);
    assert_eq!(
        all_cases
            .iter()
            .filter(|case| case["disposition"] == "exact")
            .count(),
        31
    );
    assert_eq!(
        all_cases
            .iter()
            .filter(|case| case["disposition"] == "deferred")
            .count(),
        11
    );
    let cases = all_cases
        .iter()
        .filter(|case| !case["replay_input"].is_null())
        .collect::<Vec<_>>();
    assert_eq!(cases.len(), 32);
    let library_directory =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/typescript-6.0.3/lib");
    let mut failures = Vec::new();
    for case in cases {
        let case_id = case["case_id"].as_str().unwrap();
        let compared = std::panic::catch_unwind(|| {
            let input = &case["replay_input"];
            let cwd = input["current_directory"].as_str().unwrap();
            let mut builder = MemoryCompilerHost::builder(cwd);
            for file in input["files"].as_array().unwrap() {
                builder = builder.file(
                    file["path"].as_str().unwrap(),
                    file["text"].as_str().unwrap().as_bytes(),
                );
            }
            if let Some(config) = input["config"].as_object() {
                builder = builder.file(
                    config["path"].as_str().unwrap(),
                    config["text"].as_str().unwrap().as_bytes(),
                );
            }
            for entry in std::fs::read_dir(&library_directory).unwrap() {
                let entry = entry.unwrap();
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with("lib.") && name.ends_with(".d.ts") {
                    builder =
                        builder.file(format!("/lib/{name}"), std::fs::read(entry.path()).unwrap());
                }
            }
            assert!(input["vfs_symlinks"].as_array().unwrap().is_empty());
            let host = builder.build().unwrap();
            let mut options = CompilerOptions::default();
            let effective = case["effective_declaration_options"].as_object().unwrap();
            for (key, value) in effective {
                match key.as_str() {
                    "target" => options.target = Some(value.as_i64().unwrap() as i32),
                    "module" => options.module = Some(value.as_i64().unwrap() as i32),
                    "moduleResolution" => {
                        options.module_resolution = Some(value.as_i64().unwrap() as i32)
                    }
                    "newLine" => options.new_line = Some(value.as_i64().unwrap() as i32),
                    "declaration" => options.declaration = value.as_bool(),
                    "declarationDir" => options.declaration_dir = value.as_str().map(str::to_owned),
                    "rootDir" => options.root_dir = value.as_str().map(str::to_owned),
                    "declarationMap" => options.declaration_map = value.as_bool(),
                    "isolatedDeclarations" => options.isolated_declarations = value.as_bool(),
                    "strict" => options.strict = value.as_bool(),
                    "strictBuiltinIteratorReturn" => {
                        options.strict_builtin_iterator_return = value.as_bool()
                    }
                    "emitDeclarationOnly" => options.emit_declaration_only = value.as_bool(),
                    "noEmitOnError" => options.no_emit_on_error = value.as_bool(),
                    "noResolve" => options.no_resolve = value.as_bool(),
                    "stripInternal" => options.strip_internal = value.as_bool(),
                    "sourceMap" => options.source_map = value.as_bool(),
                    "skipDefaultLibCheck" => options.skip_default_lib_check = value.as_bool(),
                    "noErrorTruncation" => options.no_error_truncation = value.as_bool(),
                    "allowJs" => options.allow_js = value.as_bool().unwrap(),
                    "checkJs" => options.check_js = value.as_bool(),
                    other => panic!("unprojected effective option {other}"),
                }
            }
            if !effective.contains_key("allowJs") {
                options.allow_js = options.check_js == Some(true);
            }
            let mut program_options = if input["config"].is_null() {
                ProgramOptions::default()
            } else {
                let config = &input["config"];
                let name = config["path"].as_str().unwrap();
                parse_config_root_plan(
                    &CompilerConfigHost::new(&host),
                    ConfigRootPlanRequest {
                        file_name: name.to_owned(),
                        text: config["text"].as_str().unwrap().to_owned(),
                        base_path: Path::new(name)
                            .parent()
                            .unwrap()
                            .to_string_lossy()
                            .into_owned(),
                    },
                )
                .unwrap()
                .program_options()
                .clone()
                .with_program_owned_config_option_diagnostics()
            };
            if let Some(default_library) = input["default_library_file_name"].as_str() {
                program_options = program_options.with_default_library_file_name(default_library);
            }
            let roots = input["roots"]
                .as_array()
                .unwrap()
                .iter()
                .map(|name| PathBuf::from(name.as_str().unwrap()))
                .collect::<Vec<_>>();
            let catalog = LibraryCatalog::typescript_6_0_3("/lib");
            let limits = ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024);
            for _ in 0..2 {
                let prepared = load_emitting_program(
                    &host,
                    &roots,
                    options.clone(),
                    program_options.clone(),
                    &catalog,
                    limits,
                )
                .unwrap();
                if let Some(option) = case["rust_expected_unsupported_option"].as_str() {
                    let mut sink = tsc_compiler::MemoryOutputSink::new();
                    let error = tsc_compiler::ProgramSession::new(prepared)
                        .emit(&mut sink)
                        .unwrap_err();
                    assert!(matches!(error, tsc_compiler::DriverError::Emit(
                        tsc_emitter::EmitFailure::UnsupportedCompilerOption{option: actual}) if actual == option));
                    assert!(sink.writes().is_empty());
                } else {
                    super::h2_7b_w4a_controls::assert_exact_observation(
                        case_id,
                        prepared,
                        &case["typescript_observation"],
                    );
                }
            }
        });
        if let Err(error) = compared {
            let detail = error
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| error.downcast_ref::<&str>().map(|text| text.to_string()))
                .unwrap_or_else(|| "non-string panic".to_owned());
            failures.push(format!("{case_id}: {detail}"));
        } else {
            eprintln!("H2.7c corpus PASS {case_id}");
        }
    }
    assert!(
        failures.is_empty(),
        "{} residuals:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

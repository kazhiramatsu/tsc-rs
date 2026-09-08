//! Exact whole-program declaration blocking, including result presence and related diagnostics.

use std::path::{Path, PathBuf};

use serde_json::Value;
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_emitting_config_program, load_emitting_program, parse_config_root_plan,
    CompilerConfigHost, CompilerOptions, ConfigRootPlanRequest, LibraryCatalog, ProgramLoadLimits,
    ProgramOptions,
};

use super::h2_7b_w4a_controls::assert_exact_observation;

#[test]
fn declaration_blocking_matches_complete_typescript_observations() {
    let artifact: Value =
        serde_json::from_slice(include_bytes!("../fixtures/declaration-blocking.json"))
            .expect("frozen TypeScript observations");
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().expect("cases");
    assert_eq!(cases.len(), 22);
    assert_cases(&artifact);
}

pub(super) fn assert_cases(artifact: &Value) {
    assert_cases_with_reporting(artifact, false);
}

pub(super) fn assert_cases_with_reporting(artifact: &Value, command_reporting: bool) {
    assert_cases_with_inspection(artifact, command_reporting, |_, _, _| {});
}

pub(super) fn assert_cases_with_inspection(
    artifact: &Value,
    command_reporting: bool,
    inspect: fn(&str, &tsc_program::PreparedProgram, &Value),
) {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cases = artifact["cases"].as_array().expect("cases");
    let mut failures = Vec::new();
    for case in cases {
        let case_id = case["case_id"].as_str().expect("case id");
        let config_path = case["config_path"]
            .as_str()
            .unwrap_or("/project/tsconfig.json");
        let config_base = Path::new(config_path).parent().expect("config directory");
        let result = std::panic::catch_unwind(|| {
            let mut builder = MemoryCompilerHost::builder("/project").case_sensitive(
                case["use_case_sensitive_file_names"]
                    .as_bool()
                    .unwrap_or(true),
            );
            let mut roots = Vec::new();
            for file in case["files"].as_array().expect("files") {
                let path = PathBuf::from(file["path"].as_str().unwrap());
                builder = builder.file(path.clone(), file["text"].as_str().unwrap().as_bytes());
                roots.push(path);
            }
            if let Some(explicit_roots) = case["roots"].as_array() {
                roots = explicit_roots
                    .iter()
                    .map(|root| PathBuf::from(root.as_str().unwrap()))
                    .collect();
            }
            for entry in std::fs::read_dir(workspace.join("vendor/typescript-6.0.3/lib")).unwrap() {
                let entry = entry.unwrap();
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with("lib.") && name.ends_with(".d.ts") {
                    builder =
                        builder.file(format!("/lib/{name}"), std::fs::read(entry.path()).unwrap());
                }
            }
            if let Some(config) = case["config"].as_str() {
                builder = builder.file(config_path, config.as_bytes());
            }
            let host = builder.build().expect("memory host");
            let mut options = CompilerOptions::default();
            let mut program_options = ProgramOptions::default();
            let program_path = |text: &str| {
                let canonical = tsc_program::canonical_emit_path(
                    Path::new(text),
                    Path::new("/project"),
                    case["use_case_sensitive_file_names"]
                        .as_bool()
                        .unwrap_or(true),
                );
                tsc_program::ProgramPath::from_trusted_parts(text, canonical).unwrap()
            };
            if let Some(config) = case["config_file_path"].as_str() {
                program_options = program_options.with_config_file_path(program_path(config));
            }
            for (key, value) in case["options"].as_object().expect("options") {
                match key.as_str() {
                    "target" => options.target = Some(value.as_i64().unwrap() as i32),
                    "module" => options.module = Some(value.as_i64().unwrap() as i32),
                    "moduleResolution" => {
                        options.module_resolution = Some(value.as_i64().unwrap() as i32)
                    }
                    "moduleDetection" => {
                        options.module_detection = Some(value.as_i64().unwrap() as i32)
                    }
                    "newLine" => options.new_line = Some(value.as_i64().unwrap() as i32),
                    "declaration" => options.declaration = value.as_bool(),
                    "declarationDir" => options.declaration_dir = value.as_str().map(str::to_owned),
                    "outDir" => options.out_dir = value.as_str().map(str::to_owned),
                    "outFile" => options.out_file = value.as_str().map(str::to_owned),
                    "rootDir" => options.root_dir = value.as_str().map(str::to_owned),
                    "noDtsResolution" => options.no_dts_resolution = value.as_bool(),
                    "typeRoots" => {
                        program_options = program_options.with_type_roots(
                            value
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|value| program_path(value.as_str().unwrap()))
                                .collect(),
                        )
                    }
                    "types" => {
                        program_options = program_options.with_types(
                            value
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|value| value.as_str().unwrap().to_owned())
                                .collect(),
                        )
                    }
                    "allowJs" => options.allow_js = value.as_bool().unwrap(),
                    "checkJs" => options.check_js = value.as_bool(),
                    "noEmitForJsFiles" => options.no_emit_for_js_files = value.as_bool(),
                    "resolveJsonModule" => options.resolve_json_module = value.as_bool(),
                    "emitBOM" => options.emit_bom = value.as_bool(),
                    "removeComments" => options.remove_comments = value.as_bool(),
                    "isolatedDeclarations" => options.isolated_declarations = value.as_bool(),
                    "strict" => options.strict = value.as_bool(),
                    "allowSyntheticDefaultImports" => {
                        options.allow_synthetic_default_imports = value.as_bool()
                    }
                    "esModuleInterop" => options.es_module_interop = value.as_bool(),
                    "emitDeclarationOnly" => options.emit_declaration_only = value.as_bool(),
                    "noEmitOnError" => options.no_emit_on_error = value.as_bool(),
                    "stripInternal" => options.strip_internal = value.as_bool(),
                    "sourceMap" => options.source_map = value.as_bool(),
                    "listEmittedFiles" => options.list_emitted_files = value.as_bool(),
                    "skipDefaultLibCheck" => options.skip_default_lib_check = value.as_bool(),
                    "noErrorTruncation" => options.no_error_truncation = value.as_bool(),
                    other => panic!("unexpected option {other}"),
                }
            }
            let catalog = LibraryCatalog::typescript_6_0_3("/lib");
            let limits = ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024);
            for _ in 0..2 {
                let prepared = if let Some(config) = case["config"].as_str() {
                    let plan = parse_config_root_plan(
                        &CompilerConfigHost::new(&host),
                        ConfigRootPlanRequest {
                            file_name: config_path.to_owned(),
                            text: config.to_owned(),
                            base_path: config_base.to_string_lossy().into_owned(),
                        },
                    )
                    .expect("config plan");
                    load_emitting_config_program(&host, &plan, &catalog, limits)
                        .expect("config program")
                } else {
                    load_emitting_program(
                        &host,
                        &roots,
                        options.clone(),
                        program_options.clone(),
                        &catalog,
                        limits,
                    )
                    .expect("direct program")
                };
                inspect(case_id, &prepared, &case["typescript_observation"]);
                // H2.8a now executes the unchanged complete TS observations for
                // historical outDir references. Other later-owner guards remain.
                if let Some(option) = case["rust_expected_unsupported_option"]
                    .as_str()
                    .filter(|option| *option != "outDir")
                {
                    let mut sink = tsc_compiler::MemoryOutputSink::new();
                    let error = tsc_compiler::ProgramSession::new(prepared)
                        .emit(&mut sink)
                        .expect_err("retained later-slice boundary");
                    assert!(
                        matches!(error, tsc_compiler::DriverError::Emit(
                            tsc_emitter::EmitFailure::UnsupportedCompilerOption { option: actual }
                        ) if actual == option),
                        "{case_id}: expected the {option} boundary",
                    );
                    assert!(
                        sink.writes().is_empty(),
                        "{case_id}: no writes before refusal"
                    );
                    continue;
                }
                let blocked = prepared.compiler_options().no_emit_on_error == Some(true)
                    && case["typescript_observation"]["emit_result"]["emit_skipped"] == true;
                let activity = if command_reporting {
                    super::h2_7b_w4a_controls::assert_command_observation(
                        case_id,
                        prepared,
                        &case["typescript_observation"],
                    )
                } else {
                    assert_exact_observation(case_id, prepared, &case["typescript_observation"])
                };
                if blocked {
                    assert_eq!(
                        activity.script_transformer_list_constructions(),
                        0,
                        "{case_id}: no JS transform"
                    );
                    assert_eq!(
                        activity.printer_constructions(),
                        0,
                        "{case_id}: no printing"
                    );
                    assert_eq!(
                        activity.javascript_artifact_creations(),
                        0,
                        "{case_id}: no JS artifact"
                    );
                    assert_eq!(
                        activity.output_sink_write_attempts(),
                        0,
                        "{case_id}: no writes"
                    );
                }
            }
        });
        if let Err(error) = result {
            failures.push(format!(
                "{case_id}: {}",
                error
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| error.downcast_ref::<&str>().copied())
                    .unwrap_or("panic")
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} divergent windows:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

//! Exact whole-program declaration blocking, including result presence and related diagnostics.

use std::path::{Path, PathBuf};

use serde_json::Value;
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_config_program, load_emitting_config_program, load_emitting_program,
    parse_config_root_plan, CompilerConfigHost, CompilerOptions, ConfigRootPlanRequest,
    LibraryCatalog, ProgramLoadLimits, ProgramOptions,
};

use super::h2_7b_w4a_controls::assert_exact_observation;

#[path = "../support/witness_libraries.rs"]
mod witness_libraries;

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
    assert_cases_with_command_inspection(
        artifact,
        command_reporting,
        |id, prepared, expected, _| inspect(id, prepared, expected),
    );
}

pub(super) fn assert_cases_with_command_inspection<F>(
    artifact: &Value,
    command_reporting: bool,
    inspect: F,
) where
    F: Fn(&str, &tsc_program::PreparedProgram, &Value, &[tsc_diagnostics::Diagnostic])
        + std::panic::RefUnwindSafe,
{
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
            for (path, bytes) in witness_libraries::files() {
                builder = builder.file(path.as_str(), bytes.as_slice());
            }
            if let Some(config) = case["config"].as_str() {
                builder = builder.file(config_path, config.as_bytes());
            }
            let host = builder.build().expect("memory host");
            let mut options = CompilerOptions::default();
            let mut program_options = ProgramOptions::default();
            let program_path = |text: &str| {
                let canonical = tsc_program::canonical_emit_path(
                    text.into(),
                    "/project".into(),
                    case["use_case_sensitive_file_names"]
                        .as_bool()
                        .unwrap_or(true),
                );
                tsc_program::ProgramPath::from_js_parts(text.into(), canonical.as_js()).unwrap()
            };
            if let Some(config) = case["config_file_path"].as_str() {
                program_options = program_options.with_config_file_path(program_path(config));
            }
            for (key, value) in case["options"].as_object().expect("options") {
                match key.as_str() {
                    "target" => options.target = Some(value.as_i64().unwrap() as i32),
                    "module" => options.module = Some(value.as_i64().unwrap() as i32),
                    "jsx" => options.jsx = Some(value.as_i64().unwrap() as i32),
                    "moduleResolution" => {
                        options.module_resolution = Some(value.as_i64().unwrap() as i32)
                    }
                    "moduleDetection" => {
                        options.module_detection = Some(value.as_i64().unwrap() as i32)
                    }
                    "newLine" => options.new_line = Some(value.as_i64().unwrap() as i32),
                    "declaration" => options.declaration = value.as_bool(),
                    "declarationMap" => options.declaration_map = value.as_bool(),
                    "declarationDir" => options.declaration_dir = value.as_str().map(Into::into),
                    "outDir" => options.out_dir = value.as_str().map(Into::into),
                    "outFile" => options.out_file = value.as_str().map(Into::into),
                    "rootDir" => options.root_dir = value.as_str().map(Into::into),
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
                                .map(|value| value.as_str().unwrap().into())
                                .collect(),
                        )
                    }
                    "allowJs" => options.allow_js = value.as_bool().unwrap(),
                    "checkJs" => options.check_js = value.as_bool(),
                    "noEmitForJsFiles" => options.no_emit_for_js_files = value.as_bool(),
                    "noEmitHelpers" => options.no_emit_helpers = value.as_bool(),
                    "resolveJsonModule" => options.resolve_json_module = value.as_bool(),
                    "emitBOM" => options.emit_bom = value.as_bool(),
                    "removeComments" => options.remove_comments = value.as_bool(),
                    "preserveConstEnums" => options.preserve_const_enums = value.as_bool(),
                    "isolatedDeclarations" => options.isolated_declarations = value.as_bool(),
                    "strict" => options.strict = value.as_bool(),
                    "allowSyntheticDefaultImports" => {
                        options.allow_synthetic_default_imports = value.as_bool()
                    }
                    "esModuleInterop" => options.es_module_interop = value.as_bool(),
                    "experimentalDecorators" => {
                        options.experimental_decorators = value.as_bool().unwrap()
                    }
                    "useDefineForClassFields" => {
                        options.use_define_for_class_fields = value.as_bool()
                    }
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
                let mut additional_options_diagnostics = Vec::new();
                let prepared = if let Some(config) = case["config"].as_str() {
                    let plan = parse_config_root_plan(
                        &CompilerConfigHost::new(&host),
                        ConfigRootPlanRequest {
                            file_name: config_path.into(),
                            text: config.to_owned(),
                            base_path: config_base
                                .to_str()
                                .expect("scalar fixture config base")
                                .into(),
                        },
                    )
                    .expect("config plan");
                    if plan.compiler_options().no_emit == Some(true) {
                        additional_options_diagnostics.extend(
                            plan.option_diagnostics()
                                .iter()
                                .filter(|diagnostic| {
                                    tsc_program::is_non_fatal_option_diagnostic(diagnostic)
                                })
                                .cloned(),
                        );
                        load_config_program(&host, &plan, &catalog, limits)
                            .expect("checking config program")
                    } else {
                        load_emitting_config_program(&host, &plan, &catalog, limits)
                            .expect("emitting config program")
                    }
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
                inspect(
                    case_id,
                    &prepared,
                    &case["typescript_observation"],
                    &additional_options_diagnostics,
                );
                if let Some(boundary) = case.get("rust_expected_parse_recovery") {
                    assert!(
                        command_reporting,
                        "parse boundary requires the complete command route"
                    );
                    assert!(!boundary["cause"].as_str().unwrap().is_empty());
                    assert_eq!(boundary["partial_writes"], serde_json::json!([]));
                    let mut sink = tsc_compiler::MemoryOutputSink::new();
                    let result = tsc_compiler::ProgramSession::new(prepared)
                        .emit_command_for_harness_with_options_diagnostics(
                            &mut sink,
                            &additional_options_diagnostics,
                        );
                    match result {
                        Err(tsc_compiler::DriverError::Emit(tsc_emitter::EmitFailure::Transform(error))) => {
                            let tsc_emitter::TransformError::ParseDiagnosticsDeferred {
                                count, recovery_events, owner_slice,
                            } = error.as_ref() else {
                                panic!("{case_id}: changed parse boundary: {error}");
                            };
                            assert_eq!(*count as u64, boundary["count"].as_u64().unwrap(), "{case_id}");
                            assert_eq!(*recovery_events as u64, boundary["recovery_events"].as_u64().unwrap(), "{case_id}");
                            assert_eq!(*owner_slice, boundary["owner_slice"].as_str().unwrap(), "{case_id}");
                        }
                        Err(error) => panic!("{case_id}: changed parse boundary: {error}"),
                        Ok(_) => panic!("{case_id}: retire the parse refusal and compare the complete TypeScript observation"),
                    }
                    assert!(
                        sink.writes().is_empty(),
                        "{case_id}: no partial writes before refusal"
                    );
                    continue;
                }
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
                let blocked = prepared.compiler_options().no_emit == Some(true)
                    || prepared.compiler_options().no_emit_on_error == Some(true)
                        && case["typescript_observation"]["emit_result"]["emit_skipped"] == true;
                let activity = if command_reporting {
                    super::h2_7b_w4a_controls::assert_command_observation_with_options(
                        case_id,
                        prepared,
                        &case["typescript_observation"],
                        &additional_options_diagnostics,
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

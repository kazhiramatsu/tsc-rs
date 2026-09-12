//! Declaration module specifiers through the ordinary complete-command route.

use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::{json, Value};
use tsc_compiler::{MemoryOutputSink, ProgramSession};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_emitting_config_program, load_emitting_program, parse_config_root_plan,
    CompilerConfigHost, CompilerOptions, ConfigRootPlanRequest, LibraryCatalog, PathMapping,
    PreparedProgram, ProgramLoadLimits, ProgramOptions, ProgramPath,
};

#[allow(dead_code)]
#[path = "integration/h2_7d_original_corpus_shared.rs"]
mod h2_7d_original_corpus_shared;

#[allow(dead_code)]
#[path = "integration/h2_7b_w4a_controls.rs"]
mod h2_7b_w4a_controls;

#[test]
fn original_monorepo_declaration_specifier_matches_complete_command() {
    let selected = ["typescript-6.0.3/compiler/declarationEmitPathMappingMonorepo2.ts#default"];
    let exact = h2_7d_original_corpus_shared::assert_output_matrix_projection(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        &selected,
    );
    assert_eq!(exact.len(), selected.len());
}

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn prepared(case: &Value) -> PreparedProgram {
    let mut builder = MemoryCompilerHost::builder("/project");
    for file in case["files"].as_array().expect("files") {
        builder = builder.file(
            file["path"].as_str().expect("path"),
            file["text"].as_str().expect("text").as_bytes(),
        );
    }
    for entry in std::fs::read_dir(workspace().join("vendor/typescript-6.0.3/lib")).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("lib.") && name.ends_with(".d.ts") {
            builder = builder.file(format!("/lib/{name}"), std::fs::read(entry.path()).unwrap());
        }
    }
    let catalog = LibraryCatalog::typescript_6_0_3("/lib");
    let limits = ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024);
    if let Some(config) = case["config"].as_str() {
        let path = case["config_path"].as_str().expect("config path");
        let host = builder.file(path, config.as_bytes()).build().expect("host");
        let plan = parse_config_root_plan(
            &CompilerConfigHost::new(&host),
            ConfigRootPlanRequest {
                file_name: path.to_owned(),
                text: config.to_owned(),
                base_path: Path::new(path)
                    .parent()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_owned(),
            },
        )
        .expect("config plan");
        return load_emitting_config_program(&host, &plan, &catalog, limits)
            .expect("config Program");
    }

    let mut options = CompilerOptions::default();
    let mut program_options = ProgramOptions::default();
    let input_options = case["options"].as_object().expect("direct options");
    for (name, value) in input_options {
        match name.as_str() {
            "target" => options.target = Some(value.as_i64().unwrap() as i32),
            "module" => options.module = Some(value.as_i64().unwrap() as i32),
            "moduleResolution" => options.module_resolution = Some(value.as_i64().unwrap() as i32),
            "declaration" => options.declaration = value.as_bool(),
            "rootDir" => options.root_dir = value.as_str().map(str::to_owned),
            "outDir" => options.out_dir = value.as_str().map(str::to_owned),
            "baseUrl" => options.base_url = value.as_str().map(str::to_owned),
            "ignoreDeprecations" => options.ignore_deprecations = value.as_str().map(str::to_owned),
            "configFilePath" => {
                let path = value.as_str().unwrap();
                program_options = program_options
                    .with_config_file_path(ProgramPath::from_trusted_parts(path, path).unwrap());
            }
            // Install entries and their origin together after reading the bag.
            "paths" | "pathsBasePath" => {}
            other => panic!("unowned direct option {other}"),
        }
    }
    if let Some(paths) = input_options.get("paths") {
        let mappings = paths
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, values)| {
                PathMapping::new(
                    key,
                    values
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|value| value.as_str().unwrap().to_owned())
                        .collect(),
                )
            })
            .collect();
        program_options = if let Some(origin) = input_options.get("pathsBasePath") {
            program_options.with_config_paths(mappings, origin.as_str().unwrap())
        } else {
            program_options.with_paths(mappings)
        };
    }
    let roots = case["roots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|path| PathBuf::from(path.as_str().unwrap()))
        .collect::<Vec<_>>();
    load_emitting_program(
        &builder.build().expect("host"),
        &roots,
        options,
        program_options,
        &catalog,
        limits,
    )
    .expect("direct Program")
}

fn program_facts(prepared: &PreparedProgram) -> Value {
    json!({
        "source_files": prepared.source_files().iter()
            .map(|source| source.path().display().to_str().unwrap()).collect::<Vec<_>>(),
        "library_files": prepared.library_files().iter()
            .map(|id| prepared.source_file(*id).unwrap().path().display().to_str().unwrap())
            .collect::<Vec<_>>(),
        "root_names": prepared.roots().iter()
            .map(|root| root.path().display().to_str().unwrap()).collect::<Vec<_>>(),
    })
}

#[test]
fn focused_declaration_specifiers_match_complete_commands() {
    let artifact: Value =
        serde_json::from_slice(include_bytes!("fixtures/h2-8a-declaration-specifiers.json"))
            .expect("frozen observations");
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    assert_eq!(artifact["upstream_failures"], json!([]));
    let cases = artifact["cases"].as_array().expect("cases");
    assert_eq!(cases.len(), 24);
    let mut failures = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        let id = case["case_id"].as_str().unwrap();
        for repetition in 0..2 {
            let result = std::panic::catch_unwind(|| {
                let prepared = prepared(case);
                let facts = program_facts(&prepared);
                let mut sink = MemoryOutputSink::new();
                let command = ProgramSession::new(prepared)
                    .emit_command_for_harness(&mut sink)
                    .expect("ordinary complete command");
                let expected = &case["typescript_observation"];
                if let Some(directory) =
                    std::env::var_os("TSC_RS_DECLARATION_SPECIFIER_CAPTURE_DIR")
                {
                    let directory = PathBuf::from(directory);
                    std::fs::create_dir_all(&directory).unwrap();
                    let writes = sink.writes().iter().map(|write| json!({
                        "path": write.path().to_str().unwrap(),
                        "kind": format!("{:?}", write.kind()),
                        "callback_utf8_base64": base64::engine::general_purpose::STANDARD.encode(write.callback_bytes()),
                        "materialized_utf8_base64": base64::engine::general_purpose::STANDARD.encode(write.materialized_bytes()),
                        "write_byte_order_mark": write.write_byte_order_mark(),
                        "source_files": write.source_files(),
                        "metadata_debug": format!("{:?}", write.metadata()),
                    })).collect::<Vec<_>>();
                    let capture = json!({"case_id": id, "repetition": repetition,
                        "program_facts": facts, "writes": writes,
                        "outcome_debug": format!("{:#?}", command.emit()),
                        "diagnostics_debug": format!("{:#?}", command.diagnostics()),
                        "status_writes": command.status_writes(), "exit_code": command.exit_code(),
                        "expected": expected});
                    let file = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(directory.join(format!("{index:02}-{repetition}.json")))
                        .unwrap();
                    serde_json::to_writer_pretty(file, &capture).unwrap();
                }
                assert_eq!(facts, expected["program_facts"], "{id}: Program facts");
                h2_7b_w4a_controls::assert_completed_observation(
                    id,
                    command.emit(),
                    command.diagnostics(),
                    Some((command.status_writes().to_vec(), command.exit_code())),
                    sink.writes(),
                    expected,
                    true,
                );
                eprintln!("DECLARATION-SPECIFIER EXACT {id} repetition={repetition}");
            });
            if result.is_err() {
                failures.push(format!("{id} repetition={repetition}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "failed complete commands: {failures:?}"
    );
}

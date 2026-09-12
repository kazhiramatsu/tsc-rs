//! Compare complete frozen observations through the original H2.6a routes.
//! This target also exposes the unchanged investigation probe without adding
//! another registration to the shared contracts target.
#[path = "integration/source_map_band_probe.rs"]
mod source_map_band_probe;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine as _;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tsc_compiler::{EmitWriteMetadata, H2RuntimeSlice, MemoryOutputSink, ProgramSession};
use tsc_diagnostics::{Diagnostic, MessageChain};
use tsc_harness::upstream_suites::execution::{
    load_compiler_emit_with_option_floor, load_qualified_compiler_emit_with_option_floor,
    load_recorded_execution_plans, CompilerExecutionPlan, CompilerExplicitRootReason,
    CompilerRootSelection, CompilerUnitId, CompilerUnitInput, EmitOptionFloor,
    UpstreamExecutionCorpus, UpstreamExecutionInput,
};
use tsc_harness::upstream_suites::OrderedSetting;
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    parse_config_root_plan, CompilerConfigHost, ConfigRootPlanRequest, PreparedProgram,
    ProgramLoadLimits,
};

const ROWS: &[&str] = &[
    "typescript-6.0.3/compiler/optionsSourcemapInlineSources.ts#default",
    "typescript-6.0.3/compiler/optionsSourcemapInlineSourcesMapRoot.ts#default",
    "typescript-6.0.3/compiler/optionsSourcemapInlineSourcesSourceRoot.ts#default",
];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn limits() -> ProgramLoadLimits {
    ProgramLoadLimits::new(256, 2_048, 64, 16 * 1_024 * 1_024, 128 * 1_024 * 1_024)
}
fn artifact(path: &str) -> Value {
    serde_json::from_slice(&std::fs::read(workspace().join(path)).unwrap()).unwrap()
}
fn decode(file: &Value) -> Vec<u8> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(file["utf8_base64"].as_str().unwrap())
        .unwrap();
    assert_eq!(sha256(&bytes), file["utf8_sha256"]);
    assert_eq!(bytes.len() as u64, file["utf8_bytes"].as_u64().unwrap());
    bytes
}
fn sha256(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}
fn settings(input: &Value) -> Vec<(String, String)> {
    input["settings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|setting| {
            (
                setting["name"].as_str().unwrap().to_owned(),
                setting["value"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}
fn input_files(input: &Value) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files: Vec<_> = input["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| (PathBuf::from(file["path"].as_str().unwrap()), decode(file)))
        .collect();
    if !input["virtual_config"].is_null() {
        let config = &input["virtual_config"];
        files.push((
            PathBuf::from(config["path"].as_str().unwrap()),
            decode(config),
        ));
    }
    files
}
fn qualified(input: &Value, floor: EmitOptionFloor) -> PreparedProgram {
    load_qualified_compiler_emit_with_option_floor(
        &workspace(),
        input["current_directory"].as_str().unwrap(),
        &input_files(input),
        &input["roots"]
            .as_array()
            .unwrap()
            .iter()
            .map(|root| PathBuf::from(root.as_str().unwrap()))
            .collect::<Vec<_>>(),
        &settings(input),
        limits(),
        floor,
    )
    .unwrap()
}
fn recorded<'a>(case: &Value, corpus: &'a UpstreamExecutionCorpus) -> &'a CompilerExecutionPlan {
    let row = corpus
        .plans
        .iter()
        .find(|row| row.provenance.case_id.as_ref() == case["case_id"].as_str().unwrap())
        .unwrap();
    assert_eq!(
        u64::from(row.provenance.case_index),
        case["expansion_case"].as_u64().unwrap()
    );
    let UpstreamExecutionInput::Compiler(plan) = &row.input else {
        panic!("compiler plan")
    };
    plan
}
fn prepare(
    case: &Value,
    corpus: &UpstreamExecutionCorpus,
    floor: EmitOptionFloor,
) -> PreparedProgram {
    match case["execution_route"].as_str().unwrap() {
        "recorded-compiler-plan" => load_compiler_emit_with_option_floor(
            &workspace(),
            recorded(case, corpus),
            limits(),
            floor,
        )
        .unwrap(),
        "qualified-vfs" => qualified(&case["input"], floor),
        route => panic!("unexpected H2.6a route {route}"),
    }
}

// Synthetic witnesses use the actual compiler-plan adapter as well as the
// qualified adapter. Only test input construction is synthetic; options are
// parsed by the production config planner and projected by the harness.
fn witness_plan(input: &Value, template: &CompilerExecutionPlan) -> CompilerExecutionPlan {
    let mut plan = template.clone();
    let mut fixture = (*plan.fixture).clone();
    let files = input_files(input);
    fixture.units = files
        .iter()
        .enumerate()
        .map(|(index, (path, bytes))| CompilerUnitInput {
            id: CompilerUnitId(index as u32),
            name: Arc::from(path.to_str().unwrap()),
            content: Some(Arc::from(std::str::from_utf8(bytes).unwrap())),
            file_options: Arc::from([]),
            original_fixture_path: Arc::from("map-option-witness.ts"),
            references: Arc::from([]),
            document_symlinks: Arc::from([]),
        })
        .collect::<Vec<_>>()
        .into();
    fixture.config_unit = None;
    fixture.config_root_plan = None;
    if !input["virtual_config"].is_null() {
        let config = &input["virtual_config"];
        let mut builder = MemoryCompilerHost::builder(input["current_directory"].as_str().unwrap());
        for (path, bytes) in &files {
            builder = builder.file(path, bytes.clone());
        }
        let host = builder.build().unwrap();
        let parsed = parse_config_root_plan(
            &CompilerConfigHost::new(&host),
            ConfigRootPlanRequest {
                file_name: config["path"].as_str().unwrap().to_owned(),
                text: String::from_utf8(decode(config)).unwrap(),
                base_path: input["current_directory"].as_str().unwrap().to_owned(),
            },
        )
        .unwrap();
        fixture.config_unit = Some(CompilerUnitId(1));
        fixture.config_root_plan = Some(Arc::new(parsed));
    }
    plan.fixture = Arc::new(fixture);
    plan.current_directory = Arc::from(input["current_directory"].as_str().unwrap());
    plan.effective_settings = settings(input)
        .into_iter()
        .map(|(name, value)| OrderedSetting { name, value })
        .collect::<Vec<_>>()
        .into();
    plan.root_selection = CompilerRootSelection::Explicit {
        reason: CompilerExplicitRootReason::AllUnits,
        root_units: Arc::from([CompilerUnitId(0)]),
        other_units: Arc::from([]),
        vfs_write_order: (0..files.len())
            .map(|id| CompilerUnitId(id as u32))
            .collect::<Vec<_>>()
            .into(),
        program_root_units: Arc::from([CompilerUnitId(0)]),
    };
    plan
}

fn message(chain: &MessageChain, indent: usize, text: &mut String) {
    if indent != 0 {
        text.push('\n');
        text.push_str(&"  ".repeat(indent));
    }
    text.push_str(&chain.text);
    for next in &chain.next {
        message(next, indent + 1, text);
    }
}
fn diagnostics(list: &[Diagnostic]) -> Value {
    json!(list.iter().map(|d| {
        let mut text = String::new(); message(&d.message, 0, &mut text);
        json!({ "code": d.code(), "category": format!("{:?}", d.category()), "file": d.file_name,
            "start": d.start, "length": d.length, "message": text })
    }).collect::<Vec<_>>())
}
fn observe(prepared: PreparedProgram) -> Value {
    let options = prepared.compiler_options();
    let effective = json!({ "sourceMap": options.source_map, "inlineSourceMap": options.inline_source_map,
        "inlineSources": options.inline_sources, "sourceRoot": options.source_root, "mapRoot": options.map_root });
    let session = ProgramSession::new(prepared);
    let bundle = session.prepare_harness_lib_bundle().unwrap();
    let mut sink = MemoryOutputSink::new();
    let (outcome, reported) = session
        .emit_with_reported_diagnostics_for_harness_with_lib_bundle(&mut sink, bundle.as_ref())
        .unwrap();
    let writes = sink.writes().iter().enumerate().map(|(index, write)| {
        let (data_present, pos, count) = match write.metadata() {
            None => (false, None, None),
            Some(EmitWriteMetadata::Text(data)) => (true, data.source_map_url_position().map(|pos| pos.value()), Some(data.diagnostics().len())),
            _ => panic!("unexpected build info"),
        };
        json!({ "index": index, "path": write.path().to_string_lossy(),
            "kind": if write.path().extension().is_some_and(|ext| ext == "map") { "source-map" } else { "javascript" },
            "callback_utf8_base64": base64::engine::general_purpose::STANDARD.encode(write.callback_bytes()),
            "callback_utf8_sha256": sha256(write.callback_bytes()), "callback_utf8_bytes": write.callback_bytes().len(),
            "write_byte_order_mark": write.write_byte_order_mark(), "materialized_utf8_sha256": sha256(write.materialized_bytes()),
            "materialized_utf8_bytes": write.materialized_bytes().len(), "on_error_callback_present": true,
            "source_files": write.source_files().map(|files| files.iter().map(|path| path.to_string_lossy()).collect::<Vec<_>>()),
            "data_present": data_present, "data_source_map_url_pos": pos, "data_diagnostics_count": count })
    }).collect::<Vec<_>>();
    json!({ "effective_map_options": effective,
        "map_runtime_activity": {
            "H2.6a": outcome.h2_activity().runtime_slice(H2RuntimeSlice::H2_6a),
            "H2.6b": outcome.h2_activity().runtime_slice(H2RuntimeSlice::H2_6b),
            "H2.6c": outcome.h2_activity().runtime_slice(H2RuntimeSlice::H2_6c),
        }, "observation": {
        "writes": writes, "reported_diagnostics": diagnostics(&reported),
        "emit_result": { "emit_skipped": outcome.emit_skipped(), "diagnostics": diagnostics(outcome.diagnostics()),
            "emitted_files": outcome.emitted_files().map(|paths| paths.iter().map(|path| path.to_string_lossy()).collect::<Vec<_>>()),
            "source_maps": outcome.source_maps().map(|maps| maps.iter().map(|map| json!({
                "input_source_file_names": map.input_source_files().iter().map(|path| path.to_string_lossy()).collect::<Vec<_>>(),
                "source_map_json": map.canonical_json() })).collect::<Vec<_>>()) },
        "status_writes": [], "exit_code": if reported.is_empty() { 0 } else if outcome.emit_skipped() { 1 } else { 2 }
    } })
}
fn expected(case: &Value) -> Value {
    let mut expected = case["typescript_observation"].clone();
    expected
        .as_object_mut()
        .unwrap()
        .remove("run_fingerprint_sha256");
    expected
}
fn capture(id: &str, floor: EmitOptionFloor, attempt: usize, actual: &Value, expected: &Value) {
    if let Some(out) = std::env::var_os("TSRS_MAP_OPTION_CAPTURE") {
        let out = PathBuf::from(out);
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join(format!("{}-{floor:?}-{attempt}.json", id.replace(['/', '#', '%'], "_"))),
            serde_json::to_vec_pretty(&json!({ "case_id": id, "floor": format!("{floor:?}"), "attempt": attempt, "actual": actual, "expected": expected })).unwrap()).unwrap();
    }
}

#[test]
fn original_floor_divergence_and_existing_map_family_parity() {
    let qualification = artifact("ratchets/h2-6a-qualification.v1.json");
    let corpus = load_recorded_execution_plans(&workspace()).unwrap();
    for (ordinal, id) in ROWS.iter().enumerate() {
        let case = qualification["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["case_id"] == *id)
            .unwrap();
        let expected = expected(case);
        for floor in [EmitOptionFloor::SourceMap, EmitOptionFloor::MapFamily] {
            let mut runs = Vec::new();
            for attempt in 0..2 {
                let actual = observe(prepare(case, &corpus, floor));
                capture(id, floor, attempt, &actual, &expected);
                runs.push(actual);
            }
            assert_eq!(runs[0], runs[1], "{id}: {floor:?} determinism");
            let actual = &runs[0]["observation"];
            if floor == EmitOptionFloor::MapFamily {
                assert_eq!(
                    actual, &expected,
                    "{id}: existing native map option implementation"
                );
            } else {
                assert_eq!(
                    actual["reported_diagnostics"],
                    expected["reported_diagnostics"]
                );
                assert_eq!(actual["exit_code"], expected["exit_code"]);
                assert_ne!(actual["emit_result"], expected["emit_result"]);
                let differing = actual["writes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .zip(expected["writes"].as_array().unwrap())
                    .filter(|(a, b)| a != b)
                    .count();
                assert_eq!(differing, if ordinal == 1 { 2 } else { 1 });
                assert_eq!(
                    runs[0]["effective_map_options"]["inlineSources"],
                    Value::Null
                );
            }
            eprintln!("{id}: {floor:?} verified x2");
        }
    }
}

#[test]
fn h2_6a_rows_and_adjacent_controls_match_complete_frozen_tuples() {
    let qualification = artifact("ratchets/h2-6a-qualification.v1.json");
    let corpus = load_recorded_execution_plans(&workspace()).unwrap();
    let controls = [
        "typescript-6.0.3/compiler/sourceMapValidationVariables.ts#default",
        "typescript-6.0.3/conformance/es6/computedProperties/computedPropertyNamesSourceMap1_ES6.ts#default",
    ];
    for id in ROWS.iter().chain(controls.iter()) {
        let case = qualification["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["case_id"] == *id)
            .unwrap();
        assert_eq!(case["disposition"], "admitted-for-execution");
        let fingerprints = case["typescript_run_fingerprints"].as_array().unwrap();
        assert_eq!(fingerprints.len(), 2);
        assert!(fingerprints
            .iter()
            .all(|fingerprint| fingerprint
                == &case["typescript_observation"]["run_fingerprint_sha256"]));
        let expected = expected(case);
        let floor = EmitOptionFloor::SourceMapWithOptions;
        let mut runs = Vec::new();
        for attempt in 0..2 {
            let actual = observe(prepare(case, &corpus, floor));
            capture(id, floor, attempt, &actual, &expected);
            runs.push(actual);
        }
        assert_eq!(runs[0], runs[1], "{id}: deterministic preparation and emit");
        assert_eq!(
            runs[0]["observation"], expected,
            "{id}: complete command tuple"
        );
        let activity = &runs[0]["map_runtime_activity"];
        assert_eq!(activity["H2.6c"], 0, "{id}: no later map owner");
        assert_eq!(
            activity["H2.6b"].as_u64().unwrap() > 0,
            ROWS.contains(id),
            "{id}: embedded sources/roots use the existing H2.6b owner"
        );
        eprintln!("{id}: H2.6a projection EXACT x2");
    }
}

#[test]
fn map_option_directives_and_virtual_configs_match_typescript() {
    let witnesses = artifact("crates/compiler/tests/fixtures/h2-6a-map-option-projection.json");
    assert_eq!(witnesses["typescript"], "6.0.3");
    assert_eq!(witnesses["cases"].as_array().unwrap().len(), 31);
    let qualification = artifact("ratchets/h2-6a-qualification.v1.json");
    let corpus = load_recorded_execution_plans(&workspace()).unwrap();
    let template = recorded(
        qualification["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["case_id"] == ROWS[0])
            .unwrap(),
        &corpus,
    );
    for case in witnesses["cases"].as_array().unwrap() {
        assert_eq!(case["repetitions"], 2);
        for route in ["qualified", "recorded"] {
            let id = format!("{route}/{}", case["case_id"].as_str().unwrap());
            let floor = EmitOptionFloor::SourceMapWithOptions;
            let mut runs = Vec::new();
            for attempt in 0..2 {
                let prepared = if route == "qualified" {
                    qualified(&case["input"], floor)
                } else {
                    load_compiler_emit_with_option_floor(
                        &workspace(),
                        &witness_plan(&case["input"], template),
                        limits(),
                        floor,
                    )
                    .unwrap()
                };
                let actual = observe(prepared);
                capture(&id, floor, attempt, &actual, &case["observation"]);
                runs.push(actual);
            }
            assert_eq!(runs[0], runs[1], "{id}: deterministic preparation and emit");
            assert_eq!(
                runs[0]["effective_map_options"], case["effective_map_options"],
                "{id}: effective options"
            );
            assert_eq!(
                runs[0]["observation"], case["observation"],
                "{id}: complete command tuple"
            );
            eprintln!("{id}: H2.6a projection EXACT x2");
        }
    }
}

#[test]
#[ignore = "pre-change witness census; captures existing route behavior without promoting it"]
fn existing_witness_route_census() {
    let witnesses = artifact("crates/compiler/tests/fixtures/h2-6a-map-option-projection.json");
    let qualification = artifact("ratchets/h2-6a-qualification.v1.json");
    let corpus = load_recorded_execution_plans(&workspace()).unwrap();
    let template = recorded(
        qualification["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["case_id"] == ROWS[0])
            .unwrap(),
        &corpus,
    );
    for case in witnesses["cases"].as_array().unwrap() {
        for (route, floor) in [
            ("qualified", EmitOptionFloor::MapFamily),
            ("recorded", EmitOptionFloor::MapFamily),
        ] {
            let id = format!("{route}/{}", case["case_id"].as_str().unwrap());
            let mut runs = Vec::new();
            for attempt in 0..2 {
                let prepared = if route == "qualified" {
                    qualified(&case["input"], floor)
                } else {
                    load_compiler_emit_with_option_floor(
                        &workspace(),
                        &witness_plan(&case["input"], template),
                        limits(),
                        floor,
                    )
                    .unwrap()
                };
                let actual = observe(prepared);
                capture(&id, floor, attempt, &actual, &case["observation"]);
                runs.push(actual);
            }
            assert_eq!(runs[0], runs[1]);
            eprintln!(
                "{id}: existing parity={}",
                runs[0]["observation"] == case["observation"]
            );
        }
    }
}

//! A41-BINDING (C02): complete TypeScript commands for the generated-name
//! collision domains of the standard-decorator transform (parsed-identifier
//! census, checker globals, nested/sibling scopes, the FileLevel versus
//! ReservedInNestedScopes `_classThis` domains, computed-key cache temps,
//! name-generation ordering, bundle and CommonJS publication).
//!
//! Independent integration target: it does not touch the shared `contracts`
//! target. `h2_7b_w4a_controls` is included only for its exact
//! complete-command comparator. The observation fixture and its input
//! manifest are read at run time from `crates/compiler/tests/fixtures/`, so
//! one binary built before an implementation change replays the same frozen
//! upstream observations before and after that change. Selection:
//! `TSC_RS_DECORATOR_BINDING_CASE_SET` = `all` (default) or a comma-separated
//! list of case-id substrings (`/reserved/`, `/esnext/set/`, ...).
#[path = "integration/h2_7b_w4a_controls.rs"]
#[allow(dead_code)]
mod h2_7b_w4a_controls;

use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde_json::{json, Value};
use sha2::Digest;
use tsc_diagnostics::{Diagnostic, JsString, MessageChain};

#[path = "../../program/tests/support/scalar_json.rs"]
mod utf16_scalar_json;
#[path = "support/witness_libraries.rs"]
mod witness_libraries;
use tsc_emitter::{
    EmitArtifact, EmitArtifactKind, EmitIoError, EmitWriteDisposition, EmitWriteMetadata,
    OutputSink,
};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_emitting_program, CompilerOptions, LibraryCatalog, ProgramLoadLimits, ProgramOptions,
};
use utf16_scalar_json::observe as scalar_json;

const EXPECTED_CASES: usize = 768;
const SELECTION_ENV: &str = "TSC_RS_DECORATOR_BINDING_CASE_SET";
/// Native divergences that remain after the candidate, frozen in
/// `fixtures/decorator-binding-known-native.json` with their owner: each
/// row's native outcome (its typed error, or the SHA-256 of every write
/// with the exit code, status writes and diagnostics) is asserted on both
/// passes and counted as `known`, never as exact. A known row that becomes
/// exact fails the run so the list cannot go stale; a known row the
/// observation fixture does not have fails at load. Nothing is excluded and
/// no arbitrary panic is accepted.
const KNOWN_NATIVE_FIXTURE: &str = "decorator-binding-known-native.json";
/// Set to a directory to write the native projection of every failing row
/// (the material a new known-native entry is frozen from).
const KNOWN_NATIVE_DUMP_ENV: &str = "TSC_RS_H2_8A_KNOWN_NATIVE_DUMP_DIR";

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Plain JSON or zstd-compressed JSON (`<name>.zst`); every SHA-256 in the
/// captures refers to the decoded bytes.
fn fixture_bytes(name: &str) -> Vec<u8> {
    let path = workspace()
        .join("crates/compiler/tests/fixtures")
        .join(name);
    if path.is_file() {
        return std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    }
    let compressed = path.with_file_name(format!("{name}.zst"));
    let bytes = std::fs::read(&compressed)
        .unwrap_or_else(|error| panic!("{}: {error}", compressed.display()));
    zstd::stream::decode_all(bytes.as_slice())
        .unwrap_or_else(|error| panic!("{}: {error}", compressed.display()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", sha2::Sha256::digest(bytes))
}

struct RecordingSink {
    writes: Vec<EmitArtifact>,
}

impl OutputSink for RecordingSink {
    fn write(&mut self, artifact: EmitArtifact) -> Result<EmitWriteDisposition, EmitIoError> {
        self.writes.push(artifact);
        Ok(EmitWriteDisposition::Written)
    }
}

#[test]
fn decorator_binding_forms_match_complete_typescript_observations() {
    let inputs_bytes = fixture_bytes("decorator-binding-inputs.json");
    let inputs: Value = serde_json::from_slice(&inputs_bytes).unwrap();
    let artifact_bytes = fixture_bytes("decorator-binding.json");
    let artifact: Value = serde_json::from_slice(&artifact_bytes).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    assert!(
        artifact["selection"].is_null(),
        "the frozen fixture is a complete observation, not a scratch selection"
    );
    assert_eq!(
        artifact["inputs"]["sha256"].as_str().unwrap(),
        sha256_hex(&inputs_bytes),
        "observations were taken from this exact input manifest"
    );
    let input_cases = inputs["cases"].as_array().unwrap();
    assert_eq!(input_cases.len(), EXPECTED_CASES);
    assert_eq!(
        inputs["variants"].as_u64().unwrap() * inputs["cases_per_variant"].as_u64().unwrap(),
        EXPECTED_CASES as u64
    );
    let input_ids = input_cases
        .iter()
        .map(|case| case["case_id"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(input_ids.len(), EXPECTED_CASES);
    let mut cases = artifact["cases"].as_array().unwrap().clone();
    let upstream_failures = artifact["upstream_failures"].as_array().unwrap();
    assert_eq!(cases.len() + upstream_failures.len(), EXPECTED_CASES);
    let observed_ids = cases
        .iter()
        .chain(upstream_failures)
        .map(|case| case["case_id"].as_str().unwrap().to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        observed_ids.iter().map(String::as_str).collect::<Vec<_>>(),
        input_ids.iter().copied().collect::<Vec<_>>()
    );
    // An upstream exception is preserved as evidence but receives no complete
    // command equality credit and no manufactured native expectation.
    for case in upstream_failures {
        assert_eq!(case["typescript_failure"]["outcome"], "exception");
        eprintln!(
            "decorator binding UPSTREAM EXCEPTION {}",
            case["case_id"].as_str().unwrap()
        );
    }
    match std::env::var(SELECTION_ENV).as_deref() {
        Err(std::env::VarError::NotPresent) | Ok("all") => {}
        Ok(selection) => {
            let needles = selection.split(',').map(str::trim).collect::<Vec<_>>();
            assert!(
                needles.iter().all(|needle| !needle.is_empty()),
                "case selection contains an empty substring"
            );
            cases.retain(|case| {
                let id = case["case_id"].as_str().unwrap();
                needles.iter().any(|needle| id.contains(needle))
            });
            eprintln!(
                "decorator binding SELECTION {selection:?} -> {} cases",
                cases.len()
            );
            assert!(!cases.is_empty(), "selection matched no case");
        }
        Err(error) => panic!("invalid decorator binding case selection: {error}"),
    }
    let fixture_sha256 = sha256_hex(&artifact_bytes);
    let known_native: Value = serde_json::from_slice(&fixture_bytes(KNOWN_NATIVE_FIXTURE)).unwrap();
    assert_eq!(known_native["route"], "native-divergence-controls");
    let known_native = known_native["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| (row["case_id"].as_str().unwrap().to_owned(), row.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    for id in known_native.keys() {
        assert!(
            observed_ids.contains(id),
            "known native divergence {id} is not an observed case"
        );
    }
    let mut failures = Vec::new();
    let mut exact = 0usize;
    let mut known = 0usize;
    for case in &cases {
        let case_id = case["case_id"].as_str().unwrap();
        if let Some(record) = known_native.get(case_id) {
            let run = || {
                replay_known_case(case, record, &fixture_sha256, 1);
                replay_known_case(case, record, &fixture_sha256, 2);
            };
            if std::panic::catch_unwind(run).is_ok() {
                known += 1;
                eprintln!("decorator binding KNOWN x2 {case_id}");
            } else {
                failures.push(case_id);
                eprintln!("decorator binding KNOWN DIVERGENCE CHANGED {case_id}");
            }
            continue;
        }
        let run = || {
            // One complete Program and command comparison per pass,
            // two independent passes for each successful row.
            replay_case(case, &fixture_sha256, 1);
            replay_case(case, &fixture_sha256, 2);
        };
        let first = std::panic::catch_unwind(run);
        if first.is_ok() {
            exact += 1;
            eprintln!("decorator binding EXACT x2 {case_id}");
        } else {
            // Re-enter only for failed cases so deterministic failures also
            // produce two complete captures. A later pass is a mismatch in
            // repeat behavior, never an exact result.
            let second = std::panic::catch_unwind(run);
            assert!(
                second.is_err(),
                "{case_id}: comparison failed and then passed"
            );
            failures.push(case_id);
            eprintln!("decorator binding REPEATED FAILURE {case_id}");
        }
    }
    eprintln!(
        "decorator binding SUMMARY exact={exact} known={known} failed={} selected={}",
        failures.len(),
        cases.len()
    );
    assert!(
        failures.is_empty(),
        "complete decorator binding failures ({}): {failures:?}",
        failures.len()
    );
}

fn replay_case(case: &Value, fixture_sha256: &str, pass: usize) {
    let case_id = case["case_id"].as_str().unwrap();
    let expected = &case["typescript_observation"];
    let prepared = prepare_program(case);
    let mut sink = RecordingSink { writes: Vec::new() };
    let command =
        match tsc_compiler::ProgramSession::new(prepared).emit_command_for_harness(&mut sink) {
            Ok(command) => command,
            Err(error) => {
                dump_native_projection(case_id, &typed_error_projection(&error));
                panic!("{case_id}: production command completes: {error}");
            }
        };
    let outcome = command.emit().clone();
    let projection = completed_projection(&command, &sink.writes);
    if !completed_projection_is_exact(&projection, expected) {
        dump_native_projection(case_id, &projection);
    }
    capture_complete_command(
        case_id,
        fixture_sha256,
        pass,
        &sink.writes,
        &outcome,
        command.diagnostics(),
        command.status_writes(),
        command.exit_code(),
        expected,
    );
    h2_7b_w4a_controls::assert_completed_observation(
        case_id,
        &outcome,
        command.diagnostics(),
        Some((
            command
                .status_writes()
                .iter()
                .map(|text| text.as_str().expect("scalar fixture status").to_owned())
                .collect(),
            command.exit_code(),
        )),
        &sink.writes,
        expected,
        true,
    );
}

/// A frozen native divergence: the native outcome must equal the recorded
/// projection on this pass, and must still differ from the upstream
/// observation (a row that became exact is retired, never silently credited).
fn replay_known_case(case: &Value, record: &Value, fixture_sha256: &str, pass: usize) {
    let case_id = case["case_id"].as_str().unwrap();
    let expected = &case["typescript_observation"];
    let prepared = prepare_program(case);
    let mut sink = RecordingSink { writes: Vec::new() };
    let projection = match tsc_compiler::ProgramSession::new(prepared)
        .emit_command_for_harness(&mut sink)
    {
        Err(error) => typed_error_projection(&error),
        Ok(command) => {
            let outcome = command.emit().clone();
            capture_complete_command(
                case_id,
                fixture_sha256,
                pass,
                &sink.writes,
                &outcome,
                command.diagnostics(),
                command.status_writes(),
                command.exit_code(),
                expected,
            );
            let projection = completed_projection(&command, &sink.writes);
            assert!(
                !completed_projection_is_exact(&projection, expected),
                "{case_id}: the recorded native divergence is exact now; retire it from {KNOWN_NATIVE_FIXTURE}"
            );
            projection
        }
    };
    assert_eq!(
        projection, record["native"],
        "{case_id} pass {pass}: the frozen native divergence changed ({})",
        record["owner"]
    );
}

/// The native outcome of a completed command: exit code, status writes,
/// diagnostic codes and the SHA-256 of every write's callback bytes.
fn completed_projection(
    command: &tsc_compiler::EmitCommandOutcome,
    writes: &[EmitArtifact],
) -> Value {
    json!({
        "outcome": "completed",
        "exit_code": command.exit_code(),
        "emit_refused": command.emit().emit_skipped(),
        "status_writes": scalar_json(command.status_writes()),
        "diagnostics": command.diagnostics().iter().map(|d| json!({"code": d.code(), "file": scalar_json(&d.file_name), "start": d.start})).collect::<Vec<_>>(),
        "writes": writes.iter().map(|artifact| json!({
            "path": artifact.path().to_string_lossy(),
            "sha256": sha256_hex(artifact.callback_bytes()),
            "bytes": artifact.callback_bytes().len(),
        })).collect::<Vec<_>>(),
    })
}

/// Whether a completed projection equals the upstream observation on every
/// compared surface (write bytes and paths, exit code, diagnostics).
fn completed_projection_is_exact(projection: &Value, expected: &Value) -> bool {
    let expected_writes = expected["writes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|write| {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(write["callback_utf8_base64"].as_str().unwrap())
                .unwrap();
            (
                write["path"].as_str().unwrap().to_owned(),
                sha256_hex(&bytes),
            )
        })
        .collect::<std::collections::BTreeSet<_>>();
    let actual_writes = projection["writes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|write| {
            (
                write["path"].as_str().unwrap().to_owned(),
                write["sha256"].as_str().unwrap().to_owned(),
            )
        })
        .collect::<std::collections::BTreeSet<_>>();
    let expected_diagnostics = expected["reported_diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| (d["code"].clone(), d["file"].clone(), d["start"].clone()))
        .collect::<Vec<_>>();
    let actual_diagnostics = projection["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| (d["code"].clone(), d["file"].clone(), d["start"].clone()))
        .collect::<Vec<_>>();
    expected_writes == actual_writes
        && expected["exit_code"] == projection["exit_code"]
        && expected_diagnostics == actual_diagnostics
}

/// The native outcome of a refused command: the typed error, with arena
/// node numbers elided (they follow the parse arena, not the divergence).
fn typed_error_projection(error: &tsc_compiler::DriverError) -> Value {
    let debug = format!("{error:?}");
    let mut text = String::with_capacity(debug.len());
    let mut rest = debug.as_str();
    while let Some(index) = rest.find("NodeId(") {
        text.push_str(&rest[..index + "NodeId(".len()]);
        rest = &rest[index + "NodeId(".len()..];
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        text.push('_');
        rest = &rest[digits..];
    }
    text.push_str(rest);
    json!({"outcome": "typed-error", "error": text})
}

fn dump_native_projection(case_id: &str, projection: &Value) {
    let Some(directory) = std::env::var_os(KNOWN_NATIVE_DUMP_ENV) else {
        return;
    };
    let directory = PathBuf::from(directory);
    std::fs::create_dir_all(&directory).unwrap();
    let value = json!({"case_id": case_id, "native": projection});
    std::fs::write(
        directory.join(format!("{}.json", sha256_hex(case_id.as_bytes()))),
        serde_json::to_vec_pretty(&value).unwrap(),
    )
    .unwrap();
}

/// Program preparation for the manifest's direct-option cases (the same host,
/// library catalog, limits and option projection as the SUPER comparator,
/// restricted to the option keys this manifest uses).
fn prepare_program(case: &Value) -> tsc_program::PreparedProgram {
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
    assert!(
        case["config"].is_null(),
        "manifest cases use direct options"
    );
    let host = builder.build().expect("memory host");
    let mut options = CompilerOptions::default();
    let program_options = ProgramOptions::default();
    for (key, value) in case["options"].as_object().expect("options") {
        match key.as_str() {
            "target" => options.target = Some(value.as_i64().unwrap() as i32),
            "module" => options.module = Some(value.as_i64().unwrap() as i32),
            "newLine" => options.new_line = Some(value.as_i64().unwrap() as i32),
            "declaration" => options.declaration = value.as_bool(),
            "declarationMap" => options.declaration_map = value.as_bool(),
            "outDir" => options.out_dir = value.as_str().map(JsString::from),
            "outFile" => options.out_file = value.as_str().map(JsString::from),
            "ignoreDeprecations" => {
                options.ignore_deprecations = value.as_str().map(JsString::from);
            }
            "allowJs" => options.allow_js = value.as_bool().unwrap(),
            "checkJs" => options.check_js = value.as_bool(),
            "removeComments" => options.remove_comments = value.as_bool(),
            "strict" => options.strict = value.as_bool(),
            "useDefineForClassFields" => options.use_define_for_class_fields = value.as_bool(),
            "sourceMap" => options.source_map = value.as_bool(),
            "skipDefaultLibCheck" => options.skip_default_lib_check = value.as_bool(),
            "noErrorTruncation" => options.no_error_truncation = value.as_bool(),
            other => panic!("unexpected option {other}"),
        }
    }
    let catalog = LibraryCatalog::typescript_6_0_3("/lib");
    let limits = ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024);
    load_emitting_program(&host, &roots, options, program_options, &catalog, limits)
        .expect("direct program")
}

// Supplemental executions retain the whole command, including fields after
// the comparator's first failure. Same schema as the SUPER captures plus the
// fixture pin and pass number (`TSC_RS_H2_8A_CAPTURE_WRITES_DIR`).
#[allow(clippy::too_many_arguments)]
fn capture_complete_command(
    case_id: &str,
    fixture_sha256: &str,
    pass: usize,
    writes: &[EmitArtifact],
    outcome: &tsc_emitter::EmitOutcome,
    reported: &[Diagnostic],
    status_writes: &[JsString],
    exit_code: i32,
    expected: &Value,
) {
    let Some(directory) = std::env::var_os("TSC_RS_H2_8A_CAPTURE_WRITES_DIR") else {
        return;
    };
    let directory = PathBuf::from(directory);
    assert!(directory.is_absolute());
    std::fs::create_dir_all(&directory).unwrap();
    let key = sha256_hex(case_id.as_bytes());
    let index = (0..)
        .find(|index| !directory.join(format!("{key}-{index}.json")).exists())
        .unwrap();
    let writes = writes
        .iter()
        .enumerate()
        .map(|(index, artifact)| captured_write(index, artifact))
        .collect::<Vec<_>>();
    let maps = outcome.source_maps().map(|maps| {
        maps.iter()
            .map(|map| {
                json!({"input_source_file_names": scalar_json(&map.input_source_files()),
                    "source_map_json": map.canonical_json()})
            })
            .collect::<Vec<_>>()
    });
    let actual = json!({"writes": writes, "reported_diagnostics": diagnostics(reported),
        "emit_refused": outcome.emit_skipped(), "emit_result": {
            "emit_skipped": outcome.emit_skipped(), "diagnostics": diagnostics(outcome.diagnostics()),
            "emitted_files": scalar_json(&outcome.emitted_files()), "source_maps": maps},
        "status_writes": scalar_json(&status_writes), "exit_code": exit_code});
    let value = json!({"case_id": case_id, "capture_index": index, "pass": pass,
        "capture_kind": "supplemental-complete-command", "fixture_sha256": fixture_sha256,
        "actual": actual, "error": Value::Null, "partial_writes": Value::Null, "expected": expected});
    std::fs::write(
        directory.join(format!("{key}-{index}.json")),
        serde_json::to_vec_pretty(&value).unwrap(),
    )
    .unwrap();
}

fn message(chain: &MessageChain, indent: usize, text: &mut String) {
    if indent != 0 {
        text.push('\n');
        text.push_str(&"  ".repeat(indent));
    }
    text.push_str(chain.text.as_str().expect("scalar fixture diagnostic"));
    for next in &chain.next {
        message(next, indent + 1, text);
    }
}

fn diagnostics(diagnostics: &[Diagnostic]) -> Value {
    json!(diagnostics.iter().map(|d| {
        let mut text = String::new(); message(&d.message, 0, &mut text);
        let related = (d.related_information_present || !d.related.is_empty()).then(|| d.related.iter().map(|r| {
            let mut text = String::new(); message(&r.message, 0, &mut text);
            json!({"code":r.message.code,"category":format!("{:?}",r.message.category),
                "file":scalar_json(&r.file_name),"start":r.start,"length":r.length,"message":text,"related_information":null})
        }).collect::<Vec<_>>());
        json!({"code":d.code(),"category":format!("{:?}",d.category()),"file":scalar_json(&d.file_name),
            "start":d.start,"length":d.length,"message":text,"related_information":related})
    }).collect::<Vec<_>>())
}

fn captured_write(index: usize, artifact: &EmitArtifact) -> Value {
    let path = artifact.path().to_string_lossy();
    let kind = match artifact.kind() {
        EmitArtifactKind::DeclarationMap => "declaration-map",
        EmitArtifactKind::JavaScriptMap => "source-map",
        EmitArtifactKind::Declaration => "declaration",
        EmitArtifactKind::JavaScript if path.ends_with(".mjs") => "mjs",
        EmitArtifactKind::JavaScript if path.ends_with(".cjs") => "cjs",
        EmitArtifactKind::JavaScript => "javascript",
        EmitArtifactKind::BuildInfo => panic!("unexpected build-info output"),
    };
    let (position, data_diagnostics) = match artifact.metadata() {
        Some(EmitWriteMetadata::Text(data)) => (
            json!(data.source_map_url_position().map(|p| p.value())),
            diagnostics(data.diagnostics()),
        ),
        None => (Value::Null, Value::Null),
        _ => panic!("unexpected original callback metadata"),
    };
    json!({"index":index,"path":path,"kind":kind,
        "callback_utf8_base64":base64::engine::general_purpose::STANDARD.encode(artifact.callback_bytes()),
        "callback_utf8_bytes":artifact.callback_bytes().len(),"write_byte_order_mark":artifact.write_byte_order_mark(),
        "materialized_utf8_base64":base64::engine::general_purpose::STANDARD.encode(artifact.materialized_bytes()),
        "materialized_utf8_bytes":artifact.materialized_bytes().len(),
        // OutputSink::write's Result is the typed equivalent of onError.
        "on_error_callback_present":true,"source_files":scalar_json(&artifact.source_files()),
        "data_present":artifact.metadata().is_some(),
        "data_source_map_url_pos":position,"data_diagnostics":data_diagnostics})
}

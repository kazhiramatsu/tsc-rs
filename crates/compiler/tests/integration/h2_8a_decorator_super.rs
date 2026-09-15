//! Complete TypeScript commands for standard-decorator static `super` forms
//! (A6-41-SUPER: read, call, tag, assignment, compound/logical assignment,
//! update, discarded values, destructuring targets, receiver frames, phase
//! order, handoff and fault boundaries).
//!
//! The observation fixture and its input manifest are read at run time from
//! `crates/compiler/tests/fixtures/` (not `include_bytes!`), so one test
//! binary built before an implementation change replays the same frozen
//! upstream observations before and after that change; each replay records
//! the fixture SHA-256 it consumed in its captures. Selection:
//! `TSC_RS_DECORATOR_SUPER_CASE_SET` = `all` (default) or a comma-separated
//! list of case-id substrings (`update/`, `es2015/set/`, ...).
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde_json::{json, Value};
use sha2::Digest;
use tsc_diagnostics::{Diagnostic, JsString, MessageChain};

#[path = "../../../program/tests/support/scalar_json.rs"]
mod utf16_scalar_json;
#[path = "../support/witness_libraries.rs"]
mod witness_libraries;
use tsc_emitter::{
    EmitArtifact, EmitArtifactKind, EmitIoError, EmitIoOperation, EmitWriteDisposition,
    EmitWriteMetadata, OutputSink,
};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_emitting_program, CompilerOptions, LibraryCatalog, ProgramLoadLimits, ProgramOptions,
};
use utf16_scalar_json::observe as scalar_json;

const EXPECTED_CASES: usize = 672;
const EXPECTED_EXTRA_CASES: usize = 42;
const EXPECTED_FOLLOWUP_CASES: usize = 156;
const EXPECTED_FOLLOWUP2_CASES: usize = 162;
const EXPECTED_FOLLOWUP3_CASES: usize = 48;

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A fixture is stored either as plain JSON or, for the multi-megabyte
/// complete-command observations, as zstd-compressed JSON (`<name>.zst`,
/// written by the observer / `zstd -19`). The decoded bytes are what every
/// SHA-256 in the captures refers to, so a compressed and a plain copy of the
/// same observations carry the same identity.
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

/// Memory sink that records every attempted artifact, including one write
/// that reports an injected failure through the typed `Result` (the
/// equivalent of the TypeScript host `onError` callback).
struct RecordingSink {
    writes: Vec<EmitArtifact>,
    failure_index: Option<usize>,
}

impl OutputSink for RecordingSink {
    fn write(&mut self, artifact: EmitArtifact) -> Result<EmitWriteDisposition, EmitIoError> {
        let index = self.writes.len();
        let path = artifact.path().to_owned();
        self.writes.push(artifact);
        if self.failure_index == Some(index) {
            return Err(EmitIoError::new(
                EmitIoOperation::WriteFile,
                path,
                "simulated output callback failure",
            ));
        }
        Ok(EmitWriteDisposition::Written)
    }
}

#[test]
fn decorator_super_forms_match_complete_typescript_observations() {
    replay_set(
        "decorator-super-inputs.json",
        "decorator-super.json",
        EXPECTED_CASES,
        "TSC_RS_DECORATOR_SUPER_CASE_SET",
    );
}

/// Extra controls kept in their own fixture so the frozen primary set stays
/// replayable by the binary built at the restored base: `_outerThis` versus
/// the hoisted cache-temp `var`, and anonymous/named heritage of a decorated
/// class (`safeExtendsExpression`).
#[test]
fn decorator_super_extra_forms_match_complete_typescript_observations() {
    replay_set(
        "decorator-super-extra-inputs.json",
        "decorator-super-extra.json",
        EXPECTED_EXTRA_CASES,
        "TSC_RS_DECORATOR_SUPER_EXTRA_CASE_SET",
    );
}

/// Follow-up witnesses (2026-09-15) in a third fixture: arrow parameter
/// defaults that hoist a super-update temp (`VariablesHoistedInParameters`),
/// unicode-escaped identifier property/member/class names
/// (`createStringLiteralFromNode` source spelling), and the class-declaration
/// shape of a decorated computed field whose decorator uses lexical `this`.
#[test]
fn decorator_super_followup_forms_match_complete_typescript_observations() {
    replay_set(
        "decorator-super-followup-inputs.json",
        "decorator-super-followup.json",
        EXPECTED_FOLLOWUP_CASES,
        "TSC_RS_DECORATOR_SUPER_FOLLOWUP_CASE_SET",
    );
}

/// Second follow-up (2026-09-15) in a fourth fixture: named evaluation of an
/// anonymous decorated class expression in every source
/// `transformESDecorators` handles (parameter default, binding element,
/// variable, assignment, property assignment, export default; escaped and
/// parenthesized forms) and unicode-escaped private member names.
#[test]
fn decorator_super_followup2_forms_match_complete_typescript_observations() {
    replay_set(
        "decorator-super-followup2-inputs.json",
        "decorator-super-followup2.json",
        EXPECTED_FOLLOWUP2_CASES,
        "TSC_RS_DECORATOR_SUPER_FOLLOWUP2_CASE_SET",
    );
}

/// Third follow-up (2026-09-15) in a fifth fixture: the emit-helper request
/// order of the class-fields transform below ES2022. tsc visits relocated
/// static blocks and static field initializers after the member pass and the
/// constructor (`addPropertyOrClassStaticBlockStatements`), so
/// `__classPrivateFieldIn` / `__classPrivateFieldGet` requested there follow
/// the helpers of private method bodies and instance initializers while two
/// relocated statics keep their own order; plain and decorated classes.
#[test]
fn decorator_super_followup3_forms_match_complete_typescript_observations() {
    replay_set(
        "decorator-super-followup3-inputs.json",
        "decorator-super-followup3.json",
        EXPECTED_FOLLOWUP3_CASES,
        "TSC_RS_DECORATOR_SUPER_FOLLOWUP3_CASE_SET",
    );
}

fn replay_set(inputs_name: &str, fixture_name: &str, expected_cases: usize, selection_env: &str) {
    let inputs_bytes = fixture_bytes(inputs_name);
    let inputs: Value = serde_json::from_slice(&inputs_bytes).unwrap();
    let artifact_bytes = fixture_bytes(fixture_name);
    let artifact: Value = serde_json::from_slice(&artifact_bytes).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    assert_eq!(
        artifact["inputs"]["sha256"].as_str().unwrap(),
        sha256_hex(&inputs_bytes),
        "observations were taken from this exact input manifest"
    );
    let input_cases = inputs["cases"].as_array().unwrap();
    assert_eq!(input_cases.len(), expected_cases);
    assert_eq!(
        inputs["variants"].as_u64().unwrap() * inputs["cases_per_variant"].as_u64().unwrap(),
        expected_cases as u64
    );
    let input_ids = input_cases
        .iter()
        .map(|case| case["case_id"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(input_ids.len(), expected_cases);
    let mut cases = artifact["cases"].as_array().unwrap().clone();
    let upstream_failures = artifact["upstream_failures"].as_array().unwrap();
    assert_eq!(cases.len() + upstream_failures.len(), expected_cases);
    let observed_ids = cases
        .iter()
        .chain(upstream_failures)
        .map(|case| case["case_id"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(observed_ids, input_ids);
    // An upstream exception is preserved as evidence but receives no complete
    // command equality credit and no manufactured native expectation.
    for case in upstream_failures {
        assert_eq!(case["typescript_failure"]["outcome"], "exception");
        eprintln!(
            "decorator super UPSTREAM EXCEPTION {}",
            case["case_id"].as_str().unwrap()
        );
    }
    match std::env::var(selection_env).as_deref() {
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
                "decorator super SELECTION {selection:?} -> {} cases",
                cases.len()
            );
            assert!(!cases.is_empty(), "selection matched no case");
        }
        Err(error) => panic!("invalid decorator super case selection: {error}"),
    }
    let fixture_sha256 = sha256_hex(&artifact_bytes);
    let mut failures = Vec::new();
    let mut exact = 0usize;
    for case in &cases {
        let case_id = case["case_id"].as_str().unwrap();
        let run = || {
            // Two complete program constructions and command comparisons per
            // pass, as the shared comparator does for every other owner.
            replay_case(case, &fixture_sha256, 1);
            replay_case(case, &fixture_sha256, 2);
        };
        let first = std::panic::catch_unwind(run);
        if first.is_ok() {
            exact += 1;
            eprintln!("decorator super EXACT x2 {case_id}");
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
            eprintln!("decorator super REPEATED FAILURE {case_id}");
        }
    }
    eprintln!(
        "decorator super SUMMARY exact={exact} failed={} selected={}",
        failures.len(),
        cases.len()
    );
    assert!(
        failures.is_empty(),
        "complete decorator super failures ({}): {failures:?}",
        failures.len()
    );
}

fn replay_case(case: &Value, fixture_sha256: &str, pass: usize) {
    let case_id = case["case_id"].as_str().unwrap();
    let expected = &case["typescript_observation"];
    let prepared = prepare_program(case);
    let mut sink = RecordingSink {
        writes: Vec::new(),
        failure_index: case["write_failure_index"]
            .as_u64()
            .map(|index| index as usize),
    };
    let command = tsc_compiler::ProgramSession::new(prepared.clone())
        .emit_command_for_harness(&mut sink)
        .unwrap_or_else(|error| panic!("{case_id}: production command completes: {error}"));
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
    let activity = super::h2_7b_w4a_controls::assert_completed_observation(
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
    let blocked = prepared.compiler_options().no_emit_on_error == Some(true)
        && expected["emit_result"]["emit_skipped"] == true;
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

/// Program preparation for the manifest's direct-option cases (the same host,
/// library catalog, limits and option projection as the shared comparator,
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
            "allowJs" => options.allow_js = value.as_bool().unwrap(),
            "checkJs" => options.check_js = value.as_bool(),
            "removeComments" => options.remove_comments = value.as_bool(),
            "strict" => options.strict = value.as_bool(),
            "experimentalDecorators" => options.experimental_decorators = value.as_bool().unwrap(),
            "useDefineForClassFields" => options.use_define_for_class_fields = value.as_bool(),
            "noEmitOnError" => options.no_emit_on_error = value.as_bool(),
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
// the comparator's first failure. Same schema as the retained-accessor
// captures plus the fixture pin and pass number.
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

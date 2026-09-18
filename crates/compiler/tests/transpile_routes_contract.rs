//! H2.8c contract: the three no-check routes against the pinned TypeScript
//! 6.0.3 observations minted twice by `scripts/observe-transpile-routes.mjs`.
//!
//!   transpile-js      tsc_compiler::transpile::transpile_module
//!   transpile-dts     tsc_compiler::transpile::transpile_declaration
//!   program-no-check  ProgramSession::with_emit_route(ProgramNoCheck).emit_command_for_harness
//!
//! Public observations are compared exactly. Rows listed in
//! `known-open.v1.json` are expected to differ (each with a recorded
//! reason); a known-open row that becomes exact fails the test so the list
//! shrinks with the implementation. Internal evidence (checked source files,
//! work counters, suppressed semantic/global diagnostics) is written to
//! `target/h2-8c/native-evidence.json` and asserted separately.

use base64::Engine;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use tsc_compiler::transpile::{
    transpile_declaration, transpile_module, JsDocParsingMode, TranspileError,
    TranspileOptionValue, TranspileOptions, TranspileOutput,
};
use tsc_compiler::{
    EmitArtifact, EmitArtifactKind, EmitIoError, EmitIoOperation, EmitOutcome, EmitRouteKind,
    EmitWriteDisposition, EmitWriteMetadata, OutputSink, ProgramSession,
};
use tsc_diagnostics::{Diagnostic, JsString, MessageChain};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_emitting_program, load_program, CompilerOptions, LibraryCatalog, PreparedProgram,
    ProgramLoadLimits, ProgramOptions,
};

#[path = "../../program/tests/support/scalar_json.rs"]
mod utf16_scalar_json;
use utf16_scalar_json::observe as scalar_json;

const INPUTS: &str = include_str!("fixtures/h2_8c_transpile/inputs.v1.json");
const EXPECTED: &str = include_str!("fixtures/h2_8c_transpile/expected.v1.json");
const KNOWN_NATIVE: &str = include_str!("fixtures/h2_8c_transpile/known-native.v1.json");
const KNOWN_OPEN: &str = include_str!("fixtures/h2_8c_transpile/known-open.v1.json");

fn fixtures() -> (Value, Value, BTreeMap<String, String>) {
    let inputs: Value = serde_json::from_str(INPUTS).unwrap();
    let expected: Value = serde_json::from_str(EXPECTED).unwrap();
    assert_eq!(inputs["schema"], "h2-8c-transpile-inputs.v1");
    assert_eq!(expected["schema"], "h2-8c-transpile-expected.v1");
    assert_eq!(inputs["typescript"]["version"], "6.0.3");
    validate_manifest(&inputs, &expected, INPUTS, 291);
    let catalog = LibraryCatalog::typescript_6_0_3("/lib");
    let target_libraries = expected["numeric_target_default_libraries"]
        .as_array()
        .unwrap();
    assert_eq!(target_libraries.len(), 3);
    for row in target_libraries {
        let options = CompilerOptions {
            target: Some(row["target"].as_i64().unwrap() as i32),
            ..CompilerOptions::default()
        };
        assert_eq!(
            catalog.default_file_name(&options),
            row["name"].as_str().unwrap()
        );
    }
    assert_eq!(
        inputs["route_counts"],
        json!({"transpile-js":153,"transpile-dts":87,"program-no-check":51})
    );
    let known: Value = serde_json::from_str(KNOWN_OPEN).unwrap();
    assert_eq!(known["schema"], "h2-8c-transpile-known-open.v1");
    assert_eq!(known["rows"].as_array().unwrap().len(), 8);
    let known: BTreeMap<String, String> = known["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["id"].as_str().unwrap().to_owned(),
                row["reason"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(known.len(), 8, "duplicate known-open ID");
    let native: Value = serde_json::from_str(KNOWN_NATIVE).unwrap();
    assert_eq!(native["schema"], "h2-8c-transpile-known-native.v1");
    let native_ids: BTreeSet<_> = native["observations"].as_object().unwrap().keys().collect();
    assert_eq!(native["case_count"], native_ids.len());
    assert_eq!(native_ids, known.keys().collect());
    let ids: BTreeSet<_> = inputs["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|case| case["id"].as_str().unwrap())
        .collect();
    assert!(
        known.keys().all(|id| ids.contains(id.as_str())),
        "unknown known-open ID"
    );
    (inputs, expected, known)
}

fn evidence_dir() -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/h2-8c");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn flatten(chain: &MessageChain, indent: usize, text: &mut String) {
    if indent != 0 {
        text.push('\n');
        text.push_str(&"  ".repeat(indent));
    }
    text.push_str(&chain.text.to_string_lossy());
    for child in &chain.next {
        flatten(child, indent + 1, text);
    }
}

fn diagnostics(values: &[Diagnostic]) -> Value {
    json!(values
        .iter()
        .map(|diagnostic| {
            let mut message = String::new();
            flatten(&diagnostic.message, 0, &mut message);
            let related = (diagnostic.related_information_present
                || !diagnostic.related.is_empty())
            .then(|| {
                diagnostic
                    .related
                    .iter()
                    .map(|related| {
                        let mut message = String::new();
                        flatten(&related.message, 0, &mut message);
                        json!({
                            "code": related.message.code,
                            "category": format!("{:?}", related.message.category),
                            "file": scalar_json(&related.file_name),
                            "start": related.start,
                            "length": related.length,
                            "message": message,
                            "related_information": null,
                        })
                    })
                    .collect::<Vec<_>>()
            });
            json!({
                "code": diagnostic.code(),
                "category": format!("{:?}", diagnostic.category()),
                "file": scalar_json(&diagnostic.file_name),
                "start": diagnostic.start,
                "length": diagnostic.length,
                "message": message,
                "related_information": related,
            })
        })
        .collect::<Vec<_>>())
}

fn text_record(text: Option<&JsString>) -> Value {
    match text {
        None => {
            json!({"present": false, "utf16_units": null, "utf8_base64": null, "utf8_sha256": null})
        }
        Some(text) => {
            let lossy = text.to_string_lossy();
            let bytes = lossy.as_bytes();
            json!({
                "present": true,
                "utf16_units": text.len_units(),
                "utf8_base64": base64::engine::general_purpose::STANDARD.encode(bytes),
                "utf8_sha256": sha256_hex(bytes),
            })
        }
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

// ---------------------------------------------------------------------------
// transpileModule / transpileDeclaration
// ---------------------------------------------------------------------------

fn option_value(value: &Value) -> TranspileOptionValue {
    match value {
        Value::Bool(value) => TranspileOptionValue::Bool(*value),
        Value::Number(value) => TranspileOptionValue::Number(value.as_f64().unwrap()),
        Value::String(value) => TranspileOptionValue::String(JsString::from(value.as_str())),
        Value::Array(values) => TranspileOptionValue::StringList(
            values
                .iter()
                .map(|value| JsString::from(value.as_str().unwrap()))
                .collect(),
        ),
        other => panic!("unsupported manifest option value {other}"),
    }
}

fn transpile_options(case: &Value) -> TranspileOptions {
    TranspileOptions {
        compiler_options: case["compilerOptions"].as_object().map(|object| {
            object
                .iter()
                .map(|(name, value)| (JsString::from(name.as_str()), option_value(value)))
                .collect()
        }),
        file_name: case["fileName"].as_str().map(JsString::from),
        report_diagnostics: case["reportDiagnostics"].as_bool(),
        module_name: case["moduleName"].as_str().map(JsString::from).or_else(|| {
            case["moduleNameUtf16"]
                .as_array()
                .map(|units| js_units(units))
        }),
        renamed_dependencies: case["renamedDependenciesUtf16"]
            .as_array()
            .map(|pairs| {
                pairs
                    .iter()
                    .map(|pair| {
                        (
                            js_units(pair[0].as_array().unwrap()),
                            js_units(pair[1].as_array().unwrap()),
                        )
                    })
                    .collect()
            })
            .or_else(|| {
                case["renamedDependencies"].as_object().map(|object| {
                    object
                        .iter()
                        .map(|(from, to)| {
                            (
                                JsString::from(from.as_str()),
                                JsString::from(to.as_str().unwrap()),
                            )
                        })
                        .collect()
                })
            }),
        jsdoc_parsing_mode: case["jsDocParsingMode"].as_str().map(|mode| match mode {
            "ParseAll" => JsDocParsingMode::ParseAll,
            "ParseNone" => JsDocParsingMode::ParseNone,
            "ParseForTypeErrors" => JsDocParsingMode::ParseForTypeErrors,
            "ParseForTypeInfo" => JsDocParsingMode::ParseForTypeInfo,
            other => panic!("unknown jsDocParsingMode {other}"),
        }),
    }
}

fn run_transpile(case: &Value) -> Result<TranspileOutput, TranspileError> {
    let text = case["text"].as_str().unwrap();
    let options = transpile_options(case);
    if case["route"] == "transpile-dts" {
        transpile_declaration(text, &options)
    } else {
        transpile_module(text, &options)
    }
}

fn transpile_observation(result: &Result<TranspileOutput, TranspileError>) -> Value {
    match result {
        Ok(output) => json!({
            "exception": null,
            "outputText": text_record(Some(&output.output_text)),
            "diagnostics_present": true,
            "diagnostics": diagnostics(&output.diagnostics),
            "sourceMapText": text_record(output.source_map_text.as_ref()),
        }),
        Err(error) => json!({
            "exception": {"message": error.to_string()},
            "outputText": null,
            "diagnostics_present": null,
            "diagnostics": null,
            "sourceMapText": null,
        }),
    }
}

fn transpile_evidence(result: &Result<TranspileOutput, TranspileError>) -> Value {
    match result {
        Ok(output) => {
            let evidence = &output.evidence;
            json!({
                "route": evidence.route.name(),
                "input_file_name": scalar_json(&evidence.input_file_name),
                "source_files": scalar_json(&evidence.source_files),
                "writes": evidence.writes.iter().map(|(path, kind)| json!([scalar_json(path), format!("{kind:?}")])).collect::<Vec<_>>(),
                "emit_skipped": evidence.emit_skipped,
                "fixup_diagnostics": evidence.fixup_diagnostics,
                "checked_source_files": evidence.checked_source_files,
                "parsed_documents": evidence.work_counters.parsed_documents(),
                "bound_documents": evidence.work_counters.bound_documents(),
                "syntactic_diagnostics": diagnostics(&evidence.syntactic_diagnostics),
                "options_diagnostics": diagnostics(&evidence.options_diagnostics),
                "global_diagnostics_evidence": diagnostics(&evidence.global_diagnostics_evidence),
                "semantic_diagnostics_evidence": diagnostics(&evidence.semantic_diagnostics_evidence),
                "no_check": evidence.effective_options.no_check,
                "isolated_modules": evidence.effective_options.isolated_modules,
                "no_resolve": evidence.effective_options.no_resolve,
                "target": evidence.effective_options.target,
                "jsx": evidence.effective_options.jsx,
            })
        }
        Err(error) => json!({"rust_error": error.to_string(), "kind": error_kind(error)}),
    }
}

fn error_kind(error: &TranspileError) -> &'static str {
    match error {
        TranspileError::OutputGenerationFailed { .. } => "output-generation-failed",
        TranspileError::MultipleOutputs { .. } => "multiple-outputs",
        TranspileError::UnsupportedOptionValue { .. } => "rust-unsupported-option-value",
        TranspileError::UnsupportedTranspileOption { .. } => "rust-unsupported-transpile-option",
        TranspileError::Program(_) => "rust-program-load",
        TranspileError::Driver(_) => "rust-driver",
    }
}

// ---------------------------------------------------------------------------
// Program noCheck complete command
// ---------------------------------------------------------------------------

fn libraries() -> Vec<(String, Vec<u8>)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/typescript-6.0.3/lib");
    std::fs::read_dir(root)
        .unwrap()
        .map(Result::unwrap)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            (name.starts_with("lib.") && name.ends_with(".d.ts"))
                .then(|| (name, std::fs::read(entry.path()).unwrap()))
        })
        .collect()
}

fn program_options(case: &Value) -> (CompilerOptions, ProgramOptions) {
    let mut raw = Vec::new();
    let mut program_options = ProgramOptions::default();
    for (name, value) in case["options"].as_object().unwrap() {
        if name == "noLib" {
            program_options = program_options.with_no_lib(value.as_bool().unwrap());
            continue;
        }
        raw.push((JsString::from(name.as_str()), option_value(value)));
    }
    let mut fixup = Vec::new();
    let options = tsc_compiler::transpile::fixup_compiler_options(&raw, &mut fixup)
        .unwrap_or_else(|error| panic!("{}: option conversion {error}", case["id"]));
    assert!(
        fixup.is_empty(),
        "{}: manifest options must convert cleanly",
        case["id"]
    );
    (options, program_options)
}

fn prepare(case: &Value, libraries: &[(String, Vec<u8>)]) -> PreparedProgram {
    let mut host = MemoryCompilerHost::builder(case["current_directory"].as_str().unwrap())
        .case_sensitive(case["use_case_sensitive_file_names"].as_bool().unwrap());
    for file in case["files"].as_array().unwrap() {
        host = host.file(
            file["path"].as_str().unwrap(),
            file["text"].as_str().unwrap().as_bytes(),
        );
    }
    for (name, bytes) in libraries {
        host = host.file(format!("/lib/{name}"), bytes.clone());
    }
    let roots = case["roots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|root| PathBuf::from(root.as_str().unwrap()))
        .collect::<Vec<_>>();
    let (options, program_options) = program_options(case);
    let load = if options.no_emit == Some(true) {
        load_program
    } else {
        load_emitting_program
    };
    load(
        &host.build().unwrap(),
        &roots,
        options,
        program_options,
        &LibraryCatalog::typescript_6_0_3("/lib"),
        ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024),
    )
    .unwrap_or_else(|error| panic!("{}: program load {error}", case["id"]))
}

struct RuleSink<'a> {
    rules: &'a Value,
    writes: Vec<Value>,
}

fn write_record(write: &EmitArtifact, index: usize) -> Value {
    let (present, keys, position, data_diagnostics) = match write.metadata() {
        Some(EmitWriteMetadata::Text(data)) => (
            true,
            json!(["sourceMapUrlPos", "diagnostics"]),
            json!(data.source_map_url_position().map(|p| p.value())),
            diagnostics(data.diagnostics()),
        ),
        None => (false, Value::Null, Value::Null, Value::Null),
        _ => panic!("buildInfo is outside this fixture"),
    };
    let kind = match write.kind() {
        EmitArtifactKind::JavaScript => "javascript",
        EmitArtifactKind::Declaration => "declaration",
        EmitArtifactKind::JavaScriptMap | EmitArtifactKind::DeclarationMap => "source-map",
        EmitArtifactKind::BuildInfo => "build-info",
    };
    json!({"index":index,"path":scalar_json(&write.path()),"kind":kind,
        "callback_utf8_base64":base64::engine::general_purpose::STANDARD.encode(write.callback_bytes()),
        "callback_utf8_bytes":write.callback_bytes().len(),"write_byte_order_mark":write.write_byte_order_mark(),
        "materialized_utf8_base64":base64::engine::general_purpose::STANDARD.encode(write.materialized_bytes()),
        "materialized_utf8_bytes":write.materialized_bytes().len(),"on_error_callback_present":true,
        "source_files":scalar_json(&write.source_files()),"data_present":present,"data_keys":keys,
        "data_source_map_url_pos":position,"data_diagnostics":data_diagnostics,"data_build_info":null,
        "sink_action":"write","on_error_messages":null})
}

impl OutputSink for RuleSink<'_> {
    fn write(&mut self, artifact: EmitArtifact) -> Result<EmitWriteDisposition, EmitIoError> {
        let action = self
            .rules
            .as_array()
            .into_iter()
            .flatten()
            .find(|rule| rule["path"].as_str().unwrap() == artifact.path().to_string_lossy())
            .map_or("write", |rule| rule["action"].as_str().unwrap());
        let mut record = write_record(&artifact, self.writes.len());
        record["sink_action"] = json!(action);
        if action == "on-error" {
            let message = "H2.8c controlled sink failure";
            record["on_error_messages"] = json!([message]);
            self.writes.push(record);
            return Err(EmitIoError::new(
                EmitIoOperation::WriteFile,
                artifact.path(),
                message,
            ));
        }
        self.writes.push(record);
        Ok(EmitWriteDisposition::Written)
    }
}

fn emit_result(outcome: &EmitOutcome) -> Value {
    json!({"emit_skipped":outcome.emit_skipped(),"diagnostics":diagnostics(outcome.diagnostics()),
        "emitted_files":scalar_json(&outcome.emitted_files()),"source_maps":outcome.source_maps().map(|maps|maps.iter().map(|map|json!({
            "input_source_file_names":scalar_json(&map.input_source_files()),"source_map_json":map.canonical_json()
        })).collect::<Vec<_>>())})
}

fn run_program(case: &Value, libraries: &[(String, Vec<u8>)]) -> (Value, Value) {
    let route = if case["options"]["noCheck"] == true {
        EmitRouteKind::ProgramNoCheck
    } else {
        EmitRouteKind::Program
    };
    let mut sink = RuleSink {
        rules: &case["sink_rules"],
        writes: Vec::new(),
    };
    let outcome = ProgramSession::new(prepare(case, libraries))
        .with_emit_route(route)
        .emit_command_for_harness(&mut sink);
    match outcome {
        Ok(outcome) => (
            json!({"kind":"ordinary-command","target_source":null,"writes":sink.writes,
                "reported_diagnostics":diagnostics(outcome.diagnostics()),
                "status_writes":scalar_json(&outcome.status_writes()),"exit_code":outcome.exit_code(),
                "emit_result":emit_result(outcome.emit()),"exception":null}),
            json!({"route": route.name(), "checked_source_files": outcome.checked_source_files()}),
        ),
        Err(error) => (
            json!({"kind":"ordinary-command","target_source":null,"writes":sink.writes,
                "reported_diagnostics":null,"status_writes":null,"exit_code":null,"emit_result":null,
                "exception":{"message":format!("{error:?}")}}),
            json!({"route": route.name(), "rust_error": format!("{error:?}")}),
        ),
    }
}

// ---------------------------------------------------------------------------
// Comparison harness
// ---------------------------------------------------------------------------

struct Report {
    exact: Vec<String>,
    known_open: Vec<String>,
    unexpected_open: Vec<String>,
    unexpected_exact: Vec<String>,
    rows: Vec<Value>,
}

fn diff_summary(actual: &Value, expected: &Value) -> Value {
    fn walk(path: &str, actual: &Value, expected: &Value, out: &mut Vec<Value>) {
        if actual == expected {
            return;
        }
        match (actual, expected) {
            (Value::Object(a), Value::Object(e)) => {
                let keys: BTreeSet<_> = a.keys().chain(e.keys()).collect();
                for key in keys {
                    walk(
                        &format!("{path}.{key}"),
                        a.get(key).unwrap_or(&Value::Null),
                        e.get(key).unwrap_or(&Value::Null),
                        out,
                    );
                }
            }
            (Value::Array(a), Value::Array(e)) if a.len() == e.len() => {
                for (index, (a, e)) in a.iter().zip(e).enumerate() {
                    walk(&format!("{path}[{index}]"), a, e, out);
                }
            }
            _ => {
                let render = |value: &Value| {
                    let text = value.to_string();
                    if text.len() > 300 {
                        format!("{}…", text.chars().take(300).collect::<String>())
                    } else {
                        text
                    }
                };
                out.push(
                    json!({"path": path, "actual": render(actual), "expected": render(expected)}),
                );
            }
        }
    }
    let mut out = Vec::new();
    walk("$", actual, expected, &mut out);
    json!(out)
}

fn decode_outputs(observation: &Value) -> Value {
    // Human-readable output text for the evidence report.
    let decode = |record: &Value| {
        record["utf8_base64"].as_str().map(|text| {
            String::from_utf8_lossy(
                &base64::engine::general_purpose::STANDARD
                    .decode(text)
                    .unwrap(),
            )
            .into_owned()
        })
    };
    json!({
        "outputText": decode(&observation["outputText"]),
        "sourceMapText": decode(&observation["sourceMapText"]),
    })
}

fn compare_route(route: &str, run: impl Fn(&Value) -> (Value, Value)) -> Report {
    let (inputs, expected, known) = fixtures();
    let expected_by_id: BTreeMap<&str, &Value> = expected["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|case| (case["id"].as_str().unwrap(), case))
        .collect();
    let mut report = Report {
        exact: Vec::new(),
        known_open: Vec::new(),
        unexpected_open: Vec::new(),
        unexpected_exact: Vec::new(),
        rows: Vec::new(),
    };
    for case in inputs["cases"].as_array().unwrap() {
        if case["route"] != route {
            continue;
        }
        let id = case["id"].as_str().unwrap();
        let expected_case = expected_by_id[id];
        let expected_observation = &expected_case["observation"];
        let (actual, evidence) =
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(case))) {
                Ok(pair) => pair,
                Err(error) => {
                    let reason = error
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| error.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_else(|| "unknown panic".to_owned());
                    (
                        json!({"exception": {"message": reason.clone()}}),
                        json!({"panic": reason}),
                    )
                }
            };
        let exact = observations_match(&actual, expected_observation, &evidence);
        let again = run(case).0;
        assert_eq!(actual, again, "{id}: repeated observation differs");
        let known_reason = known.get(id);
        let native: Value = serde_json::from_str(KNOWN_NATIVE).unwrap();
        if known_reason.is_some() && !exact {
            assert!(
                known_native_matches(id, &actual, &evidence, &native),
                "{id}: known-open native observation changed: {}",
                diff_summary(&actual, &native["observations"][id]["actual"])
            );
        }
        let status = match (exact, known_reason) {
            (true, None) => {
                report.exact.push(id.to_owned());
                "exact"
            }
            (true, Some(_)) => {
                report.unexpected_exact.push(id.to_owned());
                "known-open-now-exact"
            }
            (false, Some(_)) => {
                report.known_open.push(id.to_owned());
                "known-open"
            }
            (false, None) => {
                report.unexpected_open.push(id.to_owned());
                "open"
            }
        };
        report.rows.push(json!({
            "id": id,
            "actual": actual,
            "status": status,
            "known_open_reason": known_reason,
            "diff": if exact { Value::Null } else { diff_summary(&actual, expected_observation) },
            "actual_decoded": if exact { Value::Null } else { decode_outputs(&actual) },
            "expected_decoded": if exact { Value::Null } else { decode_outputs(expected_observation) },
            "evidence": evidence,
            "source_internal": expected_case["internal"].get("replica_public_result_matches"),
        }));
    }
    let path = evidence_dir().join(format!("native-{route}.json"));
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&json!({
            "route": route,
            "exact": report.exact.len(),
            "known_open": report.known_open.len(),
            "unexpected_open": report.unexpected_open,
            "unexpected_exact": report.unexpected_exact,
            "rows": report.rows,
        }))
        .unwrap(),
    )
    .unwrap();
    eprintln!(
        "{route}: exact={} known_open={} unexpected_open={:?} known_open_now_exact={:?} (report {})",
        report.exact.len(),
        report.known_open.len(),
        report.unexpected_open,
        report.unexpected_exact,
        path.display()
    );
    report
}

fn assert_report(report: &Report, expected_cases: usize) {
    assert_eq!(
        report.exact.len()
            + report.known_open.len()
            + report.unexpected_open.len()
            + report.unexpected_exact.len(),
        expected_cases,
        "route ran fewer cases than the manifest fixes"
    );
    assert!(
        report.unexpected_open.is_empty(),
        "rows newly differing from TypeScript: {:?}",
        report.unexpected_open
    );
    assert!(
        report.unexpected_exact.is_empty(),
        "known-open rows now exact; retire them from known-open.v1.json: {:?}",
        report.unexpected_exact
    );
}

#[test]
fn transpile_module_matches_typescript() {
    let report = compare_route("transpile-js", |case| {
        let result = run_transpile(case);
        (transpile_observation(&result), transpile_evidence(&result))
    });
    assert_report(&report, 153);
}

#[test]
fn transpile_declaration_matches_typescript() {
    let report = compare_route("transpile-dts", |case| {
        let result = run_transpile(case);
        (transpile_observation(&result), transpile_evidence(&result))
    });
    assert_report(&report, 87);
}

#[test]
fn program_no_check_commands_match_typescript() {
    let libraries = libraries();
    let report = compare_route("program-no-check", |case| run_program(case, &libraries));
    assert_report(&report, 51);
}

#[test]
fn no_check_routes_run_no_source_checking() {
    // The dedicated-route claim: every admitted noCheck command and every
    // transpile call leaves checkSourceFileWorker unexecuted, while the
    // checked controls execute it.
    let (inputs, _, _) = fixtures();
    let libraries = libraries();
    let mut no_check_cases = 0;
    let mut control_cases = 0;
    for case in inputs["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        match case["route"].as_str().unwrap() {
            "program-no-check" => {
                let (_, evidence) = run_program(case, &libraries);
                let Some(checked) = evidence["checked_source_files"].as_u64() else {
                    continue;
                };
                if case["options"]["noCheck"] == true {
                    assert_eq!(checked, 0, "{id}: noCheck route checked a source");
                    no_check_cases += 1;
                } else {
                    assert!(checked > 0, "{id}: checked control ran no source check");
                    control_cases += 1;
                }
            }
            _ => {
                if let Ok(output) = run_transpile(case) {
                    assert_eq!(
                        output.evidence.checked_source_files, 0,
                        "{id}: transpile route checked a source"
                    );
                    assert_eq!(output.evidence.effective_options.no_check, Some(true));
                    assert_eq!(output.evidence.effective_options.no_resolve, Some(true));
                    no_check_cases += 1;
                }
            }
        }
    }
    assert!(
        no_check_cases >= 150,
        "no-check evidence rows: {no_check_cases}"
    );
    assert!(control_cases >= 3, "checked control rows: {control_cases}");
}

#[test]
fn repeated_calls_keep_state_separate() {
    let (inputs, expected, _) = fixtures();
    let mut count = 0;
    for case in inputs["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        if !id.contains("/repeat/") || case["route"] == "program-no-check" {
            continue;
        }
        let first = transpile_observation(&run_transpile(case));
        let again = transpile_observation(&run_transpile(case));
        assert_eq!(first, again, "{id}: second call differs from the first");
        let expected_case = expected["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["id"] == id)
            .unwrap();
        assert_eq!(first, expected_case["observation"], "{id}");
        count += 1;
    }
    assert_eq!(count, 4);
    for row in expected["repeat_state_separation"].as_array().unwrap() {
        assert_eq!(row["matches"], true, "source repeat {}", row["id"]);
    }
}

fn js_units(units: &[Value]) -> JsString {
    JsString::from_code_units(
        &units
            .iter()
            .map(|unit| u16::try_from(unit.as_u64().unwrap()).unwrap())
            .collect::<Vec<_>>(),
    )
}

#[test]
fn review_api_facts_match_typescript() {
    let inputs: Value = serde_json::from_str(include_str!(
        "fixtures/h2_8c_transpile/review-inputs.v1.json"
    ))
    .unwrap();
    let expected: Value = serde_json::from_str(include_str!(
        "fixtures/h2_8c_transpile/review-expected.v1.json"
    ))
    .unwrap();
    validate_manifest(
        &inputs,
        &expected,
        include_str!("fixtures/h2_8c_transpile/review-inputs.v1.json"),
        14,
    );
    let mut rows = Vec::new();
    for (case, expected_case) in inputs["cases"]
        .as_array()
        .unwrap()
        .iter()
        .zip(expected["cases"].as_array().unwrap())
    {
        assert_eq!(case["id"], expected_case["id"]);
        let result = run_transpile(case);
        let actual = transpile_observation(&result);
        assert_eq!(
            actual,
            transpile_observation(&run_transpile(case)),
            "{}: repeat differs",
            case["id"]
        );
        if let Ok(output) = &result {
            assert_eq!(
                output.evidence.checked_source_files, 0,
                "{}: source checked",
                case["id"]
            );
        }
        rows.push(json!({"id":case["id"], "exact":actual==expected_case["observation"], "actual":actual, "expected":expected_case["observation"], "evidence":transpile_evidence(&result)}));
    }
    std::fs::write(
        evidence_dir().join("native-review.json"),
        serde_json::to_string_pretty(&rows).unwrap(),
    )
    .unwrap();
    let failed: Vec<_> = rows
        .iter()
        .filter(|row| row["exact"] != true)
        .map(|row| &row["id"])
        .collect();
    assert!(failed.is_empty(), "review differences: {failed:?}");
}

// These guards are part of the contract: changing a failure into another
// failure is not compatibility, and fixture maps must never discard rows.
fn validate_manifest(inputs: &Value, expected: &Value, bytes: &str, count: usize) {
    assert_eq!(inputs["case_count"], count);
    assert_eq!(expected["case_count"], count);
    assert_eq!(expected["inputs_sha256"], sha256_hex(bytes.as_bytes()));
    let ids = |record: &Value| {
        let cases = record["cases"].as_array().unwrap();
        assert_eq!(cases.len(), count);
        let ids: BTreeSet<String> = cases
            .iter()
            .map(|case| {
                let id = case["id"].as_str().unwrap();
                assert!(!id.is_empty());
                id.to_owned()
            })
            .collect();
        assert_eq!(ids.len(), count, "duplicate fixture ID");
        ids
    };
    assert_eq!(ids(inputs), ids(expected), "fixture membership drift");
    let mut routes = BTreeMap::<String, usize>::new();
    for case in inputs["cases"].as_array().unwrap() {
        let route = case["route"].as_str().unwrap();
        assert!(["transpile-js", "transpile-dts", "program-no-check"].contains(&route));
        *routes.entry(route.to_owned()).or_default() += 1;
        let oracle = expected["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == case["id"])
            .unwrap();
        assert_eq!(oracle["route"], case["route"]);
        assert_ne!(oracle["internal"]["replica_public_result_matches"], false);
    }
    assert_eq!(inputs["route_counts"], json!(routes));
}

fn observations_match(actual: &Value, expected: &Value, evidence: &Value) -> bool {
    if !evidence["panic"].is_null() {
        return false;
    }
    if expected["exception"].is_null() {
        actual == expected
    } else {
        // All three pinned TS exceptions are this specific source failure.
        expected["exception"]["message"] == "Debug Failure. Output generation failed"
            && evidence["kind"] == "output-generation-failed"
            && actual == expected
    }
}

fn known_native_matches(id: &str, actual: &Value, evidence: &Value, native: &Value) -> bool {
    evidence["panic"].is_null()
        && native["observations"]
            .get(id)
            .is_some_and(|row| row["actual"] == *actual && row["kind"] == evidence["kind"])
}

#[test]
fn comparator_rejects_rust_refusals_and_panics_as_source_exceptions() {
    let expected = transpile_observation(&Err(TranspileError::OutputGenerationFailed {
        writes: vec![],
    }));
    assert!(observations_match(
        &expected,
        &expected,
        &json!({"kind":"output-generation-failed"})
    ));
    for evidence in [
        json!({"kind":"rust-driver"}),
        json!({"kind":"rust-unsupported-option-value"}),
        json!({"kind":"multiple-outputs"}),
        json!({"panic":"failed"}),
    ] {
        assert!(!observations_match(&expected, &expected, &evidence));
    }
    let wrong = json!({"exception":{"message":"unrelated failure"}});
    assert!(!observations_match(
        &wrong,
        &expected,
        &json!({"kind":"output-generation-failed"})
    ));
    let unicode = json!("あ".repeat(150));
    assert!(!diff_summary(&unicode, &Value::Null)
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn known_open_rejects_changed_output_refusal_and_panic() {
    let native: Value = serde_json::from_str(KNOWN_NATIVE).unwrap();
    for (id, row) in native["observations"].as_object().unwrap() {
        let evidence = json!({"kind": row["kind"]});
        assert!(known_native_matches(id, &row["actual"], &evidence, &native));
        assert!(!known_native_matches(
            id,
            &json!({"exception":{"message":"new failure"}}),
            &evidence,
            &native
        ));
        assert!(!known_native_matches(
            id,
            &row["actual"],
            &json!({"kind":row["kind"],"panic":"new failure"}),
            &native
        ));
    }
    assert!(!known_native_matches(
        "missing",
        &Value::Null,
        &Value::Null,
        &native
    ));
}

#[test]
fn manifest_rejects_missing_duplicate_ids_and_changed_hash() {
    let inputs: Value = serde_json::from_str(INPUTS).unwrap();
    let expected: Value = serde_json::from_str(EXPECTED).unwrap();
    for mutation in 0..5 {
        let mut changed = expected.clone();
        match mutation {
            0 => {
                changed["cases"].as_array_mut().unwrap().pop();
            }
            1 => {
                changed["cases"][1] = changed["cases"][0].clone();
            }
            2 => {
                changed["cases"][0]["id"] = json!("unknown");
            }
            3 => {
                changed["inputs_sha256"] = json!("changed");
            }
            _ => {
                changed["cases"][0]["route"] = json!("wrong");
            }
        }
        assert!(
            std::panic::catch_unwind(|| validate_manifest(&inputs, &changed, INPUTS, 291)).is_err()
        );
    }
}

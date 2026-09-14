//! Additional accepted-review identity, recovery and tagged-target controls.
//! Retains all callback, diagnostic-value, emit-result, status and exit fields.
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tsc_compiler::{MemoryOutputSink, ProgramSession};
use tsc_diagnostics::{Diagnostic, JsStr, JsString, MessageChain};
use tsc_emitter::{EmitArtifact, EmitWriteMetadata};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_emitting_program, load_program, CompilerOptions, LibraryCatalog, PreparedProgram,
    ProgramLoadLimits, ProgramOptions,
};

const FIXTURE: &[u8] = include_bytes!("fixtures/utf16-identity-recovery-controls.json");
fn prepare(case: &Value) -> PreparedProgram {
    let files = case["files"].as_array().unwrap();
    for file in files {
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(file["text"].as_str().unwrap().as_bytes())
            ),
            file["sha256"]
        );
    }
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut host = MemoryCompilerHost::builder("/project");
    for file in files {
        host = host.file(
            file["path"].as_str().unwrap(),
            file["text"].as_str().unwrap().as_bytes().to_vec(),
        );
    }
    for entry in std::fs::read_dir(workspace.join("vendor/typescript-6.0.3/lib")).unwrap() {
        let entry = entry.unwrap();
        let name = entry
            .file_name()
            .into_string()
            .expect("scalar vendored library name");
        if name.starts_with("lib.") && name.ends_with(".d.ts") {
            host = host.file(format!("/lib/{name}"), std::fs::read(entry.path()).unwrap());
        }
    }
    let host = host.build().unwrap();
    let options = CompilerOptions {
        target: Some(i32::try_from(case["options"]["target"].as_i64().unwrap()).unwrap()),
        module: Some(i32::try_from(case["options"]["module"].as_i64().unwrap()).unwrap()),
        new_line: Some(0),
        strict: Some(true),
        declaration: case["options"]["declaration"].as_bool(),
        declaration_map: case["options"]["declarationMap"].as_bool(),
        source_map: case["options"]["sourceMap"].as_bool(),
        skip_default_lib_check: Some(true),
        no_error_truncation: Some(true),
        out_dir: Some("/project/out".into()),
        no_emit: case["options"]["noEmit"].as_bool(),
        no_emit_on_error: case["options"]["noEmitOnError"].as_bool(),
        inline_source_map: case["options"]["inlineSourceMap"].as_bool(),
        list_emitted_files: case["options"]["listEmittedFiles"].as_bool(),
        ..Default::default()
    };
    // These controls observe a complete createProgram command, so the input
    // must not pass through an older corpus floor that drops emit options.
    let loader = if options.no_emit == Some(true) {
        load_program
    } else {
        load_emitting_program
    };
    loader(
        &host,
        &case["roots"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| PathBuf::from(value.as_str().unwrap()))
            .collect::<Vec<_>>(),
        options,
        ProgramOptions::default(),
        &LibraryCatalog::typescript_6_0_3("/lib"),
        ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024),
    )
    .unwrap()
}

fn string_value<'a>(text: impl Into<JsStr<'a>>) -> Value {
    let text = text.into();
    json!({"utf16": text.to_utf16(), "utf8_base64": base64::engine::general_purpose::STANDARD.encode(text.to_string_lossy().as_bytes())})
}

fn message(chain: &MessageChain, indent: usize, text: &mut JsString) {
    if indent != 0 {
        text.push_str("\n");
        text.push_str(&"  ".repeat(indent));
    }
    text.push_js(chain.text.as_js());
    for next in &chain.next {
        message(next, indent + 1, text);
    }
}

fn diagnostics(values: &[Diagnostic]) -> Value {
    json!(values
        .iter()
        .map(|d| {
            let mut text = JsString::new();
            message(&d.message, 0, &mut text);
            let related = (d.related_information_present || !d.related.is_empty()).then(|| {
                d.related
                    .iter()
                    .map(|r| {
                        let mut text = JsString::new();
                        message(&r.message, 0, &mut text);
                        json!({"code": r.message.code, "category": r.message.category as u8,
                "file": r.file_name.as_ref().map(|value| scalar_path(value.as_js())), "start": r.start, "length": r.length,
                "message": string_value(&text), "related_information": null})
                    })
                    .collect::<Vec<_>>()
            });
            json!({"code": d.code(), "category": d.category() as u8, "file": d.file_name.as_ref().map(|value| scalar_path(value.as_js())),
            "start": d.start, "length": d.length, "message": string_value(&text),
            "related_information": related})
        })
        .collect::<Vec<_>>())
}

fn captured_write(index: usize, artifact: &EmitArtifact) -> Value {
    let (keys, position, data_diagnostics) = match artifact.metadata() {
        Some(EmitWriteMetadata::Text(data)) => (
            json!(["sourceMapUrlPos", "diagnostics"]),
            json!(data.source_map_url_position().map(|p| p.value())),
            diagnostics(data.diagnostics()),
        ),
        None => (Value::Null, Value::Null, Value::Null),
        _ => panic!("unexpected callback metadata in these controls"),
    };
    // These frozen source and callback strings are scalar. Diagnostic values retain UTF-16
    // separately above; this byte channel does not qualify arbitrary callbacks.
    let callback = std::str::from_utf8(artifact.callback_bytes()).unwrap();
    json!({"index": index, "path": scalar_path(artifact.path()),
        "callback": string_value(callback), "write_byte_order_mark": artifact.write_byte_order_mark(),
        "materialized_utf8_base64": base64::engine::general_purpose::STANDARD.encode(artifact.materialized_bytes()),
        "on_error_callback_present": true, "source_files": artifact.source_files().map(|values| values.iter().map(|value| scalar_path(value.as_js())).collect::<Vec<_>>()),
        "data_present": artifact.metadata().is_some(), "data_keys": keys,
        "data_source_map_url_pos": position, "data_diagnostics": data_diagnostics})
}

#[test]
fn identity_and_recovery_controls_match_complete_commands_twice() {
    assert_eq!(
        format!("{:x}", Sha256::digest(FIXTURE)),
        "0605b71cafcffa559630bf891158a18441a5d00b84e359f9897407248c25e343"
    );
    let fixture: Value = serde_json::from_slice(FIXTURE).unwrap();
    assert_eq!(fixture["repetitions"], 2);
    assert_eq!(fixture["typescript"], "6.0.3");
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 65);
    assert_eq!(fixture["complete_command_executions"], 130);
    compare_complete_cases(cases);
}

#[test]
fn no_emit_controls_match_complete_commands_twice() {
    let bytes = include_bytes!("fixtures/utf16-noemit-command-controls.json");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "240b8d67c357b4268574d29031263a7984956cc6bc1fd7b675be0eaf82c577a0"
    );
    let fixture: Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(fixture["repetitions"], 2);
    assert_eq!(fixture["complete_command_executions"], 28);
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 14);
    compare_complete_cases(cases);
    // The new empty-build-info result cannot admit an actual incremental
    // output request through the separate no-emit command observation.
    for options in [
        CompilerOptions {
            no_emit: Some(true),
            incremental: Some(true),
            ..Default::default()
        },
        CompilerOptions {
            no_emit: Some(true),
            composite: Some(true),
            ..Default::default()
        },
    ] {
        assert!(matches!(
            tsc_compiler::EmitOutcome::no_emit_without_build_info(&options),
            Err(tsc_compiler::EmitFailure::Unsupported(
                tsc_compiler::UnsupportedEmitFeature::BuildInfo
            ))
        ));
    }
}

fn compare_complete_cases(cases: &[Value]) {
    let mut failures = Vec::new();
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let expected = &case["complete_command_runs"][0];
        assert_eq!(*expected, case["complete_command_runs"][1]);
        let mut actuals = Vec::new();
        for attempt in 0..2 {
            let no_emit_observation = if case["options"]["noEmit"] == true {
                let outcome = ProgramSession::new(prepare(case)).run().unwrap();
                assert!(outcome.no_emit_activity().all_zero());
                Some(json!({"route":"NoEmit","all_activity_zero":true}))
            } else {
                None
            };
            let mut sink = MemoryOutputSink::new();
            let result = ProgramSession::new(prepare(case)).emit_command_for_harness(&mut sink);
            let typed_refusal = matches!(&result,
                Err(tsc_compiler::DriverError::Emit(tsc_compiler::EmitFailure::Transform(error)))
                if matches!(error.as_ref(), tsc_emitter::TransformError::ParseDiagnosticsDeferred { owner_slice: "H2.9", .. }));
            let writes = sink
                .writes()
                .iter()
                .enumerate()
                .map(|(index, artifact)| captured_write(index, artifact))
                .collect::<Vec<_>>();
            let (actual, error) = match result {
                Ok(command) => {
                    let emit = command.emit();
                    let maps = emit.source_maps().map(|maps| maps.iter().map(|map| json!({
                        "input_source_file_names": map.input_source_files().iter().map(|value| scalar_path(value.as_js())).collect::<Vec<_>>(), "source_map_json": map.canonical_json()
                    })).collect::<Vec<_>>());
                    (
                        Some(
                            json!({"writes": writes, "reported_diagnostics": diagnostics(command.diagnostics()),
                        "status_writes": command.status_writes().iter().map(string_value).collect::<Vec<_>>(),
                        "exit_code": command.exit_code(), "emit_result": {"emit_skipped": emit.emit_skipped(),
                            "diagnostics": diagnostics(emit.diagnostics()), "emitted_files": emit.emitted_files().map(|values| values.iter().map(|value| scalar_path(value.as_js())).collect::<Vec<_>>()), "source_maps": maps}}),
                        ),
                        None,
                    )
                }
                Err(error) => (None, Some(error.to_string())),
            };
            let capture = json!({"case_id": id, "attempt": attempt, "actual": actual, "error": error,
                "no_emit_observation":no_emit_observation, "typed_recovery_refusal": typed_refusal, "native_contract": case["native"], "partial_writes": if actual.is_none() { Some(writes.clone()) } else { None }, "expected": expected});
            if let Some(directory) = std::env::var_os("TSC_RS_UTF16_IDENTITY_RECOVERY_CAPTURE_DIR")
            {
                let directory = PathBuf::from(directory);
                assert!(directory.is_absolute());
                std::fs::create_dir_all(&directory).unwrap();
                let key = format!("{:x}", Sha256::digest(id.as_bytes()));
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(directory.join(format!("{key}-{attempt}.json")))
                    .unwrap();
                serde_json::to_writer_pretty(&mut file, &capture).unwrap();
            }
            actuals.push((actual, error, typed_refusal, writes.is_empty()));
        }
        assert_eq!(actuals[0], actuals[1], "{id}: repetition drift");
        let passed = if case["native"] == "typed-recovery-refusal" {
            // These negative controls retain the agreed H2.9 admission boundary.
            // Keep the upstream tuple in the capture; do not count a refusal as exact.
            actuals[0].0.is_none() && actuals[0].2 && actuals[0].3
        } else {
            actuals[0].1.is_none() && actuals[0].0.as_ref() == Some(expected)
        };
        if !passed {
            failures.push(id);
        }
    }
    assert!(
        failures.is_empty(),
        "identity/recovery control divergences: {failures:?}"
    );
}

fn scalar_path<'a>(value: JsStr<'a>) -> &'a str {
    value
        .as_str()
        .expect("these frozen control path observations are scalar")
}

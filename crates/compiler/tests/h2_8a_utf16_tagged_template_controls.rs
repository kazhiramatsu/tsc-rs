//! Native complete-command comparison of review v1/v2 and added C controls.
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
    load_emitting_program, CompilerOptions, LibraryCatalog, PreparedProgram, ProgramLoadLimits,
    ProgramOptions,
};

const FIXTURE: &[u8] = include_bytes!("fixtures/utf16-tagged-template-controls.json");
const REVIEW_V1: &[u8] = include_bytes!("fixtures/utf16-tagged-template-review-v1.json");
const REVIEW_V2: &[u8] = include_bytes!("fixtures/utf16-tagged-template-review-v2.json");

fn prepare(case: &Value) -> PreparedProgram {
    let source = case["source"].as_str().unwrap().as_bytes();
    assert_eq!(
        format!("{:x}", Sha256::digest(source)),
        case["source_sha256"]
    );
    assert_eq!(
        case["options"],
        json!({"target":case["target"],"module":99,"newLine":0,"strict":true,
        "declaration":true,"declarationMap":true,"sourceMap":true,"skipDefaultLibCheck":true,
        "noErrorTruncation":true,"outDir":"/project/out"})
    );
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut host =
        MemoryCompilerHost::builder("/project").file("/project/main.ts", source.to_vec());
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
        target: Some(i32::try_from(case["target"].as_i64().unwrap()).unwrap()),
        module: Some(99),
        new_line: Some(0),
        strict: Some(true),
        declaration: Some(true),
        declaration_map: Some(true),
        source_map: Some(true),
        skip_default_lib_check: Some(true),
        no_error_truncation: Some(true),
        out_dir: Some("/project/out".into()),
        ..Default::default()
    };
    // These controls observe a complete createProgram command, so the input
    // must not pass through an older corpus floor that drops emit options.
    load_emitting_program(
        &host,
        &[PathBuf::from("/project/main.ts")],
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
        _ => panic!("unexpected callback metadata in C controls"),
    };
    // These C control callback strings are scalar. Diagnostic values retain UTF-16
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
fn tagged_template_controls_match_complete_commands_twice() {
    assert_eq!(
        format!("{:x}", Sha256::digest(FIXTURE)),
        "e8d6d1fa07d16d1887df1cb26dbbd51c150f42407a73e495239f7ccfd2a62688"
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(REVIEW_V1)),
        "92fbb23eafa07bfbf8c33de981453f32ee9c7abe8589886f288a3819c35537ca"
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(REVIEW_V2)),
        "28b126f461adf71aa07a199d9cfb21713a78d4e297a7094806f767aba69d0df9"
    );
    let fixture: Value = serde_json::from_slice(FIXTURE).unwrap();
    assert_eq!(fixture["repetitions"], 2);
    assert_eq!(fixture["typescript"], "6.0.3");
    assert_eq!(fixture["complete_command_executions"], 32);
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 16);
    let review: Value = serde_json::from_slice(REVIEW_V2).unwrap();
    for old in review["cases"].as_array().unwrap() {
        let current = cases.iter().find(|case| case["id"] == old["id"]).unwrap();
        assert_eq!(current["source"], old["source"]);
        assert_eq!(
            current["complete_command_runs"],
            old["complete_command_runs"]
        );
    }
    let mut failures = Vec::new();
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let expected = &case["complete_command_runs"][0];
        assert_eq!(*expected, case["complete_command_runs"][1]);
        let mut actuals = Vec::new();
        for attempt in 0..2 {
            let mut sink = MemoryOutputSink::new();
            let result = ProgramSession::new(prepare(case)).emit_command_for_harness(&mut sink);
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
                "partial_writes": if actual.is_none() { Some(writes) } else { None }, "expected": expected});
            if let Some(directory) = std::env::var_os("TSC_RS_UTF16_TAGGED_CONTROLS_CAPTURE_DIR") {
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
            actuals.push((actual, error));
        }
        assert_eq!(actuals[0], actuals[1], "{id}: repetition drift");
        if actuals[0].1.is_some() || actuals[0].0.as_ref() != Some(expected) {
            failures.push(id);
        }
    }
    assert!(
        failures.is_empty(),
        "complete tagged-template control divergences: {failures:?}"
    );
}

fn scalar_path<'a>(value: JsStr<'a>) -> &'a str {
    value
        .as_str()
        .expect("these frozen C-control path observations are scalar")
}

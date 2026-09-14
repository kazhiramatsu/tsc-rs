//! Supplemental complete-command proof for the same four frozen H2.5h
//! UTF-16 rows. The earlier typed comparator and its expectations remain
//! unchanged. This target adds callback metadata, related diagnostics,
//! command status and complete emit-result fields from fresh TS observations.
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tsc_compiler::{MemoryOutputSink, ProgramSession};
use tsc_diagnostics::{Diagnostic, MessageChain};
use tsc_emitter::{EmitArtifact, EmitArtifactKind, EmitWriteMetadata};
use tsc_harness::upstream_suites::execution::{
    load_qualified_compiler_emit_with_option_floor, EmitOptionFloor,
};
use tsc_program::{PreparedProgram, ProgramLoadLimits};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn prepare(case: &Value) -> PreparedProgram {
    let input = &case["input"];
    assert!(input["virtual_config"].is_null());
    assert_eq!(input["vfs_symlinks"], json!([]));
    let files = input["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(file["utf8_base64"].as_str().unwrap())
                .unwrap();
            assert_eq!(bytes.len() as u64, file["utf8_bytes"].as_u64().unwrap());
            assert_eq!(format!("{:x}", Sha256::digest(&bytes)), file["utf8_sha256"]);
            (PathBuf::from(file["path"].as_str().unwrap()), bytes)
        })
        .collect::<Vec<_>>();
    let roots = input["roots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|root| PathBuf::from(root.as_str().unwrap()))
        .collect::<Vec<_>>();
    let settings = input["settings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["name"].as_str().unwrap().to_owned(),
                s["value"].as_str().unwrap().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    load_qualified_compiler_emit_with_option_floor(
        &workspace(),
        input["current_directory"].as_str().unwrap(),
        &files,
        &roots,
        &settings,
        ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024),
        EmitOptionFloor::Established,
    )
    .unwrap()
}

#[test]
fn utf16_original_rows_match_complete_commands() {
    let artifact: Value =
        serde_json::from_slice(include_bytes!("fixtures/utf16-original-rows-complete.json"))
            .unwrap();
    let parent_bytes =
        std::fs::read(workspace().join("ratchets/h2-5h-qualification.v1.json")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&parent_bytes)),
        artifact["parent"]["sha256"]
    );
    let parent: Value = serde_json::from_slice(&parent_bytes).unwrap();
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    assert_eq!(cases.len(), 4);
    let mut failures = Vec::new();
    for case in cases {
        let id = case["case_id"].as_str().unwrap();
        let original = parent["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["case_id"] == id)
            .unwrap();
        assert_eq!(original["input"], case["input"]);
        assert_eq!(original["execution_route"], "qualified-vfs");
        let expected = &case["typescript_observation"];
        let mut observations = Vec::new();
        for attempt in 0..2 {
            let mut sink = MemoryOutputSink::new();
            let command = ProgramSession::new(prepare(case)).emit_command_for_harness(&mut sink);
            let writes = sink
                .writes()
                .iter()
                .enumerate()
                .map(|(index, artifact)| captured_write(index, artifact))
                .collect::<Vec<_>>();
            let (actual, error) = match command {
                Ok(command) => {
                    let emit = command.emit();
                    let maps = emit.source_maps().map(|maps| {
                        maps.iter()
                            .map(|map| {
                                json!({
                                    "input_source_file_names": map.input_source_files().iter().map(|value| scalar_observation(value.as_js())).collect::<Vec<_>>(),
                                    "source_map_json": map.canonical_json()
                                })
                            })
                            .collect::<Vec<_>>()
                    });
                    (
                        Some(json!({"writes": writes,
                        "reported_diagnostics": diagnostics(command.diagnostics()),
                        "emit_refused": emit.emit_skipped(), "emit_result": {
                            "emit_skipped": emit.emit_skipped(), "diagnostics": diagnostics(emit.diagnostics()),
                            "emitted_files": emit.emitted_files().map(|values| values.iter().map(|value| scalar_observation(value.as_js())).collect::<Vec<_>>()), "source_maps": maps},
                        "status_writes": command.status_writes().iter().map(|value| scalar_observation(value.as_js())).collect::<Vec<_>>(), "exit_code": command.exit_code()})),
                        None,
                    )
                }
                Err(error) => (None, Some(error.to_string())),
            };
            let capture = json!({"case_id": id, "attempt": attempt,
                "capture_kind": "primary-complete-command", "actual": actual, "error": error,
                "partial_writes": if actual.is_none() { Some(writes) } else { None }, "expected": expected});
            if let Some(directory) = std::env::var_os("TSC_RS_UTF16_ORIGINAL_CAPTURE_DIR") {
                let directory = PathBuf::from(directory);
                assert!(directory.is_absolute());
                std::fs::create_dir_all(&directory).unwrap();
                let key = format!("{:x}", Sha256::digest(id.as_bytes()));
                let file = directory.join(format!("{key}-{attempt}.json"));
                assert!(!file.exists(), "use a fresh capture directory");
                std::fs::write(file, serde_json::to_vec_pretty(&capture).unwrap()).unwrap();
            }
            observations.push((actual, error));
        }
        assert_eq!(observations[0], observations[1], "{id}: repetition drift");
        if observations[0].1.is_some() || observations[0].0.as_ref() != Some(expected) {
            failures.push(id);
            eprintln!("{id}: full command differs; inspect the complete capture");
        }
    }
    assert!(
        failures.is_empty(),
        "full command divergences: {failures:?}"
    );
}

fn message(chain: &MessageChain, indent: usize, text: &mut String) {
    if indent != 0 {
        text.push('\n');
        text.push_str(&"  ".repeat(indent));
    }
    text.push_str(chain.text.as_str().expect("scalar frozen diagnostic text"));
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
                "file":r.file_name.as_ref().map(|value| scalar_observation(value.as_js())),"start":r.start,"length":r.length,"message":text,"related_information":null})
        }).collect::<Vec<_>>());
        json!({"code":d.code(),"category":format!("{:?}",d.category()),"file":d.file_name.as_ref().map(|value| scalar_observation(value.as_js())),
            "start":d.start,"length":d.length,"message":text,"related_information":related})
    }).collect::<Vec<_>>())
}

fn captured_write(index: usize, artifact: &EmitArtifact) -> Value {
    assert_eq!(artifact.kind(), EmitArtifactKind::JavaScript);
    let (keys, position, data_diagnostics) = match artifact.metadata() {
        Some(EmitWriteMetadata::Text(data)) => (
            json!(["sourceMapUrlPos", "diagnostics"]),
            json!(data.source_map_url_position().map(|p| p.value())),
            diagnostics(data.diagnostics()),
        ),
        None => (Value::Null, Value::Null, Value::Null),
        _ => panic!("unexpected original callback metadata"),
    };
    let callback = artifact.callback_bytes();
    let materialized = artifact.materialized_bytes();
    json!({"index": index, "path": scalar_observation(artifact.path()), "kind": "javascript",
        "callback_utf8_base64": base64::engine::general_purpose::STANDARD.encode(callback),
        "callback_utf8_sha256": format!("{:x}", Sha256::digest(callback)), "callback_utf8_bytes": callback.len(),
        "write_byte_order_mark": artifact.write_byte_order_mark(),
        "materialized_utf8_base64": base64::engine::general_purpose::STANDARD.encode(&materialized),
        "materialized_utf8_sha256": format!("{:x}", Sha256::digest(&materialized)), "materialized_utf8_bytes": materialized.len(),
        // OutputSink::write's Result is the typed equivalent of onError.
        "on_error_callback_present": true, "source_files": artifact.source_files().map(|values| values.iter().map(|value| scalar_observation(value.as_js())).collect::<Vec<_>>()),
        "data_present": artifact.metadata().is_some(), "data_keys": keys,
        "data_source_map_url_pos": position, "data_diagnostics": data_diagnostics})
}

// This legacy JSON schema owns scalar strings. Reject unexpected lone units
// explicitly at the observation boundary instead of changing their identity.
fn scalar_observation(value: tsc_diagnostics::JsStr<'_>) -> String {
    value
        .as_str()
        .expect("non-scalar JS value cannot match this scalar frozen observation")
        .to_owned()
}

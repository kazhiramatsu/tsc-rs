//! Corpus rows newly admitted by the parser-owned literal-only recovery
//! predicate (handoff §6 step 4): each row's qualified VFS is rebuilt with the
//! acceptance loader at the established floor, the production command runs,
//! and the complete tuple must equal the upstream observation twice.
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tsc_compiler::{MemoryOutputSink, ProgramSession};
use tsc_diagnostics::{Diagnostic, JsStr, JsString, MessageChain};
use tsc_emitter::{EmitArtifact, EmitWriteMetadata};
use tsc_harness::upstream_suites::execution::{
    load_qualified_compiler_emit_with_symlinks, EmitOptionFloor,
};
use tsc_program::{PreparedProgram, ProgramLoadLimits};

const FIXTURE: &[u8] = include_bytes!("fixtures/utf16-literal-recovery-corpus.json");

fn prepare(case: &Value) -> PreparedProgram {
    let input = &case["input"];
    assert_eq!(input["floor"], "established");
    assert_eq!(input["use_case_sensitive_file_names"], true);
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let files = input["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| {
            let text = file["text"].as_str().unwrap();
            assert_eq!(
                format!("{:x}", Sha256::digest(text.as_bytes())),
                file["sha256"]
            );
            (
                PathBuf::from(file["path"].as_str().unwrap()),
                text.as_bytes().to_vec(),
            )
        })
        .collect::<Vec<_>>();
    let symlinks = input["vfs_symlinks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|link| {
            (
                PathBuf::from(link["link_path"].as_str().unwrap()),
                PathBuf::from(link["target_path"].as_str().unwrap()),
            )
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
        .map(|pair| {
            (
                pair[0].as_str().unwrap().to_owned(),
                pair[1].as_str().unwrap().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    load_qualified_compiler_emit_with_symlinks(
        &workspace,
        input["current_directory"].as_str().unwrap(),
        &files,
        &symlinks,
        &roots,
        &settings,
        ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024),
        EmitOptionFloor::Established,
    )
    .unwrap_or_else(|error| panic!("{}: qualified load failed: {error}", case["case_id"]))
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
        _ => panic!("unexpected callback metadata in these corpus rows"),
    };
    // The callback bytes are the UTF-8 sink projection (U+FFFD per unpaired
    // unit), byte-identical to what TypeScript's callback text encodes.
    let callback = std::str::from_utf8(artifact.callback_bytes()).unwrap();
    json!({"index": index, "path": scalar_path(artifact.path()),
        "callback": string_value(callback), "write_byte_order_mark": artifact.write_byte_order_mark(),
        "materialized_utf8_base64": base64::engine::general_purpose::STANDARD.encode(artifact.materialized_bytes()),
        "on_error_callback_present": true, "source_files": artifact.source_files().map(|values| values.iter().map(|value| scalar_path(value.as_js())).collect::<Vec<_>>()),
        "data_present": artifact.metadata().is_some(), "data_keys": keys,
        "data_source_map_url_pos": position, "data_diagnostics": data_diagnostics})
}

fn scalar_path<'a>(value: JsStr<'a>) -> &'a str {
    value
        .as_str()
        .expect("these frozen corpus path observations are scalar")
}

#[test]
fn newly_admitted_literal_recovery_rows_match_complete_commands_twice() {
    let fixture: Value = serde_json::from_slice(FIXTURE).unwrap();
    assert_eq!(fixture["repetitions"], 2);
    assert_eq!(fixture["typescript"], "6.0.3");
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(
        fixture["complete_command_executions"],
        json!(cases.len() * 2)
    );
    let mut failures = Vec::new();
    for case in cases {
        let id = case["case_id"].as_str().unwrap();
        let expected = &case["complete_command_runs"][0];
        assert_eq!(
            *expected, case["complete_command_runs"][1],
            "{id}: upstream repetition drift"
        );
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
            let actual = match result {
                Ok(command) => {
                    let emit = command.emit();
                    let maps = emit.source_maps().map(|maps| maps.iter().map(|map| json!({
                        "input_source_file_names": map.input_source_files().iter().map(|value| scalar_path(value.as_js())).collect::<Vec<_>>(), "source_map_json": map.canonical_json()
                    })).collect::<Vec<_>>());
                    Ok(
                        json!({"writes": writes, "reported_diagnostics": diagnostics(command.diagnostics()),
                        "status_writes": command.status_writes().iter().map(string_value).collect::<Vec<_>>(),
                        "exit_code": command.exit_code(), "emit_result": {"emit_skipped": emit.emit_skipped(),
                            "diagnostics": diagnostics(emit.diagnostics()), "emitted_files": emit.emitted_files().map(|values| values.iter().map(|value| scalar_path(value.as_js())).collect::<Vec<_>>()), "source_maps": maps}}),
                    )
                }
                Err(error) => Err(format!("{error} (partial writes: {})", writes.len())),
            };
            actuals.push((attempt, actual));
        }
        assert_eq!(actuals[0].1, actuals[1].1, "{id}: native repetition drift");
        let expected_tuple = json!({"writes": expected["writes"], "reported_diagnostics": expected["reported_diagnostics"],
            "status_writes": expected["status_writes"], "exit_code": expected["exit_code"], "emit_result": expected["emit_result"]});
        match &actuals[0].1 {
            Ok(actual) if *actual == expected_tuple => {}
            Ok(actual) => failures.push(format!(
                "{id}: complete tuple differs\n  expected: {}\n  actual:   {}",
                serde_json::to_string(&expected_tuple).unwrap(),
                serde_json::to_string(actual).unwrap()
            )),
            Err(error) => failures.push(format!("{id}: production command failed: {error}")),
        }
    }
    assert!(
        failures.is_empty(),
        "newly admitted corpus row divergences ({}):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

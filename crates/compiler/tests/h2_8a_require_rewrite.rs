//! Source require rewriting through the complete Program command.
use base64::Engine as _;
use serde_json::{json, Value};
use tsc_diagnostics::{Diagnostic, MessageChain};
use tsc_emitter::{EmitArtifact, EmitArtifactKind, EmitWriteMetadata};
#[allow(dead_code)]
#[path = "integration/h2_7b_w4a_controls.rs"]
mod h2_7b_w4a_controls;
#[allow(dead_code)]
#[path = "integration/h2_7c_declaration_blocking.rs"]
mod h2_7c_declaration_blocking;
#[allow(dead_code)]
#[path = "integration/h2_7d_original_corpus_shared.rs"]
mod h2_7d_original_corpus_shared;

#[test]
fn require_rewrite_focused_complete_commands() {
    let mut artifact: Value =
        serde_json::from_slice(include_bytes!("fixtures/h2-8a-require-rewrite.json")).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    assert_eq!(artifact["cases"].as_array().unwrap().len(), 60);
    assert_eq!(artifact["upstream_failures"], json!([]));
    if let Ok(filter) = std::env::var("TSC_RS_REQUIRE_REWRITE_FILTER") {
        artifact["cases"]
            .as_array_mut()
            .unwrap()
            .retain(|case| case["case_id"].as_str().unwrap().contains(&filter));
        assert!(!artifact["cases"].as_array().unwrap().is_empty());
    }
    h2_7c_declaration_blocking::assert_cases_with_inspection(
        &artifact,
        true,
        capture_complete_command,
    );
}

#[test]
fn require_rewrite_composition_complete_commands() {
    let artifact: Value = serde_json::from_slice(include_bytes!(
        "fixtures/h2-8a-require-rewrite-composition.json"
    ))
    .unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    assert_eq!(artifact["cases"].as_array().unwrap().len(), 4);
    assert_eq!(artifact["upstream_failures"], json!([]));
    h2_7c_declaration_blocking::assert_cases_with_inspection(
        &artifact,
        true,
        capture_complete_command,
    );
}

#[test]
fn require_rewrite_original_complete_commands() {
    let selected = [
        "typescript-6.0.3/conformance/externalModules/rewriteRelativeImportExtensions/emitModuleCommonJS.ts#module%3Dcommonjs",
        "typescript-6.0.3/conformance/externalModules/rewriteRelativeImportExtensions/emitModuleCommonJS.ts#module%3Dnodenext",
    ];
    let exact = h2_7d_original_corpus_shared::assert_output_matrix_projection(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        &selected,
    );
    assert_eq!(exact.len(), selected.len());
}

// Supplemental executions retain the whole command, including fields after
// the comparator's first failure. They are counted separately in evidence.
fn capture_complete_command(
    case_id: &str,
    prepared: &tsc_program::PreparedProgram,
    expected: &Value,
) {
    use sha2::Digest;
    let Some(directory) = std::env::var_os("TSC_RS_H2_8A_CAPTURE_WRITES_DIR") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    assert!(directory.is_absolute());
    std::fs::create_dir_all(&directory).unwrap();
    let key = format!("{:x}", sha2::Sha256::digest(case_id.as_bytes()));
    let index = (0..)
        .find(|index| !directory.join(format!("{key}-{index}.json")).exists())
        .unwrap();
    let mut sink = tsc_compiler::MemoryOutputSink::new();
    let command =
        tsc_compiler::ProgramSession::new(prepared.clone()).emit_command_for_harness(&mut sink);
    let writes = sink
        .writes()
        .iter()
        .enumerate()
        .map(|(index, artifact)| {
            let mut value = captured_write(index, artifact);
            let object = value.as_object_mut().unwrap();
            object.remove("data_keys");
            object.remove("data_build_info");
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
            object.insert("kind".into(), json!(kind));
            value
        })
        .collect::<Vec<_>>();
    let (actual, error) = match command {
        Ok(command) => {
            let outcome = command.emit();
            let maps = outcome.source_maps().map(|maps| maps.iter().map(|map| json!({
                "input_source_file_names": map.input_source_files(), "source_map_json": map.canonical_json()
            })).collect::<Vec<_>>());
            (
                Some(
                    json!({"writes": writes, "reported_diagnostics": diagnostics(command.diagnostics()),
                "emit_refused": outcome.emit_skipped(), "emit_result": {
                    "emit_skipped": outcome.emit_skipped(), "diagnostics": diagnostics(outcome.diagnostics()),
                    "emitted_files": outcome.emitted_files(), "source_maps": maps},
                "status_writes": command.status_writes(), "exit_code": command.exit_code()}),
                ),
                None,
            )
        }
        Err(error) => (None, Some(error.to_string())),
    };
    let value = json!({"case_id": case_id, "capture_index": index,
        "capture_kind": "supplemental-complete-command", "actual": actual,
        "error": error, "partial_writes": if actual.is_none() { Some(writes) } else { None },
        "expected": expected});
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
    text.push_str(&chain.text);
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
                "file":r.file_name,"start":r.start,"length":r.length,"message":text,"related_information":null})
        }).collect::<Vec<_>>());
        json!({"code":d.code(),"category":format!("{:?}",d.category()),"file":d.file_name,
            "start":d.start,"length":d.length,"message":text,"related_information":related})
    }).collect::<Vec<_>>())
}

fn captured_write(index: usize, artifact: &EmitArtifact) -> Value {
    let path = artifact.path().to_string_lossy();
    let kind = match artifact.kind() {
        EmitArtifactKind::JavaScript => "javascript",
        EmitArtifactKind::Declaration => "declaration",
        EmitArtifactKind::JavaScriptMap | EmitArtifactKind::DeclarationMap => "source-map",
        EmitArtifactKind::BuildInfo => panic!("unexpected build-info output"),
    };
    let (keys, position, data_diagnostics) = match artifact.metadata() {
        Some(EmitWriteMetadata::Text(data)) => (
            json!(["sourceMapUrlPos", "diagnostics"]),
            json!(data.source_map_url_position().map(|p| p.value())),
            diagnostics(data.diagnostics()),
        ),
        None => (Value::Null, Value::Null, Value::Null),
        _ => panic!("unexpected original callback metadata"),
    };
    json!({"index":index,"path":path,"kind":kind,
        "callback_utf8_base64":base64::engine::general_purpose::STANDARD.encode(artifact.callback_bytes()),
        "callback_utf8_bytes":artifact.callback_bytes().len(),"write_byte_order_mark":artifact.write_byte_order_mark(),
        "materialized_utf8_base64":base64::engine::general_purpose::STANDARD.encode(artifact.materialized_bytes()),
        "materialized_utf8_bytes":artifact.materialized_bytes().len(),
        // OutputSink::write's Result is the typed equivalent of onError.
        "on_error_callback_present":true,"source_files":artifact.source_files(),
        "data_present":artifact.metadata().is_some(),"data_keys":keys,
        "data_source_map_url_pos":position,"data_diagnostics":data_diagnostics,"data_build_info":null})
}

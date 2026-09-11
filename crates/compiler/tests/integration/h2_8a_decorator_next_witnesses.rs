//! Complete TypeScript commands for the A6-41 decorator-next witness groups
//! (visit order, super paths, generated-name owners, and the source
//! follow-up: literal computed names, object-literal computed-name pending
//! absorption, anonymous classes in decorated computed fields, and
//! undecorated-outer computed-property hoists, including the source-file
//! owner of that hoist).
use base64::Engine as _;
use serde_json::{json, Value};
use tsc_diagnostics::{Diagnostic, MessageChain};
use tsc_emitter::{EmitArtifact, EmitArtifactKind, EmitWriteMetadata};

const GROUPS: &[(&str, &[u8], usize)] = &[
    (
        "transform-order",
        include_bytes!("../fixtures/decorator-transform-order.json"),
        60,
    ),
    (
        "super-paths",
        include_bytes!("../fixtures/decorator-super-paths.json"),
        42,
    ),
    (
        "name-owners",
        include_bytes!("../fixtures/decorator-name-owners.json"),
        24,
    ),
    (
        "source-followup",
        include_bytes!("../fixtures/decorator-source-followup.json"),
        48,
    ),
    (
        "source-followup-top-level",
        include_bytes!("../fixtures/decorator-source-followup-top-level.json"),
        18,
    ),
];

#[test]
fn decorator_next_witnesses_match_complete_typescript_observations() {
    let selection = std::env::var("TSC_RS_DECORATOR_NEXT_CASE_SET");
    let selection = match selection.as_deref() {
        Err(std::env::VarError::NotPresent) | Ok("all") => None,
        Ok(group) if GROUPS.iter().any(|(name, _, _)| *name == group) => Some(group.to_owned()),
        unexpected => panic!("invalid decorator-next case selection: {unexpected:?}"),
    };
    let filter = std::env::var("TSC_RS_DECORATOR_NEXT_CASE_FILTER").ok();
    let mut cases = Vec::new();
    for (group, bytes, count) in GROUPS {
        let artifact: Value = serde_json::from_slice(bytes).unwrap();
        assert_eq!(artifact["typescript"], "6.0.3");
        assert_eq!(artifact["repetitions"], 2);
        assert!(artifact["upstream_failures"].as_array().unwrap().is_empty());
        let group_cases = artifact["cases"].as_array().unwrap();
        assert_eq!(group_cases.len(), *count, "{group}");
        if selection
            .as_deref()
            .is_none_or(|selected| selected == *group)
        {
            cases.extend(group_cases.iter().cloned());
        }
    }
    if let Some(filter) = filter.as_deref() {
        cases.retain(|case| case["case_id"].as_str().unwrap().contains(filter));
        assert!(!cases.is_empty(), "the case filter selected nothing");
    }
    let ids = cases
        .iter()
        .map(|case| case["case_id"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), cases.len());
    let mut failures = Vec::new();
    for case in &cases {
        let case_id = case["case_id"].as_str().unwrap();
        let run = || {
            super::h2_7c_declaration_blocking::assert_cases_with_inspection(
                &json!({"cases": [case]}),
                true,
                inspect_complete_command,
            )
        };
        let first = std::panic::catch_unwind(run);
        if first.is_ok() {
            eprintln!("decorator next witnesses EXACT x2 {case_id}");
        } else {
            // Re-enter only failed cases so deterministic failures also
            // produce two complete captures (the retained-accessor pattern).
            let second = std::panic::catch_unwind(run);
            assert!(
                second.is_err(),
                "{case_id}: comparison failed and then passed"
            );
            failures.push(case_id);
            eprintln!("decorator next witnesses REPEATED FAILURE {case_id}");
        }
    }
    assert!(
        failures.is_empty(),
        "complete decorator next witness failures: {failures:?}"
    );
}

fn inspect_complete_command(
    case_id: &str,
    prepared: &tsc_program::PreparedProgram,
    expected: &Value,
) {
    capture_complete_command(case_id, prepared, expected);
    eprintln!("decorator next witnesses PRIMARY ATTEMPT {case_id}");
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

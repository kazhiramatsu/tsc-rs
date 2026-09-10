//! Complete TypeScript commands for retained accessor producers.
use base64::Engine as _;
use serde_json::{json, Value};
use tsc_diagnostics::{Diagnostic, MessageChain};
use tsc_emitter::{EmitArtifact, EmitArtifactKind, EmitWriteMetadata};

#[test]
fn retained_accessor_owners_match_complete_typescript_observations() {
    let artifact: Value =
        serde_json::from_slice(include_bytes!("../fixtures/retained-accessor-owners.json"))
            .unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    let mut cases = artifact["cases"].as_array().unwrap().clone();
    assert_eq!(cases.len(), 146);
    let inputs: Value =
        serde_json::from_slice(include_bytes!("../fixtures/retained-accessor-inputs.json"))
            .unwrap();
    let input_ids = inputs["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|case| case["case_id"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(input_ids.len(), 148);
    // Two upstream commands throw before returning an EmitResult. Preserve
    // their exception observations, but grant no complete-command equality
    // credit and do not manufacture an expected native command tuple.
    let upstream_failures = artifact["upstream_failures"].as_array().unwrap();
    assert_eq!(upstream_failures.len(), 2);
    let observed_ids = cases
        .iter()
        .chain(upstream_failures)
        .map(|case| case["case_id"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(observed_ids, input_ids);
    for case in upstream_failures {
        assert_eq!(case["typescript_failure"]["outcome"], "exception");
        eprintln!(
            "retained accessor owners UPSTREAM EXCEPTION {}",
            case["case_id"].as_str().unwrap()
        );
    }
    let helpers: Value = serde_json::from_slice(include_bytes!(
        "../fixtures/class-helper-accessor-producers.json"
    ))
    .unwrap();
    let helpers = helpers["cases"].as_array().unwrap();
    assert_eq!(helpers.len(), 144);
    cases.extend(helpers.iter().cloned());
    let aliases: Value = serde_json::from_slice(include_bytes!(
        "../fixtures/class-field-alias-map-positions.json"
    ))
    .unwrap();
    let aliases = aliases["cases"].as_array().unwrap();
    assert_eq!(aliases.len(), 384);
    let retained = aliases
        .iter()
        .filter(|case| case["options"]["target"] == 9)
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(retained.len(), 128);
    cases.extend(retained);
    assert_eq!(cases.len(), 418);
    let contexts: Value = serde_json::from_slice(include_bytes!(
        "../fixtures/decorator-receiver-context.json"
    ))
    .unwrap();
    assert_eq!(contexts["typescript"], "6.0.3");
    assert_eq!(contexts["repetitions"], 2);
    assert!(contexts["upstream_failures"].as_array().unwrap().is_empty());
    let contexts = contexts["cases"].as_array().unwrap();
    assert_eq!(contexts.len(), 36);
    cases.extend(contexts.iter().cloned());
    assert_eq!(cases.len(), 454);
    let ids = cases
        .iter()
        .map(|case| case["case_id"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), cases.len());
    match std::env::var("TSC_RS_RETAINED_ACCESSOR_CASE_SET").as_deref() {
        Err(std::env::VarError::NotPresent) | Ok("all") => {}
        Ok("additional") => {
            let selected = inputs["groups"]["retained_accessor_followup"]
                .as_array()
                .unwrap()
                .iter()
                .chain(inputs["groups"]["decorator_handoff"].as_array().unwrap())
                .map(|id| id.as_str().unwrap())
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(selected.len(), 48);
            cases.retain(|case| selected.contains(case["case_id"].as_str().unwrap()));
            assert_eq!(cases.len(), 46);
        }
        Ok("context") => {
            cases.retain(|case| {
                case["case_id"]
                    .as_str()
                    .unwrap()
                    .starts_with("decorator-receiver-context/")
            });
            assert_eq!(cases.len(), 36);
        }
        unexpected => panic!("invalid retained accessor case selection: {unexpected:?}"),
    }
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
            eprintln!("retained accessor owners EXACT x2 {case_id}");
        } else {
            // The shared comparator stops at its first failure. Re-enter it
            // only for failed cases so deterministic failures also produce two
            // complete captures. A later pass or extra capture is a mismatch
            // in repeat behavior, never an exact result.
            let second = std::panic::catch_unwind(run);
            assert!(
                second.is_err(),
                "{case_id}: comparison failed and then passed"
            );
            failures.push(case_id);
            eprintln!("retained accessor owners REPEATED FAILURE {case_id}");
        }
    }
    assert!(
        failures.is_empty(),
        "complete retained accessor owners failures: {failures:?}"
    );
}

fn inspect_complete_command(
    case_id: &str,
    prepared: &tsc_program::PreparedProgram,
    expected: &Value,
) {
    capture_complete_command(case_id, prepared, expected);
    eprintln!("retained accessor owners PRIMARY ATTEMPT {case_id}");
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

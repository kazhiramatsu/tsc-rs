//! Forced declaration-only APIs: full output/diagnostics, repeated calls and resolver scheduling.
use super::h2_7c_declaration_getters::diagnostic_json;
use base64::Engine;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tsc_compiler::{
    DeclarationSession, DriverError, EmitSelection, MemoryOutputSink, ProgramSession,
};
use tsc_diagnostics::Diagnostic;
use tsc_emitter::{EmitArtifact, EmitArtifactKind, EmitWriteMetadata};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_emitting_program, load_program, CompilerOptions, LibraryCatalog, ProgramLoadLimits,
    ProgramOptions,
};

fn write_json(write: &EmitArtifact, index: usize) -> Value {
    let (diagnostics, source_map_url_pos) = match write.metadata() {
        Some(EmitWriteMetadata::Text(metadata)) => (
            json!(metadata
                .diagnostics()
                .iter()
                .map(diagnostic_json)
                .collect::<Vec<_>>()),
            metadata
                .source_map_url_position()
                .map(|position| position.value()),
        ),
        None => (Value::Null, None),
        _ => panic!("forced declaration requires text metadata"),
    };
    assert_eq!(write.kind(), EmitArtifactKind::Declaration);
    json!({"index":index,"path":write.path(),"kind":"declaration",
        "callback_utf8_base64":base64::engine::general_purpose::STANDARD.encode(write.callback_bytes()),
        "callback_utf8_bytes":write.callback_bytes().len(),
        "write_byte_order_mark":write.write_byte_order_mark(),
        "materialized_utf8_base64":base64::engine::general_purpose::STANDARD.encode(write.materialized_bytes()),
        "materialized_utf8_bytes":write.materialized_bytes().len(),
        "on_error_callback_present":true,"source_files":write.source_files(),
        "data_present":write.metadata().is_some(),"data_source_map_url_pos":source_map_url_pos,
        "data_diagnostics":diagnostics})
}

#[test]
fn forced_declarations_match_complete_typescript_observations() {
    assert_cases(SourceFamily::TypeScriptAndJavaScript);
}

#[test]
fn forced_json_declarations_match_complete_typescript_observations() {
    assert_cases(SourceFamily::Json);
}

#[test]
fn forced_empty_program_matches_complete_typescript_observations() {
    assert_cases(SourceFamily::Empty);
}

#[test]
fn forced_preserved_reference_paths_match_typescript_without_map_api_dependencies() {
    let fixture: Value = serde_json::from_slice(include_bytes!(
        "../fixtures/declaration-reference-paths.json"
    ))
    .unwrap();
    let cases = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["options"]["declarationMap"] == false)
        .chain(
            fixture["supplemental_reference_targets"]
                .as_array()
                .unwrap(),
        )
        .collect::<Vec<_>>();
    assert_eq!(cases.len(), 7);
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut count = 0;
    for case in cases {
        let id = case["case_id"].as_str().unwrap();
        let mut builder = MemoryCompilerHost::builder(case["current_directory"].as_str().unwrap());
        for file in case["files"].as_array().unwrap() {
            builder = builder.file(
                file["path"].as_str().unwrap(),
                file["text"].as_str().unwrap().as_bytes(),
            );
        }
        for entry in std::fs::read_dir(workspace.join("vendor/typescript-6.0.3/lib")).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("lib.") && name.ends_with(".d.ts") {
                builder =
                    builder.file(format!("/lib/{name}"), std::fs::read(entry.path()).unwrap());
            }
        }
        let host = builder.build().unwrap();
        let roots = case["roots"]
            .as_array()
            .unwrap()
            .iter()
            .map(|root| PathBuf::from(root.as_str().unwrap()))
            .collect::<Vec<_>>();
        let mut options = CompilerOptions::default();
        for (key, value) in case["options"].as_object().unwrap() {
            match key.as_str() {
                "target" => options.target = Some(value.as_i64().unwrap() as i32),
                "module" => options.module = Some(value.as_i64().unwrap() as i32),
                "newLine" => options.new_line = Some(value.as_i64().unwrap() as i32),
                "declaration" => options.declaration = value.as_bool(),
                "declarationMap" => {
                    assert_eq!(value, false);
                    options.declaration_map = value.as_bool();
                }
                "listEmittedFiles" => options.list_emitted_files = value.as_bool(),
                "noResolve" => options.no_resolve = value.as_bool(),
                "strict" => options.strict = value.as_bool(),
                "skipDefaultLibCheck" => options.skip_default_lib_check = value.as_bool(),
                "noErrorTruncation" => options.no_error_truncation = value.as_bool(),
                "allowJs" => options.allow_js = value.as_bool().unwrap(),
                "noEmitForJsFiles" => options.no_emit_for_js_files = value.as_bool(),
                "resolveJsonModule" => options.resolve_json_module = value.as_bool(),
                other => panic!("unprojected reference option {other}"),
            }
        }
        for _ in 0..2 {
            let prepared = load_emitting_program(
                &host,
                &roots,
                options.clone(),
                ProgramOptions::default(),
                &LibraryCatalog::typescript_6_0_3("/lib"),
                ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024),
            )
            .unwrap();
            let mut source_order = Vec::new();
            let mut libraries = Vec::new();
            let mut sources = BTreeMap::new();
            for source in prepared.source_files() {
                let path = source.path().display().to_string_lossy().into_owned();
                if let Some(library) = path.strip_prefix("/lib/") {
                    libraries.push(library.to_owned());
                } else {
                    source_order.push(path.clone());
                }
                sources.insert(path, prepared.source_id(source.path().canonical()).unwrap());
            }
            assert_eq!(
                json!(source_order),
                case["typescript_observation"]["program_source_order"],
                "{id}"
            );
            assert_eq!(
                json!(libraries),
                case["typescript_observation"]["standard_libraries"],
                "{id}"
            );
            ProgramSession::new(prepared).with_declarations(|session| {
                // This prefix uses only the already public H2.7c API. Stop
                // before ordinary emit; the E comparator checks that suffix.
                for expected in case["typescript_observation"]["calls"].as_array().unwrap().iter().take(4) {
                    let selection = expected["target_source"].as_str().map_or(EmitSelection::WholeProgram,
                        |path| EmitSelection::TargetSourceFile(sources[path]));
                    let before = session.activity().emit_resolver_borrows();
                    let mut writes = Vec::new();
                    let mut diagnostics = Value::Null;
                    let mut emit = Value::Null;
                    if expected["kind"] == "declaration-diagnostics" {
                        diagnostics = json!(session.get_declaration_diagnostics(selection)?.iter().map(diagnostic_json).collect::<Vec<_>>());
                    } else {
                        assert_eq!(expected["kind"], "forced-declarations");
                        let mut sink = MemoryOutputSink::new();
                        let result = session.emit_forced_declarations(selection, &mut sink)?;
                        assert!(result.source_maps().is_none());
                        emit = json!({"emit_skipped":result.emit_skipped(),"diagnostics":result.diagnostics().iter().map(diagnostic_json).collect::<Vec<_>>(),
                            "emitted_files":result.emitted_files(),"source_maps":null});
                        for (index, write) in sink.writes().iter().enumerate() {
                            let mut record = write_json(write, index);
                            let record_object = record.as_object_mut().unwrap();
                            let metadata = json!({"present":record_object.remove("data_present").unwrap(),
                                "keys":["sourceMapUrlPos","diagnostics"],"source_map_url_pos":record_object.remove("data_source_map_url_pos").unwrap(),
                                "diagnostics":record_object.remove("data_diagnostics").unwrap(),"skipped_dts_write":null});
                            record_object.insert("data_before".to_owned(), metadata.clone());
                            record_object.insert("data_after".to_owned(), metadata);
                            record_object.insert("sink_action".to_owned(), json!("write"));
                            record_object.insert("sink_materialized".to_owned(), json!(true));
                            record_object.insert("on_error_messages".to_owned(), json!([]));
                            writes.push(record);
                        }
                    }
                    assert_eq!(session.activity().emit_resolver_borrows() - before,
                        expected["resolver_requests"].as_array().unwrap().len() as u64, "{id}: resolver cache requests");
                    let actual = json!({"kind":expected["kind"],"owner":expected["owner"],"target_source":expected["target_source"],
                        "materialized_write_indices":(0..writes.len()).collect::<Vec<_>>(),"writes":writes,"system_write_attempts":[],
                        "diagnostics":diagnostics,"reported_diagnostics":null,"status_writes":null,"exit_code":null,"emit_result":emit,"exception":null});
                    let mut expected = expected.clone();
                    expected.as_object_mut().unwrap().remove("resolver_requests");
                    assert_eq!(actual, expected, "{id}: complete C API prefix call");
                    count += 1;
                }
                Ok(())
            }).unwrap();
        }
    }
    assert_eq!(count, 56);
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum SourceFamily {
    TypeScriptAndJavaScript,
    Json,
    Empty,
}

pub(super) fn assert_cases(source_family: SourceFamily) {
    let artifact: Value =
        serde_json::from_slice(include_bytes!("../fixtures/forced-declarations.json")).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    assert_eq!(artifact["cases"].as_array().unwrap().len(), 37);
    assert_eq!(
        artifact["adjacent_ordinary_api_observations"]
            .as_array()
            .unwrap()
            .len(),
        37
    );
    assert_eq!(artifact["adjacent_ordinary_api_owner"], "H2.8d");
    assert!(artifact["adjacent_ordinary_api_observations"]
        .as_array()
        .unwrap()
        .iter()
        .all(|case| case["force"] == false));
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut failures = Vec::new();
    let cases = artifact["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| {
            let files = case["files"].as_array().unwrap();
            let family = if files.is_empty() {
                SourceFamily::Empty
            } else if files
                .iter()
                .any(|file| file["path"].as_str().unwrap().ends_with(".json"))
            {
                SourceFamily::Json
            } else {
                SourceFamily::TypeScriptAndJavaScript
            };
            family == source_family
        })
        .collect::<Vec<_>>();
    assert_eq!(
        cases.len(),
        match source_family {
            SourceFamily::TypeScriptAndJavaScript => 27,
            SourceFamily::Json => 9,
            SourceFamily::Empty => 1,
        }
    );
    for case in cases {
        assert_eq!(case["force"], true);
        let case_id = case["case_id"].as_str().unwrap();
        let result = std::panic::catch_unwind(|| {
            let mut builder = MemoryCompilerHost::builder("/project");
            let mut roots = Vec::new();
            for file in case["files"].as_array().unwrap() {
                let path = PathBuf::from(file["path"].as_str().unwrap());
                builder = builder.file(path.clone(), file["text"].as_str().unwrap().as_bytes());
                roots.push(path);
            }
            if let Some(selected_roots) = case["roots"].as_array() {
                roots = selected_roots
                    .iter()
                    .map(|root| PathBuf::from(root.as_str().unwrap()))
                    .collect();
            }
            for entry in std::fs::read_dir(workspace.join("vendor/typescript-6.0.3/lib")).unwrap() {
                let entry = entry.unwrap();
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with("lib.") && name.ends_with(".d.ts") {
                    builder =
                        builder.file(format!("/lib/{name}"), std::fs::read(entry.path()).unwrap());
                }
            }
            let host = builder.build().unwrap();
            let mut options = CompilerOptions::default();
            for (key, value) in case["options"].as_object().unwrap() {
                match key.as_str() {
                    "target" => options.target = Some(value.as_i64().unwrap() as i32),
                    "module" => options.module = Some(value.as_i64().unwrap() as i32),
                    "newLine" => options.new_line = Some(value.as_i64().unwrap() as i32),
                    "esModuleInterop" => options.es_module_interop = value.as_bool(),
                    "declaration" => options.declaration = value.as_bool(),
                    "declarationDir" => options.declaration_dir = value.as_str().map(str::to_owned),
                    "listEmittedFiles" => options.list_emitted_files = value.as_bool(),
                    "noEmitForJsFiles" => options.no_emit_for_js_files = value.as_bool(),
                    "isolatedDeclarations" => options.isolated_declarations = value.as_bool(),
                    "strict" => options.strict = value.as_bool(),
                    "noEmit" => options.no_emit = value.as_bool(),
                    "noEmitOnError" => options.no_emit_on_error = value.as_bool(),
                    "noCheck" => options.no_check = value.as_bool(),
                    "allowJs" => options.allow_js = value.as_bool().unwrap(),
                    "checkJs" => options.check_js = value.as_bool(),
                    "resolveJsonModule" => options.resolve_json_module = value.as_bool(),
                    "skipDefaultLibCheck" => options.skip_default_lib_check = value.as_bool(),
                    "noErrorTruncation" => options.no_error_truncation = value.as_bool(),
                    other => panic!("unexpected forced declaration option {other}"),
                }
            }

            let catalog = LibraryCatalog::typescript_6_0_3("/lib");
            let limits = ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024);
            for _ in 0..2 {
                let load = if options.no_emit == Some(true) {
                    load_program
                } else {
                    load_emitting_program
                };
                let prepared = load(
                    &host,
                    &roots,
                    options.clone(),
                    ProgramOptions::default(),
                    &catalog,
                    limits,
                )
                .unwrap();
                let sources = prepared
                    .source_files()
                    .iter()
                    .map(|source| {
                        (
                            source.path().display().to_string_lossy().into_owned(),
                            prepared.source_id(source.path().canonical()).unwrap(),
                        )
                    })
                    .collect::<BTreeMap<_, _>>();
                let selection =
                    case["target_source"]
                        .as_str()
                        .map_or(EmitSelection::WholeProgram, |path| {
                            EmitSelection::TargetSourceFile(
                                *sources.get(path).expect("selected source loaded"),
                            )
                        });
                let operation = |declarations: &mut DeclarationSession<'_, '_>,
                                 semantics: &[Diagnostic]|
                 -> Result<Value, DriverError> {
                    let before = match case["before"].as_str() {
                        Some("declaration-diagnostics") => json!(declarations
                            .get_declaration_diagnostics(EmitSelection::WholeProgram)?
                            .iter()
                            .map(diagnostic_json)
                            .collect::<Vec<_>>()),
                        Some("semantic-diagnostics") => {
                            json!(semantics.iter().map(diagnostic_json).collect::<Vec<_>>())
                        }
                        None => Value::Null,
                        Some(other) => panic!("unexpected priming operation {other}"),
                    };
                    let mut declaration_requests =
                        u64::from(case["before"] == "declaration-diagnostics");
                    assert_eq!(
                        declarations
                            .activity()
                            .runtime_slice(tsc_emitter::H2RuntimeSlice::H2_7c),
                        declaration_requests
                    );
                    let mut requests = case["owner_observation"]["before_resolver_requests"]
                        .as_array()
                        .unwrap()
                        .len() as u64;
                    assert_eq!(
                        declarations.activity().emit_resolver_borrows(),
                        requests,
                        "{case_id}: prior resolver requests"
                    );
                    let mut calls = Vec::new();
                    for trace in case["owner_observation"]["resolver_requests_by_emit"]
                        .as_array()
                        .unwrap()
                    {
                        let trace = trace.as_array().unwrap();
                        assert_eq!(trace.len(), 1);
                        assert_eq!(trace[0]["skip_diagnostics"], true);
                        // TypeScript requests a resolver even for no sources.
                        // Rust retains its existing source-less Program adapter,
                        // which owns no checker and fails closed on any query.
                        requests += u64::from(!sources.is_empty());
                        let mut sink = MemoryOutputSink::new();
                        let outcome =
                            declarations.emit_forced_declarations(selection, &mut sink)?;
                        declaration_requests += 1;
                        assert_eq!(
                            outcome
                                .h2_activity()
                                .runtime_slice(tsc_emitter::H2RuntimeSlice::H2_7c),
                            declaration_requests,
                            "{case_id}: every forced request, including empty programs"
                        );
                        assert_eq!(
                            declarations.activity().emit_resolver_borrows(),
                            requests,
                            "{case_id}: fresh borrow on every nonempty forced emit"
                        );
                        assert_eq!(
                            outcome
                                .h2_activity()
                                .script_transformer_list_constructions(),
                            0,
                            "{case_id}: declaration-only transforms"
                        );
                        assert_eq!(
                            outcome.h2_activity().javascript_artifact_creations(),
                            0,
                            "{case_id}: no JS artifact"
                        );
                        let maps = outcome.source_maps().map(|maps| {
                            assert!(maps.is_empty());
                            Vec::<Value>::new()
                        });
                        calls.push(json!({"writes":sink.writes().iter().enumerate().map(|(i,w)|write_json(w,i)).collect::<Vec<_>>(),
                            "emit_result":{"emit_skipped":outcome.emit_skipped(),"diagnostics":outcome.diagnostics().iter().map(diagnostic_json).collect::<Vec<_>>(),
                            "emitted_files":outcome.emitted_files(),"source_maps":maps}}));
                    }
                    Ok(json!({"before_diagnostics":before,"calls":calls}))
                };
                let observed = if case["before"] == "semantic-diagnostics" {
                    ProgramSession::new(prepared)
                        .with_declarations_after_semantic_for_harness(operation)
                } else {
                    ProgramSession::new(prepared)
                        .with_declarations(|declarations| operation(declarations, &[]))
                }
                .unwrap_or_else(|error| panic!("{case_id}: {error:?}"));
                assert_eq!(
                    observed, case["typescript_observation"],
                    "{case_id}: complete forced declaration observation"
                );
            }
        });
        if let Err(error) = result {
            failures.push(format!(
                "{case_id}: {}",
                error
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| error.downcast_ref::<&str>().copied())
                    .unwrap_or("panic")
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} forced declaration differences:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

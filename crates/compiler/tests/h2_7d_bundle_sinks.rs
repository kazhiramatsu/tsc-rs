//! Complete TS6 ordinary Bundle sink/listing observations.
//! Requires the separate production outFile packet. A controlled Rust panic
//! represents a direct callback throw; an OutputSink Err represents onError.
use base64::Engine;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tsc_compiler::{DriverError, ProgramSession};
use tsc_diagnostics::{Diagnostic, MessageChain};
use tsc_emitter::{
    EmitArtifact, EmitArtifactKind, EmitIoError, EmitIoOperation, EmitOutcome,
    EmitWriteDisposition, EmitWriteMetadata, OutputSink,
};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_emitting_program, CompilerOptions, LibraryCatalog, PreparedProgram, ProgramLoadLimits,
    ProgramOptions,
};

fn flatten(chain: &MessageChain, indent: usize, text: &mut String) {
    if indent != 0 {
        text.push('\n');
        text.push_str(&"  ".repeat(indent));
    }
    text.push_str(&chain.text);
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
                            "file": related.file_name,
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
                "file": diagnostic.file_name,
                "start": diagnostic.start,
                "length": diagnostic.length,
                "message": message,
                "related_information": related,
            })
        })
        .collect::<Vec<_>>())
}
fn options(case: &Value) -> CompilerOptions {
    let mut options = CompilerOptions::default();
    for (name, value) in case["options"].as_object().unwrap() {
        match name.as_str() {
            "target" => options.target = Some(value.as_i64().unwrap() as i32),
            "module" => options.module = Some(value.as_i64().unwrap() as i32),
            "newLine" => options.new_line = Some(value.as_i64().unwrap() as i32),
            "outFile" => options.out_file = value.as_str().map(str::to_owned),
            "sourceMap" => options.source_map = value.as_bool(),
            "declaration" => options.declaration = value.as_bool(),
            "declarationMap" => options.declaration_map = value.as_bool(),
            "listEmittedFiles" => options.list_emitted_files = value.as_bool(),
            "skipDefaultLibCheck" => options.skip_default_lib_check = value.as_bool(),
            other => panic!("unprojected Bundle sink option {other}"),
        }
    }
    options
}
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
        .map(|roots| {
            roots
                .iter()
                .map(|root| PathBuf::from(root.as_str().unwrap()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| {
            case["files"]
                .as_array()
                .unwrap()
                .iter()
                .map(|file| PathBuf::from(file["path"].as_str().unwrap()))
                .collect()
        });
    let options = options(case);
    let program = load_emitting_program(
        &host.build().unwrap(),
        &roots,
        options,
        ProgramOptions::default(),
        &LibraryCatalog::typescript_6_0_3("/lib"),
        ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024),
    )
    .unwrap();
    let mut sources = Vec::new();
    let mut loaded_libraries = Vec::new();
    for source in program.source_files() {
        let name = source.path().display().to_string_lossy();
        if let Some(name) = name.strip_prefix("/lib/") {
            loaded_libraries.push(name.to_owned());
        } else {
            sources.push(name.into_owned());
        }
    }
    assert_eq!(
        json!(sources),
        case["typescript_observation"]["program_source_order"],
        "{} Program source order",
        case["case_id"]
    );
    assert_eq!(
        json!(loaded_libraries),
        case["typescript_observation"]["standard_libraries"],
        "{} ordered libraries",
        case["case_id"]
    );
    program
}
fn data(artifact: &EmitArtifact) -> Value {
    let (present, keys, position, diagnostics) = match artifact.metadata() {
        Some(EmitWriteMetadata::Text(data)) => (
            true,
            json!(["sourceMapUrlPos", "diagnostics"]),
            json!(data.source_map_url_position().map(|p| p.value())),
            diagnostics(data.diagnostics()),
        ),
        None => (false, Value::Null, Value::Null, Value::Null),
        _ => panic!("build info is outside the Bundle sink fixture"),
    };
    json!({"present":present,"keys":keys,"source_map_url_pos":position,"diagnostics":diagnostics,"skipped_dts_write":null})
}
fn write_record(artifact: &EmitArtifact, index: usize) -> Value {
    let kind = match artifact.kind() {
        EmitArtifactKind::Declaration => "declaration",
        EmitArtifactKind::DeclarationMap | EmitArtifactKind::JavaScriptMap => "source-map",
        EmitArtifactKind::JavaScript => "javascript",
        _ => panic!("build info is outside the Bundle sink fixture"),
    };
    json!({"index":index,"path":artifact.path(),"kind":kind,
        "callback_utf8_base64":base64::engine::general_purpose::STANDARD.encode(artifact.callback_bytes()),
        "callback_utf8_bytes":artifact.callback_bytes().len(),"write_byte_order_mark":artifact.write_byte_order_mark(),
        "materialized_utf8_base64":base64::engine::general_purpose::STANDARD.encode(artifact.materialized_bytes()),
        "materialized_utf8_bytes":artifact.materialized_bytes().len(),"on_error_callback_present":true,
        "source_files":artifact.source_files(),"data_before":data(artifact),"data_after":data(artifact),
        "sink_action":"write","sink_materialized":false,"on_error_messages":[]})
}
struct Sink<'a> {
    rules: &'a Value,
    writes: Vec<Value>,
    materialized_files: Vec<Value>,
}
impl<'a> Sink<'a> {
    fn new(case: &'a Value) -> Self {
        Self {
            rules: &case["sink_rules"],
            writes: Vec::new(),
            materialized_files: Vec::new(),
        }
    }
}
impl OutputSink for Sink<'_> {
    fn write(&mut self, artifact: EmitArtifact) -> Result<EmitWriteDisposition, EmitIoError> {
        let action = self
            .rules
            .as_array()
            .unwrap()
            .iter()
            .find(|rule| rule["path"].as_str().unwrap() == artifact.path().to_string_lossy())
            .map_or("write", |rule| rule["action"].as_str().unwrap());
        let mut record = write_record(&artifact, self.writes.len());
        record["sink_action"] = json!(action);
        self.writes.push(record);
        let record = self.writes.last_mut().unwrap();
        match action {
            "on-error" => {
                let message = "H2.7 bundle controlled callback failure";
                record["on_error_messages"] = json!([message]);
                Err(EmitIoError::new(
                    EmitIoOperation::WriteFile,
                    artifact.path(),
                    message,
                ))
            }
            "throw" => std::panic::panic_any("H2.7 bundle controlled callback exception"),
            "skip-unchanged" => {
                if matches!(
                    artifact.kind(),
                    EmitArtifactKind::JavaScript | EmitArtifactKind::Declaration
                ) {
                    // Both TS text callbacks expose the mutation. The runtime
                    // must independently decide which kind it affects in listing.
                    record["data_after"]["keys"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!("skippedDtsWrite"));
                    record["data_after"]["skipped_dts_write"] = json!(true);
                } else {
                    assert!(!record["data_before"]["present"].as_bool().unwrap());
                }
                Ok(EmitWriteDisposition::SkippedUnchanged)
            }
            "write" => {
                // Save actual materialized artifact bytes only on this path;
                // callbacks, skipped outputs and failed writes remain separate.
                let bytes = artifact.materialized_bytes();
                self.materialized_files.push(json!({"path": artifact.path(),
                    "utf8_base64": base64::engine::general_purpose::STANDARD.encode(&bytes),
                    "utf8_bytes": bytes.len()}));
                record["sink_materialized"] = json!(true);
                Ok(EmitWriteDisposition::Written)
            }
            other => panic!("unobserved Bundle sink action {other}"),
        }
    }
}
fn emit_result(outcome: &EmitOutcome) -> Value {
    json!({"emit_skipped":outcome.emit_skipped(),"diagnostics":diagnostics(outcome.diagnostics()),
        "emitted_files":outcome.emitted_files(),"source_maps":outcome.source_maps().map(|maps|maps.iter().map(|map|json!({
            "input_source_file_names":map.input_source_files(),"source_map_json":map.canonical_json()
        })).collect::<Vec<_>>())})
}

#[test]
fn ordinary_bundle_sink_commands_match_complete_typescript_twice() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/bundle-sinks.json")).unwrap();
    assert_eq!(fixture["typescript"], "6.0.3");
    assert_eq!(fixture["repetitions"], 2);
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 10);
    let libs = libraries();
    let mut failures = Vec::new();
    let mut successful_returns = 0;
    let mut direct_exceptions = 0;
    let mut callbacks = 0;
    let mut materialized = 0;
    for case in cases {
        let id = case["case_id"].as_str().unwrap();
        for repetition in 0..2 {
            let checked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                assert_eq!(case["kind"], "ordinary-command");
                assert_eq!(case["target_source"], Value::Null);
                let mut sink = Sink::new(case);
                // Command report callbacks are installed before emit. A direct
                // sink throw aborts emit before any reporting/status callback.
                let mut fields = json!({"reported_diagnostics":[], "status_writes":[],
                    "exit_code":null, "emit_result":null, "exception":null});
                let program = prepare(case, &libs);
                let returned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                    || -> Result<(), DriverError> {
                        let outcome =
                            ProgramSession::new(program).emit_command_for_harness(&mut sink)?;
                        fields["emit_result"] = emit_result(outcome.emit());
                        fields["reported_diagnostics"] = diagnostics(outcome.diagnostics());
                        fields["status_writes"] = json!(outcome.status_writes());
                        fields["exit_code"] = json!(outcome.exit_code());
                        Ok(())
                    },
                ));
                match returned {
                    Ok(Ok(())) => {
                        successful_returns += 1;
                    }
                    Ok(Err(error)) => panic!(
                        "{id}: unexpected typed failure {error:?}; partial writes={}",
                        json!(sink.writes)
                    ),
                    Err(error) => {
                        assert_eq!(error.downcast_ref::<&str>(), Some(&"H2.7 bundle controlled callback exception"),
                            "{id}: only the controlled sink exception can correspond to the TS throw");
                        fields["exception"] = json!({"name":"Error", "message":"H2.7 bundle controlled callback exception"});
                        direct_exceptions += 1;
                    }
                }
                callbacks += sink.writes.len();
                materialized += sink.materialized_files.len();
                let indices = sink
                    .writes
                    .iter()
                    .filter(|write| write["sink_materialized"] == true)
                    .map(|write| write["index"].clone())
                    .collect::<Vec<_>>();
                let actual = json!({"kind":"ordinary-command", "target_source":null,
                    "writes":sink.writes, "reported_diagnostics":fields["reported_diagnostics"],
                    "status_writes":fields["status_writes"], "exit_code":fields["exit_code"],
                    "emit_result":fields["emit_result"], "exception":fields["exception"],
                    "materialized_write_indices":indices, "materialized_files":sink.materialized_files});
                assert_eq!(
                    actual, case["typescript_observation"]["call"],
                    "{id}: complete callback/result/reporting tuple, repetition {repetition}"
                );
            }));
            if let Err(error) = checked {
                let message = error
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| error.downcast_ref::<&str>().map(|text| text.to_string()))
                    .unwrap_or_else(|| "unknown panic".to_owned());
                failures.push(format!("{id} repetition {repetition}: {message}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} Bundle sink failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(successful_returns, 6 * 2);
    assert_eq!(direct_exceptions, 4 * 2);
    assert_eq!(callbacks, 34 * 2);
    assert_eq!(materialized, 22 * 2);
}

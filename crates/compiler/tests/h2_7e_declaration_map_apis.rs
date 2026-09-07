//! Same-Program declaration-map getters, forced and ordinary command emits.
//! The ordinary noEmit call remains an explicitly compared H2.9 refusal.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::{json, Value};
use tsc_compiler::{DeclarationSession, DriverError, ProgramSession};
use tsc_diagnostics::{Diagnostic, MessageChain};
use tsc_emitter::{
    EmitArtifact, EmitContractViolation, EmitFailure, EmitFileSystem, EmitIoError, EmitIoOperation,
    EmitOutcome, EmitSelection, EmitWriteDisposition, EmitWriteMetadata, FsOutputSink, OutputSink,
};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_emitting_program, load_program, CompilerOptions, LibraryCatalog, PreparedProgram,
    ProgramLoadLimits, ProgramOptions,
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
    for (key, value) in case["options"].as_object().unwrap() {
        match key.as_str() {
            "target" => options.target = Some(value.as_i64().unwrap() as i32),
            "module" => options.module = Some(value.as_i64().unwrap() as i32),
            "newLine" => options.new_line = Some(value.as_i64().unwrap() as i32),
            "declaration" => options.declaration = value.as_bool(),
            "declarationMap" => options.declaration_map = value.as_bool(),
            "emitDeclarationOnly" => options.emit_declaration_only = value.as_bool(),
            "listEmittedFiles" => options.list_emitted_files = value.as_bool(),
            "strict" => options.strict = value.as_bool(),
            "skipDefaultLibCheck" => options.skip_default_lib_check = value.as_bool(),
            "noErrorTruncation" => options.no_error_truncation = value.as_bool(),
            "noEmit" => options.no_emit = value.as_bool(),
            "noEmitOnError" => options.no_emit_on_error = value.as_bool(),
            "isolatedDeclarations" => options.isolated_declarations = value.as_bool(),
            "sourceMap" => options.source_map = value.as_bool(),
            "inlineSourceMap" => options.inline_source_map = value.as_bool(),
            "inlineSources" => options.inline_sources = value.as_bool(),
            "allowJs" => options.allow_js = value.as_bool().unwrap(),
            "checkJs" => options.check_js = value.as_bool(),
            "resolveJsonModule" => options.resolve_json_module = value.as_bool(),
            "sourceRoot" => options.source_root = value.as_str().map(str::to_owned),
            "declarationDir" => options.declaration_dir = value.as_str().map(str::to_owned),
            "emitBOM" => options.emit_bom = value.as_bool(),
            other => panic!("unprojected API option {other}"),
        }
    }
    options
}
fn prepared(case: &Value) -> PreparedProgram {
    let mut builder = MemoryCompilerHost::builder(case["current_directory"].as_str().unwrap());
    for file in case["files"].as_array().unwrap() {
        builder = builder.file(
            file["path"].as_str().unwrap(),
            file["text"].as_str().unwrap().as_bytes(),
        );
    }
    for entry in std::fs::read_dir(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/typescript-6.0.3/lib"),
    )
    .unwrap()
    {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("lib.") && name.ends_with(".d.ts") {
            builder = builder.file(format!("/lib/{name}"), std::fs::read(entry.path()).unwrap());
        }
    }
    let roots = case["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| PathBuf::from(file["path"].as_str().unwrap()))
        .collect::<Vec<_>>();
    let options = options(case);
    let load = if options.no_emit == Some(true) {
        load_program
    } else {
        load_emitting_program
    };
    load(
        &builder.build().unwrap(),
        &roots,
        options,
        ProgramOptions::default(),
        &LibraryCatalog::typescript_6_0_3("/lib"),
        ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024),
    )
    .unwrap()
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
        _ => panic!("build info remains outside declaration API"),
    };
    json!({"present":present,"keys":keys,"source_map_url_pos":position,"diagnostics":diagnostics,"skipped_dts_write":null})
}
fn write_record(artifact: &EmitArtifact, index: usize) -> Value {
    let kind = match artifact.kind() {
        tsc_emitter::EmitArtifactKind::Declaration => "declaration",
        tsc_emitter::EmitArtifactKind::DeclarationMap
        | tsc_emitter::EmitArtifactKind::JavaScriptMap => "source-map",
        tsc_emitter::EmitArtifactKind::JavaScript => "javascript",
        _ => panic!("build info remains outside declaration API"),
    };
    json!({"index":index,"path":artifact.path(),"kind":kind,
        "callback_utf8_base64":base64::engine::general_purpose::STANDARD.encode(artifact.callback_bytes()),
        "callback_utf8_bytes":artifact.callback_bytes().len(),"write_byte_order_mark":artifact.write_byte_order_mark(),
        "materialized_utf8_base64":base64::engine::general_purpose::STANDARD.encode(artifact.materialized_bytes()),
        "materialized_utf8_bytes":artifact.materialized_bytes().len(),"on_error_callback_present":true,
        "source_files":artifact.source_files(),"data_before":data(artifact),"data_after":data(artifact),
        "sink_action":"write","sink_materialized":false,"on_error_messages":[]})
}
struct ControlledSystem<'a> {
    rules: &'a Value,
    attempts: Vec<Value>,
}
impl EmitFileSystem for ControlledSystem<'_> {
    fn write_file(&mut self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        self.attempts.push(json!({"path":path,"callback_utf8_base64":base64::engine::general_purpose::STANDARD.encode(bytes),"write_byte_order_mark":false}));
        if self
            .rules
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["path"].as_str().unwrap() == path.to_string_lossy())
        {
            Err("H2.7e controlled system failure".to_owned())
        } else {
            Ok(())
        }
    }
    fn create_directory(&mut self, _: &Path) -> Result<(), String> {
        Ok(())
    }
    fn directory_exists(&mut self, _: &Path) -> bool {
        true
    }
}
struct Sink<'a> {
    rules: &'a Value,
    system: Option<ControlledSystem<'a>>,
    writes: Vec<Value>,
}
impl<'a> Sink<'a> {
    fn new(case: &'a Value) -> Self {
        Self {
            rules: &case["sink_rules"],
            system: (case["transport"] == "compiler-host-system").then_some(ControlledSystem {
                rules: &case["sink_rules"],
                attempts: Vec::new(),
            }),
            writes: Vec::new(),
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
        let result = if let Some(system) = &mut self.system {
            assert!(!artifact.write_byte_order_mark());
            FsOutputSink::new(system).write(artifact)
        } else {
            match action {
                "on-error" => Err(EmitIoError::new(
                    EmitIoOperation::WriteFile,
                    artifact.path(),
                    "H2.7e controlled callback failure",
                )),
                "throw" => std::panic::panic_any("H2.7e controlled callback exception"),
                "skip-unchanged" => {
                    if artifact.kind() == tsc_emitter::EmitArtifactKind::Declaration {
                        record["data_after"]["keys"]
                            .as_array_mut()
                            .unwrap()
                            .push(json!("skippedDtsWrite"));
                        record["data_after"]["skipped_dts_write"] = json!(true);
                    }
                    Ok(EmitWriteDisposition::SkippedUnchanged)
                }
                "write" => Ok(EmitWriteDisposition::Written),
                other => panic!("unhandled sink action {other}"),
            }
        };
        match &result {
            Ok(EmitWriteDisposition::Written) => record["sink_materialized"] = json!(true),
            Err(error) => record["on_error_messages"] = json!([error.message()]),
            _ => {}
        }
        result
    }
}
fn emit_result(outcome: &EmitOutcome) -> Value {
    json!({"emit_skipped":outcome.emit_skipped(),"diagnostics":diagnostics(outcome.diagnostics()),
        "emitted_files":outcome.emitted_files(),"source_maps":outcome.source_maps().map(|maps|maps.iter().map(|map|json!({
            "input_source_file_names":map.input_source_files(),"source_map_json":map.canonical_json()
        })).collect::<Vec<_>>())})
}

#[test]
fn h2_7e_stateful_program_calls_and_diagnostics_match_typescript() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/declaration-map-apis.json")).unwrap();
    assert_eq!(fixture["repetitions"], 2);
    assert_eq!(fixture["cases"].as_array().unwrap().len(), 54);
    let mut counts = BTreeMap::<String, usize>::new();
    let mut failures = Vec::new();
    for case in fixture["cases"].as_array().unwrap() {
        let id = case["case_id"].as_str().unwrap();
        let checked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            for _ in 0..2 {
                let program = prepared(case);
                let mut source_order = Vec::new();
                let mut libraries = Vec::new();
                for source in program.source_files() {
                    let name = source.path().display().to_string_lossy();
                    if let Some(name) = name.strip_prefix("/lib/") {
                        libraries.push(name.to_owned());
                    } else {
                        source_order.push(name.into_owned());
                    }
                }
                assert_eq!(
                    json!(source_order),
                    case["typescript_observation"]["program_source_order"]
                );
                assert_eq!(
                    json!(libraries),
                    case["typescript_observation"]["standard_libraries"]
                );
                let sources = program
                    .source_files()
                    .iter()
                    .map(|source| {
                        (
                            source.path().display().to_string_lossy().into_owned(),
                            program.source_id(source.path().canonical()).unwrap(),
                        )
                    })
                    .collect::<BTreeMap<_, _>>();
                ProgramSession::new(program)
                    .with_declarations(|session: &mut DeclarationSession<'_, '_>| {
                        for expected in case["typescript_observation"]["calls"].as_array().unwrap() {
                            let kind = expected["kind"].as_str().unwrap();
                            if kind == "ordinary-command" && case["options"]["noEmit"] == true {
                                // Keep the original TS call as a later-owner reference.
                                // A refusal is not counted as successful command equivalence.
                                let before = session.activity();
                                let mut sink = Sink::new(case);
                                let error = session.emit_with_reported_diagnostics(&mut sink)
                                    .err().expect("ordinary noEmit retains its H2.9 boundary");
                                assert!(matches!(error, DriverError::InvalidProgramMode {
                                    expected: tsc_program::PreparedProgramMode::Emit,
                                    actual: tsc_program::PreparedProgramMode::NoEmit,
                                }));
                                assert!(sink.writes.is_empty());
                                assert_eq!(session.activity(), before);
                                *counts.entry("ordinary-noEmit-boundary".to_owned()).or_default() += 1;
                                continue;
                            }
                            *counts.entry(kind.to_owned()).or_default() += 1;
                            let selection = expected["target_source"].as_str().map_or(
                                EmitSelection::WholeProgram,
                                |path| EmitSelection::TargetSourceFile(*sources.get(path).unwrap()),
                            );
                            let before = session.activity();
                            let mut sink = Sink::new(case);
                            let mut fields = json!({
                                "diagnostics": null,
                                "reported_diagnostics": null,
                                "status_writes": null,
                                "exit_code": null,
                                "emit_result": null,
                            });
                            if kind == "ordinary-command" {
                                // The TS observer allocates callback buffers before
                                // entering the command; unwinding preserves empty
                                // arrays because reporting runs after Program.emit.
                                fields["reported_diagnostics"] = json!([]);
                                fields["status_writes"] = json!([]);
                            }
                            let call = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                                || -> Result<(), DriverError> {
                                    match kind {
                                        "declaration-diagnostics" => {
                                            fields["diagnostics"] = diagnostics(
                                                &session.get_declaration_diagnostics(selection)?,
                                            );
                                        }
                                        "forced-declarations" => {
                                            fields["emit_result"] = emit_result(
                                                &session.emit_forced_declarations(selection, &mut sink)?,
                                            );
                                        }
                                        "ordinary-command" => {
                                            assert_eq!(selection, EmitSelection::WholeProgram);
                                            let outcome = session.emit_with_reported_diagnostics(&mut sink)?;
                                            fields["emit_result"] = emit_result(outcome.emit());
                                            fields["reported_diagnostics"] = diagnostics(outcome.diagnostics());
                                            fields["status_writes"] = json!(outcome.status_writes());
                                            fields["exit_code"] = json!(outcome.exit_code());
                                        }
                                        _ => panic!("ordinary targeted APIs remain H2.8d"),
                                    }
                                    Ok(())
                                },
                            ));
                            let exception = match call {
                                Ok(Ok(())) => Value::Null,
                                Ok(Err(DriverError::Emit(EmitFailure::Contract(
                                    EmitContractViolation::DeclarationMapPathMissing,
                                )))) => json!({"name": "Error", "message": "Debug Failure."}),
                                Ok(Err(error)) => panic!("{id}: {error:?}"),
                                Err(error) => {
                                    assert_eq!(
                                        error.downcast_ref::<&str>(),
                                        Some(&"H2.7e controlled callback exception"),
                                    );
                                    json!({
                                        "name": "Error",
                                        "message": "H2.7e controlled callback exception",
                                    })
                                }
                            };
                            let resolver_count = expected["resolver_requests"].as_array().unwrap().len() as u64;
                            let actual_borrows = session.activity().emit_resolver_borrows()
                                - before.emit_resolver_borrows();
                            assert_eq!(
                                actual_borrows,
                                if sources.is_empty() { 0 } else { resolver_count },
                                "{id}: resolver cache activity",
                            );
                            let materialized = sink.writes.iter()
                                .filter(|write| write["sink_materialized"] == true)
                                .map(|write| write["index"].clone())
                                .collect::<Vec<_>>();
                            let actual = json!({
                                "kind": kind,
                                "owner": "H2.7e",
                                "target_source": expected["target_source"],
                                "writes": sink.writes,
                                "system_write_attempts": sink.system.map(|system| system.attempts).unwrap_or_default(),
                                "diagnostics": fields["diagnostics"],
                                "reported_diagnostics": fields["reported_diagnostics"],
                                "status_writes": fields["status_writes"],
                                "exit_code": fields["exit_code"],
                                "emit_result": fields["emit_result"],
                                "exception": exception,
                                "materialized_write_indices": materialized,
                            });
                            let mut expected = expected.clone();
                            expected.as_object_mut().unwrap().remove("resolver_requests");
                            assert_eq!(
                                actual, expected,
                                "{id}: complete {kind} call excluding separately checked resolver trace count",
                            );
                        }
                        let program = session.get_program_diagnostics()?;
                        assert_eq!(json!({
                            "options": diagnostics(program.options()),
                            "syntactic": diagnostics(program.syntactic()),
                            "global": diagnostics(program.global()),
                            "semantic": diagnostics(program.semantic()),
                        }), case["typescript_observation"]["program_diagnostics_after_calls"],
                        "{id}: final same-checker Program diagnostic streams");
                        Ok(())
                    })
                    .unwrap();
            }
        }));
        if let Err(error) = checked {
            failures.push(format!(
                "{id}: {}",
                error
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| error.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default()
            ));
        } else {
            eprintln!("H2.7e stateful declaration calls PASS {id}");
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(counts.get("declaration-diagnostics"), Some(&(77 * 2)));
    assert_eq!(counts.get("forced-declarations"), Some(&(92 * 2)));
    assert_eq!(counts.get("ordinary-command"), Some(&(20 * 2)));
    assert_eq!(counts.get("ordinary-noEmit-boundary"), Some(&2));
}

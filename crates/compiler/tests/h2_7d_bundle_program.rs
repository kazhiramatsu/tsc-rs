//! Complete production command/fresh forced Bundle observations.
//! Runtime admission belongs to the separate outFile packet; no expected tuple
//! is rewritten to manufacture a pass. Same-session calls are a separate test.
use base64::Engine;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tsc_compiler::{DeclarationSession, DriverError, ProgramSession};
use tsc_diagnostics::{Diagnostic, MessageChain};
use tsc_emitter::{
    EmitArtifact, EmitArtifactKind, EmitOutcome, EmitSelection, EmitWriteMetadata, MemoryOutputSink,
};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_emitting_program, load_program, CompilerOptions, LibraryCatalog, PreparedProgram,
    ProgramLoadLimits, ProgramOptions,
};

fn fixture() -> Value {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../emitter/tests/fixtures/bundle-declarations.json"
    ))
    .unwrap();
    assert_eq!(fixture["typescript"], "6.0.3");
    assert_eq!(fixture["repetitions"], 2);
    assert_eq!(fixture["cases"].as_array().unwrap().len(), 25);
    assert_eq!(
        fixture["adjacent_owner_references"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    fixture
}
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
            "moduleResolution" => options.module_resolution = Some(value.as_i64().unwrap() as i32),
            "newLine" => options.new_line = Some(value.as_i64().unwrap() as i32),
            "declaration" => options.declaration = value.as_bool(),
            "declarationMap" => options.declaration_map = value.as_bool(),
            "sourceMap" => options.source_map = value.as_bool(),
            "outFile" => options.out_file = value.as_str().map(str::to_owned),
            "declarationDir" => options.declaration_dir = value.as_str().map(str::to_owned),
            "ignoreDeprecations" => options.ignore_deprecations = value.as_str().map(str::to_owned),
            "listEmittedFiles" => options.list_emitted_files = value.as_bool(),
            "strict" => options.strict = value.as_bool(),
            "skipDefaultLibCheck" => options.skip_default_lib_check = value.as_bool(),
            "noErrorTruncation" => options.no_error_truncation = value.as_bool(),
            "noEmitOnError" => options.no_emit_on_error = value.as_bool(),
            "noEmit" => options.no_emit = value.as_bool(),
            "noResolve" => options.no_resolve = value.as_bool(),
            "stripInternal" => options.strip_internal = value.as_bool(),
            "emitBOM" => options.emit_bom = value.as_bool(),
            "allowJs" => options.allow_js = value.as_bool().unwrap(),
            "checkJs" => options.check_js = value.as_bool(),
            "resolveJsonModule" => options.resolve_json_module = value.as_bool(),
            other => panic!("unprojected Bundle API option {other}"),
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
    let load = if options.no_emit == Some(true) {
        load_program
    } else {
        load_emitting_program
    };
    let program = load(
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
fn write_record(write: &EmitArtifact, index: usize) -> Value {
    let (present, keys, position, data_diagnostics) = match write.metadata() {
        Some(EmitWriteMetadata::Text(data)) => (
            true,
            json!(["sourceMapUrlPos", "diagnostics"]),
            json!(data.source_map_url_position().map(|p| p.value())),
            diagnostics(data.diagnostics()),
        ),
        None => (false, Value::Null, Value::Null, Value::Null),
        _ => panic!("buildInfo is outside this frozen Bundle fixture"),
    };
    let kind = match write.kind() {
        EmitArtifactKind::JavaScript => "javascript",
        EmitArtifactKind::Declaration => "declaration",
        EmitArtifactKind::JavaScriptMap | EmitArtifactKind::DeclarationMap => "source-map",
        _ => panic!("unobserved artifact kind"),
    };
    json!({"index":index,"path":write.path(),"kind":kind,
        "callback_utf8_base64":base64::engine::general_purpose::STANDARD.encode(write.callback_bytes()),
        "callback_utf8_bytes":write.callback_bytes().len(),"write_byte_order_mark":write.write_byte_order_mark(),
        "materialized_utf8_base64":base64::engine::general_purpose::STANDARD.encode(write.materialized_bytes()),
        "materialized_utf8_bytes":write.materialized_bytes().len(),"on_error_callback_present":true,
        "source_files":write.source_files(),"data_present":present,"data_keys":keys,
        "data_source_map_url_pos":position,"data_diagnostics":data_diagnostics,"data_build_info":null})
}
fn writes(sink: &MemoryOutputSink) -> Value {
    json!(sink
        .writes()
        .iter()
        .enumerate()
        .map(|(index, write)| write_record(write, index))
        .collect::<Vec<_>>())
}
fn emit_result(outcome: &EmitOutcome) -> Value {
    json!({"emit_skipped":outcome.emit_skipped(),"diagnostics":diagnostics(outcome.diagnostics()),
        "emitted_files":outcome.emitted_files(),"source_maps":outcome.source_maps().map(|maps|maps.iter().map(|map|json!({
            "input_source_file_names":map.input_source_files(),"source_map_json":map.canonical_json()
        })).collect::<Vec<_>>())})
}
fn complete_call(expected: &Value, sink: &MemoryOutputSink, fields: Value) -> Value {
    json!({"kind":expected["kind"],"target_source":expected["target_source"],"writes":writes(sink),
        "diagnostics":fields["diagnostics"],"reported_diagnostics":fields["reported_diagnostics"],
        "status_writes":fields["status_writes"],"exit_code":fields["exit_code"],"emit_result":fields["emit_result"],"exception":null})
}
fn assert_call(actual: Value, expected: &Value, id: &str) {
    // Resolver source identities are retained in the oracle; separate scoped
    // tests compare their count. Public command tuples do not invent a trace.
    let mut expected = expected.clone();
    expected
        .as_object_mut()
        .unwrap()
        .remove("resolver_requests");
    assert_eq!(actual, expected, "{id}: complete public API call");
}
fn panic_api(error: DriverError, sink: &MemoryOutputSink, id: &str) -> ! {
    panic!(
        "{id}: unexpected API failure {error:?}; partial writes={}",
        writes(sink)
    )
}
fn run_cases(cases: &[Value], mut run: impl FnMut(&Value)) {
    let mut failures = Vec::new();
    for case in cases {
        let id = case["case_id"].as_str().unwrap();
        let checked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(case)));
        if let Err(error) = checked {
            let reason = error
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| error.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "unknown panic".to_owned());
            failures.push(format!("{id}: {reason}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} Bundle API failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn assert_request_activity(
    before: tsc_emitter::H2ActivityCounters,
    after: tsc_emitter::H2ActivityCounters,
    case: &Value,
) {
    use tsc_emitter::H2RuntimeSlice;
    for (slice, requested) in [
        (
            H2RuntimeSlice::H2_7d,
            case["options"]["outFile"]
                .as_str()
                .is_some_and(|path| !path.is_empty()),
        ),
        (
            H2RuntimeSlice::H2_7e,
            case["options"]["declarationMap"] == true,
        ),
    ] {
        assert_eq!(
            after
                .runtime_slice(slice)
                .checked_sub(before.runtime_slice(slice)),
            Some(u64::from(requested)),
            "{}: {} request delta",
            case["case_id"],
            slice.name()
        );
    }
    for slice in H2RuntimeSlice::ALL {
        if slice == H2RuntimeSlice::H2_7a || slice > H2RuntimeSlice::H2_7e {
            assert_eq!(
                after.runtime_slice(slice),
                0,
                "{}: inactive {}",
                case["case_id"],
                slice.name()
            );
        }
    }
}

#[test]
fn ordinary_bundle_program_commands_match_complete_typescript_observations() {
    let fixture = fixture();
    let libs = libraries();
    let mut count = 0;
    run_cases(fixture["cases"].as_array().unwrap(), |case| {
        let id = case["case_id"].as_str().unwrap();
        let expected = &case["typescript_observation"]["calls"][0];
        assert_eq!(expected["kind"], "ordinary-command");
        assert_eq!(expected["target_source"], Value::Null);
        assert_eq!(
            expected["exception"],
            Value::Null,
            "this fixture observes no thrown command"
        );
        // The ordinary identity hook stays an independent reference; this test
        // executes the production command without injecting an API1 hook.
        let tree = &case["ordinary_declaration_tree_reference"];
        assert_eq!(expected["writes"], tree["writes"]);
        assert_eq!(expected["emit_result"], tree["emit_result"]);
        for _ in 0..2 {
            let mut sink = MemoryOutputSink::new();
            let outcome = ProgramSession::new(prepare(case, &libs))
                .emit_command_for_harness(&mut sink)
                .unwrap_or_else(|error| panic_api(error, &sink, id));
            assert_request_activity(Default::default(), outcome.emit().h2_activity(), case);
            assert_call(
                complete_call(
                    expected,
                    &sink,
                    json!({"reported_diagnostics":diagnostics(outcome.diagnostics()),
                "status_writes":outcome.status_writes(),"exit_code":outcome.exit_code(),"emit_result":emit_result(outcome.emit())}),
                ),
                expected,
                id,
            );
            count += 1;
        }
    });
    assert_eq!(count, 50);
}

#[test]
fn fresh_forced_bundle_programs_match_complete_typescript_observations() {
    let fixture = fixture();
    let libs = libraries();
    let mut count = 0;
    run_cases(fixture["cases"].as_array().unwrap(), |case| {
        let id = case["case_id"].as_str().unwrap();
        let cold = &case["declaration_tree_reference"];
        assert_eq!(
            cold["route"],
            "separate-fresh-Program-forced-afterDeclarations-identity-hook"
        );
        assert_eq!(
            cold["exception"],
            Value::Null,
            "this fixture observes no thrown fresh force"
        );
        for _ in 0..2 {
            // Fresh Program: do not reuse command-side checker state or maps.
            let mut sink = MemoryOutputSink::new();
            let outcome = ProgramSession::new(prepare(case, &libs))
                .emit_forced_declarations(EmitSelection::WholeProgram, &mut sink)
                .unwrap_or_else(|error| panic_api(error, &sink, id));
            assert_request_activity(Default::default(), outcome.h2_activity(), case);
            assert_eq!(
                json!({"writes":writes(&sink),"emit_result":emit_result(&outcome),"exception":null}),
                json!({"writes":cold["writes"],"emit_result":cold["emit_result"],"exception":cold["exception"]}),
                "{id}: fresh forced tuple"
            );
            count += 1;
        }
    });
    assert_eq!(count, 50);
}

#[test]
fn same_session_bundle_commands_getters_and_forces_match_typescript() {
    let fixture = fixture();
    let libs = libraries();
    let mut counts = BTreeMap::<String, usize>::new();
    run_cases(fixture["cases"].as_array().unwrap(), |case| {
        let id = case["case_id"].as_str().unwrap();
        for _ in 0..2 {
            let program = prepare(case, &libs);
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
            ProgramSession::new(program).with_declarations(|session: &mut DeclarationSession<'_, '_>| {
                for expected in case["typescript_observation"]["calls"].as_array().unwrap() {
                    assert_eq!(expected["exception"],Value::Null,"fixture exception coverage must be extended explicitly");
                    let selection = expected["target_source"].as_str().map_or(EmitSelection::WholeProgram,|path|EmitSelection::TargetSourceFile(sources[path]));
                    let before = session.activity();
                    let mut sink = MemoryOutputSink::new();
                    let fields = match expected["kind"].as_str().unwrap() {
                        "ordinary-command" => {
                            assert_eq!(selection,EmitSelection::WholeProgram);
                            let outcome = session.emit_with_reported_diagnostics(&mut sink).unwrap_or_else(|error|panic_api(error,&sink,id));
                            json!({"reported_diagnostics":diagnostics(outcome.diagnostics()),"status_writes":outcome.status_writes(),
                                "exit_code":outcome.exit_code(),"emit_result":emit_result(outcome.emit())})
                        },
                        "declaration-diagnostics" => json!({"diagnostics":diagnostics(&session.get_declaration_diagnostics(selection)?)}),
                        "forced-declarations" => {
                            let outcome = session.emit_forced_declarations(selection,&mut sink).unwrap_or_else(|error|panic_api(error,&sink,id));
                            json!({"emit_result":emit_result(&outcome)})
                        },
                        other => panic!("unobserved Bundle call kind {other}"),
                    };
                    let requests = expected["resolver_requests"].as_array().unwrap().len() as u64;
                    assert_eq!(session.activity().emit_resolver_borrows()-before.emit_resolver_borrows(),if sources.is_empty(){0}else{requests},"{id}: resolver request count");
                    assert_request_activity(before, session.activity(), case);
                    assert_call(complete_call(expected,&sink,fields),expected,id);
                    *counts.entry(expected["kind"].as_str().unwrap().to_owned()).or_default() += 1;
                }
                let before_diagnostics = session.activity();
                let diagnostics_after = session.get_program_diagnostics()?;
                assert_eq!(session.activity(), before_diagnostics, "{id}: final diagnostics activity");
                assert_eq!(json!({"options":diagnostics(diagnostics_after.options()),"syntactic":diagnostics(diagnostics_after.syntactic()),
                    "global":diagnostics(diagnostics_after.global()),"semantic":diagnostics(diagnostics_after.semantic())}),
                    case["typescript_observation"]["program_diagnostics_after_calls"],"{id}: same-state final diagnostics");
                Ok(())
            }).unwrap();
        }
    });
    assert_eq!(counts.get("ordinary-command"), Some(&50));
    assert_eq!(counts.get("declaration-diagnostics"), Some(&76));
    assert_eq!(counts.get("forced-declarations"), Some(&66));
}

#[test]
fn bundle_later_owner_references_remain_separate() {
    let fixture = fixture();
    let cases = fixture["adjacent_owner_references"].as_array().unwrap();
    let no_emit = cases
        .iter()
        .find(|case| case["case_id"] == "sequence/adjacent/no-emit")
        .unwrap();
    let target = cases
        .iter()
        .find(|case| case["case_id"] == "reference/ordinary-target")
        .unwrap();
    assert_eq!(target["calls"].as_array().unwrap().len(), 1);
    assert_eq!(target["calls"][0]["kind"], "ordinary-target");
    // There is no public ordinary targeted Program API in this packet. Keep
    // its source/options/target and full observation as a named H2.8d reference;
    // do not turn it into a whole-Program success comparison.
    assert!(target["owners"]
        .as_array()
        .unwrap()
        .iter()
        .any(|owner| owner == "H2.8d"));
    assert!(no_emit["owners"]
        .as_array()
        .unwrap()
        .iter()
        .any(|owner| owner == "H2.9"));
    let libs = libraries();
    for _ in 0..2 {
        let mut sink = MemoryOutputSink::new();
        let error = ProgramSession::new(prepare(no_emit, &libs))
            .emit_command_for_harness(&mut sink)
            .err()
            .expect("ordinary noEmit keeps its mode boundary");
        assert!(matches!(
            error,
            DriverError::InvalidProgramMode {
                expected: tsc_program::PreparedProgramMode::Emit,
                actual: tsc_program::PreparedProgramMode::NoEmit
            }
        ));
        assert!(sink.writes().is_empty());
        let mut sink = MemoryOutputSink::new();
        let outcome = ProgramSession::new(prepare(no_emit, &libs))
            .emit_forced_declarations(EmitSelection::WholeProgram, &mut sink)
            .unwrap_or_else(|error| panic_api(error, &sink, "noEmit fresh force"));
        let expected = &no_emit["declaration_tree_reference"];
        assert_eq!(
            json!({"writes":writes(&sink),"emit_result":emit_result(&outcome),"exception":null}),
            json!({"writes":expected["writes"],"emit_result":expected["emit_result"],"exception":expected["exception"]})
        );
        // Getter-only prefix follows the refused ordinary noEmit call in TS.
        // Its no-op emit neither requests a resolver nor changes getter caches.
        let ordinary = &no_emit["typescript_observation"]["calls"][0];
        assert_eq!(ordinary["resolver_requests"], json!([]));
        assert_eq!(ordinary["writes"], json!([]));
        ProgramSession::new(prepare(no_emit, &libs))
            .with_declarations(|session| {
                for expected in no_emit["typescript_observation"]["calls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .skip(1)
                    .take(2)
                {
                    assert_eq!(expected["kind"], "declaration-diagnostics");
                    assert_eq!(expected["target_source"], Value::Null);
                    let observed =
                        session.get_declaration_diagnostics(EmitSelection::WholeProgram)?;
                    assert_eq!(diagnostics(&observed), expected["diagnostics"]);
                }
                Ok(())
            })
            .unwrap();
    }
}

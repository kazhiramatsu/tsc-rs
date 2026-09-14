//! H2.8a G5c: JSDoc return annotation ownership through complete commands.
//! Each control is one complete command observed twice from pinned
//! TypeScript (`scripts/observe-h2-8a-jsdoc-return.mjs`); the callback value
//! is compared as UTF-16 units so an unpaired unit is distinguished from its
//! U+FFFD sink projection. The original G5c command is replayed through the
//! shared original-corpus comparator, never through a reduced projection.
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tsc_compiler::{MemoryOutputSink, ProgramSession};
use tsc_diagnostics::{Diagnostic, JsStr, JsString, MessageChain};
use tsc_emitter::{EmitArtifact, EmitWriteMetadata};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_emitting_program, load_program, CompilerOptions, LibraryCatalog, PreparedProgram,
    ProgramLoadLimits, ProgramOptions,
};

#[allow(dead_code)]
#[path = "integration/h2_7d_original_corpus_shared.rs"]
mod h2_7d_original_corpus_shared;

const FIXTURE: &[u8] = include_bytes!("fixtures/h2-8a-jsdoc-return.json");
const FIXTURE_SHA256: &str = "687d28b13459dff3b472515c430f66165c4bdaaec6f07bbbb893ca2997d54ba3";
const CASES: usize = 58;
const TRACED_FUNCTIONS: usize = 70;

// The original tuple stays a strict complete comparison; nothing is skipped.
#[test]
fn original_g5c_complete_command() {
    let case =
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsFunctionsCjs.ts#default";
    let exact = h2_7d_original_corpus_shared::assert_output_matrix_projection(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        &[case],
    );
    assert_eq!(exact.len(), 1);
}

fn prepare(case: &Value) -> PreparedProgram {
    let files = case["files"].as_array().unwrap();
    for file in files {
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(file["text"].as_str().unwrap().as_bytes())
            ),
            file["sha256"]
        );
    }
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut host = MemoryCompilerHost::builder("/project");
    for file in files {
        host = host.file(
            file["path"].as_str().unwrap(),
            file["text"].as_str().unwrap().as_bytes().to_vec(),
        );
    }
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
    let mut options = CompilerOptions::default();
    for (name, value) in case["options"].as_object().unwrap() {
        match name.as_str() {
            "target" => options.target = Some(i32::try_from(value.as_i64().unwrap()).unwrap()),
            "newLine" => options.new_line = Some(i32::try_from(value.as_i64().unwrap()).unwrap()),
            "allowJs" => options.allow_js = value.as_bool().unwrap(),
            "checkJs" => options.check_js = value.as_bool(),
            "declaration" => options.declaration = value.as_bool(),
            "declarationMap" => options.declaration_map = value.as_bool(),
            "emitDeclarationOnly" => options.emit_declaration_only = value.as_bool(),
            "noEmit" => options.no_emit = value.as_bool(),
            "noEmitOnError" => options.no_emit_on_error = value.as_bool(),
            "removeComments" => options.remove_comments = value.as_bool(),
            "skipDefaultLibCheck" => options.skip_default_lib_check = value.as_bool(),
            "noErrorTruncation" => options.no_error_truncation = value.as_bool(),
            "outDir" => options.out_dir = value.as_str().map(JsString::from),
            other => panic!("unowned control option {other}"),
        }
    }
    let loader = if options.no_emit == Some(true) {
        load_program
    } else {
        load_emitting_program
    };
    loader(
        &host,
        &case["roots"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| PathBuf::from(value.as_str().unwrap()))
            .collect::<Vec<_>>(),
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
        _ => panic!("unexpected callback metadata in these controls"),
    };
    // The callback value is the JavaScript string tsc hands to writeFile:
    // compared unit for unit. The UTF-8 channel is the sink projection.
    let callback = json!({
        "utf16": artifact.callback_units().as_ref(),
        "utf8_base64": base64::engine::general_purpose::STANDARD.encode(artifact.callback_bytes()),
    });
    json!({"index": index, "path": scalar_path(artifact.path()),
        "callback": callback, "write_byte_order_mark": artifact.write_byte_order_mark(),
        "materialized_utf8_base64": base64::engine::general_purpose::STANDARD.encode(artifact.materialized_bytes()),
        "on_error_callback_present": true, "source_files": artifact.source_files().map(|values| values.iter().map(|value| scalar_path(value.as_js())).collect::<Vec<_>>()),
        "data_present": artifact.metadata().is_some(), "data_keys": keys,
        "data_source_map_url_pos": position, "data_diagnostics": data_diagnostics})
}

#[test]
fn jsdoc_return_controls_match_complete_commands_twice() {
    assert_eq!(format!("{:x}", Sha256::digest(FIXTURE)), FIXTURE_SHA256);
    let fixture: Value = serde_json::from_slice(FIXTURE).unwrap();
    assert_eq!(fixture["repetitions"], 2);
    assert_eq!(fixture["typescript"], "6.0.3");
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), CASES);
    assert_eq!(fixture["complete_command_executions"], 2 * CASES);
    assert_eq!(
        cases
            .iter()
            .map(|case| case["return_trace"].as_array().unwrap().len())
            .sum::<usize>(),
        TRACED_FUNCTIONS
    );
    let filter = std::env::var("TSC_RS_JSDOC_RETURN_FILTER").ok();
    let mut failures = Vec::new();
    let mut executed = 0;
    for case in cases {
        let id = case["id"].as_str().unwrap();
        if filter.as_deref().is_some_and(|filter| !id.contains(filter)) {
            continue;
        }
        executed += 1;
        let expected = &case["complete_command_runs"][0];
        assert_eq!(*expected, case["complete_command_runs"][1]);
        assert_eq!(case["native"], "exact");
        let mut actuals = Vec::new();
        for attempt in 0..2 {
            if case["options"]["noEmit"] == true {
                // The separate H0 run keeps its zero-emitter-activity proof.
                let outcome = ProgramSession::new(prepare(case)).run().unwrap();
                assert!(outcome.no_emit_activity().all_zero(), "{id}");
            }
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
                Err(error) => Err(error.to_string()),
            };
            if let Some(directory) = std::env::var_os("TSC_RS_JSDOC_RETURN_CAPTURE_DIR") {
                let directory = PathBuf::from(directory);
                assert!(directory.is_absolute());
                std::fs::create_dir_all(&directory).unwrap();
                let key = format!("{:x}", Sha256::digest(id.as_bytes()));
                let capture = json!({"case_id": id, "attempt": attempt, "group": case["group"],
                    "actual": actual.as_ref().ok(), "error": actual.as_ref().err(), "expected": expected,
                    "exact": actual.as_ref().is_ok_and(|actual| actual == expected)});
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(directory.join(format!("{key}-{attempt}.json")))
                    .unwrap();
                serde_json::to_writer_pretty(&mut file, &capture).unwrap();
            }
            actuals.push(actual);
        }
        assert_eq!(actuals[0], actuals[1], "{id}: repetition drift");
        match &actuals[0] {
            Ok(actual) if actual == expected => eprintln!("jsdoc-return EXACT x2 {id}"),
            Ok(_) => failures.push(format!("{id} ({}): differs", case["group"])),
            Err(error) => failures.push(format!("{id} ({}): {error}", case["group"])),
        }
    }
    assert!(executed > 0, "the filter selected no control");
    assert!(
        failures.is_empty(),
        "{} JSDoc return control divergences: {failures:?}",
        failures.len()
    );
}

fn scalar_path<'a>(value: JsStr<'a>) -> &'a str {
    value
        .as_str()
        .expect("these frozen control path observations are scalar")
}

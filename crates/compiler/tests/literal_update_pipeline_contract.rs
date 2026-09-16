//! C01 / A40-LITERAL-UPDATE: complete-command comparison of the source-reachable
//! literal update routes (children-only template updates through the ES2020
//! lowering inside template spans at ES5 / ES2015 / ESNext, and the
//! rewriteRelativeImportExtensions string-literal rewrite). Retains every
//! callback, diagnostic-value, emit-result, status and exit field, compared
//! twice against `scripts/observe-literal-update.mjs pipeline`.
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde_json::{json, Value};
use tsc_compiler::{MemoryOutputSink, ProgramSession};
use tsc_diagnostics::{Diagnostic, JsStr, JsString, MessageChain};
use tsc_emitter::{EmitArtifact, EmitWriteMetadata};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_emitting_program, CompilerOptions, LibraryCatalog, PreparedProgram, ProgramLoadLimits,
    ProgramOptions,
};

const FIXTURE: &[u8] = include_bytes!("fixtures/literal-update-pipeline.json");

fn option_i32(value: &Value) -> Option<i32> {
    value.as_i64().map(|value| i32::try_from(value).unwrap())
}

fn prepare(case: &Value) -> PreparedProgram {
    let options = &case["options"];
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut host = MemoryCompilerHost::builder("/project");
    for file in case["files"].as_array().unwrap() {
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
    let compiler_options = CompilerOptions {
        target: option_i32(&options["target"]),
        module: option_i32(&options["module"]),
        new_line: option_i32(&options["newLine"]),
        strict: options["strict"].as_bool(),
        declaration: options["declaration"].as_bool(),
        declaration_map: options["declarationMap"].as_bool(),
        source_map: options["sourceMap"].as_bool(),
        skip_default_lib_check: options["skipDefaultLibCheck"].as_bool(),
        no_error_truncation: options["noErrorTruncation"].as_bool(),
        rewrite_relative_import_extensions: options["rewriteRelativeImportExtensions"].as_bool(),
        out_dir: options["outDir"].as_str().map(Into::into),
        ..Default::default()
    };
    let roots = case["roots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|root| PathBuf::from(root.as_str().unwrap()))
        .collect::<Vec<_>>();
    load_emitting_program(
        &host,
        &roots,
        compiler_options,
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
        _ => panic!("unexpected callback metadata in literal update commands"),
    };
    // These callback strings are scalar; diagnostic values retain UTF-16 above.
    let callback = std::str::from_utf8(artifact.callback_bytes()).unwrap();
    json!({"index": index, "path": scalar_path(artifact.path()),
        "callback": string_value(callback), "write_byte_order_mark": artifact.write_byte_order_mark(),
        "materialized_utf8_base64": base64::engine::general_purpose::STANDARD.encode(artifact.materialized_bytes()),
        "on_error_callback_present": true, "source_files": artifact.source_files().map(|values| values.iter().map(|value| scalar_path(value.as_js())).collect::<Vec<_>>()),
        "data_present": artifact.metadata().is_some(), "data_keys": keys,
        "data_source_map_url_pos": position, "data_diagnostics": data_diagnostics})
}

#[test]
fn literal_update_pipeline_commands_match_complete_typescript_twice() {
    let fixture: Value = serde_json::from_slice(FIXTURE).unwrap();
    assert_eq!(fixture["repetitions"], 2);
    assert_eq!(fixture["typescript"], "6.0.3");
    assert_eq!(fixture["group"], "pipeline");
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 22);
    let report_dir = std::env::var_os("TSC_RS_LITERAL_UPDATE_REPORT_DIR").map(PathBuf::from);
    let mut failures = Vec::new();
    let mut rows = Vec::new();
    for case in cases {
        let id = case["case_id"].as_str().unwrap();
        let expected = &case["typescript_observation"];
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
            let (actual, error) = match result {
                Ok(command) => {
                    let emit = command.emit();
                    let maps = emit.source_maps().map(|maps| maps.iter().map(|map| json!({
                        "input_source_file_names": map.input_source_files().iter().map(|value| scalar_path(value.as_js())).collect::<Vec<_>>(), "source_map_json": map.canonical_json()
                    })).collect::<Vec<_>>());
                    (
                        Some(
                            json!({"writes": writes, "reported_diagnostics": diagnostics(command.diagnostics()),
                        "status_writes": command.status_writes().iter().map(string_value).collect::<Vec<_>>(),
                        "exit_code": command.exit_code(), "emit_result": {"emit_skipped": emit.emit_skipped(),
                            "diagnostics": diagnostics(emit.diagnostics()), "emitted_files": emit.emitted_files().map(|values| values.iter().map(|value| scalar_path(value.as_js())).collect::<Vec<_>>()), "source_maps": maps}}),
                        ),
                        None,
                    )
                }
                Err(error) => (None, Some(error.to_string())),
            };
            let _ = attempt;
            actuals.push((actual, error));
        }
        assert_eq!(actuals[0], actuals[1], "{id}: repetition drift");
        let exact = actuals[0].1.is_none() && actuals[0].0.as_ref() == Some(expected);
        if !exact {
            failures.push(id);
        }
        rows.push(json!({"case_id": id, "status": if exact { "exact" } else { "divergence" },
            "error": actuals[0].1, "actual": if exact { Value::Null } else { actuals[0].0.clone().unwrap_or(Value::Null) },
            "expected": if exact { Value::Null } else { expected.clone() }}));
    }
    if let Some(directory) = report_dir {
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("pipeline.json"),
            serde_json::to_string_pretty(&json!({"group": "pipeline", "cases": cases.len(),
                "exact": cases.len() - failures.len(), "divergent": failures, "rows": rows}))
            .unwrap()
                + "\n",
        )
        .unwrap();
    }
    assert!(
        failures.is_empty(),
        "complete literal update command divergences: {failures:?}"
    );
}

fn scalar_path<'a>(value: JsStr<'a>) -> &'a str {
    value
        .as_str()
        .expect("these frozen literal update path observations are scalar")
}

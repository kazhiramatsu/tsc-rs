//! Repeated public declaration getters with full diagnostic and activity observations.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use tsc_compiler::{EmitSelection, ProgramSession};
use tsc_diagnostics::{Diagnostic, MessageChain};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_emitting_program, load_program, CompilerOptions, LibraryCatalog, ProgramLoadLimits,
    ProgramOptions,
};

fn flatten_message(chain: &MessageChain, indent: usize, output: &mut String) {
    if indent != 0 {
        output.push('\n');
        for _ in 0..indent {
            output.push_str("  ");
        }
    }
    output.push_str(&chain.text);
    for child in &chain.next {
        flatten_message(child, indent + 1, output);
    }
}

pub(super) fn diagnostic_json(diagnostic: &Diagnostic) -> Value {
    let mut message = String::new();
    flatten_message(&diagnostic.message, 0, &mut message);
    let related = if diagnostic.related_information_present || !diagnostic.related.is_empty() {
        Value::Array(diagnostic.related.iter().map(|related| {
            let mut message = String::new();
            flatten_message(&related.message, 0, &mut message);
            json!({"code":related.message.code,"category":format!("{:?}", related.message.category),
                "file":related.file_name,"start":related.start,"length":related.length,
                "message":message,"related_information":null})
        }).collect())
    } else {
        Value::Null
    };
    json!({"code":diagnostic.code(),"category":format!("{:?}", diagnostic.category()),
        "file":diagnostic.file_name,"start":diagnostic.start,"length":diagnostic.length,
        "message":message,"related_information":related})
}

#[test]
fn public_declaration_getters_match_complete_typescript_observations() {
    let artifact: Value =
        serde_json::from_slice(include_bytes!("../fixtures/declaration-getters.json"))
            .expect("frozen TypeScript getter observations");
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 19);
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut failures = Vec::new();
    for case in cases {
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
                    "declaration" => options.declaration = value.as_bool(),
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
                    other => panic!("unexpected getter option {other}"),
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
                let transformable = prepared
                    .source_files()
                    .iter()
                    .filter(|source| {
                        source.may_be_emitted()
                            && !source.path().display().to_string_lossy().ends_with(".json")
                    })
                    .map(|source| prepared.source_id(source.path().canonical()).unwrap())
                    .collect::<BTreeSet<_>>();
                let expected = &case["typescript_observation"];
                let calls = expected["calls"].as_array().unwrap();
                let observed = ProgramSession::new(prepared).with_declarations(|getter| {
                    let mut observed = Vec::new();
                    let mut transformed = BTreeSet::new();
                    let mut resolver_requests = 0;
                    let request_traces = case["owner_observation"]["resolver_requests_by_call"].as_array().unwrap();
                    for (call, requests) in calls.iter().zip(request_traces) {
                        resolver_requests += requests.as_array().unwrap().len() as u64;
                        let selection = call["source_file"].as_str().map_or(EmitSelection::WholeProgram,
                            |path| EmitSelection::TargetSourceFile(*sources.get(path).expect("selected source is loaded")));
                        match selection {
                            EmitSelection::WholeProgram => transformed.extend(transformable.iter().copied()),
                            EmitSelection::TargetSourceFile(source) => { if transformable.contains(&source) { transformed.insert(source); } },
                        }
                        let diagnostics = getter.get_declaration_diagnostics(selection)?;
                        let activity = getter.activity();
                        assert_eq!(activity.emit_resolver_borrows(), resolver_requests, "{case_id}: uncached non-d.ts resolver requests");
                        assert_eq!(activity.transform_context_constructions(), transformed.len() as u64, "{case_id}: each source transforms once");
                        assert_eq!(activity.emit_session_constructions(), 0, "{case_id}: no emit session");
                        assert_eq!(activity.output_plan_constructions(), 0, "{case_id}: no output plan");
                        assert_eq!(activity.script_transformer_list_constructions(), 0, "{case_id}: no JS transform");
                        assert_eq!(activity.printer_constructions(), 0, "{case_id}: no printer");
                        assert_eq!(activity.javascript_artifact_creations(), 0, "{case_id}: no JS artifact");
                        assert_eq!(activity.output_sink_write_attempts(), 0, "{case_id}: no writes");
                        observed.push(json!({"source_file":call["source_file"],"diagnostics":diagnostics.iter().map(diagnostic_json).collect::<Vec<_>>()}));
                    }
                    Ok(json!({"calls":observed,"writes":[]}))
                }).unwrap_or_else(|error| panic!("{case_id}: {error:?}"));
                assert_eq!(
                    &observed, expected,
                    "{case_id}: complete getter observation"
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
        "{} getter differences:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

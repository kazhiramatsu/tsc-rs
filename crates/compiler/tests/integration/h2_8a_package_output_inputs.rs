//! Local package output-to-input resolution and its complete command behavior.
use serde_json::{json, Value};
use std::collections::BTreeSet;
use tsc_diagnostics::{Diagnostic, MessageChain};
use tsc_program::{PreparedProgram, ResolutionMode, ResolutionOutcome};

#[test]
fn package_output_paths_resolve_to_inputs_with_complete_observations() {
    let artifact: Value =
        serde_json::from_slice(include_bytes!("../fixtures/package-output-inputs.json")).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    assert_eq!(artifact["cases"].as_array().unwrap().len(), 46);
    super::h2_7c_declaration_blocking::assert_cases_with_inspection(
        &artifact,
        true,
        assert_prepared_facts,
    );
}

fn message(chain: &MessageChain, depth: usize, out: &mut String) {
    if depth != 0 {
        out.push('\n');
        out.push_str(&"  ".repeat(depth));
    }
    out.push_str(&chain.text);
    for child in &chain.next {
        message(child, depth + 1, out);
    }
}

fn diagnostics(values: &[Diagnostic]) -> Value {
    json!(values.iter().map(|d| {
        let mut text = String::new();
        message(&d.message, 0, &mut text);
        let related = if d.related_information_present || !d.related.is_empty() {
            json!(d.related.iter().map(|r| {
                let mut text = String::new(); message(&r.message, 0, &mut text);
                json!({"code":r.message.code,"category":format!("{:?}",r.message.category),
                    "file":r.file_name,"start":r.start,"length":r.length,"message":text,"related_information":null})
            }).collect::<Vec<_>>())
        } else { Value::Null };
        json!({"code":d.code(),"category":format!("{:?}",d.category()),"file":d.file_name,
            "start":d.start,"length":d.length,"message":text,"related_information":related})
    }).collect::<Vec<_>>())
}

pub(super) fn assert_prepared_facts(case_id: &str, program: &PreparedProgram, expected: &Value) {
    let Some(expected_files) = expected.get("loaded_files") else {
        return;
    };
    let libraries = program
        .library_files()
        .iter()
        .map(|id| program.source_file(*id).unwrap().path().canonical())
        .collect::<BTreeSet<_>>();
    let files = program
        .source_files()
        .iter()
        .filter(|source| !libraries.contains(source.path().canonical()))
        .map(|source| source.path().display().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        json!(files),
        *expected_files,
        "{case_id}: loaded-file order"
    );
    let sources = |key| {
        program
            .source_file(
                program
                    .source_id(key)
                    .expect("request source owned by Program"),
            )
            .unwrap()
            .path()
            .display()
            .to_string_lossy()
            .into_owned()
    };
    let modules = program.resolutions().modules().filter(|(key, _)| !libraries.contains(key.source()))
        .map(|(key, resolution)| {
            let (file, extension) = match resolution.outcome() {
                ResolutionOutcome::Resolved(module) => (Some(module.target().resolved_file().display().to_string_lossy().into_owned()), Some(module.extension().as_str().to_owned())),
                ResolutionOutcome::NotFound => (None, None),
            };
            let mode = match key.mode() { ResolutionMode::CommonJs => Some(1), ResolutionMode::EsNext => Some(99), ResolutionMode::Unspecified => None };
            json!({"source":sources(key.source()),"specifier":key.specifier(),"mode":mode,
                "resolved_file":file,"extension":extension,"diagnostics":diagnostics(resolution.diagnostics())})
        }).collect::<Vec<_>>();
    assert_eq!(
        json!(modules),
        expected["resolutions"],
        "{case_id}: raw module resolution facts"
    );
    let types = program
        .resolutions()
        .type_references()
        .filter(|(key, _)| !libraries.contains(key.origin().canonical_path()))
        .map(|(key, resolution)| {
            assert!(
                !key.origin().is_automatic(),
                "fixture explicitly disables automatic type names"
            );
            let (file, primary) = match resolution.outcome() {
                ResolutionOutcome::Resolved(target) => (
                    Some(target.target().display().to_string_lossy().into_owned()),
                    Some(target.primary()),
                ),
                ResolutionOutcome::NotFound => (None, None),
            };
            // This prepared list also owns source-located TS2688 inclusion
            // errors. The complete command comparator checks those; raw
            // resolution diagnostics are the fileless rows in these controls.
            let raw = resolution
                .diagnostics()
                .iter()
                .filter(|d| d.file_name.is_none())
                .cloned()
                .collect::<Vec<_>>();
            json!({"source":sources(key.origin().canonical_path()),"specifier":key.specifier(),
                "resolved_file":file,"primary":primary,"diagnostics":diagnostics(&raw)})
        })
        .collect::<Vec<_>>();
    assert_eq!(
        json!(types),
        expected["type_resolutions"],
        "{case_id}: type-reference resolution facts"
    );
}

#[test]
fn root_diagnostic_module_format_notes_match_complete_observations() {
    let artifact: Value =
        serde_json::from_slice(include_bytes!("../fixtures/output-root-format.json")).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    assert_eq!(artifact["cases"].as_array().unwrap().len(), 50);
    super::h2_7c_declaration_blocking::assert_cases_with_reporting(&artifact, true);
}

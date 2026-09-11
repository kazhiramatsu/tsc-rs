//! CFG1c command controls exercise config discovery through ordinary emit.

use serde_json::{json, Value};
use tsc_program::{PreparedProgram, ProgramPath};

use super::h2_7c_declaration_blocking::assert_cases_with_inspection;

fn observations() -> Value {
    let artifact: Value = serde_json::from_slice(include_bytes!(
        "../fixtures/h2-8b-config-extension-commands.json"
    ))
    .expect("frozen config command observations");
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    assert_eq!(artifact["cases"].as_array().expect("cases").len(), 4);
    assert_eq!(artifact["upstream_failures"], json!([]));
    artifact
}

#[test]
fn config_extension_commands_match_complete_typescript_observations() {
    assert_every_case(record_attempt);
}

#[test]
fn config_extension_commands_match_ordered_program_facts() {
    assert_every_case(inspect_program_facts);
}

fn assert_every_case(inspect: fn(&str, &PreparedProgram, &Value)) {
    let artifact = observations();
    let mut failures = Vec::new();
    for case in artifact["cases"].as_array().expect("cases") {
        let mut single = artifact.clone();
        single["cases"] = json!([case]);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            assert_cases_with_inspection(&single, true, inspect);
        }));
        if result.is_err() {
            failures.push(case["case_id"].as_str().expect("case id").to_owned());
        }
    }
    assert!(
        failures.is_empty(),
        "failed complete command cases: {failures:?}"
    );
}

fn record_attempt(case_id: &str, _: &PreparedProgram, _: &Value) {
    eprintln!(
        "H2.8b-CFG1c-command {}",
        json!({"event": "prepared", "test": "complete-command", "case_id": case_id})
    );
}

fn display_path(path: &ProgramPath) -> &str {
    path.display().to_str().expect("Unicode prepared path")
}

fn inspect_program_facts(case_id: &str, prepared: &PreparedProgram, expected: &Value) {
    eprintln!(
        "H2.8b-CFG1c-command {}",
        json!({"event": "prepared", "test": "program-membership", "case_id": case_id})
    );
    let actual = json!({
        "source_files": prepared.source_files().iter()
            .map(|source| display_path(source.path())).collect::<Vec<_>>(),
        "library_files": prepared.library_files().iter()
            .map(|id| display_path(prepared.source_file(*id).expect("library source").path()))
            .collect::<Vec<_>>(),
        "root_names": prepared.roots().iter()
            .map(|root| display_path(root.path())).collect::<Vec<_>>(),
    });
    eprintln!(
        "H2.8b-CFG1c-command {}",
        json!({"event": "program-facts", "case_id": case_id, "actual": actual})
    );
    assert_eq!(
        actual, expected["program_facts"],
        "{case_id}: exact ordered source/library/root membership"
    );
}

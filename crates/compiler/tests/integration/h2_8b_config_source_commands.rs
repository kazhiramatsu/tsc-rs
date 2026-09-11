//! CFG1f command controls exercise config discovery through ordinary emit.

use serde_json::{json, Value};
use tsc_program::{PreparedProgram, ProgramPath};

use super::h2_7c_declaration_blocking::assert_cases_with_inspection;

fn observations() -> Value {
    let mut artifact: Value = serde_json::from_slice(include_bytes!(
        "../fixtures/h2-8b-config-source-commands.json"
    ))
    .expect("frozen config command observations");
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    assert_eq!(artifact["cases"].as_array().expect("cases").len(), 8);
    assert_eq!(artifact["upstream_failures"], json!([]));
    for case in artifact["cases"].as_array_mut().expect("cases") {
        if let Some(option) = module_boundary(case["case_id"].as_str().expect("case id")) {
            case["rust_expected_unsupported_option"] = json!(option);
        }
    }
    artifact
}

// H2.8c-MOD1 owns activation of these existing emitter guards. The CFG
// controls assert their typed refusal, no writes, and Program membership.
fn module_boundary(case_id: &str) -> Option<&'static str> {
    if case_id.ends_with("/none-isolated") {
        Some("isolatedModules")
    } else if case_id.ends_with("/none-verbatim") {
        Some("verbatimModuleSyntax")
    } else {
        None
    }
}

#[test]
fn config_source_commands_match_observations_and_module_boundaries() {
    assert_every_case(record_attempt);
}

#[test]
fn config_source_commands_match_ordered_program_facts() {
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
        "H2.8b-CFG1f-command {}",
        json!({"event": "prepared", "test": if module_boundary(case_id).is_some() { "module-boundary" } else { "complete-command" }, "case_id": case_id})
    );
}

fn display_path(path: &ProgramPath) -> &str {
    path.display().to_str().expect("Unicode prepared path")
}

fn inspect_program_facts(case_id: &str, prepared: &PreparedProgram, expected: &Value) {
    if module_boundary(case_id).is_some() {
        assert!(!prepared
            .diagnostics()
            .options()
            .iter()
            .any(|d| d.code() == 1148));
        assert!(!expected["reported_diagnostics"]
            .as_array()
            .expect("diagnostics")
            .iter()
            .any(|d| d["code"] == 1148));
    }
    eprintln!(
        "H2.8b-CFG1f-command {}",
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
        "H2.8b-CFG1f-command {}",
        json!({"event": "program-facts", "case_id": case_id, "actual": actual})
    );
    assert_eq!(
        actual, expected["program_facts"],
        "{case_id}: exact ordered source/library/root membership"
    );
}

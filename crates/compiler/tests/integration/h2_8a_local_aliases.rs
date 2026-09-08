//! Local alias declaration output and adjacent TypeScript controls.
#[test]
fn local_alias_declarations_match_complete_typescript_observations() {
    let artifact: serde_json::Value =
        serde_json::from_slice(include_bytes!("../fixtures/local-alias-declarations.json"))
            .unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 32);
    let mut failures = Vec::new();
    for case in cases {
        let case_id = case["case_id"].as_str().unwrap();
        let result = std::panic::catch_unwind(|| {
            super::h2_7c_declaration_blocking::assert_cases_with_reporting(
                &serde_json::json!({"cases":[case]}),
                true,
            );
        });
        if result.is_err() {
            failures.push(case_id);
        } else {
            eprintln!("Local alias declarations EXACT x2 {case_id}");
        }
    }
    assert!(
        failures.is_empty(),
        "complete local alias failures: {failures:?}"
    );
}

#[test]
fn ambient_alias_target_names_match_complete_typescript_observations() {
    let artifact: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../fixtures/ambient-alias-target-names.json"
    ))
    .unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 8);
    let mut failures = Vec::new();
    for case in cases {
        let case_id = case["case_id"].as_str().unwrap();
        let result = std::panic::catch_unwind(|| {
            super::h2_7c_declaration_blocking::assert_cases_with_reporting(
                &serde_json::json!({"cases":[case]}),
                true,
            );
        });
        if result.is_err() {
            failures.push(case_id);
        } else {
            eprintln!("Ambient alias target names EXACT x2 {case_id}");
        }
    }
    assert!(
        failures.is_empty(),
        "complete ambient alias failures: {failures:?}"
    );
}

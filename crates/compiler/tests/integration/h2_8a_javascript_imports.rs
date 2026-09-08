//! JavaScript import retention and adjacent TypeScript erasure controls.
#[test]
fn javascript_import_retention_matches_complete_typescript_observations() {
    let artifact: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../fixtures/javascript-import-retention.json"
    ))
    .unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 42);
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
            eprintln!("JavaScript import retention EXACT x2 {case_id}");
        }
    }
    assert!(
        failures.is_empty(),
        "complete import retention failures: {failures:?}"
    );
}

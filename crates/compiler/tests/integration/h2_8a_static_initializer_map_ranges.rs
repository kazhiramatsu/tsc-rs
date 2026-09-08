//! Complete TypeScript commands for Static initializer source-map ranges.
#[test]
fn static_initializer_map_ranges_matches_complete_typescript_observations() {
    let artifact: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../fixtures/static-initializer-map-ranges.json"
    ))
    .unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 104);
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
            eprintln!("Static initializer source-map ranges EXACT x2 {case_id}");
        }
    }
    assert!(
        failures.is_empty(),
        "complete Static initializer source-map ranges failures: {failures:?}"
    );
}

#[test]
fn property_literal_key_ranges_matches_complete_typescript_observations() {
    let artifact: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../fixtures/property-literal-key-ranges.json"
    ))
    .unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 20);
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
            eprintln!("Property literal key ranges EXACT x2 {case_id}");
        }
    }
    assert!(
        failures.is_empty(),
        "complete Property literal key ranges failures: {failures:?}"
    );
}

//! Complete clean TypeScript commands for async function body ranges.
use serde_json::{json, Value};

#[test]
fn async_arrow_body_ranges_matches_complete_typescript_observations() {
    let artifact: Value =
        serde_json::from_slice(include_bytes!("../fixtures/async-arrow-body-ranges.json")).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 72);
    let mut failures = Vec::new();
    for case in cases {
        let id = case["case_id"].as_str().unwrap();
        let result = std::panic::catch_unwind(|| {
            super::h2_7c_declaration_blocking::assert_cases_with_command_inspection(
                &json!({"cases": [case]}),
                true,
                super::h2_8a_import_helpers::capture_complete_command,
            );
        });
        if result.is_err() {
            failures.push(id);
        } else {
            eprintln!("async-arrow body ranges recovery EXACT x2 {id}");
        }
    }
    eprintln!(
        "async-arrow body ranges recovery SUMMARY exact={} failed={} selected={}",
        cases.len() - failures.len(),
        failures.len(),
        cases.len()
    );
    assert!(
        failures.is_empty(),
        "complete async-arrow body ranges command failures: {failures:?}"
    );
}

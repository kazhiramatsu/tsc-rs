//! Complete clean TypeScript commands for async function body ranges.
use serde_json::{json, Value};

#[test]
fn async_arrow_body_ranges_matches_complete_typescript_observations() {
    let artifacts = [
        (
            include_bytes!("../fixtures/async-arrow-body-ranges.json").as_slice(),
            72,
        ),
        (
            include_bytes!("../fixtures/async-arrow-comment-boundaries.json").as_slice(),
            96,
        ),
        (
            include_bytes!("../fixtures/async-capture-source-ranges.json").as_slice(),
            204,
        ),
    ]
    .map(|(bytes, count)| {
        let artifact: Value = serde_json::from_slice(bytes).unwrap();
        assert_eq!(artifact["typescript"], "6.0.3");
        assert_eq!(artifact["repetitions"], 2);
        assert_eq!(artifact["cases"].as_array().unwrap().len(), count);
        artifact
    });
    let cases: Vec<_> = artifacts
        .iter()
        .flat_map(|artifact| artifact["cases"].as_array().unwrap())
        .collect();
    assert_eq!(cases.len(), 372);
    let mut failures = Vec::new();
    for case in &cases {
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

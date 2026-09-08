//! Root-directory and config-derived output layouts through production emission.

#[test]
fn output_roots_match_complete_typescript_observations() {
    let artifact: serde_json::Value =
        serde_json::from_slice(include_bytes!("../fixtures/output-roots.json"))
            .expect("frozen TypeScript root observations");
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    assert_eq!(artifact["cases"].as_array().unwrap().len(), 27);
    super::h2_7c_declaration_blocking::assert_cases_with_reporting(&artifact, true);
}

#[test]
fn output_root_inclusion_and_eligibility_match_typescript_observations() {
    let artifact: serde_json::Value =
        serde_json::from_slice(include_bytes!("../fixtures/output-roots.json")).unwrap();
    let cases = &artifact["supplemental_cases"];
    assert_eq!(cases.as_array().unwrap().len(), 13);
    super::h2_7c_declaration_blocking::assert_cases_with_reporting(
        &serde_json::json!({"cases": cases}),
        true,
    );
}

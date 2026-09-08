//! Direct-option output paths, text policies and preflight collisions.

#[test]
fn output_directories_match_complete_typescript_observations() {
    let artifact: serde_json::Value =
        serde_json::from_slice(include_bytes!("../fixtures/output-directories.json"))
            .expect("frozen TypeScript output observations");
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    assert_eq!(artifact["cases"].as_array().unwrap().len(), 34);
    super::h2_7c_declaration_blocking::assert_cases_with_reporting(&artifact, true);
}

#[test]
fn module_export_identifiers_match_complete_typescript_observations() {
    let artifact: serde_json::Value =
        serde_json::from_slice(include_bytes!("../fixtures/module-export-identifiers.json"))
            .expect("frozen TypeScript identifier observations");
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    assert_eq!(artifact["cases"].as_array().unwrap().len(), 8);
    super::h2_7c_declaration_blocking::assert_cases_with_reporting(&artifact, true);
}

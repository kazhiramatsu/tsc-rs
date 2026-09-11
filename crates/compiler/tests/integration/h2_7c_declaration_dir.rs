//! Declaration directory options and diagnostic locations through production emission.

#[test]
fn declaration_dir_match_complete_typescript_observations() {
    let artifact: serde_json::Value =
        serde_json::from_slice(include_bytes!("../fixtures/declaration-dir.json"))
            .expect("frozen TypeScript observations");
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    assert_eq!(artifact["cases"].as_array().unwrap().len(), 34);
    assert_eq!(
        artifact["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|case| case["rust_expected_unsupported_option"] == "outDir")
            .count(),
        3,
        "three historical H2.8a references now execute their unchanged exact observations",
    );
    super::h2_7c_declaration_blocking::assert_cases(&artifact);
}

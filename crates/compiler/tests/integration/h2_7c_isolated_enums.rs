//! Isolated declaration enums diagnostics through production emission.

#[test]
fn isolated_enums_match_complete_typescript_observations() {
    let artifact: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../fixtures/isolated-declaration-enums.json"
    ))
    .expect("frozen TypeScript observations");
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    assert_eq!(artifact["cases"].as_array().unwrap().len(), 21);
    super::h2_7c_declaration_blocking::assert_cases(&artifact);
}

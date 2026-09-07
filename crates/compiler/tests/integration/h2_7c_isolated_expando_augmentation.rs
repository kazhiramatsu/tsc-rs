//! Isolated declaration expando-augmentation diagnostics through production emission.

#[test]
fn isolated_expando_augmentation_match_complete_typescript_observations() {
    let artifact: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../fixtures/isolated-declaration-expando-augmentation.json"
    ))
    .expect("frozen TypeScript observations");
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    assert_eq!(artifact["cases"].as_array().unwrap().len(), 27);
    super::h2_7c_declaration_blocking::assert_cases(&artifact);
}

//! Private type names and class/return inference through production emission.

#[test]
fn isolated_private_types_match_complete_typescript_observations() {
    let artifact: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../fixtures/isolated-declaration-private-types.json"
    ))
    .expect("frozen TypeScript observations");
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    assert_eq!(artifact["cases"].as_array().unwrap().len(), 18);
    super::h2_7c_declaration_blocking::assert_cases(&artifact);
}

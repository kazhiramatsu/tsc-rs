
use super::*;

#[test]
fn successor_syntax_pin_is_required_without_changing_census_authority() {
    let original = "a".repeat(40);
    let successor = "b".repeat(40);
    let mut selection = json!({"syntax_tree_hash":original});
    assert_eq!(selected_syntax_tree(&selection).unwrap(), original);
    selection["successor"] = json!({"syntax_tree_hash":successor});
    assert_eq!(selected_syntax_tree(&selection).unwrap(), successor);
    assert_eq!(selection["syntax_tree_hash"], original);
    for invalid in [Value::Null, json!({}), json!({"syntax_tree_hash":"wrong"})] {
        selection["successor"] = invalid;
        assert!(selected_syntax_tree(&selection).is_err());
    }
}

#[test]
fn canonical_input_hash_ignores_nested_object_insertion_order() {
    let first: Value = serde_json::from_str(r#"{"z":[{"y":2,"a":1}],"a":0}"#).unwrap();
    let second: Value = serde_json::from_str(r#"{"a":0,"z":[{"a":1,"y":2}]}"#).unwrap();
    let actual = serde_json::to_vec(&canonical_input(&first).unwrap()).unwrap();
    assert_eq!(actual, br#"{"a":0,"z":[{"a":1,"y":2}]}"#);
    assert_eq!(
        actual,
        serde_json::to_vec(&canonical_input(&second).unwrap()).unwrap()
    );
}

#[test]
fn canonical_input_rejects_float_and_unsafe_integer_encodings() {
    for text in ["1.0", "0.25", "9007199254740992", "-9007199254740992"] {
        assert!(canonical_input(&serde_json::from_str(text).unwrap()).is_err());
    }
}

#[test]
fn matrix_identity_ignores_case_id_and_retains_configuration() {
    let mut input = json!({"route":"recorded-compiler","fixture_path":"a.ts","fixture_blob_sha1":"b","variant":{"key":"target=es5","configuration_index":2},"case_id":"display"});
    let key = input_key(&input).unwrap();
    input["case_id"] = json!("other-display");
    assert_eq!(input_key(&input).unwrap(), key);
    input["variant"]["configuration_index"] = json!(3);
    assert_ne!(input_key(&input).unwrap(), key);
}

#[test]
fn document_identity_and_absent_content_are_distinct() {
    let hash = sha256(b"");
    let pool = documents(&json!({"documents":{hash.clone():""}})).unwrap();
    verify_document_references(
        &json!([{"content_sha256":null},{"content_sha256":hash}]),
        &pool,
    )
    .unwrap();
    assert!(verify_document_references(&json!({"content_sha256":"missing"}), &pool).is_err());
    assert!(documents(&json!({"documents":{"bad-hash":""}})).is_err());
}

#[test]
fn full_loader_verification_rejects_write_order_and_source_drift() {
    let expected = json!({"prepared":{"roots":["a.ts"],"source_files":[{"path":"a.ts","sha256":"x"}],"compiler_options_debug_sha256":"c","program_options_debug_sha256":"p"},"vfs_write_order":[0,1]});
    same_input(&expected, &expected).unwrap();
    let mut actual = expected.clone();
    actual["vfs_write_order"] = json!([1, 0]);
    assert!(same_input(&expected, &actual).is_err());
    actual = expected.clone();
    actual["prepared"]["source_files"][0]["sha256"] = json!("y");
    assert!(same_input(&expected, &actual).is_err());
}

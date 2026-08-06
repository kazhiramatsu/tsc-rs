use std::path::Path;

use serde_json::{json, Value};

use super::{
    decode_user_object_key, json_number_as_f64, json_object_get, json_object_own_get,
    jsonc_prototype, parse_json_object, MAX_PACKAGE_JSON_DEPTH,
};

#[test]
fn strict_and_jsonc_objects_share_the_same_owned_projection() {
    let path = Path::new("/types/pkg/package.json");
    let strict = r#"{"name":"pkg","exports":{".":"./index.d.ts"}}"#.to_owned();
    let (retained, object) = parse_json_object(path, strict.clone());
    assert_eq!(retained, strict);
    assert_eq!(object["name"], json!("pkg"));
    assert_eq!(object["exports"], json!({".": "./index.d.ts"}));

    let jsonc = r#"{/* comment */"typings":null,"nested":[1,-2,{}],}"#.to_owned();
    let (retained, object) = parse_json_object(path, jsonc.clone());
    assert_eq!(retained, jsonc);
    assert_eq!(object["typings"], Value::Null);
    assert_eq!(object["nested"], json!([1.0, -2.0, {}]));
}

#[test]
fn duplicate_keys_are_last_wins_and_jsonc_numbers_are_converted() {
    let (_, object) = parse_json_object(
        Path::new("package.json"),
        r#"{/* fallback */"typings":"first","typings":null,"hex":0x10,"negativeZero":-0}"#
            .to_owned(),
    );
    assert_eq!(object["typings"], Value::Null);
    assert_eq!(object["hex"], json!(16.0));
    let (_, rounded) = parse_json_object(
        Path::new("package.json"),
        r#"{/* fallback */"value":9007199254740993}"#.to_owned(),
    );
    assert_eq!(rounded["value"].as_f64(), Some(9_007_199_254_740_992.0));
    assert!(object["negativeZero"]
        .as_f64()
        .expect("negative zero remains numeric")
        .is_sign_negative());

    let (_, overflowing) = parse_json_object(
        Path::new("package.json"),
        r#"{/* fallback */"positive":1e309,"negative":-1e309}"#.to_owned(),
    );
    assert_eq!(
        overflowing["positive"]
            .as_number()
            .and_then(json_number_as_f64),
        Some(f64::INFINITY)
    );
    assert_eq!(
        overflowing["negative"]
            .as_number()
            .and_then(json_number_as_f64),
        Some(f64::NEG_INFINITY)
    );
}

#[test]
fn jsonc_prototypes_are_inherited_without_colliding_with_strict_user_keys() {
    let path = Path::new("package.json");
    let marker_spelling = "\0tsc-rs:jsonc-prototype\0";
    let (_, strict) = parse_json_object(
        path,
        r#"{
            "\u0000tsc-rs:jsonc-prototype\u0000":"user",
            "__proto__":{"strict":true}
        }"#
        .to_owned(),
    );
    assert_eq!(
        json_object_get(&strict, marker_spelling),
        Some(&json!("user"))
    );
    assert_eq!(
        json_object_get(&strict, "__proto__"),
        Some(&json!({"strict": true}))
    );
    assert!(jsonc_prototype(&strict).is_none());

    let (_, jsonc) = parse_json_object(
        path,
        r#"{/* force convertToJson */
            "__proto__":{"inherited":"yes"},"own":"yes"
        }"#
        .to_owned(),
    );
    assert_eq!(json_object_get(&jsonc, "inherited"), Some(&json!("yes")));
    assert_eq!(json_object_own_get(&jsonc, "inherited"), None);
    assert_eq!(json_object_get(&jsonc, "own"), Some(&json!("yes")));
    assert_eq!(
        jsonc
            .keys()
            .filter_map(|key| decode_user_object_key(key))
            .collect::<Vec<_>>(),
        vec!["own"]
    );

    let (_, array_prototype) = parse_json_object(
        path,
        r#"{/* force convertToJson */"__proto__":["m"]}"#.to_owned(),
    );
    assert_eq!(json_object_get(&array_prototype, "0"), Some(&json!("m")));

    let (_, null_then_data) = parse_json_object(
        path,
        r#"{/* force convertToJson */
            "__proto__":null,"__proto__":{"name":"own"},
        }"#
        .to_owned(),
    );
    assert!(matches!(
        jsonc_prototype(&null_then_data),
        Some(Value::Null)
    ));
    assert_eq!(
        json_object_own_get(&null_then_data, "__proto__"),
        Some(&json!({"name": "own"}))
    );
    assert_eq!(json_object_get(&null_then_data, "name"), None);

    let (_, replaceable_prototype) = parse_json_object(
        path,
        r#"{/* force convertToJson */
            "__proto__":{"name":"first"},
            "__proto__":{"name":"last"},
        }"#
        .to_owned(),
    );
    assert_eq!(
        json_object_get(&replaceable_prototype, "name"),
        Some(&json!("last"))
    );
    assert_eq!(
        json_object_own_get(&replaceable_prototype, "__proto__"),
        None
    );

    for strict_serde_rejection in [
        r#"{"overflow":1e309,"__proto__":{"inherited":"no"}}"#,
        r#"{"surrogate":"\ud800","__proto__":{"inherited":"no"}}"#,
    ] {
        let (_, strict) = parse_json_object(path, strict_serde_rejection.to_owned());
        assert!(jsonc_prototype(&strict).is_none());
        assert!(json_object_own_get(&strict, "__proto__").is_some());
        assert_eq!(json_object_get(&strict, "inherited"), None);
    }
    let (_, strict_overflow) =
        parse_json_object(path, r#"{"overflow":1e309,"__proto__":{}}"#.to_owned());
    assert_eq!(
        strict_overflow["overflow"]
            .as_number()
            .and_then(json_number_as_f64),
        Some(f64::INFINITY)
    );
}

#[test]
fn jsonc_fallback_matches_modifier_and_structural_depth_boundaries() {
    let (_, object) = parse_json_object(
        Path::new("package.json"),
        r#"{readonly "name":"pkg","typings"!:null}"#.to_owned(),
    );
    assert_eq!(object["name"], json!("pkg"));
    assert_eq!(object["typings"], Value::Null);

    let at_limit = format!(
        "{{\"value\":{}0{}}}",
        "[".repeat(MAX_PACKAGE_JSON_DEPTH - 1),
        "]".repeat(MAX_PACKAGE_JSON_DEPTH - 1)
    );
    let (_, object) = parse_json_object(Path::new("package.json"), at_limit);
    assert!(object.contains_key("value"));

    let too_deep = format!(
        "{{\"value\":{}0{}}}",
        "[".repeat(MAX_PACKAGE_JSON_DEPTH),
        "]".repeat(MAX_PACKAGE_JSON_DEPTH)
    );
    let (_, object) = parse_json_object(Path::new("package.json"), too_deep);
    assert!(object.is_empty());

    for invalid in [
        format!(
            "{{\"value\":{}0{}}}",
            "(".repeat(MAX_PACKAGE_JSON_DEPTH + 1),
            ")".repeat(MAX_PACKAGE_JSON_DEPTH + 1)
        ),
        format!("{{\"value\":{}0}}", "!".repeat(MAX_PACKAGE_JSON_DEPTH + 1)),
        format!(
            "[{}0{}",
            "}[".repeat(MAX_PACKAGE_JSON_DEPTH + 1),
            "]".repeat(MAX_PACKAGE_JSON_DEPTH + 2)
        ),
    ] {
        let (_, object) = parse_json_object(Path::new("package.json"), invalid);
        assert!(object.is_empty());
    }
}

#[test]
fn invalid_empty_and_non_object_inputs_expose_an_empty_object() {
    for input in [
        "",
        "null",
        "true",
        "[null]",
        "{typings: null}",
        "{'typings': null}",
        r#"{"typings": null, "x": 'value'}"#,
        r#"{"typings": null, "x": {bad: 1}}"#,
        r#"{"typings": null, "x": undefined}"#,
        r#"{"typings": null, "x": [1,,2]}"#,
        r#"{"typings"?: null}"#,
        r#"{"typings": null"#,
    ] {
        let (_, object) = parse_json_object(Path::new("package.json"), input.to_owned());
        assert!(
            object.is_empty(),
            "input must behave like readJson: {input}"
        );
    }
}

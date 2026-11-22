use tsc_diagnostics::JsStr;

use crate::json_value::JsonValue as Value;

// Keep the existing scalar observations verbatim; construct the lossless
// value type from those scalar JSON literals at this test boundary.
macro_rules! json {
    ($($tokens:tt)*) => {
        Value::from(serde_json::json!($($tokens)*))
    };
}

use super::{
    decode_user_object_key, json_number_as_f64, json_object_get, json_object_own_get,
    jsonc_prototype, parse_json_object, MAX_PACKAGE_JSON_DEPTH,
};

#[test]
fn strict_and_jsonc_objects_share_the_same_owned_projection() {
    let path = JsStr::from("/types/pkg/package.json");
    let strict = r#"{"name":"pkg","exports":{".":"./index.d.ts"}}"#.to_owned();
    let (retained, object) = parse_json_object(path, strict.clone());
    assert_eq!(retained.text(), strict);
    assert_eq!(object["name"], json!("pkg"));
    assert_eq!(object["exports"], json!({".": "./index.d.ts"}));

    let jsonc = r#"{/* comment */"typings":null,"nested":[1,-2,{}],}"#.to_owned();
    let (retained, object) = parse_json_object(path, jsonc.clone());
    assert_eq!(retained.text(), jsonc);
    assert_eq!(object["typings"], Value::Null);
    assert_eq!(object["nested"], json!([1.0, -2.0, {}]));
}

#[test]
fn duplicate_keys_are_last_wins_and_jsonc_numbers_are_converted() {
    let (_, object) = parse_json_object(
        JsStr::from("package.json"),
        r#"{/* fallback */"typings":"first","typings":null,"hex":0x10,"negativeZero":-0}"#
            .to_owned(),
    );
    assert_eq!(object["typings"], Value::Null);
    assert_eq!(object["hex"], json!(16.0));
    let (_, rounded) = parse_json_object(
        JsStr::from("package.json"),
        r#"{/* fallback */"value":9007199254740993}"#.to_owned(),
    );
    assert_eq!(rounded["value"].as_f64(), Some(9_007_199_254_740_992.0));
    assert!(object["negativeZero"]
        .as_f64()
        .expect("negative zero remains numeric")
        .is_sign_negative());

    let (_, overflowing) = parse_json_object(
        JsStr::from("package.json"),
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
    let path = JsStr::from("package.json");
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
            .filter_map(decode_user_object_key)
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
        JsStr::from("package.json"),
        r#"{readonly "name":"pkg","typings"!:null}"#.to_owned(),
    );
    assert_eq!(object["name"], json!("pkg"));
    assert_eq!(object["typings"], Value::Null);

    let at_limit = format!(
        "{{\"value\":{}0{}}}",
        "[".repeat(MAX_PACKAGE_JSON_DEPTH - 1),
        "]".repeat(MAX_PACKAGE_JSON_DEPTH - 1)
    );
    let (_, object) = parse_json_object(JsStr::from("package.json"), at_limit);
    assert!(object.contains_key("value"));

    let too_deep = format!(
        "{{\"value\":{}0{}}}",
        "[".repeat(MAX_PACKAGE_JSON_DEPTH),
        "]".repeat(MAX_PACKAGE_JSON_DEPTH)
    );
    let (_, object) = parse_json_object(JsStr::from("package.json"), too_deep);
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
        let (_, object) = parse_json_object(JsStr::from("package.json"), invalid);
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
        let (_, object) = parse_json_object(JsStr::from("package.json"), input.to_owned());
        assert!(
            object.is_empty(),
            "input must behave like readJson: {input}"
        );
    }
}

fn observed_value(value: &Value) -> serde_json::Value {
    use crate::json_value::append_json_quoted;
    match value {
        Value::Null => serde_json::json!(["null"]),
        Value::Bool(value) => serde_json::json!(["boolean", value]),
        Value::Number(number) => {
            let value = json_number_as_f64(number).expect("JSON number");
            let text = if value == f64::INFINITY {
                "Infinity".to_owned()
            } else if value == f64::NEG_INFINITY {
                "-Infinity".to_owned()
            } else if value == 0.0 {
                "0".to_owned()
            } else {
                value.to_string()
            };
            serde_json::json!(["number", text])
        }
        Value::String(value) => {
            let mut quoted = String::new();
            append_json_quoted(value.as_js(), &mut quoted);
            serde_json::json!(["string", value.to_utf16(), quoted])
        }
        Value::Array(values) => {
            serde_json::json!([
                "array",
                values.iter().map(observed_value).collect::<Vec<_>>()
            ])
        }
        Value::Object(object) => {
            let mut entries = object
                .iter()
                .filter_map(|(key, value)| Some((decode_user_object_key(key)?, value)))
                .collect::<Vec<_>>();
            // Object.keys enumerates canonical decimal indices first. The
            // numeric parser's scalar check is sound for that ASCII domain.
            entries.sort_by_cached_key(|(key, _)| {
                key.as_str()
                    .and_then(|text| text.parse::<u32>().ok())
                    .filter(|index| *index != u32::MAX && *key == index.to_string().as_str())
                    .map(|index| (0, index))
                    .unwrap_or((1, 0))
            });
            let entries = entries
                .into_iter()
                .map(|(key, value)| {
                    // The query uses canonical values, including lone surrogates
                    // and the escaped user spelling of the prototype marker.
                    assert_eq!(json_object_own_get(object, key), Some(value));
                    serde_json::json!([key.to_utf16(), observed_value(value)])
                })
                .collect::<Vec<_>>();
            let prototype = match jsonc_prototype(object) {
                None => serde_json::json!("default"),
                Some(Value::Null) => serde_json::json!("null"),
                Some(value) => observed_value(value),
            };
            serde_json::json!(["object", entries, prototype])
        }
    }
}

#[test]
fn utf16_json_values_match_typescript_observations() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../fixtures/utf16-json-values.json")).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 8);
    for case in cases {
        let text = case["source"].as_str().unwrap();
        let (snapshot, object) = parse_json_object(JsStr::from("package.json"), text.to_owned());
        assert_eq!(snapshot.text(), text);
        assert_eq!(
            observed_value(&Value::Object(object)),
            case["expected"],
            "{}",
            case["id"]
        );
    }
}

#[test]
fn json_string_output_preserves_units_and_matches_well_formed_stringify() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../fixtures/utf16-json-values.json")).unwrap();
    for case in fixture["quoting"].as_array().unwrap() {
        let units = case["units"]
            .as_array()
            .unwrap()
            .iter()
            .map(|unit| u16::try_from(unit.as_u64().unwrap()).unwrap())
            .collect::<Vec<_>>();
        let value = tsc_diagnostics::JsString::from_code_units(&units);
        let mut quoted = String::new();
        crate::json_value::append_json_quoted(value.as_js(), &mut quoted);
        assert_eq!(quoted, case["json"].as_str().unwrap());
        // Read the serialized value through the actual program JSON parser:
        // serde itself cannot decode this round trip for non-scalar strings.
        let (_, object) = parse_json_object(
            JsStr::from("package.json"),
            format!("{{\"value\":{quoted}}}"),
        );
        assert_eq!(object["value"].as_js().unwrap().to_utf16(), units);
    }
}

#[test]
fn canonical_object_queries_keep_distinct_names_after_removal_and_reinsertion() {
    use crate::json_value::JsonObject;
    use tsc_diagnostics::JsString;
    let names = [
        vec![0xd800],
        vec![0xd801],
        vec![0xdc00],
        vec![0xfffd],
        vec![0xd800, 0xdc00],
    ]
    .map(|units| JsString::from_code_units(&units));
    let mut object = JsonObject::new();
    for name in &names {
        object.insert(name.clone(), Value::String(name.clone()));
    }
    assert_eq!(
        object.remove(&names[1]),
        Some(Value::String(names[1].clone()))
    );
    assert!(!object.contains_key(&names[1]));
    for name in [&names[0], &names[2], &names[3], &names[4]] {
        assert_eq!(object.get(name), Some(&Value::String(name.clone())));
    }
    object.insert(names[1].clone(), Value::String(names[1].clone()));
    assert_eq!(
        object.keys().collect::<Vec<_>>(),
        vec![&names[0], &names[2], &names[3], &names[4], &names[1]]
    );
    // Scalar and canonical UTF-16 spellings of the pair query the same entry.
    assert_eq!(object.get("\u{10000}"), object.get(&names[4]));
}

fn package_accessor_observations() -> serde_json::Value {
    serde_json::from_str(include_str!("../../fixtures/utf16-package-accessors.json"))
        .expect("immutable TypeScript package accessor observations")
}

fn observed_js_units(value: &serde_json::Value) -> Vec<u16> {
    value
        .as_array()
        .expect("UTF-16 unit array")
        .iter()
        .map(|unit| u16::try_from(unit.as_u64().expect("integer unit")).expect("u16 unit"))
        .collect()
}

#[test]
fn package_string_properties_and_own_entry_order_match_typescript() {
    let fixture = package_accessor_observations();
    for case in fixture["packages"].as_array().expect("package cases") {
        let text = case["text"].as_str().expect("manifest text");
        let snapshot =
            tsc_diagnostics::TextSnapshot::new(text, tsc_diagnostics::DocumentVersion::default());
        let object =
            crate::read_package_json_object_from_snapshot("/pkg/package.json".into(), &snapshot);
        assert_eq!(
            snapshot.text(),
            text,
            "original readFile text remains owned"
        );
        let actual_entries: Vec<_> = crate::package_json_own_entries(&object)
            .into_iter()
            .map(|(key, value)| (key.to_utf16(), value.as_js().map(JsStr::to_utf16)))
            .collect();
        let expected_entries: Vec<_> = case["entries"]
            .as_array()
            .expect("entries")
            .iter()
            .map(|entry| {
                (
                    observed_js_units(&entry["key"]),
                    entry["value"].get("units").map(observed_js_units),
                )
            })
            .collect();
        assert_eq!(
            actual_entries, expected_entries,
            "own entry order for {text:?}"
        );
        let value = Value::Object(object);
        for property in case["properties"].as_array().expect("properties") {
            let key =
                tsc_diagnostics::JsString::from_code_units(&observed_js_units(&property["key"]));
            // Package consumers ask for string fields; this does not assert
            // general Object.prototype reflection or callable builtin values.
            for (label, actual) in [
                (
                    "inherited",
                    crate::package_json_property(&value, key.as_js()),
                ),
                (
                    "own",
                    crate::package_json_own_property(value.as_object().unwrap(), key.as_js()),
                ),
            ] {
                assert_eq!(
                    actual.and_then(Value::as_js).map(JsStr::to_utf16),
                    property[label].get("units").map(observed_js_units),
                    "{label} string query {key:?} for {text:?}"
                );
            }
        }
    }
}

#[test]
fn scoped_package_name_mangling_retains_typescript_units() {
    for case in package_accessor_observations()["scoped_names"]
        .as_array()
        .expect("scoped cases")
    {
        let name = tsc_diagnostics::JsString::from_code_units(&observed_js_units(&case["name"]));
        assert_eq!(
            crate::mangle_scoped_package_name(name.as_js()).to_utf16(),
            observed_js_units(&case["mangled"])
        );
    }
}

#[test]
fn shared_filename_case_profile_retains_typescript_units() {
    for case in package_accessor_observations()["file_names"]
        .as_array()
        .expect("filename cases")
    {
        let name = tsc_diagnostics::JsString::from_code_units(&observed_js_units(&case["name"]));
        assert_eq!(
            crate::to_file_name_lower_case_js(name.as_js()).to_utf16(),
            observed_js_units(&case["folded"])
        );
    }
}

#[test]
fn strict_package_values_preserve_member_order_and_number_bits() {
    let text = r#"{"exports":[{"default":"first","\u0000x":-0,"__proto__":{"n":9007199254740993},"default":"last"}]}"#;
    let (snapshot, root) = parse_json_object("package.json".into(), text.into());
    assert_eq!(snapshot.text(), text);
    let exports = root["exports"][0].as_object().unwrap();
    assert_eq!(
        exports
            .keys()
            .filter_map(decode_user_object_key)
            .collect::<Vec<_>>(),
        ["default", "\0x", "__proto__"]
    );
    assert_eq!(exports["default"], json!("last"));
    assert_eq!(
        json_object_get(exports, "\0x")
            .unwrap()
            .as_f64()
            .unwrap()
            .to_bits(),
        (-0.0f64).to_bits()
    );
    assert_eq!(
        exports["__proto__"]["n"].as_f64(),
        Some(9_007_199_254_740_992.0)
    );
    assert!(jsonc_prototype(exports).is_none());
}

#[test]
fn package_resolution_observes_duplicate_export_conditions_in_place() {
    use crate::{CompilerOptions, ModuleResolver, ResolutionMode, ResolutionOutcome};
    use tsc_host::MemoryCompilerHost;
    let options = CompilerOptions {
        module: Some(199),
        module_resolution: Some(99),
        ..CompilerOptions::default()
    };
    for extra in [
        r#""extra":0"#,
        r#""extra":"\ud800""#,
        r#"/* JSONC */ "extra":0"#,
    ] {
        // Replacing the first 'types' keeps its slot before 'default'.
        let manifest = format!(
            r#"{{{extra},"name":"pkg","version":"1","exports":{{".":{{
            "\u0000types":"./bad.d.ts","types":"./bad.d.ts","default":"./bad.d.ts","types":"./good.d.ts"
        }}}}}}"#
        );
        let host = MemoryCompilerHost::builder("/work")
            .file("/work/node_modules/pkg/package.json", manifest.into_bytes())
            .file("/work/node_modules/pkg/bad.d.ts", b"export {};".to_vec())
            .file("/work/node_modules/pkg/good.d.ts", b"export {};".to_vec())
            .build()
            .unwrap();
        let result = ModuleResolver::new(&host, &options)
            .unwrap()
            .resolve("/work/main.cts", "pkg", ResolutionMode::CommonJs)
            .unwrap();
        let ResolutionOutcome::Resolved(module) = result else {
            panic!("expected package target")
        };
        assert_eq!(
            module.resolved_file().display(),
            "/work/node_modules/pkg/good.d.ts"
        );
        assert_eq!(module.package_id().unwrap().submodule_name(), "good.d.ts");
    }
}

#[test]
fn package_readers_match_the_original_object_and_peer_version() {
    use super::{
        read_package_json_fallback, read_package_json_object_from_snapshot, read_peer_version,
    };
    use tsc_diagnostics::{DocumentVersion, TextSnapshot};

    fn compare(left: &Value, right: &Value) {
        match (left, right) {
            (Value::Object(left), Value::Object(right)) => {
                assert!(left.keys().eq(right.keys()));
                for (a, b) in left.values().zip(right.values()) {
                    compare(a, b);
                }
            }
            (Value::Array(left), Value::Array(right)) => {
                assert_eq!(left.len(), right.len());
                assert!(left.capacity() <= right.capacity());
                for (a, b) in left.iter().zip(right) {
                    compare(a, b);
                }
            }
            (Value::Number(a), Value::Number(b)) => {
                assert_eq!(
                    json_number_as_f64(a).map(f64::to_bits),
                    json_number_as_f64(b).map(f64::to_bits)
                );
            }
            _ => assert_eq!(left, right),
        }
    }

    let path = JsStr::from("package.json");
    for input in [
        r#"{"version":false,"exports":{"default":-0,"x":[1.5,9007199254740993],"default":-0.0},"\u0076ersion":"last","\u0000x":"y"}"#,
        r#"{"version":"first","version":{"version":"nested"}}"#,
        r#"{"version":"1","unknown":[true,{"a":}]}"#,
        r#"{"version":"1","unknown":1e999,"__proto__":{"inherited":true}}"#,
        r#"{"version":"\ud800","\udc00":[null]}"#,
        r#"{"version":"1","unknown":"\ud800"}"#,
        r#"{"\u0024serde_json::private::RawValue":"{\"version\":\"raw\"}"}"#,
        r#"{"version":{"$serde_json::private::RawValue":"\"inside\""},"x":true}"#,
        r#"{"version":"1","unknown":[{"$serde_json::private::RawValue":"bad"}]}"#,
        r#"{"version":"1","$serde_json::private::RawValue":true}"#,
        r#"{/* inherited */ "__proto__":{"version":"from prototype"},}"#,
        r#"{"version":"1"} trailing"#,
        r#"["version","ignored"]"#,
        r#""not an object""#,
    ] {
        let snapshot = TextSnapshot::new(input.to_owned(), DocumentVersion::default());
        if !input.contains("RawValue") {
            let strict = serde_json::from_str::<serde_json::Value>(input).is_ok();
            for dest in [
                super::PackageDestination::Peer,
                super::PackageDestination::Object,
            ] {
                assert_eq!(dest.read(input).is_ok(), strict, "{input}");
            }
        }
        let expected = read_package_json_fallback(path, &snapshot);
        let actual = read_package_json_object_from_snapshot(path, &snapshot);
        assert_eq!(
            read_peer_version(path, &snapshot)
                .as_ref()
                .map(|s| s.as_js()),
            json_object_get(&expected, "version").and_then(Value::as_js),
            "{input}"
        );
        compare(&Value::Object(actual), &Value::Object(expected));
    }
}

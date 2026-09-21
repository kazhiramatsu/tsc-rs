use super::*;

#[test]
fn module_basenames_match_typescript_values_and_root_boundaries() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../fixtures/utf16-generated-module-names.json"
    ))
    .unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        assert_eq!(
            base_file_name(value(&case["value_utf16"]).as_js()),
            value(&case["expected"]["base_name_utf16"]),
            "{}",
            case["case_id"]
        );
    }
}

#[test]
fn file_option_value_phases_match_repeated_typescript_config_observations() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../fixtures/utf16-config-path-values.json")).unwrap();
    assert_eq!(fixture["repetitions"], 2);
    for case in fixture["cases"].as_array().unwrap() {
        let input = value(&case["value_utf16"]);
        let base = case["base"].as_str().unwrap();
        let written = normalize_slashes(input.as_js());
        let converted = if starts_with_config_dir_template(&written) {
            written
        } else {
            normalized_config_value_path(written.as_js(), base)
        };
        let finalized =
            normalized_config_dir_value_path(converted.as_js(), base).unwrap_or(converted);
        assert_eq!(
            finalized,
            value(&case["out_dir_utf16"]),
            "{}",
            case["case_id"]
        );
    }
}
fn value(units: &serde_json::Value) -> JsString {
    JsString::from_code_units(
        &units
            .as_array()
            .unwrap()
            .iter()
            .map(|unit| unit.as_u64().unwrap() as u16)
            .collect::<Vec<_>>(),
    )
}
#[test]
fn lexical_paths_match_repeated_typescript_values() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../fixtures/utf16-lexical-paths.json")).unwrap();
    assert_eq!(fixture["repetitions"], 2);
    for case in fixture["cases"].as_array().unwrap() {
        let path = value(&case["path_utf16"]);
        let base = value(&case["base_utf16"]);
        let root_length = prefix(path.as_js(), root_end_byte(path.as_js())).len_units();
        assert_eq!(
            root_length,
            case["root_length"].as_u64().unwrap() as usize,
            "root {}",
            case["case_id"]
        );
        assert_eq!(
            normalized_absolute_path(path.as_js(), base.as_js()),
            value(&case["normalized_utf16"]),
            "normalize {}",
            case["case_id"]
        );
        assert_eq!(
            combine_paths(base.as_js(), path.as_js()),
            value(&case["combined_utf16"]),
            "combine {}",
            case["case_id"]
        );
    }
}

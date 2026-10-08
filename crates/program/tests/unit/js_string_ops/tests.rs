use super::*;

#[test]
fn types_package_names_match_repeated_typescript_observations() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../fixtures/utf16-package-names.json")).unwrap();
    assert_eq!(fixture["repetitions"], 2);
    for case in fixture["cases"].as_array().unwrap() {
        assert_eq!(
            types_package_name(&value(&case["name_utf16"])),
            value(&case["types_package_utf16"]),
            "{}",
            case["case_id"],
        );
    }
}

fn value(units: &serde_json::Value) -> JsString {
    units
        .as_array()
        .unwrap()
        .iter()
        .map(|unit| u16::try_from(unit.as_u64().unwrap()).unwrap())
        .collect()
}

#[test]
fn value_replacements_match_javascript() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../fixtures/utf16-string-replacement.json")).unwrap();
    assert_eq!(fixture["cases"].as_array().unwrap().len(), 14);
    for case in fixture["cases"].as_array().unwrap() {
        let target = value(&case["target"]);
        let replacement = value(&case["replacement"]);
        assert_eq!(
            replace_first_star_value(&target, &replacement),
            value(&case["first"]),
            "{}: value first",
            case["id"]
        );
        assert_eq!(
            replace_all_stars_value(&target, &replacement),
            value(&case["all"]),
            "{}: value all",
            case["id"]
        );
    }
}

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
fn replacements_and_canonical_allocation_lengths_match_javascript() {
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
        for (name, actual) in [
            (
                "first",
                js_replace_first_star(&target, &replacement).unwrap(),
            ),
            ("all", js_replace_all_stars(&target, &replacement).unwrap()),
        ] {
            assert_eq!(actual, value(&case[name]), "{}: {name}", case["id"]);
            assert_eq!(
                actual.as_bytes().len() as u64,
                case[format!("{name}_bytes")].as_u64().unwrap()
            );
        }
    }
}

#[test]
fn expansion_stays_within_the_existing_native_allocation_budget() {
    use crate::resolution_error::ResolutionErrorKind;
    let scalar = "*".repeat(4096);
    assert_eq!(
        js_replace_all_stars(&scalar, "$`").unwrap_err().kind(),
        ResolutionErrorKind::ResourceLimit
    );
    let mut non_scalar = JsString::from_code_units(&[0xd800]);
    non_scalar.push_str(&scalar);
    assert_eq!(
        js_replace_all_stars(&non_scalar, "$`").unwrap_err().kind(),
        ResolutionErrorKind::ResourceLimit
    );
}

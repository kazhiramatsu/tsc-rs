use super::*;
use serde_json::Value;

#[test]
fn lossless_option_names_match_exact_lookup_and_spelling_observations() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../fixtures/utf16-config-names.json")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let units = case["units"]
            .as_array()
            .unwrap()
            .iter()
            .map(|unit| u16::try_from(unit.as_u64().unwrap()).unwrap())
            .collect::<Vec<_>>();
        let name = tsc_diagnostics::JsString::from_code_units(&units);
        assert_eq!(
            compiler_option_declaration(&name).map(|declaration| declaration.name()),
            case["exact"].as_str(),
            "exact lookup: {units:x?}"
        );
        assert_eq!(
            compiler_option_spelling_suggestion(&name).map(|declaration| declaration.name()),
            case["suggestion"].as_str(),
            "suggestion: {units:x?}"
        );
    }
}

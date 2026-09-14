use tsc_diagnostics::JsString;
use tsc_syntax::{is_entity_name_js_text, is_identifier_text_for_target};
use tsc_types::ScriptTarget;

#[test]
fn entity_and_identifier_predicates_match_typescript_utf16_values() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/utf16-entity-names.json")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let units: Vec<u16> = case["value_utf16"]
            .as_array()
            .unwrap()
            .iter()
            .map(|unit| u16::try_from(unit.as_u64().unwrap()).unwrap())
            .collect();
        let value = JsString::from_code_units(&units);
        let target = ScriptTarget::from_bits(case["target"].as_i64().unwrap() as i32);
        assert_eq!(
            is_entity_name_js_text(value.as_js(), target),
            case["expected"]["entity"].as_bool().unwrap(),
            "{} {:?}",
            case["case_id"],
            value
        );
        assert_eq!(
            value
                .as_str()
                .is_some_and(|text| is_identifier_text_for_target(text, target)),
            case["expected"]["identifier"].as_bool().unwrap(),
            "{} {:?}",
            case["case_id"],
            value
        );
        assert_eq!(
            value.to_utf16(),
            units,
            "predicate must not change the diagnostic value"
        );
    }
}

#[test]
fn isolated_entity_components_keep_identifier_values_after_comment_and_escape_parsing() {
    // TypeScript JSX emit uses R.createElement for both forms (the option
    // value itself remains unchanged for diagnostics).
    let mut commented = JsString::from("R/*");
    commented.push_code_unit(0xd800);
    commented.push_str("*/.createElement");
    for value in [commented, JsString::from(r"R.\u0063reateElement")] {
        assert_eq!(
            tsc_syntax::parse_entity_name_components(value.as_js(), ScriptTarget::ES2015),
            Some(vec!["R".into(), "createElement".into()]),
        );
    }
    let invalid = JsString::from_code_units(&[82, 0xd800]);
    assert!(
        tsc_syntax::parse_entity_name_components(invalid.as_js(), ScriptTarget::ES2015).is_none()
    );
}

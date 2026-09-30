use super::*;
use crate::InternalSymbolName;
use std::collections::HashMap;

#[test]
fn escape_round_trip_for_all_units_and_underscore_prefixes() {
    for unit in 0..=u16::MAX {
        for prefix in ["", "_", "__", "___", "____"] {
            let mut raw = JsString::from(prefix);
            raw.push_code_unit(unit);
            assert_eq!(EscapedName::escape(raw.as_js()).unescape(), raw.as_js());
        }
    }
}

#[test]
fn internal_names_and_distinct_surrogates_have_distinct_keys() {
    let internal = EscapedName::internal(InternalSymbolName::CALL);
    let user = EscapedName::escape(JsStr::from_str("__call"));
    assert_ne!(internal, user);
    assert_eq!(user.as_str(), Some("___call"));
    let values = [[0xD800], [0xD801], [0xDC00], [0xFFFD]];
    let mut table = HashMap::new();
    for (i, units) in values.iter().enumerate() {
        table.insert(
            EscapedName::escape(JsString::from_code_units(units).as_js()),
            i,
        );
    }
    for (i, units) in values.iter().enumerate() {
        let query = EscapedName::escape(JsString::from_code_units(units).as_js());
        assert_eq!(table.get(&query), Some(&i));
    }
    table.insert(internal, 4);
    table.insert(user, 5);
    assert_eq!(table.get(&EscapedName::internal("__call")), Some(&4));
    assert_eq!(
        table.get(&EscapedName::escape(JsStr::from_str("__call"))),
        Some(&5)
    );
}

#[test]
fn byte_order_and_explicit_js_order_remain_distinct() {
    let bmp = EscapedName::escape(JsStr::from_str("\u{E000}"));
    let supplementary = EscapedName::escape(JsStr::from_str("\u{10000}"));
    assert!(bmp < supplementary);
    assert_eq!(bmp.cmp_utf16(&supplementary), Ordering::Greater);
    let bmp_bytes = bmp.as_js().as_bytes();
    let supplementary_bytes = supplementary.as_js().as_bytes();
    assert_eq!(bmp.cmp(&supplementary), bmp_bytes.cmp(supplementary_bytes));
}

#[test]
fn template_values_bridge_without_losing_surrogates() {
    let units = [0xD800, 0x41, 0xDC00, 0xD83D, 0xDE00, 0xFFFD];
    let literal = crate::TemplateText::from_utf16(&units);
    let value = literal.to_js_string();
    assert_eq!(value.to_utf16(), units);
    assert_eq!(crate::TemplateText::from_js(value.as_js()), literal);
    let key = EscapedName::escape(value.as_js());
    assert_eq!(crate::TemplateText::from_js(key.unescape()), literal);
}

#[test]
fn scanner_values_and_template_units_share_literal_type_identity() {
    let mut tables = crate::TypeTables::new(false, false);
    let mut ids = Vec::new();
    for units in [
        &[0xD800][..],
        &[0xD801],
        &[0xDC00],
        &[0xFFFD],
        &[0xD83D, 0xDE00],
    ] {
        let value = JsString::from_code_units(units);
        let id = tables.get_string_literal_type(value.as_js());
        assert_eq!(tables.get_string_literal_type_from_utf16(units), id);
        if let Some(scalar) = value.as_str() {
            assert_eq!(tables.get_string_literal_type(scalar), id);
        }
        assert!(!ids.contains(&id));
        ids.push(id);
    }
    assert_eq!(tables.get_string_literal_type("😀"), ids[4]);
    assert_ne!(tables.get_string_literal_type("\\uD800"), ids[0]);
}

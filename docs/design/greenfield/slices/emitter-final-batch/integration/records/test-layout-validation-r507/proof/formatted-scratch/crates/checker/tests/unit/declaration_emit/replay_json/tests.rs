
use super::{JsonWireText, Map, Value};
use crate::declaration_emit::replay_json;
use tsc_types::JsString;

#[test]
fn nested_replay_values_keep_units_until_json_encoding() {
    let high = JsString::from_code_units(&[0xd800]);
    let other = JsString::from_code_units(&[0xd801]);
    let pair = JsString::from_code_units(&[0xd800, 0xdc00]);
    let absent: Option<JsString> = None;
    let tree = json!({
        "name": high,
        "node": [0, 1, -1, 17],
        "extra": {"values": [other, pair, absent], "enabled": true},
    });
    assert_eq!(tree["name"].as_js().unwrap().to_utf16(), [0xd800]);
    assert_eq!(
        tree["extra"]["values"][1].as_js().unwrap().to_utf16(),
        [0xd800, 0xdc00]
    );
    // Direct JSON.stringify observation, with the same object/array shape.
    assert_eq!(tree.json_text(), "{\"name\":\"\\ud800\",\"node\":[0,1,-1,17],\"extra\":{\"values\":[\"\\ud801\",\"𐀀\",null],\"enabled\":true}}");
}

#[test]
fn replay_keys_distinguish_lone_units_and_literal_escape_spellings() {
    let high = JsString::from_code_units(&[0xd800]);
    let other = JsString::from_code_units(&[0xd801]);
    let mut object = Map::new();
    object.insert(high.clone(), json!("first"));
    object.insert(other.clone(), json!("second"));
    object.insert("\\ud800", json!("literal"));
    assert_eq!(object.len(), 3);
    assert_eq!(
        object.get(high.as_js()).unwrap().as_js(),
        Some("first".into())
    );
    assert_eq!(
        object.get(other.as_js()).unwrap().as_js(),
        Some("second".into())
    );
    assert_eq!(
        Value::Object(object).json_text(),
        r#"{"\ud800":"first","\ud801":"second","\\ud800":"literal"}"#
    );
}

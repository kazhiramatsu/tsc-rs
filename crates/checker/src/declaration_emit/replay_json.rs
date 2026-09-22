//! Replay JSON owns JavaScript strings. Conversion to JSON text is an explicit
//! wire boundary; no compiler name receives an implicit serde/Display projection.

pub(super) use tsc_program::{JsonObject as Map, JsonValue as Value};
use tsc_types::{JsStr, JsString};

pub(super) trait JsonPart {
    fn json_part(&self) -> Value;
}
impl<T: JsonPart + ?Sized> JsonPart for &T {
    fn json_part(&self) -> Value {
        (*self).json_part()
    }
}
impl JsonPart for Value {
    fn json_part(&self) -> Value {
        self.clone()
    }
}
impl JsonPart for Map {
    fn json_part(&self) -> Value {
        Value::Object(self.clone())
    }
}
impl JsonPart for serde_json::Value {
    fn json_part(&self) -> Value {
        self.clone().into()
    }
}
impl JsonPart for str {
    fn json_part(&self) -> Value {
        Value::String(self.into())
    }
}
impl JsonPart for String {
    fn json_part(&self) -> Value {
        self.as_str().json_part()
    }
}
impl JsonPart for JsString {
    fn json_part(&self) -> Value {
        Value::String(self.clone())
    }
}
impl JsonPart for JsStr<'_> {
    fn json_part(&self) -> Value {
        Value::String((*self).to_owned())
    }
}
impl JsonPart for bool {
    fn json_part(&self) -> Value {
        Value::Bool(*self)
    }
}
impl JsonPart for serde_json::Number {
    fn json_part(&self) -> Value {
        Value::Number(self.clone())
    }
}
impl<T: JsonPart> JsonPart for Option<T> {
    fn json_part(&self) -> Value {
        self.as_ref().map_or(Value::Null, JsonPart::json_part)
    }
}
impl<T: JsonPart> JsonPart for [T] {
    fn json_part(&self) -> Value {
        Value::Array(self.iter().map(JsonPart::json_part).collect())
    }
}
impl<T: JsonPart> JsonPart for Vec<T> {
    fn json_part(&self) -> Value {
        self.as_slice().json_part()
    }
}
impl<T: JsonPart, const N: usize> JsonPart for [T; N] {
    fn json_part(&self) -> Value {
        self.as_slice().json_part()
    }
}
macro_rules! integers {
    ($($ty:ty),*) => { $(impl JsonPart for $ty {
        fn json_part(&self) -> Value { Value::Number((*self).into()) }
    })* };
}
integers!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize);

pub(super) fn value(part: &impl JsonPart) -> Value {
    part.json_part()
}

// JSON structure uses literal field names; expressions at leaves must have an
// explicit JsonPart implementation. Arbitrary structs cannot silently serialize.
macro_rules! json {
    (@array [$($done:expr,)*]) => { vec![$($done,)*] };
    (@array [$($done:expr,)*] , $($rest:tt)*) => { json!(@array [$($done,)*] $($rest)*) };
    (@array [$($done:expr,)*] null, $($rest:tt)*) => { json!(@array [$($done,)* replay_json::Value::Null,] $($rest)*) };
    (@array [$($done:expr,)*] [$($nested:tt)*], $($rest:tt)*) => { json!(@array [$($done,)* json!([$($nested)*]),] $($rest)*) };
    (@array [$($done:expr,)*] {$($nested:tt)*}, $($rest:tt)*) => { json!(@array [$($done,)* json!({$($nested)*}),] $($rest)*) };
    (@array [$($done:expr,)*] $next:expr, $($rest:tt)*) => { json!(@array [$($done,)* replay_json::value(&$next),] $($rest)*) };
    (@object $object:ident,) => {};
    (@object $object:ident, , $($rest:tt)*) => { json!(@object $object, $($rest)*); };
    (@object $object:ident, $key:tt : null, $($rest:tt)*) => { $object.insert($key, replay_json::Value::Null); json!(@object $object, $($rest)*); };
    (@object $object:ident, $key:tt : [$($nested:tt)*], $($rest:tt)*) => { $object.insert($key, json!([$($nested)*])); json!(@object $object, $($rest)*); };
    (@object $object:ident, $key:tt : {$($nested:tt)*}, $($rest:tt)*) => { $object.insert($key, json!({$($nested)*})); json!(@object $object, $($rest)*); };
    (@object $object:ident, $key:tt : $next:expr, $($rest:tt)*) => { $object.insert($key, replay_json::value(&$next)); json!(@object $object, $($rest)*); };
    (null) => { replay_json::Value::Null };
    ([$($tokens:tt)*]) => { replay_json::Value::Array(json!(@array [] $($tokens)* ,)) };
    ({$($tokens:tt)*}) => {{ let mut object = replay_json::Map::new(); json!(@object object, $($tokens)* ,); replay_json::Value::Object(object) }};
    ($value:expr) => { replay_json::value(&$value) };
}
pub(super) use json;

/// Encode the same JSON wire schema with well-formed string escaping. The
/// replay owner uses ordinary object keys, never the package JSONC metadata.
pub(super) fn to_json_text(value: &Value) -> String {
    fn append(value: &Value, text: &mut String) {
        match value {
            Value::Null => text.push_str("null"),
            Value::Bool(value) => text.push_str(if *value { "true" } else { "false" }),
            Value::Number(value) => text.push_str(&value.to_string()),
            Value::String(value) => tsc_program::append_json_quoted(value.as_js(), text),
            Value::Array(values) => {
                text.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        text.push(',');
                    }
                    append(value, text);
                }
                text.push(']');
            }
            Value::Object(values) => {
                text.push('{');
                for (index, (key, value)) in values.iter().enumerate() {
                    if index != 0 {
                        text.push(',');
                    }
                    tsc_program::append_json_quoted(key.as_js(), text);
                    text.push(':');
                    append(value, text);
                }
                text.push('}');
            }
        }
    }
    let mut text = String::new();
    append(value, &mut text);
    text
}

/// Explicit JSON text for replay comparison keys and mismatch reports.
pub(super) trait JsonWireText {
    fn json_text(&self) -> String;
}
impl JsonWireText for Value {
    fn json_text(&self) -> String {
        to_json_text(self)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/declaration_emit/replay_json/tests.rs"]
mod tests;

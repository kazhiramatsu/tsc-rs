//! JSON values retain JavaScript strings until an explicit output boundary.
//!
//! `serde_json::Value` cannot represent an unpaired surrogate in either a
//! string or an object key. It is used only for the scalar strict-JSON fast
//! path, then converted here. The syntax-based converter constructs this same
//! representation directly, including values which JSON.parse accepts but
//! serde rejects. Object queries accept canonical views, never arbitrary bytes.

use std::collections::HashMap;
use std::fmt::Write;
use std::ops::Index;

use serde_json::Number;
use tsc_diagnostics::{JsStr, JsString};

#[derive(Clone, Debug, Default, PartialEq)]
pub enum JsonValue {
    #[default]
    Null,
    Bool(bool),
    Number(Number),
    String(JsString),
    Array(Vec<JsonValue>),
    Object(JsonObject),
}

/// Own string properties in insertion order. Numeric-first JavaScript
/// enumeration remains an explicit operation at the existing callers.
#[derive(Clone, Debug, Default)]
pub struct JsonObject {
    entries: Vec<(JsString, JsonValue)>,
    indices: HashMap<JsString, usize>,
}

impl PartialEq for JsonObject {
    fn eq(&self, other: &Self) -> bool {
        // Like serde's map equality, object equality ignores insertion order.
        self.len() == other.len()
            && self
                .iter()
                .all(|(key, value)| other.get(key) == Some(value))
    }
}

impl JsonObject {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get<'n>(&self, name: impl Into<JsStr<'n>>) -> Option<&JsonValue> {
        self.indices
            .get(name.into().as_bytes())
            .map(|&index| &self.entries[index].1)
    }

    pub fn get_mut<'n>(&mut self, name: impl Into<JsStr<'n>>) -> Option<&mut JsonValue> {
        let index = *self.indices.get(name.into().as_bytes())?;
        Some(&mut self.entries[index].1)
    }

    pub fn contains_key<'n>(&self, name: impl Into<JsStr<'n>>) -> bool {
        self.indices.contains_key(name.into().as_bytes())
    }

    pub fn insert(&mut self, name: impl Into<JsString>, value: JsonValue) -> Option<JsonValue> {
        let name = name.into();
        if let Some(&index) = self.indices.get(name.as_bytes()) {
            return Some(std::mem::replace(&mut self.entries[index].1, value));
        }
        self.indices.insert(name.clone(), self.entries.len());
        self.entries.push((name, value));
        None
    }

    pub fn remove<'n>(&mut self, name: impl Into<JsStr<'n>>) -> Option<JsonValue> {
        let index = self.indices.remove(name.into().as_bytes())?;
        let (_, value) = self.entries.remove(index);
        for (index, (key, _)) in self.entries.iter().enumerate().skip(index) {
            *self
                .indices
                .get_mut(key.as_bytes())
                .expect("remaining entry has an index") = index;
        }
        Some(value)
    }

    pub fn iter(
        &self,
    ) -> impl DoubleEndedIterator<Item = (&JsString, &JsonValue)> + ExactSizeIterator + Clone {
        self.entries.iter().map(|(key, value)| (key, value))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&JsString, &mut JsonValue)> {
        self.entries.iter_mut().map(|(key, value)| (&*key, value))
    }

    pub fn keys(&self) -> impl DoubleEndedIterator<Item = &JsString> + ExactSizeIterator + Clone {
        self.entries.iter().map(|(key, _)| key)
    }

    pub fn values(
        &self,
    ) -> impl DoubleEndedIterator<Item = &JsonValue> + ExactSizeIterator + Clone {
        self.entries.iter().map(|(_, value)| value)
    }
}

impl IntoIterator for JsonObject {
    type Item = (JsString, JsonValue);
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

impl<'a> IntoIterator for &'a JsonObject {
    type Item = (&'a JsString, &'a JsonValue);
    type IntoIter = std::iter::Map<
        std::slice::Iter<'a, (JsString, JsonValue)>,
        fn(&'a (JsString, JsonValue)) -> (&'a JsString, &'a JsonValue),
    >;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter().map(|(key, value)| (key, value))
    }
}

impl<K: Into<JsString>> FromIterator<(K, JsonValue)> for JsonObject {
    fn from_iter<T: IntoIterator<Item = (K, JsonValue)>>(iter: T) -> Self {
        let mut object = Self::new();
        for (key, value) in iter {
            object.insert(key, value);
        }
        object
    }
}

impl Index<&str> for JsonObject {
    type Output = JsonValue;

    fn index(&self, key: &str) -> &Self::Output {
        self.get(key).expect("object key exists")
    }
}

impl JsonValue {
    pub fn as_js(&self) -> Option<JsStr<'_>> {
        match self {
            Self::String(value) => Some(value.as_js()),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&JsonObject> {
        match self {
            Self::Object(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_object_mut(&mut self) -> Option<&mut JsonObject> {
        match self {
            Self::Object(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<JsonValue>> {
        match self {
            Self::Array(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Number(value) => value.as_f64(),
            _ => None,
        }
    }

    pub fn as_number(&self) -> Option<&Number> {
        match self {
            Self::Number(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Number(value) => value.as_i64(),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Self::Number(value) => value.as_u64(),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub fn is_boolean(&self) -> bool {
        matches!(self, Self::Bool(_))
    }

    pub fn is_number(&self) -> bool {
        matches!(self, Self::Number(_))
    }

    pub fn is_string(&self) -> bool {
        matches!(self, Self::String(_))
    }

    pub fn is_array(&self) -> bool {
        matches!(self, Self::Array(_))
    }

    pub fn is_object(&self) -> bool {
        matches!(self, Self::Object(_))
    }

    pub fn get<'n>(&self, key: impl Into<JsStr<'n>>) -> Option<&JsonValue> {
        self.as_object()?.get(key)
    }
}

impl Index<&str> for JsonValue {
    type Output = JsonValue;

    fn index(&self, key: &str) -> &Self::Output {
        self.get(key).unwrap_or(&JsonValue::Null)
    }
}

impl Index<usize> for JsonValue {
    type Output = JsonValue;

    fn index(&self, key: usize) -> &Self::Output {
        self.as_array()
            .and_then(|array| array.get(key))
            .unwrap_or(&JsonValue::Null)
    }
}

impl From<serde_json::Value> for JsonValue {
    fn from(value: serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => Self::Null,
            serde_json::Value::Bool(value) => Self::Bool(value),
            serde_json::Value::Number(value) => Self::Number(value),
            serde_json::Value::String(value) => Self::String(value.into()),
            serde_json::Value::Array(values) => {
                Self::Array(values.into_iter().map(Self::from).collect())
            }
            serde_json::Value::Object(values) => Self::Object(
                values
                    .into_iter()
                    .map(|(key, value)| (key, Self::from(value)))
                    .collect(),
            ),
        }
    }
}

impl From<i32> for JsonValue {
    fn from(value: i32) -> Self {
        Self::Number(Number::from(value))
    }
}

/// JSON.stringify's well-formed string quoting. Scalar values remain UTF-8;
/// lone surrogates become JSON escapes, preserving their original code units
/// on parse. This is serialization, never a key or a compiler string value.
pub fn append_json_quoted(value: JsStr<'_>, result: &mut String) {
    result.push('"');
    for scalar in char::decode_utf16(value.code_units()) {
        match scalar {
            Ok('"') => result.push_str("\\\""),
            Ok('\\') => result.push_str("\\\\"),
            Ok('\u{8}') => result.push_str("\\b"),
            Ok('\u{c}') => result.push_str("\\f"),
            Ok('\n') => result.push_str("\\n"),
            Ok('\r') => result.push_str("\\r"),
            Ok('\t') => result.push_str("\\t"),
            Ok(ch) if ch < ' ' => {
                write!(result, "\\u{:04x}", ch as u32).expect("String write succeeds")
            }
            Ok(ch) => result.push(ch),
            Err(error) => write!(result, "\\u{:04x}", error.unpaired_surrogate())
                .expect("String write succeeds"),
        }
    }
    result.push('"');
}

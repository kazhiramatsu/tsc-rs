//! The decoding rules of tsgo's protocol types (`lsproto/structcodec.go` and
//! the generated `UnmarshalJSONFrom` methods), with tsgo's error messages.
//!
//! A value is decoded from a parsed JSON value (object members in their input
//! order). A struct must be an object, its required members must be present,
//! and a member that is a pointer, slice or map in Go rejects `null` unless
//! the protocol says it is nullable; unknown members are skipped. A Go
//! scalar (string, number, boolean) takes its zero value from `null`, as Go's
//! decoder leaves it.

use std::collections::BTreeMap;
use std::fmt;

use serde::Serialize;
use serde_json::{Map, Value};

/// A value that does not decode, with tsgo's message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodeError(pub String);

impl fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for DecodeError {}

/// A protocol value decoded from JSON as tsgo decodes it.
pub trait Decode: Sized {
    fn decode(value: &Value) -> Result<Self, DecodeError>;
}

/// Decodes JSON text.
pub fn from_str<T: Decode>(text: &str) -> Result<T, DecodeError> {
    let value: Value =
        serde_json::from_str(text).map_err(|error| DecodeError(error.to_string()))?;
    T::decode(&value)
}

/// The JSON kind of `value`, as Go's `jsontext.Kind`.
pub fn json_kind(value: &Value) -> char {
    match value {
        Value::Null => 'n',
        Value::Bool(true) => 't',
        Value::Bool(false) => 'f',
        Value::Number(_) => '0',
        Value::String(_) => '"',
        Value::Array(_) => '[',
        Value::Object(_) => '{',
    }
}

/// Go's `jsontext.Kind.String`.
fn kind_name(kind: char) -> &'static str {
    match kind {
        'n' => "null",
        'f' => "false",
        't' => "true",
        '"' => "string",
        '0' => "number",
        '{' => "{",
        '[' => "[",
        _ => "invalid",
    }
}

/// tsgo `errNotObject`: a struct's value is an object.
pub fn expect_object(value: &Value) -> Result<&Map<String, Value>, DecodeError> {
    value.as_object().ok_or_else(|| {
        DecodeError(format!(
            "expected object start, but encountered {}",
            kind_name(json_kind(value))
        ))
    })
}

/// tsgo `errNull`: a member that may not be null.
pub fn reject_null(field: &str, value: &Value) -> Result<(), DecodeError> {
    if value.is_null() {
        return Err(DecodeError(format!(
            "null value is not allowed for field \"{field}\""
        )));
    }
    Ok(())
}

/// tsgo `errMissing`: the required members that are absent, in their
/// declaration order.
pub fn check_required(fields: &[(&str, bool)]) -> Result<(), DecodeError> {
    let missing = fields
        .iter()
        .filter(|(_, present)| !present)
        .map(|(name, _)| *name)
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }
    Err(DecodeError(format!(
        "missing required properties: {}",
        missing.join(", ")
    )))
}

/// tsgo `errInvalidKind`.
pub fn err_invalid_kind(type_name: &str, kind: char) -> DecodeError {
    DecodeError(format!("invalid {type_name}: got {}", kind_name(kind)))
}

/// tsgo `errInvalidValue`.
pub fn err_invalid_value(type_name: &str, value: &Value) -> DecodeError {
    DecodeError(format!("invalid {type_name}: {value}"))
}

/// tsgo `jsonObjectHasKey`: of the object's members in their order, the
/// first that is one of `keys`, as its index in `keys`.
pub fn json_object_has_key(value: &Value, keys: &[&str]) -> Option<usize> {
    value
        .as_object()?
        .keys()
        .find_map(|name| keys.iter().position(|key| key == name))
}

/// tsgo `scanDiscriminatedStruct`: the JSON text of an object's
/// discriminator member, when it has one.
pub fn discriminator_of(value: &Value, field: &str) -> Result<Option<String>, DecodeError> {
    Ok(expect_object(value)?.get(field).map(Value::to_string))
}

/// tsgo `invalidDiscriminator`.
pub fn err_invalid_discriminator(type_name: &str, field: &str, value: Option<&str>) -> DecodeError {
    match value {
        None => DecodeError(format!(
            "invalid {type_name}: missing discriminator \"{field}\""
        )),
        Some(value) => DecodeError(format!(
            "invalid {type_name} discriminator \"{field}\": {value}"
        )),
    }
}

/// tsgo `errLiteralMismatch`: a literal type's value is its literal.
pub fn expect_literal(value: &Value, type_name: &str, json: &str) -> Result<(), DecodeError> {
    let text = value.to_string();
    if text == json {
        return Ok(());
    }
    Err(DecodeError(format!(
        "expected {type_name} value {json}, got {text}"
    )))
}

/// A literal type's value.
pub fn literal_serialize<S: serde::Serializer>(
    json: &str,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let value: Value = serde_json::from_str(json).map_err(serde::ser::Error::custom)?;
    value.serialize(serializer)
}

/// Go's decoding error for a value of another kind.
fn cannot_unmarshal(value: &Value, go_type: &str) -> DecodeError {
    DecodeError(format!(
        "json: cannot unmarshal {} into Go value of type {go_type}",
        kind_name(json_kind(value))
    ))
}

impl Decode for String {
    fn decode(value: &Value) -> Result<Self, DecodeError> {
        match value {
            Value::String(text) => Ok(text.clone()),
            Value::Null => Ok(String::new()),
            other => Err(cannot_unmarshal(other, "string")),
        }
    }
}

impl Decode for bool {
    fn decode(value: &Value) -> Result<Self, DecodeError> {
        match value {
            Value::Bool(flag) => Ok(*flag),
            Value::Null => Ok(false),
            other => Err(cannot_unmarshal(other, "bool")),
        }
    }
}

impl Decode for i32 {
    fn decode(value: &Value) -> Result<Self, DecodeError> {
        match value {
            Value::Number(number) => number
                .as_i64()
                .and_then(|number| i32::try_from(number).ok())
                .ok_or_else(|| {
                    DecodeError(format!(
                        "json: cannot unmarshal number {number} into Go value of type int32"
                    ))
                }),
            Value::Null => Ok(0),
            other => Err(cannot_unmarshal(other, "int32")),
        }
    }
}

impl Decode for u32 {
    fn decode(value: &Value) -> Result<Self, DecodeError> {
        match value {
            Value::Number(number) => number
                .as_u64()
                .and_then(|number| u32::try_from(number).ok())
                .ok_or_else(|| {
                    DecodeError(format!(
                        "json: cannot unmarshal number {number} into Go value of type uint32"
                    ))
                }),
            Value::Null => Ok(0),
            other => Err(cannot_unmarshal(other, "uint32")),
        }
    }
}

impl Decode for f64 {
    fn decode(value: &Value) -> Result<Self, DecodeError> {
        match value {
            Value::Number(number) => Ok(number.as_f64().unwrap_or_default()),
            Value::Null => Ok(0.0),
            other => Err(cannot_unmarshal(other, "float64")),
        }
    }
}

impl Decode for Value {
    fn decode(value: &Value) -> Result<Self, DecodeError> {
        Ok(value.clone())
    }
}

impl<T: Decode> Decode for Box<T> {
    fn decode(value: &Value) -> Result<Self, DecodeError> {
        T::decode(value).map(Box::new)
    }
}

impl<T: Decode> Decode for Vec<T> {
    fn decode(value: &Value) -> Result<Self, DecodeError> {
        match value {
            Value::Array(items) => items.iter().map(T::decode).collect(),
            Value::Null => Ok(Vec::new()),
            other => Err(cannot_unmarshal(other, "slice")),
        }
    }
}

impl<const N: usize> Decode for [u32; N] {
    fn decode(value: &Value) -> Result<Self, DecodeError> {
        let mut out = [0; N];
        match value {
            Value::Array(items) => {
                for (slot, item) in out.iter_mut().zip(items) {
                    *slot = u32::decode(item)?;
                }
                Ok(out)
            }
            Value::Null => Ok(out),
            other => Err(cannot_unmarshal(other, "array")),
        }
    }
}

/// A map key decoded from a JSON member name.
pub trait DecodeKey: Sized {
    fn decode_key(name: &str) -> Self;
}

impl DecodeKey for String {
    fn decode_key(name: &str) -> Self {
        name.to_owned()
    }
}

impl<K: DecodeKey + Ord, V: Decode> Decode for BTreeMap<K, V> {
    fn decode(value: &Value) -> Result<Self, DecodeError> {
        match value {
            Value::Object(members) => members
                .iter()
                .map(|(name, value)| Ok((K::decode_key(name), V::decode(value)?)))
                .collect(),
            Value::Null => Ok(BTreeMap::new()),
            other => Err(cannot_unmarshal(other, "map")),
        }
    }
}

/// Go's zero value test (`omitzero` on a scalar).
pub trait IsZero {
    fn is_zero(&self) -> bool;
}

impl IsZero for String {
    fn is_zero(&self) -> bool {
        self.is_empty()
    }
}

impl IsZero for std::borrow::Cow<'_, str> {
    fn is_zero(&self) -> bool {
        self.is_empty()
    }
}

impl IsZero for bool {
    fn is_zero(&self) -> bool {
        !*self
    }
}

impl IsZero for i32 {
    fn is_zero(&self) -> bool {
        *self == 0
    }
}

impl IsZero for u32 {
    fn is_zero(&self) -> bool {
        *self == 0
    }
}

impl IsZero for f64 {
    fn is_zero(&self) -> bool {
        *self == 0.0
    }
}

impl IsZero for Value {
    fn is_zero(&self) -> bool {
        self.is_null()
    }
}

impl<T> IsZero for Vec<T> {
    fn is_zero(&self) -> bool {
        self.is_empty()
    }
}

impl<T> IsZero for Option<T> {
    fn is_zero(&self) -> bool {
        self.is_none()
    }
}

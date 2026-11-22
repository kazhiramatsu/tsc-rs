use std::borrow::Cow;
use std::fmt;

use serde::de::{DeserializeSeed, Deserializer, Error, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;
use serde_json::Number;

use super::{encode_user_object_key, Map, Value};

/// Discarded members still go through deserialize_any. Skipping raw tokens
/// would bypass serde's number, Unicode and nesting checks.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum PackageDestination {
    Object,
    Peer,
    VersionString,
    Discard,
}

impl PackageDestination {
    pub(super) fn read(self, text: &str) -> serde_json::Result<Value> {
        let mut parser = serde_json::Deserializer::from_str(text);
        let result = self.deserialize(&mut parser)?;
        parser.end()?;
        Ok(result)
    }
}

impl<'de> DeserializeSeed<'de> for PackageDestination {
    type Value = Value;

    fn deserialize<D: Deserializer<'de>>(self, parser: D) -> Result<Value, D::Error> {
        parser.deserialize_any(self)
    }
}

#[derive(Deserialize)]
struct Key<'a>(#[serde(borrow)] Cow<'a, str>);

impl<'de> Visitor<'de> for PackageDestination {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_bool<E>(self, value: bool) -> Result<Value, E> {
        Ok(if self == Self::Object {
            Value::Bool(value)
        } else {
            Value::Null
        })
    }

    fn visit_i64<E: Error>(self, value: i64) -> Result<Value, E> {
        self.visit_f64(value as f64)
    }

    fn visit_u64<E: Error>(self, value: u64) -> Result<Value, E> {
        self.visit_f64(value as f64)
    }

    fn visit_f64<E: Error>(self, value: f64) -> Result<Value, E> {
        Ok(if self == Self::Object {
            Value::Number(Number::from_f64(value).ok_or_else(|| E::custom("nonfinite number"))?)
        } else {
            Value::Null
        })
    }

    fn visit_str<E>(self, value: &str) -> Result<Value, E> {
        Ok(if matches!(self, Self::Object | Self::VersionString) {
            Value::String(value.into())
        } else {
            Value::Null
        })
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        if self == Self::Object {
            let mut values = Vec::new();
            while let Some(value) = seq.next_element_seed(self)? {
                values.push(value);
            }
            // The original tree conversion knew the final array length.
            values.shrink_to_fit();
            return Ok(Value::Array(values));
        }
        while seq.next_element_seed(Self::Discard)?.is_some() {}
        Ok(Value::Null)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut members: A) -> Result<Value, A::Error> {
        let mut object = Map::new();
        let mut version = Value::Null;
        let mut first = true;
        while let Some(Key(key)) = members.next_key()? {
            // serde_json::Value interprets this decoded first key specially
            // when built with raw_value. Let the original reader handle it.
            if first && key == "$serde_json::private::RawValue" {
                return Err(A::Error::custom("raw value requires the package fallback"));
            }
            first = false;
            if self == Self::Object {
                let value = members.next_value_seed(self)?;
                object.insert(encode_user_object_key(key.as_ref().into()), value);
            } else if self == Self::Peer && key == "version" {
                version = members.next_value_seed(Self::VersionString)?;
            } else {
                members.next_value_seed(Self::Discard)?;
            }
        }
        Ok(if self == Self::Object {
            Value::Object(object)
        } else {
            version
        })
    }
}

//! tsgo `lsproto`: the language server protocol's types. The types are
//! generated from tsgo's (`generated.rs`, by `scripts/generate_lsp_protocol.py`);
//! this module has the handwritten ones of tsgo's `lsp.go` and the decoding
//! rules (`codec`).

pub mod codec;
#[allow(clippy::all)]
mod generated;

use serde::{Serialize, Serializer};
use serde_json::Value;

pub use codec::{from_str, Decode, DecodeError};
pub use generated::*;

/// tsgo `DocumentUri`: a document's URI as the client wrote it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct DocumentUri(pub String);

impl DocumentUri {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for DocumentUri {
    fn from(uri: &str) -> Self {
        Self(uri.to_owned())
    }
}

impl Decode for DocumentUri {
    fn decode(value: &Value) -> Result<Self, DecodeError> {
        String::decode(value).map(Self)
    }
}

impl codec::DecodeKey for DocumentUri {
    fn decode_key(name: &str) -> Self {
        Self(name.to_owned())
    }
}

/// tsgo `URI`: a URI that is not a document's.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct Uri(pub String);

impl Decode for Uri {
    fn decode(value: &Value) -> Result<Self, DecodeError> {
        String::decode(value).map(Self)
    }
}

/// Go's `struct{}` in the protocol: an object without members.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmptyObject;

impl Serialize for EmptyObject {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        serializer.serialize_map(Some(0))?.end()
    }
}

impl Decode for EmptyObject {
    fn decode(value: &Value) -> Result<Self, DecodeError> {
        codec::expect_object(value)?;
        Ok(Self)
    }
}

/// tsgo `Null`: a response that is always `null`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Null;

impl Serialize for Null {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_unit()
    }
}

impl Decode for Null {
    fn decode(value: &Value) -> Result<Self, DecodeError> {
        if value.is_null() {
            return Ok(Self);
        }
        Err(DecodeError(format!("expected null, got {value}")))
    }
}

/// tsgo `NoParams`: the params of a method that takes none (they must be
/// absent; see the base protocol's message decoding).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoParams;

impl Serialize for NoParams {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_unit()
    }
}

impl Decode for NoParams {
    fn decode(_value: &Value) -> Result<Self, DecodeError> {
        Ok(Self)
    }
}

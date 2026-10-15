//! tsgo `lsproto`: the language server protocol's types and messages. The
//! types are generated from tsgo's (`generated.rs`, by
//! `scripts/generate_lsp_protocol.py`); this crate has the handwritten ones
//! of tsgo's `lsp.go`, the decoding rules (`codec`) and the base protocol's
//! messages (`message`).

pub mod codec;
#[allow(clippy::all)]
mod generated;
pub mod message;

use serde::{Serialize, Serializer};
use serde_json::Value;

pub use codec::{from_str, Decode, DecodeError};
pub use generated::*;
pub use message::{
    decode_params, parse_message, Id, LspError, Message, RequestMessage, ResponseError,
    ResponseMessage,
};

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

impl CodeActionKind {
    /// tsgo `CodeActionKindSourceFixAllTs`.
    pub const SOURCE_FIX_ALL_TS: Self = Self(std::borrow::Cow::Borrowed("source.fixAll.ts"));
    /// tsgo `CodeActionKindSourceOrganizeImportsTs`.
    pub const SOURCE_ORGANIZE_IMPORTS_TS: Self =
        Self(std::borrow::Cow::Borrowed("source.organizeImports.ts"));
    /// tsgo `CodeActionKindSourceRemoveUnusedImportsTs`.
    pub const SOURCE_REMOVE_UNUSED_IMPORTS_TS: Self =
        Self(std::borrow::Cow::Borrowed("source.removeUnusedImports.ts"));
    /// tsgo `CodeActionKindSourceSortImportsTs`.
    pub const SOURCE_SORT_IMPORTS_TS: Self =
        Self(std::borrow::Cow::Borrowed("source.sortImports.ts"));

    /// tsgo `CodeActionKind.Contains`: whether `other` is this kind or one
    /// of its children.
    pub fn contains(&self, other: &Self) -> bool {
        self == other
            || *self == Self::EMPTY
            || other
                .0
                .strip_prefix(self.0.as_ref())
                .is_some_and(|rest| rest.starts_with('.'))
    }
}

impl StringOrMarkupContent {
    /// tsgo `AsString`: the text, a markup's value when not a plain string.
    pub fn as_string(&self) -> &str {
        match self {
            Self::String(text) => text,
            Self::MarkupContent(markup) => &markup.value,
        }
    }
}

impl IntegerOrString {
    /// tsgo `AsString`.
    pub fn as_string(&self) -> String {
        match self {
            Self::Integer(code) => code.to_string(),
            Self::String(code) => code.clone(),
        }
    }
}

impl Diagnostic {
    /// tsgo `Diagnostic.AsString`: `code (line:character-line:character):
    /// message`.
    pub fn as_string(&self) -> String {
        format!(
            "{} ({}:{}-{}:{}): {}",
            self.code_text(),
            self.range.start.line,
            self.range.start.character,
            self.range.end.line,
            self.range.end.character,
            self.message.as_string()
        )
    }

    /// tsgo `Diagnostic.CodeAsString`: `Code(code)`.
    pub fn code_as_string(&self) -> String {
        format!("Code({})", self.code_text())
    }

    fn code_text(&self) -> String {
        self.code
            .as_ref()
            .map_or_else(|| "-1".to_owned(), IntegerOrString::as_string)
    }

    /// tsgo `diagnosticsEqual`: the same code (both strings or both
    /// integers), message (both plain or both the same markup) and range.
    fn same_as(&self, other: &Self) -> bool {
        let codes = match (&self.code, &other.code) {
            (Some(IntegerOrString::String(left)), Some(IntegerOrString::String(right))) => {
                left == right
            }
            (Some(IntegerOrString::Integer(left)), Some(IntegerOrString::Integer(right))) => {
                left == right
            }
            _ => false,
        };
        let messages = match (&self.message, &other.message) {
            (StringOrMarkupContent::String(left), StringOrMarkupContent::String(right)) => {
                left == right
            }
            (
                StringOrMarkupContent::MarkupContent(left),
                StringOrMarkupContent::MarkupContent(right),
            ) => left.kind == right.kind && left.value == right.value,
            _ => false,
        };
        codes && messages && self.range == other.range
    }
}

/// tsgo `CompareDiagnostics`: the diagnostics of `second` not in `first`,
/// and those of `first` not in `second`.
pub fn compare_diagnostics<'a>(
    first: &'a [Diagnostic],
    second: &'a [Diagnostic],
) -> (Vec<&'a Diagnostic>, Vec<&'a Diagnostic>) {
    let missing = |list: &'a [Diagnostic], from: &'a [Diagnostic]| {
        list.iter()
            .filter(|diagnostic| !from.iter().any(|other| diagnostic.same_as(other)))
            .collect::<Vec<_>>()
    };
    (missing(second, first), missing(first, second))
}

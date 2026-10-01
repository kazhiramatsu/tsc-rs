//! The compiler-runner fixture model shared by the native (TypeScript 7.x)
//! suite expansion ([`native`]) and its execution ([`execution`]): the test
//! file's `// @name: value` directives, its `@filename` units and `@link`
//! directives (Strada's `harnessIO.makeUnitsFromTest`, which Go's runner
//! keeps), and the virtual root the units are mounted under.

use serde::{Deserialize, Serialize};

use crate::HarnessError;

mod compiler;
pub mod execution;
pub mod native;

pub const VIRTUAL_SOURCE_ROOT: &str = "/.src";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SourceEncoding {
    #[serde(rename = "utf-8")]
    Utf8,
    #[serde(rename = "utf-8-bom")]
    Utf8Bom,
    #[serde(rename = "utf-16le")]
    Utf16Le,
    #[serde(rename = "utf-16be")]
    Utf16Be,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OrderedSetting {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompilerLink {
    pub target: String,
    pub link_path: String,
}

/// Decode a test file the way TypeScript's harness reads it: a UTF-8 or
/// UTF-16 byte-order mark selects the encoding, anything else is UTF-8.
fn decode_source(raw: &[u8]) -> (SourceEncoding, String) {
    if let Some(raw) = raw.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        return (
            SourceEncoding::Utf8Bom,
            String::from_utf8_lossy(raw).into_owned(),
        );
    }
    if let Some(raw) = raw.strip_prefix(&[0xff, 0xfe]) {
        return (SourceEncoding::Utf16Le, decode_utf16(raw, false));
    }
    if let Some(raw) = raw.strip_prefix(&[0xfe, 0xff]) {
        return (SourceEncoding::Utf16Be, decode_utf16(raw, true));
    }
    (
        SourceEncoding::Utf8,
        String::from_utf8_lossy(raw).into_owned(),
    )
}

fn decode_utf16(raw: &[u8], big_endian: bool) -> String {
    let code_units = raw.chunks_exact(2).map(|pair| {
        if big_endian {
            u16::from_be_bytes([pair[0], pair[1]])
        } else {
            u16::from_le_bytes([pair[0], pair[1]])
        }
    });
    char::decode_utf16(code_units)
        .map(|result| result.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

fn error(message: impl Into<String>) -> HarnessError {
    HarnessError::new(message)
}

#[cfg(test)]
#[path = "../tests/unit/upstream_suites/tests.rs"]
mod tests;

//! Canonical JavaScript strings, including unpaired UTF-16 surrogates.
//!
//! WTF-8 encodes adjacent lead/trail surrogates as one four-byte scalar.
//! All constructors and concatenations preserve that canonical form, so byte
//! equality is exactly UTF-16 code-unit equality. No arbitrary byte constructor
//! or mutable byte view is exposed. `Ord` is **byte order**, as required by
//! `Borrow<[u8]>`; JavaScript string comparers must call `cmp_utf16` instead.

use std::borrow::{Borrow, Cow};
use std::cmp::Ordering;
use std::fmt::{self, Write};
use std::hash::{Hash, Hasher};
use std::iter::FusedIterator;

/// An owned canonical WTF-8 string. It has no lossy `Display` or `str` deref.
#[derive(Clone, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct JsString(Vec<u8>);

/// A safe, zero-copy borrowed view of canonical WTF-8.
///
/// This view is passed by value; it needs no unsized reference casts or unsafe
/// code. Its constructors accept valid UTF-8 or borrow a canonical owner.
/// Like the owner, `Ord` is byte order, not JavaScript string order.
#[derive(Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct JsStr<'a> {
    bytes: &'a [u8],
}

impl JsString {
    pub const fn new() -> Self {
        Self(Vec::new())
    }

    pub fn with_capacity(bytes: usize) -> Self {
        Self(Vec::with_capacity(bytes))
    }

    pub fn from_code_units(units: &[u16]) -> Self {
        units.iter().copied().collect()
    }

    pub fn as_js(&self) -> JsStr<'_> {
        JsStr { bytes: &self.0 }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn as_str(&self) -> Option<&str> {
        self.as_js().as_str()
    }

    pub fn code_units(&self) -> CodeUnits<'_> {
        self.as_js().code_units()
    }

    pub fn to_utf16(&self) -> Vec<u16> {
        self.code_units().collect()
    }

    pub fn len_units(&self) -> usize {
        self.code_units().count()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn starts_with(&self, prefix: &str) -> bool {
        self.as_js().starts_with(prefix)
    }

    /// JavaScript's case-sensitive lexicographic UTF-16 comparison.
    pub fn cmp_utf16(&self, other: JsStr<'_>) -> Ordering {
        self.as_js().cmp_utf16(other)
    }

    /// Explicit UTF-8 output boundary: each unpaired surrogate becomes U+FFFD.
    pub fn to_string_lossy(&self) -> Cow<'_, str> {
        self.as_js().to_string_lossy()
    }

    pub fn push_str(&mut self, text: &str) {
        // UTF-8 cannot start with an encoded trail surrogate, so the junction
        // cannot form a new surrogate pair.
        self.0.extend_from_slice(text.as_bytes());
    }

    pub fn push(&mut self, ch: char) {
        self.push_str(ch.encode_utf8(&mut [0; 4]));
    }

    /// Append a UTF-16 unit, canonicalizing a pair formed at the junction.
    pub fn push_code_unit(&mut self, unit: u16) {
        if is_trail(unit) {
            if let Some(lead) = trailing_lead(&self.0) {
                self.0.truncate(self.0.len() - 3);
                append_code_point(&mut self.0, combine_pair(lead, unit));
                return;
            }
        }
        append_code_point(&mut self.0, u32::from(unit));
    }

    /// Append the scanner's Unicode escape value, accepting surrogate values.
    ///
    /// # Panics
    /// Panics above U+10FFFF; scanner error recovery must select its value
    /// before calling this operation.
    pub fn push_code_point(&mut self, point: u32) {
        assert!(
            point <= 0x10FFFF,
            "JavaScript code point is at most U+10FFFF"
        );
        if point <= 0xFFFF {
            self.push_code_unit(point as u16);
        } else {
            append_code_point(&mut self.0, point);
        }
    }

    /// JavaScript concatenation, including lead/trail pairing across operands.
    pub fn push_js(&mut self, text: JsStr<'_>) {
        let mut bytes = text.bytes;
        if let (Some(lead), Some(trail)) = (trailing_lead(&self.0), leading_trail(bytes)) {
            self.0.truncate(self.0.len() - 3);
            append_code_point(&mut self.0, combine_pair(lead, trail));
            bytes = &bytes[3..];
        }
        self.0.extend_from_slice(bytes);
    }
}

impl<'a> JsStr<'a> {
    pub const fn from_str(text: &'a str) -> Self {
        Self {
            bytes: text.as_bytes(),
        }
    }

    pub fn as_bytes(self) -> &'a [u8] {
        self.bytes
    }

    /// Returns `None` exactly when the value contains an unpaired surrogate.
    pub fn as_str(self) -> Option<&'a str> {
        std::str::from_utf8(self.bytes).ok()
    }

    pub fn is_empty(self) -> bool {
        self.bytes.is_empty()
    }

    pub fn starts_with(self, prefix: &str) -> bool {
        self.bytes.starts_with(prefix.as_bytes())
    }

    pub fn strip_prefix(self, prefix: &str) -> Option<Self> {
        // A valid UTF-8 prefix ends on a code-point boundary and cannot split
        // an encoded pair or surrogate. The remaining suffix is canonical.
        self.bytes
            .strip_prefix(prefix.as_bytes())
            .map(|bytes| Self { bytes })
    }

    pub fn code_units(self) -> CodeUnits<'a> {
        CodeUnits {
            bytes: self.bytes,
            pending_trail: None,
        }
    }

    pub fn to_utf16(self) -> Vec<u16> {
        self.code_units().collect()
    }

    pub fn len_units(self) -> usize {
        self.code_units().count()
    }

    pub fn to_owned(self) -> JsString {
        JsString(self.bytes.to_vec())
    }

    pub fn cmp_utf16(self, other: JsStr<'_>) -> Ordering {
        self.code_units().cmp(other.code_units())
    }

    pub fn to_string_lossy(self) -> Cow<'a, str> {
        match self.as_str() {
            Some(text) => Cow::Borrowed(text),
            None => Cow::Owned(
                char::decode_utf16(self.code_units())
                    .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
                    .collect(),
            ),
        }
    }
}

impl From<&str> for JsString {
    fn from(text: &str) -> Self {
        Self(text.as_bytes().to_vec())
    }
}

impl From<String> for JsString {
    fn from(text: String) -> Self {
        Self(text.into_bytes())
    }
}

impl From<JsStr<'_>> for JsString {
    fn from(text: JsStr<'_>) -> Self {
        text.to_owned()
    }
}

impl FromIterator<u16> for JsString {
    fn from_iter<T: IntoIterator<Item = u16>>(iter: T) -> Self {
        let iter = iter.into_iter();
        let mut result = Self::with_capacity(iter.size_hint().0);
        result.extend(iter);
        result
    }
}

impl Extend<u16> for JsString {
    fn extend<T: IntoIterator<Item = u16>>(&mut self, iter: T) {
        for unit in iter {
            self.push_code_unit(unit);
        }
    }
}

impl Borrow<[u8]> for JsString {
    fn borrow(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl Hash for JsString {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_bytes().hash(state);
    }
}

impl Hash for JsStr<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.bytes.hash(state);
    }
}

impl PartialEq<str> for JsStr<'_> {
    fn eq(&self, other: &str) -> bool {
        self.bytes == other.as_bytes()
    }
}

impl PartialEq<str> for JsString {
    fn eq(&self, other: &str) -> bool {
        self.0 == other.as_bytes()
    }
}

impl fmt::Debug for JsStr<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_char('"')?;
        for unit in char::decode_utf16(self.code_units()) {
            match unit {
                Ok(ch) => {
                    for escaped in ch.escape_debug() {
                        f.write_char(escaped)?;
                    }
                }
                Err(error) => write!(f, "\\u{{{:04X}}}", error.unpaired_surrogate())?,
            }
        }
        f.write_char('"')
    }
}

impl fmt::Debug for JsString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_js().fmt(f)
    }
}

/// Allocation-free UTF-16 iteration over an invariant-checked owner/view.
#[derive(Clone, Debug)]
pub struct CodeUnits<'a> {
    bytes: &'a [u8],
    pending_trail: Option<u16>,
}

impl Iterator for CodeUnits<'_> {
    type Item = u16;

    fn next(&mut self) -> Option<u16> {
        if let Some(trail) = self.pending_trail.take() {
            return Some(trail);
        }
        let first = *self.bytes.first()?;
        let (point, width) = match first {
            0..=0x7F => (u32::from(first), 1),
            0xC2..=0xDF => (
                (u32::from(first & 0x1F) << 6) | u32::from(self.bytes[1] & 0x3F),
                2,
            ),
            0xE0..=0xEF => (
                (u32::from(first & 0x0F) << 12)
                    | (u32::from(self.bytes[1] & 0x3F) << 6)
                    | u32::from(self.bytes[2] & 0x3F),
                3,
            ),
            0xF0..=0xF4 => (
                (u32::from(first & 7) << 18)
                    | (u32::from(self.bytes[1] & 0x3F) << 12)
                    | (u32::from(self.bytes[2] & 0x3F) << 6)
                    | u32::from(self.bytes[3] & 0x3F),
                4,
            ),
            _ => unreachable!("only canonical WTF-8 owners construct code-unit iterators"),
        };
        self.bytes = &self.bytes[width..];
        if point <= 0xFFFF {
            Some(point as u16)
        } else {
            let pair = point - 0x10000;
            self.pending_trail = Some(0xDC00 | (pair & 0x3FF) as u16);
            Some(0xD800 | (pair >> 10) as u16)
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let pending = usize::from(self.pending_trail.is_some());
        (
            self.bytes.len().div_ceil(3) + pending,
            Some(self.bytes.len() + pending),
        )
    }
}

impl FusedIterator for CodeUnits<'_> {}

fn is_trail(unit: u16) -> bool {
    (0xDC00..=0xDFFF).contains(&unit)
}

fn trailing_lead(bytes: &[u8]) -> Option<u16> {
    let tail = bytes.get(bytes.len().checked_sub(3)?..)?;
    (tail[0] == 0xED && (0xA0..=0xAF).contains(&tail[1]))
        .then(|| 0xD000 | (u16::from(tail[1] & 0x3F) << 6) | u16::from(tail[2] & 0x3F))
}

fn leading_trail(bytes: &[u8]) -> Option<u16> {
    let head = bytes.get(..3)?;
    (head[0] == 0xED && (0xB0..=0xBF).contains(&head[1]))
        .then(|| 0xD000 | (u16::from(head[1] & 0x3F) << 6) | u16::from(head[2] & 0x3F))
}

fn combine_pair(lead: u16, trail: u16) -> u32 {
    0x10000 + ((u32::from(lead) - 0xD800) << 10) + u32::from(trail) - 0xDC00
}

fn append_code_point(bytes: &mut Vec<u8>, point: u32) {
    match point {
        0..=0x7F => bytes.push(point as u8),
        0x80..=0x7FF => {
            bytes.extend_from_slice(&[0xC0 | (point >> 6) as u8, 0x80 | (point & 0x3F) as u8])
        }
        0x800..=0xFFFF => bytes.extend_from_slice(&[
            0xE0 | (point >> 12) as u8,
            0x80 | ((point >> 6) & 0x3F) as u8,
            0x80 | (point & 0x3F) as u8,
        ]),
        _ => bytes.extend_from_slice(&[
            0xF0 | (point >> 18) as u8,
            0x80 | ((point >> 12) & 0x3F) as u8,
            0x80 | ((point >> 6) & 0x3F) as u8,
            0x80 | (point & 0x3F) as u8,
        ]),
    }
}

#[cfg(test)]
mod tests;

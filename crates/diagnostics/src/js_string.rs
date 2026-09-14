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

/// Checked canonical byte length of a sequence of JavaScript string pieces.
/// A concatenation may join two three-byte surrogates into one four-byte
/// scalar. Allocation preflights must account for that seam as well as the
/// individual piece lengths; this accumulator does so without allocating.
#[derive(Clone, Copy, Debug, Default)]
pub struct JsStringByteLength {
    bytes: usize,
    ends_with_lead: bool,
}

impl JsStringByteLength {
    pub fn append(&mut self, piece: JsStr<'_>) -> Option<()> {
        if piece.is_empty() {
            return Some(());
        }
        let reduction =
            usize::from(self.ends_with_lead && leading_trail(piece.as_bytes()).is_some()) * 2;
        let bytes = self.bytes.checked_add(piece.as_bytes().len() - reduction)?;
        self.bytes = bytes;
        self.ends_with_lead = trailing_lead(piece.as_bytes()).is_some();
        Some(())
    }

    pub fn bytes(self) -> usize {
        self.bytes
    }
}

impl JsString {
    pub const fn new() -> Self {
        Self(Vec::new())
    }

    pub fn with_capacity(bytes: usize) -> Self {
        Self(Vec::with_capacity(bytes))
    }

    /// Reserve canonical WTF-8 storage at a caller-owned allocation boundary.
    pub fn try_reserve_exact(
        &mut self,
        additional_bytes: usize,
    ) -> Result<(), std::collections::TryReserveError> {
        self.0.try_reserve_exact(additional_bytes)
    }

    pub fn from_code_units(units: &[u16]) -> Self {
        units.iter().copied().collect()
    }

    pub fn from_code_point(point: u32) -> Self {
        let mut result = Self::new();
        result.push_code_point(point);
        result
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

    /// Truncate at a whole WTF-8 code-point boundary. Invalid byte offsets
    /// leave the string untouched; splitting a pair by UTF-16 unit uses
    /// `substring` instead. Every such prefix is canonical.
    pub fn truncate_bytes(&mut self, length: usize) -> bool {
        if self.as_js().split_at_byte(length).is_none() {
            return false;
        }
        self.0.truncate(length);
        true
    }

    pub fn starts_with(&self, prefix: &str) -> bool {
        self.as_js().starts_with(prefix)
    }

    pub fn ends_with(&self, suffix: &str) -> bool {
        self.as_js().ends_with(suffix)
    }

    pub fn contains(&self, scalar_pattern: &str) -> bool {
        self.as_js().contains(scalar_pattern)
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

    /// Split at an explicitly measured byte offset, if it is a whole WTF-8
    /// code-point boundary. This cannot split a canonically encoded pair.
    /// JavaScript string positions must use `substring` instead.
    pub fn split_at_byte(self, index: usize) -> Option<(Self, Self)> {
        if index > self.bytes.len()
            || self
                .bytes
                .get(index)
                .is_some_and(|byte| byte & 0xc0 == 0x80)
        {
            return None;
        }
        let (before, after) = self.bytes.split_at(index);
        Some((Self { bytes: before }, Self { bytes: after }))
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

    pub fn ends_with(self, suffix: &str) -> bool {
        self.bytes.ends_with(suffix.as_bytes())
    }

    pub fn contains(self, scalar_pattern: &str) -> bool {
        self.split_once(scalar_pattern).is_some()
    }

    /// Arbitrary JavaScript prefix comparison. Unlike the scalar fast path,
    /// this may match only the leading unit of a canonically encoded pair.
    pub fn starts_with_js(self, prefix: JsStr<'_>) -> bool {
        if let Some(prefix) = prefix.as_str() {
            return self.starts_with(prefix);
        }
        let mut units = self.code_units();
        prefix.code_units().all(|unit| units.next() == Some(unit))
    }

    /// Arbitrary JavaScript suffix comparison, including a trailing unit
    /// which shares a four-byte scalar with the preceding leading unit.
    pub fn ends_with_js(self, suffix: JsStr<'_>) -> bool {
        if let Some(suffix) = suffix.as_str() {
            return self.ends_with(suffix);
        }
        let Some(start) = self.len_units().checked_sub(suffix.len_units()) else {
            return false;
        };
        self.code_units().skip(start).eq(suffix.code_units())
    }

    /// String.prototype.substring for nonnegative integer arguments. Bounds
    /// are clamped and swapped as in JavaScript. An owned result is necessary:
    /// either bound can split a surrogate pair in canonical WTF-8.
    pub fn substring(self, start: usize, end: usize) -> JsString {
        let length = self.len_units();
        let start = start.min(length);
        let end = end.min(length);
        self.code_units()
            .skip(start.min(end))
            .take(start.abs_diff(end))
            .collect()
    }

    pub fn strip_prefix(self, prefix: &str) -> Option<Self> {
        // A valid UTF-8 prefix ends on a code-point boundary and cannot split
        // an encoded pair or surrogate. The remaining suffix is canonical.
        self.bytes
            .strip_prefix(prefix.as_bytes())
            .map(|bytes| Self { bytes })
    }

    pub fn strip_suffix(self, suffix: &str) -> Option<Self> {
        self.bytes
            .strip_suffix(suffix.as_bytes())
            .map(|bytes| Self { bytes })
    }

    /// Split on a Unicode-scalar separator. Its UTF-8 bytes can only match
    /// whole code points in canonical WTF-8, so both views stay canonical.
    pub fn split_once(self, separator: &str) -> Option<(Self, Self)> {
        let index = if separator.is_empty() {
            0
        } else {
            self.bytes
                .windows(separator.len())
                .position(|part| part == separator.as_bytes())?
        };
        Some((
            Self {
                bytes: &self.bytes[..index],
            },
            Self {
                bytes: &self.bytes[index + separator.len()..],
            },
        ))
    }

    /// Split at the last Unicode-scalar separator. Both borrowed results
    /// retain canonical WTF-8; this never projects a path to scalar text.
    pub fn rsplit_once(self, separator: &str) -> Option<(Self, Self)> {
        let index = if separator.is_empty() {
            self.bytes.len()
        } else {
            self.bytes
                .windows(separator.len())
                .rposition(|part| part == separator.as_bytes())?
        };
        Some((
            Self {
                bytes: &self.bytes[..index],
            },
            Self {
                bytes: &self.bytes[index + separator.len()..],
            },
        ))
    }

    /// Split at an ASCII delimiter without allocating or splitting a WTF-8
    /// code point. Empty components are retained, as by `str::split`.
    pub fn split_ascii(self, separator: u8) -> impl DoubleEndedIterator<Item = Self> + Clone {
        assert!(separator.is_ascii(), "delimiter must be ASCII");
        self.bytes
            .split(move |byte| *byte == separator)
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

impl From<&String> for JsString {
    fn from(value: &String) -> Self {
        Self::from(value.as_str())
    }
}

impl From<&JsString> for JsString {
    fn from(value: &JsString) -> Self {
        value.clone()
    }
}

impl From<JsStr<'_>> for JsString {
    fn from(text: JsStr<'_>) -> Self {
        text.to_owned()
    }
}

impl<'a> From<&'a str> for JsStr<'a> {
    fn from(text: &'a str) -> Self {
        Self::from_str(text)
    }
}

impl<'a> From<&'a String> for JsStr<'a> {
    fn from(text: &'a String) -> Self {
        Self::from_str(text)
    }
}

impl<'a> From<&'a JsString> for JsStr<'a> {
    fn from(text: &'a JsString) -> Self {
        text.as_js()
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

impl PartialEq<&str> for JsString {
    fn eq(&self, other: &&str) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl PartialEq<&str> for JsStr<'_> {
    fn eq(&self, other: &&str) -> bool {
        self.bytes == other.as_bytes()
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

//! A faithful representation of a JavaScript string value.
//!
//! JS strings are sequences of UTF-16 code units and may contain *lone*
//! (unpaired) surrogates — e.g. `"\uD800"` is a valid one-unit string. Rust's
//! `String`/`&str` are guaranteed well-formed UTF-8, which by definition cannot
//! encode surrogates, so they can represent only a strict subset of JS string
//! values. Collapsing lone surrogates to U+FFFD (the only `String`-compatible
//! fallback) is lossy: distinct values such as `"\uD800"`, `"\uD801"` and a real
//! `"\uFFFD"` all become indistinguishable, which makes string *literal types*
//! compare wrongly.
//!
//! [`JsString`] wraps a `Wtf8Buf` (WTF-8 — a superset of UTF-8 that additionally
//! encodes lone surrogates). Well-formed text is stored byte-for-byte as UTF-8,
//! so the common case stays cheap and exposes a `&str` fast path via
//! [`JsString::as_str`]; only lone surrogates use the WTF-8 extension. Equality,
//! hashing and interning operate on the faithful value, so two literal types are
//! identical iff their JS values are identical.

use std::borrow::Cow;
use std::fmt;
use std::hash::{Hash, Hasher};

use wtf8::{CodePoint, Wtf8Buf};

#[derive(Clone, PartialEq, Eq)]
pub struct JsString(Wtf8Buf);

impl JsString {
    pub fn new() -> Self {
        JsString(Wtf8Buf::new())
    }

    pub fn from_wtf8_buf(buf: Wtf8Buf) -> Self {
        JsString(buf)
    }

    /// Append an ASCII/Unicode scalar value.
    pub fn push_char(&mut self, c: char) {
        self.0.push_char(c);
    }

    /// Append well-formed UTF-8 text.
    pub fn push_str(&mut self, s: &str) {
        self.0.push_str(s);
    }

    /// Append a single code point given as a `u32`. Values in the surrogate
    /// range (0xD800..=0xDFFF) are kept as lone surrogates, exactly as JS would.
    /// Values outside 0..=0x10FFFF are ignored (callers report those upstream).
    pub fn push_code_point_u32(&mut self, v: u32) {
        if let Some(cp) = CodePoint::from_u32(v) {
            self.0.push(cp);
        }
    }

    /// Append another `JsString` (used to cook template literal values), keeping
    /// fidelity even across a high/low surrogate that straddles the boundary.
    pub fn push_js(&mut self, other: &JsString) {
        self.0.push_wtf8(&other.0);
    }

    /// The string as `&str` when it is well-formed UTF-8 (the overwhelmingly
    /// common case), or `None` if it contains a lone surrogate.
    pub fn as_str(&self) -> Option<&str> {
        self.0.as_str()
    }

    /// A `&str` view, replacing any lone surrogate with U+FFFD. Suitable for
    /// uses where a lone surrogate could never match anyway (property lookup,
    /// `typeof` results) — never for value identity.
    pub fn to_str_lossy(&self) -> Cow<'_, str> {
        self.0.to_string_lossy()
    }

    pub fn into_string_lossy(self) -> String {
        self.0.into_string_lossy()
    }

    pub fn is_empty(&self) -> bool {
        self.0.len() == 0
    }

    pub fn is_well_formed(&self) -> bool {
        self.0.as_str().is_some()
    }

    /// The value's UTF-16 code units (lone surrogates included) — the canonical
    /// JS view, used for `.length`, indexing and faithful comparison.
    pub fn code_units(&self) -> impl Iterator<Item = u16> + '_ {
        self.0.to_ill_formed_utf16()
    }

    /// Render the value the way tsc prints a string literal type: scalar values
    /// as themselves (with the usual `"`, `\`, `\n`, `\t`, `\r` escapes) and any
    /// lone surrogate as a lowercase `\uXXXX` escape (e.g. `\ud800`). The result
    /// does not include the surrounding quotes.
    pub fn display_escaped(&self) -> String {
        let mut out = String::new();
        for cp in self.0.code_points() {
            match cp.to_char() {
                Some('"') => out.push_str("\\\""),
                Some('\\') => out.push_str("\\\\"),
                Some('\n') => out.push_str("\\n"),
                Some('\t') => out.push_str("\\t"),
                Some('\r') => out.push_str("\\r"),
                Some(c) => out.push(c),
                None => {
                    // lone surrogate
                    out.push_str(&format!("\\u{:04x}", cp.to_u32()));
                }
            }
        }
        out
    }
}

impl Default for JsString {
    fn default() -> Self {
        JsString::new()
    }
}

impl From<&str> for JsString {
    fn from(s: &str) -> Self {
        JsString(Wtf8Buf::from_str(s))
    }
}

impl From<String> for JsString {
    fn from(s: String) -> Self {
        JsString(Wtf8Buf::from_string(s))
    }
}

impl PartialEq<str> for JsString {
    fn eq(&self, other: &str) -> bool {
        // A lone-surrogate value is never equal to a (well-formed) Rust `&str`.
        self.0.as_str() == Some(other)
    }
}

impl PartialEq<&str> for JsString {
    fn eq(&self, other: &&str) -> bool {
        self.0.as_str() == Some(*other)
    }
}

impl Hash for JsString {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Hash over the faithful UTF-16 code-unit view. This is a deterministic
        // function of the WTF-8 bytes, so it is consistent with `Eq` (equal
        // values hash equally) while distinguishing values that a lossy UTF-8
        // hash would conflate.
        for unit in self.0.to_ill_formed_utf16() {
            unit.hash(state);
        }
        // length-delimit so that e.g. ["a","b"] and ["ab"] cannot collide
        u32::MAX.hash(state);
    }
}

impl fmt::Debug for JsString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0.to_string_lossy())
    }
}

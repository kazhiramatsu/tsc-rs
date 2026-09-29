//! TypeScript's branded escaped symbol names (`__String`).

use std::borrow::Borrow;
use std::cmp::Ordering;
use std::hash::{Hash, Hasher};
use tsc_diagnostics::{JsStr, JsString};

/// An already escaped canonical symbol key, distinct from a raw JS value.
///
/// `Ord` is byte order to agree with `Borrow<[u8]>`, not JavaScript order.
/// Use `cmp_utf16` for upstream explicit string comparison; do not build
/// identity BTreeMaps or use default sorting to implement JS name order.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct EscapedName(JsString);

impl EscapedName {
    /// Bytes this name owns on the heap (zero while inline), for memory
    /// accounting.
    pub fn heap_bytes(&self) -> usize {
        self.0.heap_bytes()
    }

    /// tsc escapeLeadingUnderscores (_tsc.js:11438).
    pub fn escape(raw: JsStr<'_>) -> Self {
        if raw.starts_with("__") {
            let mut text = JsString::from("_");
            text.push_js(raw);
            Self(text)
        } else {
            Self(raw.to_owned())
        }
    }

    /// InternalSymbolName constants are inserted verbatim, without escaping.
    pub fn internal(name: &'static str) -> Self {
        Self(JsString::from(name))
    }

    /// The caller has obtained parser/factory-owned escaped identifier text.
    pub fn from_identifier_escaped_text(text: &str) -> Self {
        Self(JsString::from(text))
    }

    /// An upstream producer has explicitly constructed an already escaped
    /// name (for example a quoted ambient module or a private-name key).
    /// This must not be used instead of `escape` for raw property values.
    pub fn from_escaped_value(text: JsString) -> Self {
        Self(text)
    }

    /// tsc's quoted external/ambient module key. Quotes are part of the key;
    /// this is not source escaping or a display serialization.
    pub fn quoted_module(raw: JsStr<'_>) -> Self {
        let mut text = JsString::from("\"");
        text.push_js(raw);
        text.push('"');
        Self(text)
    }

    /// tsc unescapeLeadingUnderscores (_tsc.js:11441).
    pub fn unescape(&self) -> JsStr<'_> {
        let text = self.as_js();
        if text.starts_with("___") {
            text.strip_prefix("_")
                .expect("three underscores start with one")
        } else {
            text
        }
    }

    pub fn as_js(&self) -> JsStr<'_> {
        self.0.as_js()
    }

    pub fn as_str(&self) -> Option<&str> {
        self.0.as_str()
    }

    pub fn starts_with(&self, prefix: &str) -> bool {
        self.0.starts_with(prefix)
    }

    pub fn cmp_utf16(&self, other: &Self) -> Ordering {
        self.0.cmp_utf16(other.as_js())
    }
}

impl Borrow<[u8]> for EscapedName {
    fn borrow(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl Hash for EscapedName {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.as_bytes().hash(state);
    }
}

impl PartialEq<str> for EscapedName {
    fn eq(&self, other: &str) -> bool {
        self.0 == *other
    }
}

impl PartialEq<&str> for EscapedName {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl<'a> From<&'a EscapedName> for JsStr<'a> {
    fn from(name: &'a EscapedName) -> Self {
        name.as_js()
    }
}

#[cfg(test)]
#[path = "../tests/unit/escaped_name/tests.rs"]
mod tests;

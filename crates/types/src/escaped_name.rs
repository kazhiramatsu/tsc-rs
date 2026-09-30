//! TypeScript's branded escaped symbol names (`__String`).

use std::cmp::Ordering;
use std::fmt;
use tsc_diagnostics::{JsStr, JsString};

use crate::names;

/// An already escaped canonical symbol key, distinct from a raw JS value.
///
/// A name is a 32-bit id into the process-wide name table (see
/// [`crate::names`]): copying, comparing for equality and keying a symbol
/// table cost what a `u32` costs, and the text is read back through
/// [`EscapedName::as_js`] without a lock.
///
/// `Eq` and `Hash` compare ids: two names are equal exactly when their
/// texts are, so a hash-keyed container hashes a `u32`. `Ord` is the text's
/// byte order, not JavaScript order: use `cmp_utf16` for upstream explicit
/// string comparison; do not build identity BTreeMaps or use default sorting
/// to implement JS name order.
#[derive(Clone, Copy, Eq, PartialEq, Hash)]
pub struct EscapedName(u32);

impl EscapedName {
    /// Bytes this name owns on the heap: none, the text lives in the shared
    /// name table (see [`EscapedName::interned_names`]).
    pub fn heap_bytes(&self) -> usize {
        0
    }

    /// Interned names and their text bytes, for memory accounting.
    pub fn interned_names() -> (usize, usize) {
        names::interned_names()
    }

    /// The name's position in the name table, for tables keyed by name.
    #[inline]
    pub fn index(self) -> u32 {
        self.0
    }

    /// tsc escapeLeadingUnderscores (_tsc.js:11438).
    #[cfg_attr(feature = "perf-counters", track_caller)]
    pub fn escape(raw: JsStr<'_>) -> Self {
        crate::perf::name_site(std::panic::Location::caller());
        crate::perf::bump(crate::perf::PerfCounter::NamesEscapeCalls);
        if raw.starts_with("__") {
            let mut text = JsString::from("_");
            text.push_js(raw);
            Self(names::intern(text.as_js()))
        } else {
            Self(names::intern(raw))
        }
    }

    /// InternalSymbolName constants are inserted verbatim, without escaping.
    #[cfg_attr(feature = "perf-counters", track_caller)]
    pub fn internal(name: &'static str) -> Self {
        crate::perf::name_site(std::panic::Location::caller());
        Self(names::intern(JsStr::from(name)))
    }

    /// The caller has obtained parser/factory-owned escaped identifier text.
    #[cfg_attr(feature = "perf-counters", track_caller)]
    pub fn from_identifier_escaped_text(text: &str) -> Self {
        crate::perf::name_site(std::panic::Location::caller());
        Self(names::intern(JsStr::from(text)))
    }

    /// An upstream producer has explicitly constructed an already escaped
    /// name (for example a quoted ambient module or a private-name key).
    /// This must not be used instead of `escape` for raw property values.
    #[cfg_attr(feature = "perf-counters", track_caller)]
    pub fn from_escaped_value(text: JsString) -> Self {
        crate::perf::name_site(std::panic::Location::caller());
        Self(names::intern(text.as_js()))
    }

    /// An already escaped name borrowed from its producer (a table key or an
    /// internal name); see [`EscapedName::from_escaped_value`].
    #[cfg_attr(feature = "perf-counters", track_caller)]
    pub fn from_escaped_text(text: JsStr<'_>) -> Self {
        crate::perf::name_site(std::panic::Location::caller());
        Self(names::intern(text))
    }

    /// tsc's quoted external/ambient module key. Quotes are part of the key;
    /// this is not source escaping or a display serialization.
    #[cfg_attr(feature = "perf-counters", track_caller)]
    pub fn quoted_module(raw: JsStr<'_>) -> Self {
        crate::perf::name_site(std::panic::Location::caller());
        let mut text = JsString::from("\"");
        text.push_js(raw);
        text.push('"');
        Self(names::intern(text.as_js()))
    }

    /// tsc unescapeLeadingUnderscores (_tsc.js:11441).
    #[cfg_attr(feature = "perf-counters", track_caller)]
    pub fn unescape(&self) -> JsStr<'static> {
        let text = self.as_js();
        if text.starts_with("___") {
            text.strip_prefix("_")
                .expect("three underscores start with one")
        } else {
            text
        }
    }

    #[inline]
    #[cfg_attr(feature = "perf-counters", track_caller)]
    pub fn as_js(&self) -> JsStr<'static> {
        names::text(self.0).as_js()
    }

    #[cfg_attr(feature = "perf-counters", track_caller)]
    pub fn as_str(&self) -> Option<&'static str> {
        names::text(self.0).as_str()
    }

    /// The text of an identifier's name, which is always UTF-8.
    #[cfg_attr(feature = "perf-counters", track_caller)]
    pub fn identifier_text(&self) -> &'static str {
        self.as_str().expect("identifier text is UTF-8")
    }

    #[cfg_attr(feature = "perf-counters", track_caller)]
    pub fn is_empty(&self) -> bool {
        names::text(self.0).is_empty()
    }

    #[cfg_attr(feature = "perf-counters", track_caller)]
    pub fn starts_with(&self, prefix: &str) -> bool {
        self.as_js().starts_with(prefix)
    }

    #[cfg_attr(feature = "perf-counters", track_caller)]
    pub fn cmp_utf16(&self, other: &Self) -> Ordering {
        self.as_js().cmp_utf16(other.as_js())
    }
}

/// The empty name.
impl Default for EscapedName {
    fn default() -> Self {
        Self::from_identifier_escaped_text("")
    }
}

/// The text, with a lone surrogate shown as U+FFFD.
impl fmt::Display for EscapedName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.as_js().to_string_lossy())
    }
}

impl PartialEq<String> for EscapedName {
    #[cfg_attr(feature = "perf-counters", track_caller)]
    fn eq(&self, other: &String) -> bool {
        *names::text(self.0) == **other
    }
}

impl PartialEq<EscapedName> for String {
    fn eq(&self, other: &EscapedName) -> bool {
        other == self
    }
}

impl fmt::Debug for EscapedName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("EscapedName").field(&self.as_js()).finish()
    }
}

impl Ord for EscapedName {
    #[cfg_attr(feature = "perf-counters", track_caller)]
    fn cmp(&self, other: &Self) -> Ordering {
        if self.0 == other.0 {
            Ordering::Equal
        } else {
            names::text(self.0).cmp(names::text(other.0))
        }
    }
}

impl PartialOrd for EscapedName {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq<str> for EscapedName {
    #[cfg_attr(feature = "perf-counters", track_caller)]
    fn eq(&self, other: &str) -> bool {
        *names::text(self.0) == *other
    }
}

impl PartialEq<&str> for EscapedName {
    #[cfg_attr(feature = "perf-counters", track_caller)]
    fn eq(&self, other: &&str) -> bool {
        *names::text(self.0) == **other
    }
}

impl<'a> From<&'a EscapedName> for JsStr<'a> {
    fn from(name: &'a EscapedName) -> Self {
        name.as_js()
    }
}

impl From<EscapedName> for JsStr<'static> {
    fn from(name: EscapedName) -> Self {
        name.as_js()
    }
}

/// The name of a literal, interned once per use site: for comparisons and
/// lookups on hot paths, where interning the text on every call would cost a
/// hash and a shard lock.
#[macro_export]
macro_rules! known_name {
    ($text:literal) => {{
        static NAME: ::std::sync::OnceLock<$crate::EscapedName> = ::std::sync::OnceLock::new();
        *NAME.get_or_init(|| $crate::EscapedName::internal($text))
    }};
}

#[cfg(test)]
#[path = "../tests/unit/escaped_name/tests.rs"]
mod tests;

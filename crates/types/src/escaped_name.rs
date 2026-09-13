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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InternalSymbolName;
    use std::collections::HashMap;

    #[test]
    fn escape_round_trip_for_all_units_and_underscore_prefixes() {
        for unit in 0..=u16::MAX {
            for prefix in ["", "_", "__", "___", "____"] {
                let mut raw = JsString::from(prefix);
                raw.push_code_unit(unit);
                assert_eq!(EscapedName::escape(raw.as_js()).unescape(), raw.as_js());
            }
        }
    }

    #[test]
    fn internal_names_and_distinct_surrogates_have_distinct_keys() {
        let internal = EscapedName::internal(InternalSymbolName::CALL);
        let user = EscapedName::escape(JsStr::from_str("__call"));
        assert_ne!(internal, user);
        assert_eq!(user.as_str(), Some("___call"));
        let values = [[0xD800], [0xD801], [0xDC00], [0xFFFD]];
        let mut table = HashMap::new();
        for (i, units) in values.iter().enumerate() {
            table.insert(
                EscapedName::escape(JsString::from_code_units(units).as_js()),
                i,
            );
        }
        for (i, units) in values.iter().enumerate() {
            let query = JsString::from_code_units(units);
            assert_eq!(table.get(query.as_bytes()), Some(&i));
        }
        table.insert(internal, 4);
        table.insert(user, 5);
        assert_eq!(table.get("__call".as_bytes()), Some(&4));
        assert_eq!(table.get("___call".as_bytes()), Some(&5));
    }

    #[test]
    fn byte_order_and_explicit_js_order_remain_distinct() {
        let bmp = EscapedName::escape(JsStr::from_str("\u{E000}"));
        let supplementary = EscapedName::escape(JsStr::from_str("\u{10000}"));
        assert!(bmp < supplementary);
        assert_eq!(bmp.cmp_utf16(&supplementary), Ordering::Greater);
        let bmp_bytes: &[u8] = bmp.borrow();
        let supplementary_bytes: &[u8] = supplementary.borrow();
        assert_eq!(bmp.cmp(&supplementary), bmp_bytes.cmp(supplementary_bytes));
    }

    #[test]
    fn template_values_bridge_without_losing_surrogates() {
        let units = [0xD800, 0x41, 0xDC00, 0xD83D, 0xDE00, 0xFFFD];
        let literal = crate::TemplateText::from_utf16(&units);
        let value = literal.to_js_string();
        assert_eq!(value.to_utf16(), units);
        assert_eq!(crate::TemplateText::from_js(value.as_js()), literal);
        let key = EscapedName::escape(value.as_js());
        assert_eq!(crate::TemplateText::from_js(key.unescape()), literal);
    }
}

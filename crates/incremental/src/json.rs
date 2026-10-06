//! The compact JSON writer for the build info document: the bytes
//! `encoding/json/v2` (tsgo's `internal/json`) produces for the
//! `BuildInfo` structures — no whitespace, fields in struct order, zero
//! values omitted, strings escaped only where JSON requires it.

use std::fmt::Write as _;

/// A string in JSON: `"` and `\` escaped, the five short escapes for their
/// control characters, `\u00XX` for the other control characters, every
/// other character verbatim (json/v2 escapes neither `<>&` nor U+2028).
pub(crate) fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", ch as u32);
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

/// The writer of one JSON object's fields: a comma before every field but
/// the first.
pub(crate) struct ObjectWriter<'o> {
    out: &'o mut String,
    fields: usize,
}

impl<'o> ObjectWriter<'o> {
    pub(crate) fn begin(out: &'o mut String) -> Self {
        out.push('{');
        Self { out, fields: 0 }
    }

    fn key(&mut self, name: &str) {
        if self.fields > 0 {
            self.out.push(',');
        }
        self.fields += 1;
        write_string(self.out, name);
        self.out.push(':');
    }

    pub(crate) fn string(&mut self, name: &str, value: &str) {
        self.key(name);
        write_string(self.out, value);
    }

    /// A string field with `omitzero`: absent when empty.
    pub(crate) fn string_omitzero(&mut self, name: &str, value: &str) {
        if !value.is_empty() {
            self.string(name, value);
        }
    }

    pub(crate) fn bool_omitzero(&mut self, name: &str, value: bool) {
        if value {
            self.key(name);
            self.out.push_str("true");
        }
    }

    pub(crate) fn number_omitzero(&mut self, name: &str, value: u32) {
        if value != 0 {
            self.key(name);
            let _ = write!(self.out, "{value}");
        }
    }

    pub(crate) fn strings_omitzero(&mut self, name: &str, values: &[String]) {
        if values.is_empty() {
            return;
        }
        self.key(name);
        self.out.push('[');
        for (index, value) in values.iter().enumerate() {
            if index > 0 {
                self.out.push(',');
            }
            write_string(self.out, value);
        }
        self.out.push(']');
    }

    /// A list field with `omitzero`, each element written by `element`.
    pub(crate) fn list_omitzero<T>(
        &mut self,
        name: &str,
        values: &[T],
        element: impl FnMut(&mut String, &T),
    ) {
        if values.is_empty() {
            return;
        }
        self.list(name, values, element);
    }

    /// A list field written even when empty (a non-nil Go slice under
    /// `omitzero`), each element written by `element`.
    pub(crate) fn list<T>(
        &mut self,
        name: &str,
        values: &[T],
        mut element: impl FnMut(&mut String, &T),
    ) {
        self.key(name);
        self.out.push('[');
        for (index, value) in values.iter().enumerate() {
            if index > 0 {
                self.out.push(',');
            }
            element(self.out, value);
        }
        self.out.push(']');
    }

    /// A field whose value `value` writes itself (an object or a map).
    pub(crate) fn raw(&mut self, name: &str, value: impl FnOnce(&mut String)) {
        self.key(name);
        value(self.out);
    }

    pub(crate) fn end(self) {
        self.out.push('}');
    }
}

pub(crate) fn write_number(out: &mut String, value: u32) {
    let _ = write!(out, "{value}");
}

pub(crate) fn write_numbers(out: &mut String, values: &[u32]) {
    out.push('[');
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        write_number(out, *value);
    }
    out.push(']');
}

#[cfg(test)]
mod tests {
    use super::{write_string, ObjectWriter};

    #[test]
    fn escapes_only_what_json_requires() {
        let mut out = String::new();
        write_string(&mut out, "a\"b\\c\n\t\u{1}<&>/é\u{2028}");
        assert_eq!(out, "\"a\\\"b\\\\c\\n\\t\\u0001<&>/é\u{2028}\"");
    }

    #[test]
    fn omits_zero_values_and_separates_fields() {
        let mut out = String::new();
        let mut object = ObjectWriter::begin(&mut out);
        object.string("version", "7.1.0-dev");
        object.bool_omitzero("errors", false);
        object.bool_omitzero("checkPending", true);
        object.number_omitzero("pos", 0);
        object.number_omitzero("end", 3);
        object.strings_omitzero("empty", &[]);
        object.end();
        assert_eq!(
            out,
            r#"{"version":"7.1.0-dev","checkPending":true,"end":3}"#
        );
    }

    #[test]
    fn writes_an_empty_list_only_without_omitzero() {
        let mut out = String::new();
        let mut object = ObjectWriter::begin(&mut out);
        object.list_omitzero("absent", &[] as &[u32], |out, value| {
            super::write_number(out, *value)
        });
        object.list("present", &[] as &[u32], |out, value| {
            super::write_number(out, *value)
        });
        object.list("items", &[1, 2], |out, value| {
            super::write_number(out, *value)
        });
        object.end();
        assert_eq!(out, r#"{"present":[],"items":[1,2]}"#);
    }
}

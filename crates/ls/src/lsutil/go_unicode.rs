//! Go's `unicode` and `strings` case functions over the tables generated
//! from them (`unicode_tables.rs`), for the language service code that
//! tsgo writes with them.

use super::unicode_tables::{LOWERCASE_LETTER, TO_LOWER, TO_UPPER, UPPERCASE_LETTER};

/// Whether a character is in a table of Go `unicode` ranges.
pub(crate) fn in_ranges(table: &[(u32, u32, u32)], ch: char) -> bool {
    let code = ch as u32;
    let index = table.partition_point(|&(_, hi, _)| hi < code);
    table
        .get(index)
        .is_some_and(|&(lo, _, stride)| code >= lo && (code - lo).is_multiple_of(stride))
}

/// A character's mapping in a table of Go's changed characters.
fn mapped(table: &[(u32, u32)], ch: char) -> char {
    match table.binary_search_by_key(&(ch as u32), |&(from, _)| from) {
        Ok(index) => char::from_u32(table[index].1).unwrap_or(ch),
        Err(_) => ch,
    }
}

/// Go `unicode.IsUpper`.
pub(crate) fn is_upper(ch: char) -> bool {
    in_ranges(UPPERCASE_LETTER, ch)
}

/// Go `unicode.IsLower`.
pub(crate) fn is_lower(ch: char) -> bool {
    in_ranges(LOWERCASE_LETTER, ch)
}

/// Go `unicode.ToLower`.
pub(crate) fn to_lower_char(ch: char) -> char {
    if ch.is_ascii() {
        return ch.to_ascii_lowercase();
    }
    mapped(TO_LOWER, ch)
}

/// Go `unicode.ToUpper`.
pub(crate) fn to_upper_char(ch: char) -> char {
    if ch.is_ascii() {
        return ch.to_ascii_uppercase();
    }
    mapped(TO_UPPER, ch)
}

/// Go `strings.ToLower`: each character's simple lower case.
pub(crate) fn to_lower(text: &str) -> String {
    if text.is_ascii() {
        return text.to_ascii_lowercase();
    }
    text.chars().map(to_lower_char).collect()
}

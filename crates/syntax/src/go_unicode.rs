//! Go's `unicode` and `strings` case functions over the tables generated
//! from Go's packages (`tables`), for the code tsgo writes with them (the
//! language service's comparers and auto-import index, vfsmatch's case
//! folding).

pub mod tables;

use tables::{LOWERCASE_LETTER, SIMPLE_FOLD, TO_LOWER, TO_UPPER, UPPERCASE_LETTER};

/// Whether a character is in a table of Go `unicode` ranges.
pub fn in_ranges(table: &[(u32, u32, u32)], ch: char) -> bool {
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
pub fn is_upper(ch: char) -> bool {
    in_ranges(UPPERCASE_LETTER, ch)
}

/// Go `unicode.IsLower`.
pub fn is_lower(ch: char) -> bool {
    in_ranges(LOWERCASE_LETTER, ch)
}

/// Go `unicode.ToLower`.
pub fn to_lower_char(ch: char) -> char {
    if ch.is_ascii() {
        return ch.to_ascii_lowercase();
    }
    mapped(TO_LOWER, ch)
}

/// Go `unicode.ToUpper`.
pub fn to_upper_char(ch: char) -> char {
    if ch.is_ascii() {
        return ch.to_ascii_uppercase();
    }
    mapped(TO_UPPER, ch)
}

/// Go `strings.ToLower`: each character's simple lower case.
pub fn to_lower(text: &str) -> String {
    if text.is_ascii() {
        return text.to_ascii_lowercase();
    }
    text.chars().map(to_lower_char).collect()
}

/// Go `unicode.SimpleFold`: the next character of the case orbit.
fn simple_fold(ch: char) -> char {
    mapped(SIMPLE_FOLD, ch)
}

/// Go `strings.EqualFold`: equal under simple Unicode case folding.
pub fn equal_fold(a: &str, b: &str) -> bool {
    let mut b_chars = b.chars();
    for a_char in a.chars() {
        let Some(b_char) = b_chars.next() else {
            return false;
        };
        if a_char == b_char {
            continue;
        }
        let (small, large) = if a_char < b_char {
            (a_char, b_char)
        } else {
            (b_char, a_char)
        };
        if large.is_ascii() {
            if small.is_ascii_uppercase() && large == small.to_ascii_lowercase() {
                continue;
            }
            return false;
        }
        let mut folded = simple_fold(small);
        while folded != small && folded < large {
            folded = simple_fold(folded);
        }
        if folded != large {
            return false;
        }
    }
    b_chars.next().is_none()
}

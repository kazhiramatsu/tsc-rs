//! tsgo `ls/autoimport/index.go`: the exports of a bucket, indexed by the
//! first letter of each word of their names (a name's start upper case,
//! the other words' starts lower case).

use std::collections::HashMap;
use std::sync::Arc;

use crate::lsutil::go_unicode::{is_lower, is_upper, to_lower, to_lower_char, to_upper_char};

use super::export::Export;

/// tsgo `Index[*Export]`.
#[derive(Clone, Debug, Default)]
pub struct Index {
    entries: Vec<Arc<Export>>,
    index: HashMap<char, Vec<usize>>,
}

impl Index {
    /// tsgo `SearchWordPrefix`: each export with a word starting with the
    /// prefix's first letter whose name holds the prefix's letters in order
    /// (ignoring case); every export for an empty prefix.
    pub(crate) fn search_word_prefix(&self, prefix: &str) -> Vec<Arc<Export>> {
        if self.entries.is_empty() {
            return Vec::new();
        }
        if prefix.is_empty() {
            return self.entries.clone();
        }
        let prefix = to_lower(prefix);
        let first = prefix.chars().next().expect("a nonempty prefix");
        let first_upper = to_upper_char(first);
        let first_lower = to_lower_char(first);
        let name_starts = self
            .index
            .get(&first_upper)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let word_starts = if first_upper != first_lower {
            self.index
                .get(&first_lower)
                .map(Vec::as_slice)
                .unwrap_or_default()
        } else {
            &[]
        };
        let mut results = Vec::with_capacity(name_starts.len() + word_starts.len());
        for starts in [name_starts, word_starts] {
            for &index in starts {
                let entry = &self.entries[index];
                if contains_chars_in_order(entry.name(), &prefix) {
                    results.push(Arc::clone(entry));
                }
            }
        }
        results
    }

    /// tsgo `insertAsWords`: the export keyed by the first letter of each
    /// word of its name.
    pub(crate) fn insert_as_words(&mut self, value: Arc<Export>) {
        let name = value.name().to_owned();
        assert!(!name.is_empty(), "Cannot index entry with empty name");
        let entry_index = self.entries.len();
        self.entries.push(value);
        let mut seen = Vec::new();
        for (word, start) in word_indices(&name).into_iter().enumerate() {
            let Some(first) = name[start..].chars().next() else {
                continue;
            };
            if word == 0 {
                // Name start keyed by uppercase
                let key = to_upper_char(first);
                self.index.entry(key).or_default().push(entry_index);
                seen.push(key); // (Still set seenRunes in case first character is non-alphabetic)
            } else {
                // Subsequent word starts keyed by lowercase
                let key = to_lower_char(first);
                if !seen.contains(&key) {
                    self.index.entry(key).or_default().push(entry_index);
                    seen.push(key);
                }
            }
        }
    }
}

/// tsgo `containsCharsInOrder`: whether `text` holds the pattern's
/// characters in order (ignoring case).
fn contains_chars_in_order(text: &str, pattern: &str) -> bool {
    let text = to_lower(text);
    let pattern = to_lower(pattern);
    let mut rest = pattern.chars().peekable();
    for ch in text.chars() {
        if rest.peek() == Some(&ch) {
            rest.next();
        }
    }
    rest.peek().is_none()
}

/// tsgo `wordIndices`: the byte starts of the words of an identifier
/// (camelCase and snake_case), the first always 0.
///   - CamelCase
///     ^    ^
///   - snake_case
///     ^     ^
///   - ParseURL
///     ^    ^
///   - __proto__
///     ^
pub(crate) fn word_indices(text: &str) -> Vec<usize> {
    let mut indices = Vec::new();
    let bytes = text.as_bytes();
    for (byte_index, ch) in text.char_indices() {
        if byte_index == 0 {
            indices.push(byte_index);
            continue;
        }
        if ch == '_' {
            if byte_index + 1 < text.len() && bytes[byte_index + 1] != b'_' {
                indices.push(byte_index + 1);
            }
            continue;
        }
        if is_upper(ch) {
            let previous_lower = text[..byte_index].chars().next_back().is_some_and(is_lower);
            // tsgo decodes the byte after the letter's first byte.
            let next_lower = text
                .get(byte_index + 1..)
                .and_then(|rest| rest.chars().next())
                .is_some_and(is_lower);
            if previous_lower || next_lower {
                indices.push(byte_index);
            }
        }
    }
    indices
}

//! tsgo `stringtable.go`: the strings of an encoded tree. The file text is
//! the first string data; a node's string that equals its slice of the text
//! points into it, any other string is appended after the text.

use super::{KIND_NO_SUBSTITUTION_TEMPLATE_LITERAL, KIND_STRING_LITERAL, KIND_TEMPLATE_TAIL};

pub(crate) struct StringTable<'a> {
    file_text: &'a [u8],
    other_strings: Vec<u8>,
    /// Start and end pairs, into the file text followed by the other strings.
    offsets: Vec<u32>,
}

impl<'a> StringTable<'a> {
    pub(crate) fn new(file_text: &'a [u8]) -> Self {
        Self {
            file_text,
            other_strings: Vec::new(),
            offsets: Vec::new(),
        }
    }

    /// tsgo `stringTable.add`: the index of `text`, the string of a node of
    /// tsgo kind `kind` at UTF-8 `pos..end`. A node's leading trivia is not
    /// part of `pos..end`'s text, so the slice ending at the node's end
    /// (before a closing quote or backtick) is tried.
    pub(crate) fn add(&mut self, text: &[u8], kind: u32, pos: usize, end: usize) -> u32 {
        let index = self.offsets.len() as u32;
        let length = text.len();
        if end > pos && end <= self.file_text.len() {
            let end_offset = usize::from(matches!(
                kind,
                KIND_STRING_LITERAL | KIND_TEMPLATE_TAIL | KIND_NO_SUBSTITUTION_TEMPLATE_LITERAL
            ));
            if let Some(end) = end.checked_sub(end_offset) {
                if let Some(start) = end.checked_sub(length) {
                    if &self.file_text[start..end] == text {
                        self.offsets.extend([start as u32, end as u32]);
                        return index;
                    }
                }
            }
        }
        let offset = self.file_text.len() + self.other_strings.len();
        self.other_strings.extend_from_slice(text);
        self.offsets
            .extend([offset as u32, (offset + length) as u32]);
        index
    }

    /// tsgo's source file text entry: the whole text, `pos..end`.
    pub(crate) fn add_file_text(&mut self, pos: u32, end: u32) -> u32 {
        let index = self.offsets.len() as u32;
        self.offsets.extend([pos, end]);
        index
    }

    pub(crate) fn offsets_len(&self) -> usize {
        self.offsets.len()
    }

    pub(crate) fn string_length(&self) -> usize {
        self.file_text.len() + self.other_strings.len()
    }

    pub(crate) fn encode_into(&self, out: &mut Vec<u8>) {
        for offset in &self.offsets {
            out.extend_from_slice(&offset.to_le_bytes());
        }
        out.extend_from_slice(self.file_text);
        out.extend_from_slice(&self.other_strings);
    }
}

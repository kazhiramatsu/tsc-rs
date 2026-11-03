//! The literal texts of a tree: each text is a word with its byte length and
//! then its bytes, four to a word, in a [`StoreVec`] of [`TextWord`]s that the
//! tree leases and publishes like its nodes. A text's identity is the store
//! position of its length word, so a published text is read with one load of
//! the length and a slice, and an unpublished one from its tree's words.
//!
//! The bytes are canonical WTF-8 (UTF-8 for every text but a string literal's
//! value with a lone surrogate); this module stores and returns them as bytes.

use crate::{get, slice, StoreRecord, StoreVec};

#[cfg(not(target_endian = "little"))]
compile_error!("the text store reads its words as bytes in little-endian order");

/// The region of the texts' words.
pub(crate) const TEXT_WORD_REGION: usize = 12;

/// A word of a tree's text store (see the module documentation). All zeros is
/// the length of the empty text.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(transparent)]
pub struct TextWord(pub u32);

impl StoreRecord for TextWord {
    const REGION: usize = TEXT_WORD_REGION;
}

/// The words a text of `len` bytes takes: its length word and its bytes.
pub const fn text_words(len: usize) -> usize {
    1 + len.div_ceil(4)
}

/// Append `bytes` to `words` as one text; returns the index of its length
/// word.
pub fn push_text(words: &mut StoreVec<TextWord>, bytes: &[u8]) -> usize {
    let len = u32::try_from(bytes.len()).expect("a literal text longer than 4 GiB");
    let index = words.len();
    words.reserve(text_words(bytes.len()));
    words.push(TextWord(len));
    let (chunks, rest) = bytes.as_chunks::<4>();
    for chunk in chunks {
        words.push(TextWord(u32::from_le_bytes(*chunk)));
    }
    if !rest.is_empty() {
        let mut last = [0u8; 4];
        last[..rest.len()].copy_from_slice(rest);
        words.push(TextWord(u32::from_le_bytes(last)));
    }
    index
}

/// The bytes of the text whose length word is `words[index]`.
pub fn text_in(words: &[TextWord], index: usize) -> &[u8] {
    let len = words[index].0 as usize;
    let body = &words[index + 1..index + 1 + len.div_ceil(4)];
    // SAFETY: `TextWord` is a transparent `u32`, so `body` is `4 * body.len()`
    // initialized bytes with no stricter alignment for `u8`, and `len` is at
    // most that many (the words were written by `push_text`, little-endian).
    unsafe { std::slice::from_raw_parts(body.as_ptr().cast::<u8>(), len) }
}

/// The bytes of the published text whose length word is at `position`.
///
/// `position` names a text of a live published tree (see `crate::get`).
#[inline]
pub fn published_text(position: u32) -> &'static [u8] {
    let len = get::<TextWord>(position).0;
    let body = slice::<TextWord>(position + 1, len.div_ceil(4));
    // SAFETY: as in `text_in`.
    unsafe { std::slice::from_raw_parts(body.as_ptr().cast::<u8>(), len as usize) }
}

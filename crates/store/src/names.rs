//! The storage of the process-wide name table: each name's text, written
//! once and never moved, read by id with one load of its record.
//!
//! The interner (`tsc_types::names`) looks a text up under the lock of the
//! text's shard and calls [`push`] for a text it has not seen. The new id
//! reaches other threads only through that lock (or through what was built
//! from it, such as a document behind an `Arc`), after its record and text
//! are written: the contract of [`crate::get`].
//!
//! A name's record (region 8) is its text's start in the text region
//! (region 9) and its byte length, with a flag for text that is not Unicode
//! scalars. The text is canonical WTF-8, copied verbatim from the interned
//! string.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use crate::{StoreRecord, FIRST_REGION, REGION_BYTES};

/// The region of the names' records.
pub(crate) const RECORD_REGION: usize = 8;
/// The region of the names' text bytes.
pub(crate) const TEXT_REGION: usize = 9;
/// The start of the text region.
const TEXT: usize = FIRST_REGION + TEXT_REGION * REGION_BYTES;

/// The bits of a record's `len_flags` that hold the text's byte length.
const LEN_MASK: u32 = (1 << 30) - 1;
/// A record's flag: the text has a lone surrogate, so it is no `str`.
const NOT_SCALAR: u32 = 1 << 31;

/// A name: where its text starts in the text region, and the text's byte
/// length and flags. All zeros is the empty text.
#[derive(Clone, Copy)]
#[repr(C)]
struct NameRecord {
    start: u32,
    len_flags: u32,
}

impl StoreRecord for NameRecord {
    const REGION: usize = RECORD_REGION;
}

/// The next name's id.
static NEXT: AtomicU32 = AtomicU32::new(0);
/// The next free byte of the text region.
static CURSOR: AtomicUsize = AtomicUsize::new(0);

/// Store a new name's text and return its id. Every call makes a new id;
/// the caller hands the id to readers after this returns.
pub fn push(text: &[u8]) -> u32 {
    let flags = if std::str::from_utf8(text).is_ok() {
        0
    } else {
        NOT_SCALAR
    };
    push_bytes(text, flags)
}

/// Store a name whose UTF-8 validity is carried by its type.
pub fn push_str(text: &str) -> u32 {
    push_bytes(text.as_bytes(), 0)
}

fn push_bytes(text: &[u8], flags: u32) -> u32 {
    crate::reserve();
    let len = u32::try_from(text.len())
        .ok()
        .filter(|&len| len <= LEN_MASK)
        .expect("a name has at most 1 GiB of text");
    let start = CURSOR.fetch_add(text.len(), Ordering::Relaxed);
    assert!(
        start + text.len() <= u32::MAX as usize,
        "the name table holds 4 GiB of text"
    );
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    assert!(id < u32::MAX, "the name table holds 2^32 - 1 names");
    let record = crate::address::<NameRecord>(id);
    crate::system::commit(TEXT + start, text.len());
    crate::system::commit(record, std::mem::size_of::<NameRecord>());
    // SAFETY: this call alone claimed the text range (the cursor's
    // `fetch_add`) and the record (the id's `fetch_add`); both lie in the
    // reservation, which `reserve` mapped, and no reader has the id yet.
    unsafe {
        std::ptr::copy_nonoverlapping(text.as_ptr(), (TEXT + start) as *mut u8, text.len());
        (record as *mut NameRecord).write(NameRecord {
            start: start as u32,
            len_flags: len | flags,
        });
    }
    id
}

/// The text bytes of a record.
#[inline(always)]
fn bytes_of(record: &NameRecord) -> &'static [u8] {
    // SAFETY: `push` wrote the record's range of the text region before the
    // record's id reached a reader, and nothing writes it again; an id that
    // was never handed out reads an all-zero record, the empty text.
    unsafe {
        std::slice::from_raw_parts(
            (TEXT + record.start as usize) as *const u8,
            (record.len_flags & LEN_MASK) as usize,
        )
    }
}

/// The text of name `id`, canonical WTF-8.
#[inline(always)]
pub fn text(id: u32) -> &'static [u8] {
    bytes_of(crate::get::<NameRecord>(id))
}

/// The text of name `id` as a `str`, unless it has a lone surrogate.
#[inline(always)]
pub fn scalar_text(id: u32) -> Option<&'static str> {
    let record = crate::get::<NameRecord>(id);
    if record.len_flags & NOT_SCALAR != 0 {
        return None;
    }
    // SAFETY: `push` validates bytes; `push_str` accepts only valid UTF-8.
    // Both leave `NOT_SCALAR` clear exactly for UTF-8 text.
    Some(unsafe { std::str::from_utf8_unchecked(bytes_of(record)) })
}

/// The names stored and the bytes they take (records and text), for the
/// memory report.
pub fn usage() -> (usize, usize) {
    let names = NEXT.load(Ordering::Relaxed) as usize;
    let text = CURSOR.load(Ordering::Relaxed);
    (names, text + names * std::mem::size_of::<NameRecord>())
}

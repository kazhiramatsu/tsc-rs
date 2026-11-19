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
//!
//! A thread takes ids and text room in runs of its own ([`ID_RUN`],
//! [`TEXT_RUN`]) and fills them without touching memory another thread
//! writes: ten parsers that appended name by name to one cursor passed its
//! cache line and the lines of the newest records and texts from core to
//! core at every new name. A thread that ends hands what it has not used to
//! the next thread that stores a name ([`Supply::hand_on`]): a process that
//! starts a thread per request keeps filling the same runs. An id nobody has
//! used yet reads as the empty text, like every id never handed out.

use std::cell::Cell;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

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
/// The interner proved that this scalar name cannot need a contextual diagnostic.
const NOT_CONTEXTUAL_IDENTIFIER: u32 = 1 << 30;

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

/// The ids a thread takes at once: 2 KiB of records, whole cache lines on
/// every supported target.
const ID_RUN: u32 = 256;
/// The text bytes a thread takes at once.
const TEXT_RUN: usize = 8 << 10;
/// A text from this length that does not fit what is left of the thread's
/// run takes a range of its own, in whole [`TEXT_ALIGN`]s, and leaves the
/// run as it is; a shorter one starts a new run and gives up less than this
/// of the old one.
const LONG_TEXT: usize = TEXT_RUN / 4;
/// The alignment of a range taken from the text region: no two threads'
/// ranges share a cache line.
const TEXT_ALIGN: usize = 128;

/// What a thread has taken and not yet used.
#[derive(Clone, Copy)]
struct Run {
    id: u32,
    id_end: u32,
    text: usize,
    text_end: usize,
}

impl Run {
    /// Nothing taken: a thread before its first name. `id_end` is zero in
    /// this state only (a run of ids ends at 256 or later).
    const NONE: Self = Self {
        id: 0,
        id_end: 0,
        text: 0,
        text_end: 0,
    };
}

/// Where the runs come from: the two cursors of the regions, and the runs
/// that ended threads did not finish.
struct Supply {
    /// The first id no thread has taken.
    next: AtomicU32,
    /// The first byte of the text region no thread has taken.
    cursor: AtomicUsize,
    /// Unfinished runs of threads that have ended, each for one later thread.
    /// At most as many as threads have stored names at the same time.
    left: Mutex<Vec<Run>>,
}

static SUPPLY: Supply = Supply::new();

impl Supply {
    const fn new() -> Self {
        Self {
            next: AtomicU32::new(0),
            cursor: AtomicUsize::new(0),
            left: Mutex::new(Vec::new()),
        }
    }

    /// An id and `len` text bytes that the holder of `run` alone owns.
    #[inline]
    fn claim(&self, run: &mut Run, len: usize) -> (u32, usize) {
        if run.id == run.id_end {
            self.refill_ids(run);
        }
        let id = run.id;
        run.id += 1;
        let start = if len <= run.text_end - run.text {
            let start = run.text;
            run.text += len;
            start
        } else if len >= LONG_TEXT {
            self.cursor
                .fetch_add(len.next_multiple_of(TEXT_ALIGN), Ordering::Relaxed)
        } else {
            let start = self.cursor.fetch_add(TEXT_RUN, Ordering::Relaxed);
            run.text = start + len;
            run.text_end = start + TEXT_RUN;
            start
        };
        (id, start)
    }

    /// A thread's first name continues a run an ended thread left, ids and
    /// text room; where none is left, or the ids of the one taken were used
    /// up, a new run of ids comes from the cursor.
    #[cold]
    fn refill_ids(&self, run: &mut Run) {
        if run.id_end == 0 {
            let left = self
                .left
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop();
            if let Some(left) = left {
                *run = left;
            }
        }
        if run.id == run.id_end {
            run.id = self.next.fetch_add(ID_RUN, Ordering::Relaxed);
            assert!(
                run.id < u32::MAX - ID_RUN,
                "the name table holds 2^32 - 1 names"
            );
            run.id_end = run.id + ID_RUN;
        }
    }

    /// Keep what an ending thread has not used for a later thread.
    fn hand_on(&self, run: Run) {
        if run.id < run.id_end || run.text < run.text_end {
            self.left
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(run);
        }
    }
}

/// A thread's run; handed on when the thread ends.
struct ThreadRun(Cell<Run>);

impl Drop for ThreadRun {
    fn drop(&mut self) {
        SUPPLY.hand_on(self.0.get());
    }
}

thread_local! {
    static RUN: ThreadRun = const { ThreadRun(Cell::new(Run::NONE)) };
}

/// An id and `len` text bytes that this call alone owns.
#[inline]
fn claim(len: usize) -> (u32, usize) {
    RUN.try_with(|thread| {
        let mut run = thread.0.get();
        let claimed = SUPPLY.claim(&mut run, len);
        thread.0.set(run);
        claimed
    })
    .unwrap_or_else(|_| {
        // The thread's locals are being destroyed and one of them stores a
        // name: a run for this one name, the rest of it for a later thread.
        let mut run = Run::NONE;
        let claimed = SUPPLY.claim(&mut run, len);
        SUPPLY.hand_on(run);
        claimed
    })
}

/// Store a new name's text and return its id. Every call makes a new id;
/// the caller hands the id to readers after this returns.
pub fn push(text: &[u8]) -> u32 {
    push_with_contextual_hint(text, true)
}

/// Store with the interner's contextual-name classification. A false hint
/// must rule out every contextual identifier; unclassified callers use `push`.
pub fn push_with_contextual_hint(text: &[u8], contextual: bool) -> u32 {
    let flags = if std::str::from_utf8(text).is_ok() {
        0
    } else {
        NOT_SCALAR
    };
    push_bytes(
        text,
        flags
            | if contextual || flags & NOT_SCALAR != 0 {
                0
            } else {
                NOT_CONTEXTUAL_IDENTIFIER
            },
    )
}

/// Store a name whose UTF-8 validity is carried by its type.
pub fn push_str(text: &str) -> u32 {
    push_str_with_contextual_hint(text, true)
}

/// The scalar-text form of [`push_with_contextual_hint`].
pub fn push_str_with_contextual_hint(text: &str, contextual: bool) -> u32 {
    push_bytes(
        text.as_bytes(),
        if contextual {
            0
        } else {
            NOT_CONTEXTUAL_IDENTIFIER
        },
    )
}

/// A conservative name hint: legacy records and non-scalar names still take
/// the caller's full validation path. The record's size and text are unchanged.
#[inline(always)]
pub fn may_be_contextual_identifier(id: u32) -> bool {
    let flags = crate::get::<NameRecord>(id).len_flags;
    flags & NOT_CONTEXTUAL_IDENTIFIER == 0
}

fn push_bytes(text: &[u8], flags: u32) -> u32 {
    crate::reserve();
    let len = u32::try_from(text.len())
        .ok()
        .filter(|&len| len <= LEN_MASK)
        .expect("a name has at most 1 GiB of text");
    let (id, start) = claim(text.len());
    assert!(
        start + text.len() <= u32::MAX as usize,
        "the name table holds 4 GiB of text"
    );
    let record = crate::address::<NameRecord>(id);
    crate::system::commit(TEXT + start, text.len());
    crate::system::commit(record, std::mem::size_of::<NameRecord>());
    // SAFETY: this call alone owns the text range and the record (`claim`
    // hands each id and byte of a run out once; a run comes from the
    // cursors' `fetch_add` and has one holder at a time: the thread that
    // took it, then, under the supply's lock, the thread that continues
    // it); both lie in the reservation, which `reserve` mapped, and no
    // reader has the id yet.
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

/// The ids and the bytes (records and text) the threads have taken, for
/// the memory report: the names stored, and beside them the unfinished
/// runs — at most one of each kind per thread that stores names, running
/// or waiting to be continued.
pub fn usage() -> (usize, usize) {
    let names = SUPPLY.next.load(Ordering::Relaxed) as usize;
    let text = SUPPLY.cursor.load(Ordering::Relaxed);
    (names, text + names * std::mem::size_of::<NameRecord>())
}

#[cfg(test)]
#[path = "../tests/unit/names/tests.rs"]
mod tests;

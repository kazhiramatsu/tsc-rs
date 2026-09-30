//! The process-wide table of interned symbol names.
//!
//! Every [`EscapedName`](crate::EscapedName) is a 32-bit id into this table.
//! A name's text is written once, when the name is first interned, and lives
//! for the rest of the process: the readers that print, compare or hash a
//! name take no lock, and the parsers interning identifier tokens in parallel
//! contend only on the shard that owns the name's hash.
//!
//! The table is append-only. A name is at most a few bytes and a program's
//! vocabulary is bounded, so a long-lived process that compiles many programs
//! keeps the union of their names, not a copy per program.

use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use hashbrown::hash_table::{Entry, HashTable};
use rustc_hash::FxHasher;
use tsc_diagnostics::{JsStr, JsString};

/// Names per chunk of the id → text table.
const CHUNK: usize = 1 << 12;
/// Chunks of the id → text table: 16,777,216 names in all.
const CHUNKS: usize = 1 << 12;
/// Lookup shards; a name's shard is chosen by its hash.
const SHARDS: usize = 64;

type Chunk = Box<[OnceLock<&'static JsString>]>;

struct Interner {
    /// id → text, filled once per id before the id is handed out.
    chunks: [OnceLock<Chunk>; CHUNKS],
    /// text → id, sharded by hash. A shard's table is created on first use.
    shards: [OnceLock<Mutex<HashTable<u32>>>; SHARDS],
    next: AtomicU32,
    /// Bytes of interned text, for the memory report.
    bytes: AtomicUsize,
}

static NAMES: Interner = Interner {
    chunks: [const { OnceLock::new() }; CHUNKS],
    shards: [const { OnceLock::new() }; SHARDS],
    next: AtomicU32::new(0),
    bytes: AtomicUsize::new(0),
};

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut hasher = FxHasher::default();
    bytes.hash(&mut hasher);
    hasher.finish()
}

/// The text of an interned name.
#[inline]
#[cfg_attr(feature = "perf-counters", track_caller)]
pub(crate) fn text(id: u32) -> &'static JsString {
    crate::perf::bump(crate::perf::PerfCounter::NamesTextReads);
    crate::perf::name_site(std::panic::Location::caller());
    let id = id as usize;
    NAMES.chunks[id / CHUNK]
        .get()
        .and_then(|chunk| chunk[id % CHUNK].get())
        .expect("a name id is handed out only after its text is written")
}

/// Slots of a thread's cache of recent interns.
const RECENT: usize = 1 << 13;
/// Text bytes a cache slot keeps inline, so that a hit on a short name
/// compares no memory outside the slot.
const RECENT_INLINE: usize = 19;

/// A recently interned name: its hash, id and, when short, its text.
#[derive(Clone, Copy)]
struct Recent {
    hash: u64,
    id: u32,
    len: u8,
    text: [u8; RECENT_INLINE],
}

impl Recent {
    const EMPTY: Self = Self {
        hash: 0,
        id: u32::MAX,
        len: 0,
        text: [0; RECENT_INLINE],
    };

    fn matches(&self, hash: u64, bytes: &[u8]) -> bool {
        self.hash == hash
            && self.id != u32::MAX
            && if bytes.len() <= RECENT_INLINE {
                &self.text[..self.len as usize] == bytes
            } else {
                self::text(self.id).as_bytes() == bytes
            }
    }

    fn remember(hash: u64, id: u32, bytes: &[u8]) -> Self {
        let mut recent = Self {
            hash,
            id,
            len: 0,
            text: [0; RECENT_INLINE],
        };
        if bytes.len() <= RECENT_INLINE {
            recent.len = bytes.len() as u8;
            recent.text[..bytes.len()].copy_from_slice(bytes);
        }
        recent
    }
}

thread_local! {
    /// The names this thread interned recently, direct-mapped by hash: a
    /// parser or binder repeats its names, and a hit takes no shard lock.
    static RECENT_NAMES: RefCell<Box<[Recent]>> =
        RefCell::new(vec![Recent::EMPTY; RECENT].into_boxed_slice());
}

/// The id of `text`, interning it on first sight.
pub(crate) fn intern(text: JsStr<'_>) -> u32 {
    crate::perf::bump(crate::perf::PerfCounter::NamesInternCalls);
    let bytes = text.as_bytes();
    let hash = hash_bytes(bytes);
    let slot = hash as usize % RECENT;
    let recent = RECENT_NAMES.with(|recent| recent.borrow()[slot]);
    if recent.matches(hash, bytes) {
        return recent.id;
    }
    let id = intern_shared(text, hash);
    RECENT_NAMES.with(|recent| recent.borrow_mut()[slot] = Recent::remember(hash, id, bytes));
    id
}

/// The id of `text` in the shared table, interning it on first sight.
fn intern_shared(text: JsStr<'_>, hash: u64) -> u32 {
    let bytes = text.as_bytes();
    let shard = NAMES.shards[(hash >> 58) as usize % SHARDS].get_or_init(Default::default);
    let mut table = shard.lock().expect("name shard poisoned");
    match table.entry(
        hash,
        |&id| self::text(id).as_bytes() == bytes,
        |&id| hash_bytes(self::text(id).as_bytes()),
    ) {
        Entry::Occupied(entry) => *entry.get(),
        Entry::Vacant(entry) => {
            crate::perf::bump(crate::perf::PerfCounter::NamesInternInserts);
            let id = NAMES.next.fetch_add(1, Ordering::Relaxed);
            let slot = id as usize;
            assert!(
                slot < CHUNK * CHUNKS,
                "the name table holds {} names",
                CHUNK * CHUNKS
            );
            let chunk = NAMES.chunks[slot / CHUNK]
                .get_or_init(|| (0..CHUNK).map(|_| OnceLock::new()).collect());
            let stored: &'static JsString = Box::leak(Box::new(text.to_owned()));
            chunk[slot % CHUNK]
                .set(stored)
                .expect("a fresh name id has no text yet");
            NAMES.bytes.fetch_add(bytes.len(), Ordering::Relaxed);
            entry.insert(id);
            id
        }
    }
}

/// Interned names and their text bytes, for the memory report.
pub fn interned_names() -> (usize, usize) {
    (
        NAMES.next.load(Ordering::Relaxed) as usize,
        NAMES.bytes.load(Ordering::Relaxed),
    )
}

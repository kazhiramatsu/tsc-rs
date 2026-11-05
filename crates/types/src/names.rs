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
use std::sync::{Mutex, OnceLock};

use hashbrown::hash_table::{Entry, HashTable};
use rustc_hash::FxHasher;
use tsc_diagnostics::JsStr;

/// Lookup shards; a name's shard is chosen by its hash.
const SHARDS: usize = 64;

/// text → id, sharded by hash. A shard's table is created on first use. The
/// texts themselves live in the store (`tsc_store::names`): a name's record
/// sits at a fixed address computed from its id.
static SHARDS_BY_HASH: [OnceLock<Mutex<HashTable<u32>>>; SHARDS] =
    [const { OnceLock::new() }; SHARDS];

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut hasher = FxHasher::default();
    bytes.hash(&mut hasher);
    hasher.finish()
}

/// The text of an interned name.
#[inline]
#[cfg_attr(feature = "perf-counters", track_caller)]
pub(crate) fn text(id: u32) -> JsStr<'static> {
    crate::perf::bump(crate::perf::PerfCounter::NamesTextReads);
    crate::perf::name_site(std::panic::Location::caller());
    JsStr::from_canonical_bytes(tsc_store::names::text(id))
}

/// The text of an interned name as a `str`, unless it has an unpaired
/// surrogate (checked once, when interned, so readers of a name's scalar
/// text do no UTF-8 validation).
#[inline]
#[cfg_attr(feature = "perf-counters", track_caller)]
pub(crate) fn scalar_text(id: u32) -> Option<&'static str> {
    crate::perf::bump(crate::perf::PerfCounter::NamesTextReads);
    crate::perf::name_site(std::panic::Location::caller());
    tsc_store::names::scalar_text(id)
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
                tsc_store::names::text(self.id) == bytes
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
    // The shard comes from middle bits of the hash: the shard's table takes
    // its bucket from the low bits and its 7-bit tag from the top bits, and
    // a shard chosen by top bits would leave its tags almost all alike.
    let shard = SHARDS_BY_HASH[(hash >> 32) as usize % SHARDS].get_or_init(Default::default);
    let mut table = shard.lock().expect("name shard poisoned");
    match table.entry(
        hash,
        |&id| tsc_store::names::text(id) == bytes,
        |&id| hash_bytes(tsc_store::names::text(id)),
    ) {
        Entry::Occupied(entry) => *entry.get(),
        Entry::Vacant(entry) => {
            crate::perf::bump(crate::perf::PerfCounter::NamesInternInserts);
            // The text and record are written before the id is in the
            // shard, where other threads find it under the lock.
            let id = tsc_store::names::push(bytes);
            entry.insert(id);
            id
        }
    }
}

/// Interned names and the bytes they take (records and text), for the
/// memory report.
pub fn interned_names() -> (usize, usize) {
    tsc_store::names::usage()
}

/// Pilot (never merged): a thread's cache of the identifier names its parser
/// interns, taken by the parser once per file ([`take_name_cache`]) and given
/// back when the parse ends ([`return_name_cache`]), so a lookup reaches it
/// without a thread-local access. A name of at most 16 bytes is keyed exactly
/// by its length and two overlapping little-endian words; a longer one by its
/// hash, its length and its text. A miss goes through `EscapedName::escape`.
pub struct NameCache {
    short: Box<[ShortSlot]>,
    long: Box<[LongSlot]>,
}

const SHORT_BITS: u32 = 13;
const LONG_BITS: u32 = 13;

#[derive(Clone, Copy)]
struct ShortSlot {
    a: u64,
    b: u64,
    len: u32,
    id: u32,
}

#[derive(Clone, Copy)]
struct LongSlot {
    hash: u64,
    id: u32,
    len: u32,
    text: [u8; 24],
}

/// The exact key of a name of at most 16 bytes: two overlapping
/// little-endian words (or bytes) that together cover every byte.
#[inline(always)]
fn short_key(bytes: &[u8]) -> (u64, u64) {
    let len = bytes.len();
    if len >= 8 {
        (
            u64::from_le_bytes(bytes[..8].try_into().expect("eight bytes")),
            u64::from_le_bytes(bytes[len - 8..].try_into().expect("eight bytes")),
        )
    } else if len >= 4 {
        (
            u64::from(u32::from_le_bytes(bytes[..4].try_into().expect("four bytes"))),
            u64::from(u32::from_le_bytes(bytes[len - 4..].try_into().expect("four bytes"))),
        )
    } else if len > 0 {
        (
            u64::from(bytes[0]) | u64::from(bytes[len / 2]) << 8 | u64::from(bytes[len - 1]) << 16,
            0,
        )
    } else {
        (0, 0)
    }
}

impl NameCache {
    fn new() -> Self {
        let short = ShortSlot { a: 0, b: 0, len: u32::MAX, id: u32::MAX };
        let long = LongSlot { hash: 0, id: u32::MAX, len: u32::MAX, text: [0; 24] };
        Self {
            short: vec![short; 1 << SHORT_BITS].into_boxed_slice(),
            long: vec![long; 1 << LONG_BITS].into_boxed_slice(),
        }
    }

    /// The id of `raw` escaped (`EscapedName::escape`).
    #[inline]
    pub(crate) fn escape(&mut self, raw: JsStr<'_>) -> u32 {
        let bytes = raw.as_bytes();
        if bytes.len() <= 16 {
            let (a, b) = short_key(bytes);
            let hash = (a ^ b.rotate_left(29) ^ (bytes.len() as u64) << 59)
                .wrapping_mul(0x9E37_79B9_7F4A_7C15);
            let index = (hash >> (64 - SHORT_BITS)) as usize;
            let slot = &mut self.short[index];
            if slot.len as usize == bytes.len() && slot.a == a && slot.b == b {
                return slot.id;
            }
            let id = crate::EscapedName::escape(raw).index();
            // A name the escape changes (a leading "__") is not cached.
            if !bytes.starts_with(b"__") {
                *slot = ShortSlot { a, b, len: bytes.len() as u32, id };
            }
            id
        } else {
            let hash = hash_bytes(bytes);
            let slot = &mut self.long[hash as usize & ((1 << LONG_BITS) - 1)];
            if slot.hash == hash
                && slot.len as usize == bytes.len()
                && if bytes.len() <= 24 {
                    slot.text[..bytes.len()] == *bytes
                } else {
                    tsc_store::names::text(slot.id) == bytes
                }
            {
                return slot.id;
            }
            let id = crate::EscapedName::escape(raw).index();
            if !bytes.starts_with(b"__") {
                slot.hash = hash;
                slot.id = id;
                slot.len = bytes.len() as u32;
                if bytes.len() <= 24 {
                    slot.text[..bytes.len()].copy_from_slice(bytes);
                }
            }
            id
        }
    }
}

thread_local! {
    static NAME_CACHE: std::cell::Cell<Option<NameCache>> = const { std::cell::Cell::new(None) };
}

/// The thread's name cache for a parse; a new one when the thread has none.
pub fn take_name_cache() -> NameCache {
    NAME_CACHE.with(|cell| cell.take()).unwrap_or_else(NameCache::new)
}

/// Give a parse's name cache back to its thread.
pub fn return_name_cache(cache: NameCache) {
    NAME_CACHE.with(|cell| cell.set(Some(cache)));
}

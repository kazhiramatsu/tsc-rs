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

/// A thread's cache of the escaped names its parses look up. A name of at
/// most 16 bytes is keyed exactly by its length and two overlapping
/// little-endian words, so a hit is a hash multiply and three word compares;
/// a longer one by its hash, its length and its text. A parse holds the
/// cache ([`HeldNameCache`]), so a lookup takes no thread-local access, no
/// borrow flag and no call.
struct NameCache {
    short: Box<[ShortSlot]>,
    long: Box<[LongSlot]>,
}

/// Slots of each of the cache's tables.
const NAME_CACHE_BITS: u32 = 13;

#[derive(Clone, Copy)]
struct ShortSlot {
    a: u64,
    b: u64,
    /// `u32::MAX` while empty (no name is that long).
    len: u32,
    id: u32,
}

#[derive(Clone, Copy)]
struct LongSlot {
    hash: u64,
    id: u32,
    /// `u32::MAX` while empty.
    len: u32,
    /// The text when it fits; a longer text is compared with the table's.
    text: [u8; 24],
}

/// The exact key of a name of at most 16 bytes: two overlapping
/// little-endian words (or, below four bytes, its bytes) that together
/// cover every byte, with the length beside them.
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
            u64::from(u32::from_le_bytes(
                bytes[..4].try_into().expect("four bytes"),
            )),
            u64::from(u32::from_le_bytes(
                bytes[len - 4..].try_into().expect("four bytes"),
            )),
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
        let short = ShortSlot {
            a: 0,
            b: 0,
            len: u32::MAX,
            id: u32::MAX,
        };
        let long = LongSlot {
            hash: 0,
            id: u32::MAX,
            len: u32::MAX,
            text: [0; 24],
        };
        Self {
            short: vec![short; 1 << NAME_CACHE_BITS].into_boxed_slice(),
            long: vec![long; 1 << NAME_CACHE_BITS].into_boxed_slice(),
        }
    }

    /// No tables: what a held cache keeps after giving its tables back.
    fn empty() -> Self {
        Self {
            short: Box::default(),
            long: Box::default(),
        }
    }

    /// The id of `raw` escaped. A name the escape changes (a leading `__`)
    /// is not cached.
    #[inline]
    fn escaped_id(&mut self, raw: JsStr<'_>) -> u32 {
        let bytes = raw.as_bytes();
        if bytes.len() <= 16 {
            let (a, b) = short_key(bytes);
            let hash = (a ^ b.rotate_left(29) ^ (bytes.len() as u64) << 59)
                .wrapping_mul(0x9E37_79B9_7F4A_7C15);
            let slot = &mut self.short[(hash >> (64 - NAME_CACHE_BITS)) as usize];
            if slot.len as usize == bytes.len() && slot.a == a && slot.b == b {
                return slot.id;
            }
            let id = crate::escaped_name::escaped_id(raw);
            if !bytes.starts_with(b"__") {
                *slot = ShortSlot {
                    a,
                    b,
                    len: bytes.len() as u32,
                    id,
                };
            }
            id
        } else {
            let hash = hash_bytes(bytes);
            let slot = &mut self.long[hash as usize & ((1 << NAME_CACHE_BITS) - 1)];
            if slot.hash == hash
                && slot.len as usize == bytes.len()
                && if bytes.len() <= slot.text.len() {
                    slot.text[..bytes.len()] == *bytes
                } else {
                    tsc_store::names::text(slot.id) == bytes
                }
            {
                return slot.id;
            }
            let id = crate::escaped_name::escaped_id(raw);
            if !bytes.starts_with(b"__") {
                slot.hash = hash;
                slot.id = id;
                slot.len = bytes.len() as u32;
                if bytes.len() <= slot.text.len() {
                    slot.text[..bytes.len()].copy_from_slice(bytes);
                }
            }
            id
        }
    }
}

thread_local! {
    /// The name cache of this thread's parses, while no parse holds it.
    static NAME_CACHE: std::cell::Cell<Option<NameCache>> = const { std::cell::Cell::new(None) };
}

/// The thread's name cache, held by one parse: taken from the thread when
/// the parse starts and given back when dropped, so that the parses of a
/// thread share it. A parse that starts while another holds it gets a new
/// one.
pub struct HeldNameCache {
    cache: NameCache,
}

impl HeldNameCache {
    pub fn take() -> Self {
        let cache = NAME_CACHE
            .try_with(std::cell::Cell::take)
            .ok()
            .flatten()
            .unwrap_or_else(NameCache::new);
        Self { cache }
    }

    /// [`EscapedName::escape`](crate::EscapedName::escape) of `raw`, through
    /// the cache.
    #[inline]
    #[cfg_attr(feature = "perf-counters", track_caller)]
    pub fn escape(&mut self, raw: JsStr<'_>) -> crate::EscapedName {
        crate::perf::name_site(std::panic::Location::caller());
        crate::perf::bump(crate::perf::PerfCounter::NamesEscapeCalls);
        crate::EscapedName::from_index(self.cache.escaped_id(raw))
    }
}

impl Drop for HeldNameCache {
    fn drop(&mut self) {
        let cache = std::mem::replace(&mut self.cache, NameCache::empty());
        // A thread past its locals' destruction drops the cache instead.
        let _ = NAME_CACHE.try_with(|cell| cell.set(Some(cache)));
    }
}

#[cfg(test)]
#[path = "../tests/unit/names/tests.rs"]
mod tests;

#[cfg(feature = "mem-accounting")]
pub fn account_memory(a: &mut tsc_store::accounting::Account) {
    tsc_store::names::account_memory(a);
    for shard in &SHARDS_BY_HASH {
        if let Some(shard) = shard.get() {
            let table = shard
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            a.hash_table(
                "names.intern_indexes",
                table.len(),
                4,
                table.num_buckets(),
                table.allocation_size(),
            );
        }
    }
    a.unknown("names.thread_local_recent_caches");
}

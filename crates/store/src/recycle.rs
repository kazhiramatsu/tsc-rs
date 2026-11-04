//! Checker scratch only: allocation-log reset, monotonic escape and nonmoving
//! allocation units. Symbols, Types and Signatures must use permanent owners.
//!
//! Reset cannot overlap a payload borrow:
//! ```compile_fail
//! use tsc_store::recycle::RecyclableArena;
//! let mut arena = RecyclableArena::new();
//! let mark = arena.mark();
//! let id = arena.alloc(&[1u32]);
//! let view = arena.get(id).unwrap();
//! arena.reset(mark);
//! assert_eq!(view[0], 1);
//! ```

use std::marker::PhantomData;
use std::mem::{align_of, size_of};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{StorageStats, StoreRecord};

/// K+17 from the B-3 region ledger, where K is 15 in this baseline.
pub const SCRATCH_WORD_REGION: usize = crate::claimed::FIRST_RECORD_REGION + 17;

#[derive(Clone, Copy)]
#[repr(transparent)]
struct Word {
    _value: u32,
}
impl StoreRecord for Word {
    const REGION: usize = SCRATCH_WORD_REGION;
}

#[cfg(not(miri))]
type Backing = crate::rooms::Room<Word>;

// The same owner/escape/poison code can be checked by Miri without reserving
// fixed OS addresses. This backend is deliberately private to scratch.
#[cfg(any(miri, test))]
#[repr(align(64))]
#[derive(Clone, Copy)]
struct Aligned {
    _bytes: [u8; 64],
}
#[cfg(any(miri, test))]
struct HeapBacking {
    words: std::ptr::NonNull<std::mem::MaybeUninit<Aligned>>,
    capacity: u32,
    dirty: u32,
}
#[cfg(any(miri, test))]
impl HeapBacking {
    fn new(capacity: u32) -> Self {
        Self {
            words: std::ptr::NonNull::new(
                Box::into_raw(Box::<[Aligned]>::new_uninit_slice(capacity as usize / 16))
                    .cast::<std::mem::MaybeUninit<Aligned>>(),
            )
            .unwrap(),
            capacity,
            dirty: 0,
        }
    }
    fn pointer(&self, offset: u32) -> *mut Word {
        assert!(offset <= self.capacity);
        // SAFETY: offset is within the aligned, uniquely owned allocation;
        // no reference to the uninitialized/poisoned payload is formed here.
        unsafe {
            self.words
                .as_ptr()
                .cast::<u8>()
                .add(offset as usize * 4)
                .cast()
        }
    }
    fn initialize_through(&mut self, end: u32) {
        assert!(end <= self.capacity);
        self.dirty = self.dirty.max(end);
    }
}

#[cfg(any(miri, test))]
impl Drop for HeapBacking {
    fn drop(&mut self) {
        // SAFETY: this owner kept the complete allocation and no view can
        // outlive it. MaybeUninit accepts both poison and untouched bytes.
        unsafe {
            drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                self.words.as_ptr(),
                self.capacity as usize / 16,
            )));
        }
    }
}
#[cfg(any(miri, test))]
// SAFETY: the pointer uniquely owns storage, and the enclosing arena controls
// payload access with T's Send/Sync bounds and exclusive mutation.
unsafe impl Send for HeapBacking {}
#[cfg(any(miri, test))]
// SAFETY: shared reads never mutate the backing allocation.
unsafe impl Sync for HeapBacking {}

#[cfg(miri)]
type Backing = HeapBacking;

static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);

/// A checked allocation capability, not a semantic identity. Its out-of-band
/// generation detects stale handles even if the payload and entry are reused.
/// Do not truncate this token to a stored semantic u32 ID.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Allocation<T> {
    owner: u64,
    slot: u32,
    generation: u64,
    marker: PhantomData<fn() -> T>,
}

/// Owned, single-use LIFO mark. It contains no borrowed payload pointers.
/// Checker wrappers also record/restore their semantic stack depths before
/// calling reset; the store cannot observe checker stacks or journals.
#[derive(Debug)]
pub struct ScratchMark {
    owner: u64,
    serial: u64,
    log_len: usize,
}

#[derive(Clone, Copy)]
struct Scope {
    serial: u64,
    log_len: usize,
    created: u64,
}

/// Debug verification can quarantine dead payloads and metadata slots, making
/// an omitted publication barrier fail even without immediate reuse.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReusePolicy {
    Reuse,
    #[cfg(debug_assertions)]
    NeverReuse,
}

/// Result of consuming one scope's allocation-log suffix.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ResetStats {
    pub reclaimed: usize,
    pub retained: usize,
}

#[derive(Clone, Copy)]
struct Run {
    offset: u32,
    words: u32,
}

struct Block {
    room: Backing,
    cursor: u32,
    free: Vec<Run>,
}

struct Entry<T> {
    block: usize,
    run: Run,
    len: u32,
    generation: u64,
    live: bool,
    escaped: bool,
    edges: Vec<Allocation<T>>,
}

/// Single-writer scratch rooms. Reset takes `&mut self`, so no view can remain
/// live across poison, reuse or teardown. Escape graph edges are registered
/// with `add_edge`; the checker must cover every payload edge/publication.
/// `T` may contain niches: only initialized live allocations are decoded.
pub struct RecyclableArena<T: Copy> {
    owner: u64,
    blocks: Vec<Block>,
    entries: Vec<Entry<T>>,
    free_entries: Vec<u32>,
    log: Vec<Allocation<T>>,
    scopes: Vec<Scope>,
    next_scope: u64,
    block_words: u32,
    policy: ReusePolicy,
    created: u64,
    recycled: u64,
    live: usize,
    peak_live: usize,
    marker: PhantomData<T>,
}

impl<T: Copy> RecyclableArena<T> {
    pub fn new() -> Self {
        Self::with_options(1 << 16, ReusePolicy::Reuse)
    }

    pub fn with_options(block_words: u32, policy: ReusePolicy) -> Self {
        assert!(
            size_of::<T>() != 0 && align_of::<T>() <= 64,
            "unsupported scratch element layout"
        );
        let block_words = block_words
            .max(16)
            .checked_next_multiple_of(16)
            .expect("scratch room capacity overflow");
        let owner = NEXT_OWNER
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .expect("scratch owner serial overflow");
        Self {
            owner,
            blocks: Vec::new(),
            entries: Vec::new(),
            free_entries: Vec::new(),
            log: Vec::new(),
            scopes: Vec::new(),
            next_scope: 0,
            block_words,
            policy,
            created: 0,
            recycled: 0,
            live: 0,
            peak_live: 0,
            marker: PhantomData,
        }
    }

    /// Begin a trial scope. Reset in reverse nesting order, after ending all
    /// worker borrows, consuming journals and retaining durable result roots.
    pub fn mark(&mut self) -> ScratchMark {
        self.next_scope = self
            .next_scope
            .checked_add(1)
            .expect("scratch scope serial overflow");
        let scope = Scope {
            serial: self.next_scope,
            log_len: self.log.len(),
            created: self.created,
        };
        self.scopes.push(scope);
        ScratchMark {
            owner: self.owner,
            serial: scope.serial,
            log_len: scope.log_len,
        }
    }

    /// Allocate one indivisible contiguous unit. Outside a marked scope the
    /// unit is born escaped. Oversized units get their own sufficiently large
    /// room, and no allocation ever crosses rooms.
    pub fn alloc(&mut self, elements: &[T]) -> Allocation<T> {
        let len = u32::try_from(elements.len()).expect("scratch allocation exceeds u32");
        let bytes = elements
            .len()
            .checked_mul(size_of::<T>())
            .expect("scratch byte length overflow");
        let alignment = (align_of::<T>().max(4) / 4) as u32;
        let words = u32::try_from(bytes.div_ceil(4))
            .expect("scratch word length overflow")
            .max(1)
            .checked_next_multiple_of(alignment)
            .expect("scratch allocation overflow");
        let generation = self
            .created
            .checked_add(1)
            .expect("scratch generation overflow");
        let (block, run) = self.take_run(words);
        let storage = &mut self.blocks[block];
        // Freed bytes are never decoded as T. On a reused (possibly rewound)
        // prefix, verify poison before initializing the entire new payload.
        #[cfg(debug_assertions)]
        if run.offset < storage.room.dirty {
            let count = words.min(storage.room.dirty - run.offset) as usize * 4;
            // SAFETY: these bytes belonged only to reclaimed allocation units
            // and were fully poisoned, including alignment padding.
            let bytes = unsafe {
                std::slice::from_raw_parts(storage.room.pointer(run.offset).cast::<u8>(), count)
            };
            assert!(
                bytes.iter().all(|&byte| byte == 0xA5),
                "scratch poison was modified after reset"
            );
        }
        storage.room.initialize_through(run.offset + words);
        // SAFETY: the disjoint run is committed and aligned for T; only its
        // initialized element prefix is exposed, never end padding.
        unsafe {
            std::ptr::copy_nonoverlapping(
                elements.as_ptr(),
                storage.room.pointer(run.offset).cast::<T>(),
                elements.len(),
            );
        }
        let entry = Entry {
            block,
            run,
            len,
            generation,
            live: true,
            escaped: self.scopes.is_empty(),
            edges: Vec::new(),
        };
        let slot = if let Some(slot) = self.free_entries.pop() {
            self.entries[slot as usize] = entry;
            slot
        } else {
            let slot = u32::try_from(self.entries.len()).expect("scratch entry count exceeds u32");
            self.entries.push(entry);
            slot
        };
        let id = Allocation {
            owner: self.owner,
            slot,
            generation,
            marker: PhantomData,
        };
        if !self.scopes.is_empty() {
            self.log.push(id);
        }
        self.created = generation;
        self.live += 1;
        self.peak_live = self.peak_live.max(self.live);
        id
    }

    fn take_run(&mut self, words: u32) -> (usize, Run) {
        for (index, block) in self.blocks.iter_mut().enumerate() {
            if let Some(hole) = block.free.iter().position(|run| run.words >= words) {
                let run = block.free[hole];
                if run.words == words {
                    block.free.remove(hole);
                } else {
                    block.free[hole] = Run {
                        offset: run.offset + words,
                        words: run.words - words,
                    };
                }
                return (
                    index,
                    Run {
                        offset: run.offset,
                        words,
                    },
                );
            }
            if block.room.capacity - block.cursor >= words {
                let run = Run {
                    offset: block.cursor,
                    words,
                };
                block.cursor += words;
                return (index, run);
            }
        }
        let capacity = words
            .max(self.block_words)
            .checked_next_multiple_of(16)
            .expect("scratch room overflow");
        self.blocks.push(Block {
            room: Backing::new(capacity),
            cursor: words,
            free: Vec::new(),
        });
        (self.blocks.len() - 1, Run { offset: 0, words })
    }

    fn entry(&self, id: Allocation<T>) -> Option<&Entry<T>> {
        if id.owner != self.owner {
            return None;
        }
        self.entries
            .get(id.slot as usize)
            .filter(|entry| entry.live && entry.generation == id.generation)
    }

    /// Checked live/generation lookup, before any typed decoding of payload.
    pub fn get(&self, id: Allocation<T>) -> Option<&[T]> {
        let entry = self.entry(id)?;
        // SAFETY: entry validated owner, generation and liveness. The elements
        // were initialized together and &self excludes reset or mutation.
        Some(unsafe {
            std::slice::from_raw_parts(
                self.blocks[entry.block]
                    .room
                    .pointer(entry.run.offset)
                    .cast::<T>(),
                entry.len as usize,
            )
        })
    }

    /// Exclusive payload edit. Any newly stored allocation edge must also go
    /// through add_edge before a younger scope can reset. Scalar edits alone
    /// do not change liveness. Does not permit resizing an allocation unit.
    pub fn get_mut(&mut self, id: Allocation<T>) -> Option<&mut [T]> {
        let entry = self.entry(id)?;
        let (block, run, len) = (entry.block, entry.run, entry.len);
        // SAFETY: as in get, plus &mut self excludes every payload borrow.
        Some(unsafe {
            std::slice::from_raw_parts_mut(
                self.blocks[block].room.pointer(run.offset).cast::<T>(),
                len as usize,
            )
        })
    }

    /// Register a strong graph edge, including later backing replacements.
    /// Escaped owners immediately escape new children. An older allocation
    /// receiving a younger-scope child conservatively pins that child too.
    pub fn add_edge(&mut self, parent: Allocation<T>, child: Allocation<T>) {
        let parent_entry = self.entry(parent).expect("invalid scratch parent");
        self.entry(child).expect("invalid scratch child");
        let must_escape = parent_entry.escaped
            || self
                .scopes
                .last()
                .is_some_and(|scope| parent_entry.generation <= scope.created);
        self.entries[parent.slot as usize].edges.push(child);
        if must_escape {
            self.escape(child);
        }
    }

    /// Retain the complete registered graph. Mark before visiting children so
    /// context/mapper cycles terminate. Escaped bits never become clear.
    pub fn escape(&mut self, root: Allocation<T>) {
        let mut work = vec![root];
        while let Some(id) = work.pop() {
            let entry = self.entry(id).expect("invalid scratch escape root/edge");
            if entry.escaped {
                continue;
            }
            let entry = &mut self.entries[id.slot as usize];
            entry.escaped = true;
            work.extend_from_slice(&entry.edges);
        }
    }

    pub fn is_escaped(&self, id: Allocation<T>) -> Option<bool> {
        self.entry(id).map(|entry| entry.escaped)
    }

    /// Consume only this scope's log suffix. Escaped allocations keep their
    /// original position, become permanent, and are never freed by an outer
    /// reset. No semantic state, creation counter or prior scope is rolled back.
    /// Panics for a foreign, stale or non-LIFO mark without reclaiming anything.
    pub fn reset(&mut self, mark: ScratchMark) -> ResetStats {
        let scope = self.scopes.last().expect("reset without a scratch scope");
        assert!(
            mark.owner == self.owner
                && mark.serial == scope.serial
                && mark.log_len == scope.log_len,
            "foreign or non-LIFO scratch mark"
        );
        self.scopes.pop();
        let suffix = self.log.split_off(mark.log_len);
        let mut result = ResetStats::default();
        for id in suffix {
            let entry = self.entry(id).expect("invalid scratch allocation log");
            if entry.escaped {
                result.retained += 1;
                continue;
            }
            let (block, run) = (entry.block, entry.run);
            // First revoke typed access. Poison is only raw byte storage.
            let entry = &mut self.entries[id.slot as usize];
            entry.live = false;
            entry.edges.clear();
            #[cfg(debug_assertions)]
            // SAFETY: all payload references ended before &mut self, and this
            // unit is dead. Poison need not be a valid T or contain valid IDs.
            unsafe {
                std::ptr::write_bytes(
                    self.blocks[block].room.pointer(run.offset).cast::<u8>(),
                    0xA5,
                    run.words as usize * 4,
                );
            }
            if self.policy == ReusePolicy::Reuse {
                self.return_run(block, run);
                self.free_entries.push(id.slot);
            }
            self.live -= 1;
            self.recycled += 1;
            result.reclaimed += 1;
        }
        result
    }

    fn return_run(&mut self, index: usize, run: Run) {
        let block = &mut self.blocks[index];
        block.free.push(run);
        block.free.sort_unstable_by_key(|run| run.offset);
        let mut index = 0;
        while index + 1 < block.free.len() {
            let a = block.free[index];
            let b = block.free[index + 1];
            if a.offset + a.words == b.offset {
                block.free[index].words += b.words;
                block.free.remove(index + 1);
            } else {
                index += 1;
            }
        }
        // Only a completely dead physical suffix rewinds the cursor. Escaped
        // holes keep the same storage, irrespective of log consumption.
        while block
            .free
            .last()
            .is_some_and(|run| run.offset + run.words == block.cursor)
        {
            block.cursor = block.free.pop().unwrap().offset;
        }
    }

    pub fn stats(&self) -> StorageStats {
        StorageStats {
            rooms: self.blocks.len(),
            live_bytes: self
                .entries
                .iter()
                .filter(|e| e.live)
                .map(|e| e.len as usize * size_of::<T>())
                .sum(),
            reserved_bytes: self
                .blocks
                .iter()
                .map(|b| b.room.capacity as usize * 4)
                .sum(),
            touched_bytes: self.blocks.iter().map(|b| b.room.dirty as usize * 4).sum(),
            metadata_bytes: self.blocks.capacity() * size_of::<Block>()
                + self.entries.capacity() * size_of::<Entry<T>>()
                + self
                    .entries
                    .iter()
                    .map(|e| e.edges.capacity() * size_of::<Allocation<T>>())
                    .sum::<usize>()
                + self
                    .blocks
                    .iter()
                    .map(|b| b.free.capacity() * size_of::<Run>())
                    .sum::<usize>()
                + self.free_entries.capacity() * size_of::<u32>()
                + self.log.capacity() * size_of::<Allocation<T>>()
                + self.scopes.capacity() * size_of::<Scope>(),
            created: self.created,
            escaped: self.entries.iter().filter(|e| e.live && e.escaped).count(),
            recycled: self.recycled,
            peak_live: self.peak_live,
        }
    }
}

impl<T: Copy> Default for RecyclableArena<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heap_backend_keeps_raw_storage_aligned_for_miri() {
        let mut room = HeapBacking::new(32);
        room.initialize_through(16);
        let pointer = room.pointer(0).cast::<u8>();
        assert_eq!(pointer as usize % 64, 0);
        // SAFETY: committed heap storage, no typed value or payload borrow.
        unsafe {
            std::ptr::write_bytes(pointer, 0xA5, 64);
            assert_eq!(*pointer.add(63), 0xA5);
            pointer.cast::<u64>().write(42);
            assert_eq!(*pointer.cast::<u64>(), 42);
        }
    }

    #[test]
    fn reset_keeps_escaped_holes_and_reuses_dead_units_with_new_generations() {
        let mut arena = RecyclableArena::with_options(16, ReusePolicy::Reuse);
        let mark = arena.mark();
        let dead = arena.alloc(&[1u32; 4]);
        let live = arena.alloc(&[2; 4]);
        let tail = arena.alloc(&[3; 4]);
        let pointer = arena.get(live).unwrap().as_ptr();
        arena.escape(live);
        assert_eq!(
            arena.reset(mark),
            ResetStats {
                reclaimed: 2,
                retained: 1
            }
        );
        assert!(arena.get(dead).is_none());
        assert!(arena.get(tail).is_none());
        assert_eq!(arena.get(live).unwrap(), &[2; 4]);
        assert_eq!(arena.get(live).unwrap().as_ptr(), pointer);
        let mark = arena.mark();
        let replacement = arena.alloc(&[7; 4]);
        assert_eq!(arena.get(replacement).unwrap(), &[7; 4]);
        assert!(arena.get(dead).is_none());
        assert!(arena.get(tail).is_none());
        arena.reset(mark);
        assert_eq!(arena.stats().created, 4);
        assert_eq!(arena.stats().recycled, 3);
        assert_eq!(arena.stats().escaped, 1);
        assert_eq!(arena.stats().live_bytes, 16);
    }

    #[test]
    fn cyclic_escape_is_transitive_monotonic_and_late_backing_is_pinned() {
        let mut arena = RecyclableArena::new();
        let mark = arena.mark();
        let context = arena.alloc(&[1u32]);
        let mapper = arena.alloc(&[2]);
        let candidates = arena.alloc(&[3; 8]);
        arena.add_edge(context, mapper);
        arena.add_edge(mapper, context);
        arena.add_edge(mapper, candidates);
        arena.escape(mapper);
        arena.escape(context);
        let later = arena.alloc(&[4]);
        arena.add_edge(context, later);
        assert_eq!(
            arena.reset(mark),
            ResetStats {
                reclaimed: 0,
                retained: 4
            }
        );
        for id in [context, mapper, candidates, later] {
            assert_eq!(arena.is_escaped(id), Some(true));
            assert!(arena.get(id).is_some());
        }
    }

    #[test]
    fn nested_scope_retains_younger_children_of_an_unescaped_outer_owner() {
        let mut arena = RecyclableArena::new();
        let outer_mark = arena.mark();
        let outer = arena.alloc(&[1u32]);
        let inner_mark = arena.mark();
        let returned = arena.alloc(&[2]);
        let dead = arena.alloc(&[3]);
        arena.add_edge(outer, returned);
        assert_eq!(
            arena.reset(inner_mark),
            ResetStats {
                reclaimed: 1,
                retained: 1
            }
        );
        assert_eq!(arena.is_escaped(outer), Some(false));
        assert!(arena.get(returned).is_some());
        assert!(arena.get(dead).is_none());
        assert_eq!(arena.reset(outer_mark).reclaimed, 1);
        assert!(arena.get(returned).is_some());
        assert!(arena.get(outer).is_none());
    }

    #[test]
    fn outside_scope_is_permanent_and_independent_owners_reject_handles() {
        let mut arena = RecyclableArena::new();
        let permanent = arena.alloc(&[9u32]);
        let mut other = RecyclableArena::new();
        let foreign = other.alloc(&[10u32]);
        assert!(arena.get(foreign).is_none());
        assert!(other.get(permanent).is_none());
        let mark = arena.mark();
        let backing = arena.alloc(&[11]);
        arena.add_edge(permanent, backing);
        arena.reset(mark);
        assert!(arena.get(backing).is_some());
    }

    #[test]
    fn many_failed_and_successful_trials_grow_without_moving_survivors() {
        let mut arena = RecyclableArena::with_options(16, ReusePolicy::Reuse);
        let survivor = arena.alloc(&[6u32; 5]);
        let pointer = arena.get(survivor).unwrap().as_ptr();
        for trial in 0..100 {
            let mark = arena.mark();
            let large = arena.alloc(&[trial; 100]);
            if trial % 10 == 0 {
                arena.escape(large);
            }
            arena.reset(mark);
            assert_eq!(arena.get(large).is_some(), trial % 10 == 0);
        }
        assert_eq!(arena.get(survivor).unwrap().as_ptr(), pointer);
        assert_eq!(arena.stats().created, 101);
        assert_eq!(arena.stats().recycled, 90);
        assert_eq!(arena.stats().escaped, 11);
        assert!(arena.stats().rooms > 1);
    }

    #[test]
    fn missing_escape_barrier_is_a_negative_control() {
        let mut arena = RecyclableArena::new();
        let mark = arena.mark();
        let forgotten_mapper = arena.alloc(&[123u32]);
        // A checker cache retaining this handle without escape is a bug.
        arena.reset(mark);
        assert!(arena.get(forgotten_mapper).is_none());
        arena.alloc(&[456]);
        assert!(arena.get(forgotten_mapper).is_none());
    }

    #[test]
    fn mutable_views_are_short_and_contiguous_units_are_indivisible() {
        let mut arena = RecyclableArena::new();
        let mark = arena.mark();
        let infos = arena.alloc(&[1u32, 2, 3]);
        arena.get_mut(infos).unwrap()[1] = 8;
        arena.escape(infos); // one retained row pins the whole allocation unit
        arena.reset(mark);
        assert_eq!(arena.get(infos).unwrap(), &[1, 8, 3]);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn poison_is_raw_for_nonzero_elements_and_verified_before_reuse() {
        use std::num::NonZeroU32;
        let mut arena = RecyclableArena::new();
        let mark = arena.mark();
        let id = arena.alloc(&[NonZeroU32::new(1).unwrap()]);
        let entry = arena.entry(id).unwrap();
        let (block, run) = (entry.block, entry.run);
        arena.reset(mark);
        assert!(arena.get(id).is_none());
        let pointer = arena.blocks[block].room.pointer(run.offset).cast::<u8>();
        // No typed view survives reset; inspect/corrupt poison for a negative
        // control, then prove allocation detects it before typed decoding.
        unsafe {
            assert_eq!(*pointer, 0xA5);
            *pointer = 0;
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            arena.alloc(&[NonZeroU32::new(2).unwrap()]);
        }));
        assert!(result.is_err());
    }

    #[cfg(debug_assertions)]
    #[test]
    fn never_reuse_quarantines_dead_payloads() {
        let mut arena = RecyclableArena::with_options(16, ReusePolicy::NeverReuse);
        let mark = arena.mark();
        let id = arena.alloc(&[3u32]);
        let entry = arena.entry(id).unwrap();
        let pointer = arena.blocks[entry.block]
            .room
            .pointer(entry.run.offset)
            .cast::<u8>();
        arena.reset(mark);
        let next = arena.alloc(&[4]);
        assert_ne!(
            pointer.cast_const(),
            arena.get(next).unwrap().as_ptr().cast::<u8>()
        );
        assert!(arena.get(id).is_none());
        assert_eq!(unsafe { *pointer }, 0xA5);
    }

    #[test]
    fn foreign_marks_are_rejected_before_reclamation() {
        let mut first = RecyclableArena::<u32>::new();
        let mut second = RecyclableArena::<u32>::new();
        let foreign = second.mark();
        first.mark();
        let allocation = first.alloc(&[1]);
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| first.reset(foreign)));
        assert!(result.is_err());
        assert_eq!(first.get(allocation), Some([1].as_slice()));
    }
}

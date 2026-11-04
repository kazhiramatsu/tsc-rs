//! Append-only immutable spans. Appending through a shared owner only writes
//! unused slots; exposed elements and old rooms never move or change.
//!
//! A view cannot outlive the owner:
//! ```compile_fail
//! use tsc_store::frozen::FrozenSpans;
//! let view = {
//!     let owner = FrozenSpans::new();
//!     let span = owner.push(&[1u32]);
//!     owner.get(span).unwrap()
//! };
//! assert_eq!(view[0], 1);
//! ```

use std::cell::RefCell;
use std::marker::PhantomData;
use std::mem::{align_of, size_of};

use crate::rooms::Room;
use crate::{StorageStats, StoreRecord};

/// Frozen semantic element words, after A-1a's text/tail regions.
pub const ELEMENT_REGION: usize = crate::tails::TAIL_REGION + 1;

#[derive(Clone, Copy)]
#[repr(transparent)]
struct Word {
    _value: u32,
}
impl StoreRecord for Word {
    const REGION: usize = ELEMENT_REGION;
}

/// A typed span. `start` is a word position in the element region, not a
/// semantic ID or a byte offset. A zero length always denotes an empty slice.
/// Resolve through the owner; this value alone does not keep storage alive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct Span<T> {
    pub start: u32,
    pub len: u32,
    marker: PhantomData<fn() -> T>,
}

impl<T> Span<T> {
    /// Decode stored words. Resolution checks that the complete span was
    /// initialized by this owner; arbitrary input never creates a reference.
    pub const fn from_words(start: u32, len: u32) -> Self {
        Self {
            start,
            len,
            marker: PhantomData,
        }
    }

    pub const fn empty() -> Self {
        Self::from_words(0, 0)
    }
}

/// One word naming a length prefix. Zero is absent; a nonzero ID with a zero
/// prefix is present-empty. Content interning belongs to the consumer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct ListId<T> {
    position: u32,
    marker: PhantomData<fn() -> T>,
}

impl<T> ListId<T> {
    pub const fn absent() -> Self {
        Self::from_word(0)
    }

    /// Decode a stored ID; [`FrozenSpans::get_list`] validates its owner.
    pub const fn from_word(position: u32) -> Self {
        Self {
            position,
            marker: PhantomData,
        }
    }

    pub const fn word(self) -> u32 {
        self.position
    }
}

#[derive(Clone, Copy)]
struct Entry<T> {
    span: Span<T>,
    prefix: u32,
}

struct Block {
    room: Room<Word>,
    used: u32,
}

struct State<T> {
    blocks: Vec<Block>,
    entries: Vec<Entry<T>>,
    live_bytes: usize,
}

/// Nonmoving, single-writer frozen storage. The owner is `Send` when `T` is
/// `Send`, but is not `Sync`: cross-thread publication needs an immutable
/// containing owner, not concurrent calls to this allocation cursor.
///
/// `T` need not be zero-valid: only initialized `Copy` elements are decoded.
/// No references are ever formed over unused, zeroed or released room bytes.
pub struct FrozenSpans<T: Copy> {
    state: RefCell<State<T>>,
    block_words: u32,
    marker: PhantomData<T>,
}

impl<T: Copy> FrozenSpans<T> {
    pub fn new() -> Self {
        Self::with_block_words(1 << 16)
    }

    /// Choose a virtual room estimate. Oversized spans get a sufficiently
    /// large room; a span never straddles rooms. Alignment is at most 64 bytes.
    pub fn with_block_words(words: u32) -> Self {
        assert!(
            size_of::<T>() != 0 && align_of::<T>() <= 64,
            "unsupported frozen element layout"
        );
        let block_words = words
            .max(16)
            .checked_next_multiple_of(16)
            .expect("frozen room capacity overflow");
        Self {
            state: RefCell::new(State {
                blocks: Vec::new(),
                entries: Vec::new(),
                live_bytes: 0,
            }),
            block_words,
            marker: PhantomData,
        }
    }

    /// Copy initialized elements into unused storage. Earlier views remain
    /// valid, including when this append needs a new room.
    pub fn push(&self, elements: &[T]) -> Span<T> {
        if elements.is_empty() {
            return Span::empty();
        }
        self.append(elements, false).span
    }

    /// Append a length-prefixed list, including an allocated present-empty
    /// list. Does not intern or change order/duplicate multiplicity.
    pub fn push_list(&self, elements: &[T]) -> ListId<T> {
        ListId::from_word(self.append(elements, true).prefix)
    }

    fn append(&self, elements: &[T], prefixed: bool) -> Entry<T> {
        let len = u32::try_from(elements.len()).expect("a frozen span exceeds u32");
        let bytes = elements
            .len()
            .checked_mul(size_of::<T>())
            .expect("frozen byte length overflow");
        let alignment = (align_of::<T>().max(4) / 4) as u32;
        let header = if prefixed { alignment } else { 0 };
        let data_words = u32::try_from(bytes.div_ceil(4)).expect("frozen word length overflow");
        let needed = header
            .checked_add(data_words.max(1))
            .and_then(|n| n.checked_next_multiple_of(alignment))
            .expect("frozen allocation overflow");
        let mut state = self.state.borrow_mut();
        if state
            .blocks
            .last()
            .is_none_or(|block| block.room.capacity - block.used < needed)
        {
            let capacity = needed
                .max(self.block_words)
                .checked_next_multiple_of(16)
                .expect("frozen room overflow");
            state.blocks.push(Block {
                room: Room::new(capacity),
                used: 0,
            });
        }
        let block = state.blocks.last_mut().unwrap();
        let offset = block.used;
        let start = block.room.start + offset + header;
        block.room.initialize_through(offset + needed);
        // SAFETY: the new allocation is disjoint from all initialized spans,
        // aligned for T, and entirely in one owned, committed room. Copying
        // initialized T values does not require an all-zero T to be valid.
        unsafe {
            if prefixed {
                block.room.pointer(offset).cast::<u32>().write(len);
            }
            std::ptr::copy_nonoverlapping(
                elements.as_ptr(),
                block.room.pointer(offset + header).cast::<T>(),
                elements.len(),
            );
        }
        let entry = Entry {
            span: Span::from_words(start, len),
            prefix: if prefixed {
                block.room.start + offset
            } else {
                0
            },
        };
        block.used += needed;
        state.live_bytes += bytes + if prefixed { 4 } else { 0 };
        state.entries.push(entry);
        entry
    }

    /// Resolve an exact initialized span through its owner. A forged,
    /// cross-owner, header-overlapping or straddling span is rejected.
    pub fn get(&self, span: Span<T>) -> Option<&[T]> {
        if span.len == 0 {
            return Some(&[]);
        }
        let state = self.state.borrow();
        let index = state
            .entries
            .binary_search_by_key(&span.start, |entry| entry.span.start)
            .ok()?;
        let entry = state.entries[index];
        if entry.span.len != span.len {
            return None;
        }
        // SAFETY: only exact initialized allocations are accepted. No method
        // mutates them, rooms remain until self drops, and the lifetime is
        // that of self, independent of the temporary directory borrow.
        Some(unsafe {
            std::slice::from_raw_parts(
                crate::address::<Word>(span.start) as *const T,
                span.len as usize,
            )
        })
    }

    /// Resolve a present list. None means absent or not allocated by this
    /// owner. Present-empty returns Some(&[]).
    pub fn get_list(&self, id: ListId<T>) -> Option<&[T]> {
        if id.position == 0 {
            return None;
        }
        let state = self.state.borrow();
        // Entries follow monotonically increasing region positions. Bare
        // spans sort between prefixed allocations, so compare their base.
        let index = state
            .entries
            .binary_search_by_key(&id.position, |entry| {
                if entry.prefix == 0 {
                    entry.span.start
                } else {
                    entry.prefix
                }
            })
            .ok()?;
        let entry = state.entries[index];
        if entry.prefix != id.position {
            return None;
        }
        // SAFETY: this entry's prefix was initialized with a u32 length and
        // never changes. Never interpret header/padding bytes as T.
        let len = unsafe { *(crate::address::<Word>(id.position) as *const u32) };
        debug_assert_eq!(len, entry.span.len);
        let span = entry.span;
        drop(state);
        self.get(span)
    }

    /// End allocation and transfer the complete owner into an immutable pool
    /// that can be shared between program/checker threads when T: Send + Sync.
    pub fn seal(self) -> FrozenPool<T> {
        FrozenPool {
            state: self.state.into_inner(),
            marker: PhantomData,
        }
    }

    pub fn stats(&self) -> StorageStats {
        storage_stats(&self.state.borrow())
    }
}

/// Immutable published owner for program-lifetime pools. It has no append or
/// mutation API; thread sharing follows T's Send/Sync bounds. Dropping the last
/// containing Arc returns its rooms after all borrowed views have ended.
pub struct FrozenPool<T: Copy> {
    state: State<T>,
    marker: PhantomData<T>,
}

impl<T: Copy> FrozenPool<T> {
    pub fn get(&self, span: Span<T>) -> Option<&[T]> {
        if span.len == 0 {
            return Some(&[]);
        }
        let index = self
            .state
            .entries
            .binary_search_by_key(&span.start, |entry| entry.span.start)
            .ok()?;
        if self.state.entries[index].span.len != span.len {
            return None;
        }
        // SAFETY: exact initialized span, immutable owner, lifetime tied to
        // self. Sealing transfers the rooms without copying their payload.
        Some(unsafe {
            std::slice::from_raw_parts(
                crate::address::<Word>(span.start) as *const T,
                span.len as usize,
            )
        })
    }

    pub fn get_list(&self, id: ListId<T>) -> Option<&[T]> {
        if id.position == 0 {
            return None;
        }
        let index = self
            .state
            .entries
            .binary_search_by_key(&id.position, |entry| {
                if entry.prefix == 0 {
                    entry.span.start
                } else {
                    entry.prefix
                }
            })
            .ok()?;
        let entry = self.state.entries[index];
        if entry.prefix != id.position {
            return None;
        }
        self.get(entry.span)
    }

    pub fn stats(&self) -> StorageStats {
        storage_stats(&self.state)
    }
}

fn storage_stats<T: Copy>(state: &State<T>) -> StorageStats {
    StorageStats {
        rooms: state.blocks.len(),
        live_bytes: state.live_bytes,
        reserved_bytes: state
            .blocks
            .iter()
            .map(|b| b.room.capacity as usize * 4)
            .sum(),
        touched_bytes: state.blocks.iter().map(|b| b.room.dirty as usize * 4).sum(),
        metadata_bytes: state.blocks.capacity() * size_of::<Block>()
            + state.entries.capacity() * size_of::<Entry<T>>(),
        created: state.entries.len() as u64,
        peak_live: state.entries.len(),
        ..StorageStats::default()
    }
}

impl<T: Copy> Default for FrozenSpans<T> {
    fn default() -> Self {
        Self::new()
    }
}

const _: () = {
    assert!(size_of::<Span<u32>>() == 8);
    assert!(std::mem::offset_of!(Span<u32>, len) == 4);
    assert!(size_of::<ListId<u32>>() == 4);
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU32;

    #[test]
    fn growth_keeps_live_views_and_never_straddles() {
        let owner = FrozenSpans::with_block_words(16);
        let first = owner.push(&[1u32, 2, 3]);
        let view = owner.get(first).unwrap();
        let pointer = view.as_ptr();
        let large = owner.push(&[9; 100]);
        for _ in 0..50 {
            owner.push(&[4; 16]);
        }
        assert_eq!(view, &[1, 2, 3]);
        assert_eq!(pointer, owner.get(first).unwrap().as_ptr());
        assert_eq!(owner.get(large).unwrap(), &[9; 100]);
        assert!(owner.stats().rooms > 2);
        assert!(owner.get(Span::from_words(first.start, 4)).is_none());
        assert!(owner.get(Span::from_words(first.start + 1, 2)).is_none());
    }

    #[test]
    fn nonzero_elements_empty_and_absent_are_distinct() {
        let owner = FrozenSpans::<NonZeroU32>::new();
        let absent = ListId::absent();
        let empty = owner.push_list(&[]);
        let list = owner.push_list(&[NonZeroU32::new(7).unwrap()]);
        assert_ne!(empty.word(), 0);
        assert!(owner.get_list(absent).is_none());
        assert_eq!(owner.get_list(empty), Some([].as_slice()));
        assert_eq!(owner.get_list(list).unwrap()[0].get(), 7);
        assert_eq!(
            owner.get(Span::from_words(u32::MAX, 0)),
            Some([].as_slice())
        );
        assert!(owner.get(Span::from_words(list.word(), 1)).is_none());
    }

    #[test]
    fn owners_reject_each_others_positions_even_after_drop() {
        let first = FrozenSpans::new();
        let span = first.push(&[7u32]);
        let list = first.push_list(&[8]);
        let second = FrozenSpans::new();
        second.push_list(&[9u32]);
        assert!(second.get(span).is_none());
        assert!(second.get_list(list).is_none());
        drop(first);
        let third = FrozenSpans::new();
        third.push_list(&[10u32]);
        assert!(third.get(span).is_none());
    }

    #[test]
    fn append_can_copy_an_existing_view_and_alignment_is_preserved() {
        #[derive(Clone, Copy)]
        #[repr(align(64))]
        struct Aligned(u32);
        let owner = FrozenSpans::with_block_words(16);
        let first = owner.push_list(&[Aligned(42)]);
        let view = owner.get_list(first).unwrap();
        let second = owner.push_list(view);
        assert_eq!(owner.get_list(second).unwrap()[0].0, 42);
        assert_eq!(view.as_ptr() as usize % 64, 0);
        assert_eq!(owner.get_list(second).unwrap().as_ptr() as usize % 64, 0);
    }

    #[test]
    fn sealed_program_pool_is_shared_without_moving_payload() {
        let owner = FrozenSpans::new();
        let list = owner.push_list(&[1u32, 2, 3]);
        let pointer = owner.get_list(list).unwrap().as_ptr();
        let pool = std::sync::Arc::new(owner.seal());
        assert_eq!(pointer, pool.get_list(list).unwrap().as_ptr());
        let thread_owner = std::sync::Arc::clone(&pool);
        std::thread::spawn(move || {
            assert_eq!(thread_owner.get_list(list), Some([1, 2, 3].as_slice()))
        })
        .join()
        .unwrap();
        assert_eq!(pool.stats().created, 1);
    }

    #[test]
    fn present_empty_does_not_hide_the_next_bare_span() {
        let owner = FrozenSpans::new();
        let empty = owner.push_list(&[]);
        let span = owner.push(&[5u32]);
        assert_eq!(owner.get_list(empty), Some([].as_slice()));
        assert_eq!(owner.get(span), Some([5].as_slice()));
        assert!(owner.get_list(ListId::from_word(span.start)).is_none());
    }

    #[test]
    fn drop_scrubs_initialized_payload_and_header() {
        let owner = FrozenSpans::new();
        let list = owner.push_list(&[17u32, 18]);
        let position = list.word();
        drop(owner);
        // Raw-word inspection inside store, not a retained typed view.
        assert_eq!(
            unsafe { *(crate::address::<Word>(position) as *const u32) },
            0
        );
        assert_eq!(
            unsafe { *(crate::address::<Word>(position + 1) as *const u32) },
            0
        );
    }

    #[test]
    #[should_panic(expected = "capacity overflow")]
    fn room_estimate_overflow_is_checked() {
        FrozenSpans::<u32>::with_block_words(u32::MAX);
    }
}

//! The cold words of large node payloads, the records' tails, at fixed
//! positions of their own region.
//!
//! A node record keeps three payload words; a larger payload with a cold field
//! present spills to a tail, and the record names it by position. A payload
//! view is read from a record alone, on a tree that is still being built as on
//! a published one, so a tail cannot live in a buffer of its tree that moves:
//! it is written once at a position that stays, and read with [`tail`].
//!
//! A tree owns its tails in chunks ([`Tails`]), taken from a process-wide
//! allocator and given back, zeroed, when the tree is dropped. Position 0 is
//! never handed out, so a zero word names no tail.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use crate::{address, reserve, slice, system, StoreRecord};

/// The region of the tails' words.
pub(crate) const TAIL_REGION: usize = 13;

/// A word of a tail.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(transparent)]
pub struct TailWord(pub u32);

impl StoreRecord for TailWord {
    const REGION: usize = TAIL_REGION;
}

/// Chunk sizes: `MIN_CHUNK_WORDS << class` words for class `0..CLASSES`. A
/// tree's first chunk is the smallest and each next one doubles, so a tree's
/// unused room stays below its tails' size.
const MIN_CHUNK_WORDS: u32 = 16;
const CLASSES: usize = 9;

/// The next position no chunk has held; position 0 stays unused.
static NEXT: AtomicU32 = AtomicU32::new(MIN_CHUNK_WORDS);

/// The chunks given back, by class, zeroed.
static FREE: Mutex<[Vec<u32>; CLASSES]> = Mutex::new([const { Vec::new() }; CLASSES]);

const fn chunk_words(class: usize) -> u32 {
    MIN_CHUNK_WORDS << class
}

/// A chunk of `class`, zeroed, for the caller alone.
fn take_chunk(class: usize) -> u32 {
    let reused = FREE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)[class]
        .pop();
    if let Some(position) = reused {
        return position;
    }
    reserve();
    let words = chunk_words(class);
    let position = NEXT.fetch_add(words, Ordering::Relaxed);
    assert!(
        position.checked_add(words).is_some(),
        "the tail region is exhausted"
    );
    system::commit(
        address::<TailWord>(position),
        words as usize * std::mem::size_of::<TailWord>(),
    );
    position
}

/// Give back a chunk whose owner is gone: zeroed for its next owner, as a
/// stale read of a dropped tree reads zeros elsewhere in the store.
fn return_chunk(position: u32, class: usize) {
    // SAFETY: the chunk was taken by the owner that returns it, lies in the
    // reservation, and nothing reads it once its tree is dropped.
    unsafe {
        std::ptr::write_bytes(
            address::<TailWord>(position) as *mut TailWord,
            0,
            chunk_words(class) as usize,
        );
    }
    FREE.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)[class]
        .push(position);
}

/// The tails of one tree, in chunks it owns.
#[derive(Debug, Default)]
pub struct Tails {
    /// Each chunk's position and class.
    chunks: Vec<(u32, u8)>,
    /// The next free position of the last chunk, and its end.
    next: u32,
    end: u32,
}

impl Tails {
    pub const fn new() -> Self {
        Self {
            chunks: Vec::new(),
            next: 0,
            end: 0,
        }
    }

    /// Store `words` as one tail and return its position (never 0).
    pub fn push(&mut self, words: &[u32]) -> u32 {
        let len = u32::try_from(words.len()).expect("a tail longer than u32");
        if self.end - self.next < len {
            self.grow(len);
        }
        let position = self.next;
        // SAFETY: the slots lie in a chunk this owner took (see `grow`), and
        // nothing reads them before this returns their position.
        unsafe {
            std::ptr::copy_nonoverlapping(
                words.as_ptr(),
                address::<TailWord>(position) as *mut u32,
                words.len(),
            );
        }
        self.next += len;
        position
    }

    /// Rewrite the tail at `position`, which this owner pushed with as many
    /// words, while no other thread can read it (a tree is shared only after
    /// its last edit).
    pub fn rewrite(&mut self, position: u32, words: &[u32]) {
        let len = u32::try_from(words.len()).expect("a tail longer than u32");
        assert!(
            self.owns(position, len),
            "a tail rewritten by a tree that does not own it"
        );
        // SAFETY: the slots lie in a chunk of this owner (checked above), and
        // `&mut self` stands for the tree's exclusive use.
        unsafe {
            std::ptr::copy_nonoverlapping(
                words.as_ptr(),
                address::<TailWord>(position) as *mut u32,
                words.len(),
            );
        }
    }

    /// The words of the tail at `position`, which this owner pushed with
    /// `len` words, for rewriting in place (see [`rewrite`](Self::rewrite)).
    pub fn words_mut(&mut self, position: u32, len: u32) -> &mut [u32] {
        assert!(
            self.owns(position, len),
            "a tail rewritten by a tree that does not own it"
        );
        // SAFETY: the slots lie in a chunk of this owner (checked above), and
        // `&mut self` stands for the tree's exclusive use; `TailWord` is a
        // transparent `u32`.
        unsafe {
            std::slice::from_raw_parts_mut(address::<TailWord>(position) as *mut u32, len as usize)
        }
    }

    fn owns(&self, position: u32, len: u32) -> bool {
        self.chunks.iter().any(|&(start, class)| {
            position >= start
                && u64::from(position) + u64::from(len)
                    <= u64::from(start) + u64::from(chunk_words(usize::from(class)))
        })
    }

    #[cold]
    fn grow(&mut self, len: u32) {
        let mut class = self.chunks.len().min(CLASSES - 1);
        while chunk_words(class) < len {
            class += 1;
            assert!(class < CLASSES, "a tail longer than a chunk");
        }
        let position = take_chunk(class);
        self.chunks.push((
            position,
            u8::try_from(class).expect("a chunk class fits u8"),
        ));
        self.next = position;
        self.end = position + chunk_words(class);
    }

    /// Heap metadata, separate from the mapped tail-word chunks.
    pub fn heap_metadata_bytes(&self) -> usize {
        self.chunks.capacity() * std::mem::size_of::<(u32, u8)>()
    }

    /// The words of the chunks this tree holds, for memory accounting.
    pub fn chunk_words(&self) -> usize {
        self.chunks
            .iter()
            .map(|&(_, class)| chunk_words(usize::from(class)) as usize)
            .sum()
    }
}

impl Drop for Tails {
    fn drop(&mut self) {
        for &(position, class) in &self.chunks {
            return_chunk(position, usize::from(class));
        }
    }
}

/// The `len` words of the tail at `position`.
///
/// `position` names a tail of a live tree (see the crate documentation).
#[inline(always)]
pub fn tail(position: u32, len: u32) -> &'static [u32] {
    let words: &'static [TailWord] = slice(position, len);
    // SAFETY: `TailWord` is a transparent `u32`.
    unsafe { &*(std::ptr::from_ref(words) as *const [u32]) }
}

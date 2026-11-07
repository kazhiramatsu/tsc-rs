//! Lexically borrowed scratch storage. No static handles are issued.

use std::alloc::{alloc, dealloc, handle_alloc_error, Layout};
use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::mem::{align_of, size_of};
use std::ptr::NonNull;

use crate::arenas::Registration;
use crate::BLOCK_BYTES;

struct Block {
    pointer: NonNull<u8>,
    layout: Layout,
}
impl Drop for Block {
    fn drop(&mut self) {
        // SAFETY: uniquely owned allocation; all scope borrows have ended.
        unsafe {
            dealloc(self.pointer.as_ptr(), self.layout);
        }
    }
}
// SAFETY: blocks move only with the entire exclusively held scratch arena.
unsafe impl Send for Block {}

#[derive(Clone, Copy)]
struct Mark {
    block: usize,
    offset: usize,
    used: usize,
}
struct State {
    blocks: Vec<Block>,
    cursor: Mark,
    scopes: Vec<(u64, Mark)>,
    serial: u64,
    committed: usize,
    registration: Registration,
}

impl State {
    fn allocate(&mut self, scope: u64, bytes: usize, alignment: usize) -> *mut u8 {
        assert_eq!(
            self.scopes.last().map(|s| s.0),
            Some(scope),
            "allocate only in the innermost scratch scope"
        );
        let bytes = bytes.max(1);
        loop {
            if let Some(block) = self.blocks.get(self.cursor.block) {
                let offset = self
                    .cursor
                    .offset
                    .checked_add(alignment - 1)
                    .expect("scratch offset overflow")
                    & !(alignment - 1);
                if alignment <= block.layout.align()
                    && offset <= block.layout.size()
                    && bytes <= block.layout.size() - offset
                {
                    self.cursor.offset = offset + bytes;
                    self.cursor.used += bytes;
                    self.registration.update(self.cursor.used, self.committed);
                    // SAFETY: checked bounds/alignment, unused part of this block.
                    return unsafe { block.pointer.as_ptr().add(offset) };
                }
                self.cursor.block += 1;
                self.cursor.offset = 0;
                continue;
            }
            let layout = Layout::from_size_align(bytes.max(BLOCK_BYTES), alignment.max(8))
                .expect("scratch layout overflow");
            // SAFETY: valid nonzero layout; initialized before a typed read.
            let pointer = unsafe { alloc(layout) };
            let pointer = NonNull::new(pointer).unwrap_or_else(|| handle_alloc_error(layout));
            self.blocks.push(Block { pointer, layout });
            self.committed += layout.size();
        }
    }
}

/// One scratch owner per checker thread. Blocks are retained between scopes.
pub struct ScratchArena {
    state: RefCell<State>,
}
impl Default for ScratchArena {
    fn default() -> Self {
        Self::new()
    }
}
impl ScratchArena {
    pub fn new() -> Self {
        Self::with_owner(0)
    }
    /// Register this scratch arena under a checker owner. `new()` uses owner 0.
    pub fn with_owner(owner: u32) -> Self {
        Self {
            state: RefCell::new(State {
                blocks: Vec::new(),
                cursor: Mark {
                    block: 0,
                    offset: 0,
                    used: 0,
                },
                scopes: Vec::new(),
                serial: 0,
                committed: 0,
                registration: Registration::new(owner, "scratch"),
            }),
        }
    }
    pub fn scope(&mut self) -> ScratchScope<'_> {
        ScratchScope::new(self)
    }
    pub fn used_bytes(&self) -> usize {
        self.state.borrow().cursor.used
    }
    pub fn committed_bytes(&self) -> usize {
        self.state.borrow().committed
    }
}

/// A scope resets its bump cursor on drop. References borrow the scope itself,
/// not merely the underlying arena lifetime, so they cannot survive its drop.
///
/// Children borrow their parent. Parent allocations are rejected while a child
/// is active, enforcing stack discipline even with `child(&self)`.
pub struct ScratchScope<'s> {
    arena: &'s ScratchArena,
    id: u64,
}
impl<'s> ScratchScope<'s> {
    fn new(arena: &'s ScratchArena) -> Self {
        let mut state = arena.state.borrow_mut();
        state.serial = state
            .serial
            .checked_add(1)
            .expect("scratch scope serial exhausted");
        let id = state.serial;
        let mark = state.cursor;
        state.scopes.push((id, mark));
        Self { arena, id }
    }

    /// Interior mutation avoids returning `&mut T` from a shared scope borrow.
    /// The returned reference is tied to this borrow, preventing early reset.
    pub fn alloc<T: Copy>(&self, value: T) -> &ScratchCell<T> {
        let pointer = self
            .arena
            .state
            .borrow_mut()
            .allocate(
                self.id,
                size_of::<ScratchCell<T>>(),
                align_of::<ScratchCell<T>>(),
            )
            .cast::<ScratchCell<T>>();
        // SAFETY: unique, aligned, unused storage; the reference borrows this
        // scope, whose drop is the earliest this storage can be reused.
        unsafe {
            pointer.write(ScratchCell(Cell::new(value)));
            &*pointer
        }
    }

    pub fn vec<T: Copy>(&self) -> ScratchVec<'_, T> {
        ScratchVec {
            state: &self.arena.state,
            scope: self.id,
            pointer: NonNull::dangling(),
            len: 0,
            capacity: 0,
            marker: PhantomData,
        }
    }

    pub fn child(&self) -> ScratchScope<'_> {
        assert_eq!(
            self.arena.state.borrow().scopes.last().map(|s| s.0),
            Some(self.id),
            "child requires innermost scope"
        );
        ScratchScope::new(self.arena)
    }
}
impl Drop for ScratchScope<'_> {
    fn drop(&mut self) {
        let mut state = self.arena.state.borrow_mut();
        let index = state
            .scopes
            .iter()
            .position(|s| s.0 == self.id)
            .expect("missing scratch scope");
        state.cursor = state.scopes[index].1;
        // A forgotten child has no usable borrowed values and may be reclaimed
        // with its parent. This also avoids a double panic during unwinding.
        state.scopes.truncate(index);
        state
            .registration
            .update(state.cursor.used, state.committed);
    }
}

/// Copyable scratch payload with scoped, shared interior mutation.
#[repr(transparent)]
pub struct ScratchCell<T: Copy>(Cell<T>);
impl<T: Copy> ScratchCell<T> {
    pub fn get(&self) -> T {
        self.0.get()
    }
    pub fn set(&self, value: T) {
        self.0.set(value);
    }
}

/// A growing vector whose backing storage belongs to a borrowed scratch scope.
/// Growth copies into a larger allocation and keeps all older storage until reset.
pub struct ScratchVec<'s, T: Copy> {
    state: &'s RefCell<State>,
    scope: u64,
    pointer: NonNull<T>,
    len: usize,
    capacity: usize,
    marker: PhantomData<&'s mut T>,
}
impl<T: Copy> ScratchVec<'_, T> {
    fn reserve(&mut self, additional: usize) {
        let needed = self
            .len
            .checked_add(additional)
            .expect("scratch vector length overflow");
        if needed <= self.capacity {
            return;
        }
        let capacity = needed.max(self.capacity.saturating_mul(2)).max(4);
        let bytes = capacity
            .checked_mul(size_of::<T>())
            .expect("scratch vector size overflow");
        let pointer = self
            .state
            .borrow_mut()
            .allocate(self.scope, bytes, align_of::<T>())
            .cast::<T>();
        // SAFETY: new distinct allocation, valid initialized old elements.
        // NonNull::dangling is aligned and valid for a zero-element copy.
        unsafe {
            std::ptr::copy_nonoverlapping(self.pointer.as_ptr(), pointer, self.len);
        }
        self.pointer = NonNull::new(pointer).unwrap();
        self.capacity = capacity;
    }
    pub fn push(&mut self, value: T) {
        self.reserve(1);
        // SAFETY: reserved unused element, uniquely borrowed vector.
        unsafe {
            self.pointer.as_ptr().add(self.len).write(value);
        }
        self.len += 1;
    }
    pub fn extend(&mut self, values: &[T]) {
        self.reserve(values.len());
        // SAFETY: source cannot alias this mutably borrowed vector; target is unused.
        unsafe {
            std::ptr::copy_nonoverlapping(
                values.as_ptr(),
                self.pointer.as_ptr().add(self.len),
                values.len(),
            );
        }
        self.len += values.len();
    }
    pub fn as_slice(&self) -> &[T] {
        // SAFETY: initialized elements; borrow prevents mutation and scope reset.
        unsafe { std::slice::from_raw_parts(self.pointer.as_ptr(), self.len) }
    }
    pub fn clear(&mut self) {
        self.len = 0;
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

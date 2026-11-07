use std::cell::{Cell, RefCell, UnsafeCell};
use std::collections::BTreeMap;
use std::mem::{align_of, size_of};
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::memory::{self, Run};
use crate::{BlockAllocator, Bytes, List, ListHeader, Record, Ref, Span, BLOCK_BYTES};

/// Snapshot of one live arena. Dropped arenas are removed from the registry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArenaStats {
    pub owner: u32,
    pub kind: &'static str,
    pub used: usize,
    pub committed: usize,
    pub released: bool,
}
struct Entry {
    owner: u32,
    kind: &'static str,
    used: AtomicUsize,
    committed: AtomicUsize,
    released: AtomicBool,
}
pub(crate) struct Registration {
    id: u64,
    entry: Arc<Entry>,
}
impl Registration {
    pub(crate) fn new(owner: u32, kind: &'static str) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let id = NEXT_ID
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .expect("arena registry id exhausted");
        let entry = Arc::new(Entry {
            owner,
            kind,
            used: AtomicUsize::new(0),
            committed: AtomicUsize::new(0),
            released: AtomicBool::new(false),
        });
        registry()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, entry.clone());
        Self { id, entry }
    }
    pub(crate) fn update(&self, used: usize, committed: usize) {
        self.entry.used.store(used, Ordering::Relaxed);
        self.entry.committed.store(committed, Ordering::Relaxed);
    }
}
impl Drop for Registration {
    fn drop(&mut self) {
        self.entry.released.store(true, Ordering::Relaxed);
        registry()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
    }
}
fn registry() -> &'static Mutex<BTreeMap<u64, Arc<Entry>>> {
    static REGISTRY: OnceLock<Mutex<BTreeMap<u64, Arc<Entry>>>> = OnceLock::new();
    REGISTRY.get_or_init(Mutex::default)
}
/// All live arenas, in registration order. Mutable permanent arenas publish
/// used/committed when adding a run; used can lag by the rest of the current
/// run. Their used_bytes() methods compute the exact total on demand. Freeze
/// publishes exact totals. Scratch publishes on allocation/reset. Reading these
/// atomic snapshots never races the owning thread's non-atomic bump pointer.
pub fn report() -> Vec<ArenaStats> {
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .map(|e| ArenaStats {
            owner: e.owner,
            kind: e.kind,
            used: e.used.load(Ordering::Relaxed),
            committed: e.committed.load(Ordering::Relaxed),
            released: e.released.load(Ordering::Relaxed),
        })
        .collect()
}

struct ArenaRun {
    run: Run,
    // Totals for closed runs, captured once at rollover. The current run's
    // usage comes from next, with no counter update on individual allocations.
    used_before: usize,
    committed_before: usize,
}
struct Core {
    next: Cell<usize>,
    end: Cell<usize>,
    runs: RefCell<Vec<ArenaRun>>,
    registration: Registration,
    #[cfg(debug_assertions)]
    thread: Cell<u64>,
    #[cfg(debug_assertions)]
    local: bool,
}
impl Core {
    fn new(owner: u32, kind: &'static str) -> Self {
        assert!(
            owner < u32::MAX - 1,
            "owner collides with reserved owner-table values"
        );
        memory::reserve();
        Self {
            next: Cell::new(0),
            end: Cell::new(0),
            runs: RefCell::new(Vec::new()),
            registration: Registration::new(owner, kind),
            #[cfg(debug_assertions)]
            thread: Cell::new(memory::current_thread_id()),
            #[cfg(debug_assertions)]
            local: kind == "local",
        }
    }

    #[inline(always)]
    fn allocate(&self, bytes: usize, alignment: usize) -> usize {
        assert!(alignment <= 8, "record alignment exceeds eight");
        let bytes = bytes
            .max(1)
            .checked_next_multiple_of(8)
            .expect("arena allocation size overflow");
        assert!(
            bytes as u64 <= crate::RESERVE_BYTES - BLOCK_BYTES as u64,
            "arena allocation exceeds address space"
        );
        #[cfg(debug_assertions)]
        if self.local {
            assert_eq!(
                self.thread.get(),
                memory::current_thread_id(),
                "adopt local arena before thread re-entry"
            );
        }
        let next = self.next.get();
        #[cfg(unix)]
        {
            // Both next and bytes are bounded by the fixed reservation's
            // constants, far below usize::MAX on the supported native targets.
            let bumped = next + bytes;
            if bumped <= self.end.get() {
                self.next.set(bumped);
                return next;
            }
        }
        #[cfg(target_arch = "wasm32")]
        if bytes <= self.end.get() - next {
            self.next.set(next + bytes);
            return next;
        }
        self.allocate_run(bytes)
    }

    #[cold]
    #[inline(never)]
    fn allocate_run(&self, bytes: usize) -> usize {
        let run = BlockAllocator::global()
            .allocate(bytes.div_ceil(BLOCK_BYTES), self.registration.entry.owner);
        #[cfg(debug_assertions)]
        if self.local {
            run.set_thread(self.thread.get());
        }
        let mut runs = self.runs.borrow_mut();
        let (used_before, committed_before) = runs.last().map_or((0, 0), |previous| {
            (
                previous.used_before + (self.next.get() - previous.run.address()),
                previous.committed_before + previous.run.bytes(),
            )
        });
        let pointer = run.address();
        self.next.set(pointer + bytes);
        self.end.set(pointer + run.bytes());
        // Only the slow path publishes snapshots. Prefix totals exclude each
        // closed run's unused tail and avoid rescanning all previous runs.
        runs.push(ArenaRun {
            run,
            used_before,
            committed_before,
        });
        self.registration
            .update(used_before + bytes, committed_before + run.bytes());
        pointer
    }

    #[inline(always)]
    fn alloc<T: Record>(&self, value: T) -> Ref<T> {
        let pointer = self.allocate(size_of::<T>(), align_of::<T>());
        // SAFETY: an aligned, exclusively owned new record slot. Every issued
        // address produces a nonzero handle, including zero-sized records.
        unsafe {
            (pointer as *mut T).write(value);
            Ref::from_allocated(pointer)
        }
    }
    fn slice<T: Record>(&self, values: &[T]) -> Span<T> {
        if values.is_empty() {
            return Span::empty();
        }
        let len = u32::try_from(values.len()).expect("arena slice length exceeds u32");
        let pointer = self.allocate(std::mem::size_of_val(values), align_of::<T>());
        // SAFETY: Record permits bitwise copying; the target is one distinct,
        // contiguous allocation, with no interior or trailing record padding.
        unsafe {
            std::ptr::copy_nonoverlapping(values.as_ptr(), pointer as *mut T, values.len());
        }
        Span::new(memory::raw(pointer), len)
    }
    fn list<T: Record>(&self, values: &[T]) -> Ref<List<T>> {
        let len = u32::try_from(values.len()).expect("arena list length exceeds u32");
        let bytes = std::mem::size_of_val(values)
            .checked_add(8)
            .expect("arena list size overflow");
        let pointer = self.allocate(bytes, align_of::<T>());
        // SAFETY: initialized header and padding-free records in one allocation;
        // the eight-byte header preserves every allowed alignment. Its handle
        // is nonzero by the block allocator's issuance invariant.
        unsafe {
            (pointer as *mut ListHeader).write(ListHeader { len, extra: 0 });
            std::ptr::copy_nonoverlapping(values.as_ptr(), (pointer + 8) as *mut T, values.len());
            Ref::from_allocated(pointer)
        }
    }
    fn bytes(&self, values: &[u8]) -> Ref<Bytes> {
        Ref::from_raw(self.list(values).raw()).unwrap()
    }
    fn owns<T: Record>(&self, r: Ref<T>) -> bool {
        let pointer = memory::address(r.raw());
        let size = size_of::<T>();
        self.runs.borrow().iter().any(|r| {
            pointer >= r.run.address()
                && pointer < r.run.address() + r.run.bytes()
                && size <= r.run.address() + r.run.bytes() - pointer
        })
    }
    fn used(&self) -> usize {
        let runs = self.runs.borrow();
        let Some(current) = runs.last() else {
            return 0;
        };
        current.used_before + (self.next.get() - current.run.address())
    }
    fn committed(&self) -> usize {
        self.runs
            .borrow()
            .last()
            .map_or(0, |r| r.committed_before + r.run.bytes())
    }
    fn adopt(&self) {
        #[cfg(debug_assertions)]
        {
            let current = memory::current_thread_id();
            self.thread.set(current);
            for r in self.runs.borrow().iter() {
                r.run.set_thread(current);
            }
        }
    }
}
impl Drop for Core {
    fn drop(&mut self) {
        for r in self.runs.get_mut().drain(..) {
            BlockAllocator::global().release(r.run);
        }
    }
}

/// One writer before freeze. Send, not Sync; allocations use an interior bump.
pub struct SharedArena {
    core: Core,
}
impl SharedArena {
    pub fn new(owner: u32) -> Self {
        Self {
            core: Core::new(owner, "shared"),
        }
    }
    #[inline(always)]
    pub fn alloc<T: Record + Sync>(&self, value: T) -> Ref<T> {
        self.core.alloc(value)
    }
    pub fn alloc_slice<T: Record + Sync>(&self, values: &[T]) -> Span<T> {
        self.core.slice(values)
    }
    pub fn alloc_list<T: Record + Sync>(&self, values: &[T]) -> Ref<List<T>> {
        self.core.list(values)
    }
    pub fn alloc_bytes(&self, bytes: &[u8]) -> Ref<Bytes> {
        self.core.bytes(bytes)
    }
    /// Exclusive pre-freeze mutation. End all overlapping Ref views first;
    /// the argument must be an issued T of this arena, not a forged handle.
    pub fn get_mut<T: Record>(&mut self, r: Ref<T>) -> &mut T {
        assert!(
            self.core.owns(r),
            "mutable handle belongs to a different arena"
        );
        // SAFETY: &mut self excludes arena mutation; holder contract additionally
        // excludes aliased Ref views, and proves the original allocation's type.
        unsafe { &mut *(memory::address(r.raw()) as *mut T) }
    }
    pub fn freeze(self) -> FrozenArena {
        let mut registry = registry().lock().unwrap_or_else(|e| e.into_inner());
        // The sole live Arc is in Core plus the registry. Under the registry
        // lock no report retains an Arc; replace the immutable kind metadata.
        let mut core = self.core;
        let entry = Arc::new(Entry {
            owner: core.registration.entry.owner,
            kind: "frozen",
            used: AtomicUsize::new(core.used()),
            committed: AtomicUsize::new(core.committed()),
            released: AtomicBool::new(false),
        });
        registry.insert(core.registration.id, entry.clone());
        core.registration.entry = entry;
        FrozenArena { core }
    }
    pub fn used_bytes(&self) -> usize {
        self.core.used()
    }
    pub fn committed_bytes(&self) -> usize {
        self.core.committed()
    }
}

/// Read-only after publication. The last Arc's drop releases all its blocks.
pub struct FrozenArena {
    core: Core,
}
// SAFETY: construction consumes SharedArena; there is no mutation API. All
// records were admitted with Sync, and the last owner exclusively drops Core.
unsafe impl Sync for FrozenArena {}
impl FrozenArena {
    pub fn used_bytes(&self) -> usize {
        self.core.registration.entry.used.load(Ordering::Relaxed)
    }
    pub fn committed_bytes(&self) -> usize {
        self.core
            .registration
            .entry
            .committed
            .load(Ordering::Relaxed)
    }
    /// Optional explicit consuming release, equivalent to dropping this value.
    /// There must be no remaining payload accesses (crate lifetime contract).
    pub fn release(self) {
        drop(self);
    }
}

/// Single-threaded permanent mutable records. Move handles only with their
/// exclusive checker owner, then adopt its new thread before re-entry.
pub struct LocalArena {
    core: Core,
}
impl LocalArena {
    pub fn new(owner: u32) -> Self {
        Self {
            core: Core::new(owner, "local"),
        }
    }
    #[inline(always)]
    pub fn alloc<T: Record>(&self, value: T) -> Ref<T> {
        self.core.alloc(value)
    }
    pub fn alloc_slice<T: Record>(&self, values: &[T]) -> Span<T> {
        self.core.slice(values)
    }
    pub fn alloc_list<T: Record>(&self, values: &[T]) -> Ref<List<T>> {
        self.core.list(values)
    }
    pub fn alloc_bytes(&self, bytes: &[u8]) -> Ref<Bytes> {
        self.core.bytes(bytes)
    }
    pub fn used_bytes(&self) -> usize {
        self.core.used()
    }
    pub fn committed_bytes(&self) -> usize {
        self.core.committed()
    }
    /// Debug bookkeeping after an exclusive Owned transfer; no-op otherwise.
    pub fn adopt_current_thread(&self) {
        self.core.adopt();
    }
}

/// Mutable POD field. Not Sync: cell-bearing records cannot enter SharedArena.
#[repr(transparent)]
pub struct ArenaCell<T: Copy>(UnsafeCell<T>);
impl<T: Copy> ArenaCell<T> {
    pub const fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }
    #[inline(always)]
    fn assert_thread(&self) {
        #[cfg(debug_assertions)]
        memory::assert_thread(self as *const _ as usize);
    }
    #[inline(always)]
    pub fn get(&self) -> T {
        self.assert_thread();
        // SAFETY: !Sync prevents concurrent shared access; Owned transfers
        // require exclusive access and thread adoption under the crate contract.
        unsafe { *self.0.get() }
    }
    #[inline(always)]
    pub fn set(&self, value: T) {
        self.assert_thread();
        // SAFETY: same single-thread ownership as get; UnsafeCell permits writes.
        unsafe {
            *self.0.get() = value;
        }
    }
}
// SAFETY: transparent Copy payload accepts all bits; UnsafeCell permits its
// interior mutation. Neither field has Drop glue or imposes extra validity.
unsafe impl<T: Copy + Record> Record for ArenaCell<T> {}

/// Audited checker-pool escape hatch: exclusive access, one thread at a time;
/// the wrapped value's handles are used only through this exclusive access.
///
/// The caller must ensure the complete wrapped value may migrate threads and
/// has no outside aliases to thread-affine state (e.g. cloned Rc values). Adopt
/// all local arenas before re-entry. This blanket wrapper does not prove those
/// conditions; using it is part of the legacy caller contract, like Ref::get.
pub struct Owned<T>(pub T);
// SAFETY: conditional on the explicit exclusive migration contract above.
unsafe impl<T> Send for Owned<T> {}
impl<T> Deref for Owned<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}
impl<T> DerefMut for Owned<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

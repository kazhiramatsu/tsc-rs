//! Fixed-address stores of a program's records.
//!
//! tsgo's checker reads `node.Parent` and `symbol.Flags` by following a
//! pointer. tsc-rs names nodes and symbols with `u32` identities, and an
//! identity used to reach its record through a per-file route: the owning
//! file's interval, its slice, a bounds check. Those routes were a tenth of
//! the instructions of a check, and the identities are process-unique, so an
//! identity can simply be the record's position: each record type has one
//! reserved address range, and `base + identity * size` is its address. The
//! bases are constants, so a read is one multiply-add and one load, with no
//! load of a base, a table or a bound.
//!
//! This is the one crate of the compiler with `unsafe` code. Every other
//! crate stays `#![forbid(unsafe_code)]` and sees two safe faces:
//!
//! - [`StoreVec`], a `Vec`-like owner: a tree or symbol arena is built on the
//!   heap at private identities, then [`published`](StoreVec::publish) to its
//!   final leased range, where it stays until it is dropped.
//! - [`get`] and [`slice`], the identity-indexed reads.
//! - [`names`], the storage of the process-wide name table.
//! - [`texts`], the literal texts of a tree, stored as words.
//! - [`tails`], the cold words of large node payloads, at fixed positions.
//!
//! New data-layer owners use owner-scoped reads: [`claimed::ClaimedRecords`]
//! for permanent headers, [`frozen::FrozenSpans`] for immutable lists, and
//! [`recycle::RecyclableArena`] for scratch. These never return bare static
//! payload views. Legacy get/slice retain their existing caller contract.
//!
//! [`SmallList`] is the list a stored record can hold.
//!
//! **Contract.** A range is published by the one holder of its identity lease
//! (leases never overlap, in any identity domain), and no record is read
//! before the publication reached the reader, which the `Arc` that hands a
//! document to another thread guarantees. Reads name identities of live
//! documents: a Program holds an `Arc` of every document it reads. Records
//! are zeroed when they are dropped, and an all-zero record of the record
//! types stored here is a valid value, so a stale identity reads an empty
//! record rather than a dangling one.
//!
//! The stores take address space only: untouched pages cost nothing. They
//! need a user address space of 47 bits (macOS, Windows and Linux on x86-64
//! and arm64 with 48-bit addressing); a system that refuses a range stops the
//! process where the range is first needed, with the reason.
//!
//! On Unix a region is mapped as its ranges are committed, a gigabyte of
//! address space at a time (see `system::commit`): the kernel's work for a
//! mapping grows with its size whether or not a page is touched (on macOS
//! one mapping of the whole 10 TiB span cost every process about 27 ms and
//! 77 M instructions, 10 ms in `mmap` and the rest when the process exits),
//! and a compile uses a few gigabytes of it. A region's mapped prefix only
//! grows, so every range that was ever published stays readable.

#[cfg(not(target_pointer_width = "64"))]
compile_error!("the stores need a 64-bit address space");

use std::mem::{ManuallyDrop, MaybeUninit};
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;
use std::sync::Once;

#[cfg(not(target_family = "wasm"))]
mod allocator;
#[cfg(not(target_family = "wasm"))]
pub use allocator::MiMalloc;

/// Resident bytes in complete pages entirely inside a Vec's unused requested
/// capacity. This observes mappings without reading/faulting the spare room.
/// It is neither allocator usable size nor a physical-footprint measurement.
/// Unsupported platforms or failed OS queries return None.
pub fn resident_vec_tail_bytes<T>(values: &Vec<T>) -> Option<usize> {
    if values.capacity() == values.len() || std::mem::size_of::<T>() == 0 {
        return Some(0);
    }
    #[cfg(unix)]
    {
        // SAFETY: sysconf takes a constant selector and no pointers.
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        let page_size = usize::try_from(page_size).ok().filter(|size| *size != 0)?;
        let start = values.as_ptr() as usize;
        let used_end = start.checked_add(values.len().checked_mul(std::mem::size_of::<T>())?)?;
        let allocated_end =
            start.checked_add(values.capacity().checked_mul(std::mem::size_of::<T>())?)?;
        let first = used_end.checked_add(page_size - 1)? / page_size * page_size;
        let end = allocated_end / page_size * page_size;
        if end <= first {
            return Some(0);
        }
        let bytes = end - first;
        let mut residency = vec![0u8; bytes / page_size];
        // SAFETY: the query range contains only complete pages within the
        // live Vec allocation. mincore observes page state without reading
        // its uninitialized contents. The output holds one byte per page.
        let result = unsafe {
            libc::mincore(
                first as *mut libc::c_void,
                bytes,
                residency.as_mut_ptr().cast(),
            )
        };
        (result == 0).then(|| residency.iter().filter(|flag| **flag & 1 != 0).count() * page_size)
    }
    #[cfg(not(unix))]
    {
        None
    }
}

pub mod claimed;
pub mod frozen;
pub mod recycle;
mod rooms;

pub use rooms::{allocator_stats, AllocatorStats, RegionStats, StorageStats};

pub mod names;
pub mod shared_text;
pub mod tails;
pub mod texts;

/// Cumulative retired instructions for opt-in phase tracing. A missing or
/// unavailable platform counter is reported as `None`, never as zero work.
pub fn process_instructions() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        let mut info = std::mem::MaybeUninit::<libc::rusage_info_v4>::zeroed();
        // SAFETY: the SDK V4 call writes a full, correctly aligned V4 buffer.
        let result = unsafe {
            libc::proc_pid_rusage(
                libc::getpid(),
                libc::RUSAGE_INFO_V4,
                info.as_mut_ptr().cast(),
            )
        };
        if result != 0 {
            return None;
        }
        // SAFETY: the successful call initialized the zeroed V4 buffer.
        let instructions = unsafe { info.assume_init() }.ri_instructions;
        (instructions != 0).then_some(instructions)
    }
    #[cfg(not(target_os = "macos"))]
    None
}
pub use shared_text::SharedText;

/// Where the reservation starts: clear of every placement seen on macOS and
/// Linux (binaries, shared caches and heaps sit well below, stacks and PIE
/// binaries above), and a single 16-bit chunk so that one `movz` on arm64
/// materializes each region's base.
const FIRST_REGION: usize = 0x4000_0000_0000;

/// One record type's address range: `2^32` slots of up to 64 bytes each.
const REGION_BYTES: usize = 1 << 38;

/// The regions the reservation holds. A record type names its region with
/// [`StoreRecord::REGION`]; regions 8 and 9 hold the name table
/// ([`names`]), region 12 the trees' literal texts ([`texts`]), region 13 the
/// node records' tails ([`tails`]). Region 14 holds frozen elements; 15..40
/// are B-3's typed records and scratch words (see [`claimed`], [`recycle`]),
/// 37..40 the declaration items and the flow nodes and items.
/// Two further regions belong only to the store unit-test binary.
const REGIONS: usize = claimed::RECORD_REGION_END + 2 * cfg!(test) as usize;

/// The size of the pages the kernel hands out, which covers both 4 KiB and
/// 16 KiB systems: only whole multiples of it are returned to the kernel.
const RETURN_GRANULE: usize = 1 << 14;

/// Below this many bytes a dropped range is zeroed in place, and its pages
/// stay resident for the next tree.
const RETURN_THRESHOLD: usize = 1 << 20;

/// How much of a claimed room a debug build reads to see that the room is
/// free (see [`StoreVec::claim_room`]).
const ROOM_HEAD_BYTES: usize = RETURN_GRANULE;

/// A record kept in a store.
///
/// All-zero bytes must be a valid `Self` (see the crate documentation), and
/// `REGION` must be unique among the record types of the process.
pub trait StoreRecord: Sized + 'static {
    /// Which of the reservation's regions holds this record type.
    const REGION: usize;
}

const fn base<T: StoreRecord>() -> usize {
    assert!(T::REGION < REGIONS, "a store region out of range");
    assert!(
        T::REGION != names::TEXT_REGION,
        "the name table's text region holds no records"
    );
    assert!(
        std::mem::size_of::<T>() <= 64,
        "a record larger than a region's slots"
    );
    assert!(
        std::mem::align_of::<T>() <= 64,
        "a record aligned beyond its slots"
    );
    FIRST_REGION + T::REGION * REGION_BYTES
}

/// The address of the record at `position`.
#[inline(always)]
const fn address<T: StoreRecord>(position: u32) -> usize {
    (const { base::<T>() }) + position as usize * std::mem::size_of::<T>()
}

/// The record at `position`.
///
/// `position` lies in a range published by a live [`StoreVec`] (see the crate
/// documentation).
#[inline(always)]
pub fn get<T: StoreRecord>(position: u32) -> &'static T {
    debug_assert!(reserved(), "a store read before any publication");
    // SAFETY: the address lies in the reservation, which is mapped before any
    // range is published and so before any identity reaches a reader; slots
    // are aligned to `T`, the base being a multiple of every record alignment
    // and the offset a multiple of the size. Every slot holds a published `T`
    // or zeros, which are a valid `T` (`StoreRecord`).
    unsafe { &*(address::<T>(position) as *const T) }
}

/// The `len` records from `position` (see [`get`]).
#[inline(always)]
pub fn slice<T: StoreRecord>(position: u32, len: u32) -> &'static [T] {
    debug_assert!(reserved(), "a store read before any publication");
    debug_assert!(u64::from(position) + u64::from(len) <= 1 << 32);
    // SAFETY: as in `get`, for each of the `len` slots.
    unsafe { std::slice::from_raw_parts(address::<T>(position) as *const T, len as usize) }
}

static RESERVE: Once = Once::new();

/// Whether the reservation exists.
fn reserved() -> bool {
    RESERVE.is_completed()
}

/// Prepare the address range; the first publication does this once. Windows
/// reserves the whole span here; Unix maps each region as it is committed.
fn reserve() {
    RESERVE.call_once(|| {
        let bytes = REGIONS * REGION_BYTES;
        assert!(
            system::reserve(FIRST_REGION, bytes),
            "could not reserve {bytes} bytes of address space at {FIRST_REGION:#x}: {}",
            std::io::Error::last_os_error()
        );
    });
}

/// Whether the `bytes` from `start` read as zeros.
///
/// # Safety
///
/// The bytes are committed memory of a region.
unsafe fn reads_as_zeros(start: usize, bytes: usize) -> bool {
    // SAFETY: the caller's contract, for each byte.
    (0..bytes).all(|offset| unsafe { *((start + offset) as *const u8) } == 0)
}

/// Return the bytes of a dropped range to the zeroed state.
fn scrub(start: usize, bytes: usize) {
    if bytes < RETURN_THRESHOLD {
        // SAFETY: the caller owned this published range and dropped its
        // records; the store's memory is writable.
        unsafe { std::ptr::write_bytes(start as *mut u8, 0, bytes) };
        return;
    }
    let first = start.next_multiple_of(RETURN_GRANULE);
    let last = (start + bytes) / RETURN_GRANULE * RETURN_GRANULE;
    // SAFETY: as above, for the partial pages at both ends of the range.
    unsafe {
        std::ptr::write_bytes(start as *mut u8, 0, first - start);
        std::ptr::write_bytes(last as *mut u8, 0, start + bytes - last);
    }
    system::reset(first, last - first);
}

/// The system's side of the stores on Unix, where a private anonymous
/// mapping reads as zeros and takes memory only for the pages written.
#[cfg(unix)]
mod system {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Mutex, PoisonError};

    use super::{FIRST_REGION, REGIONS, REGION_BYTES};

    /// How many bytes of each region are mapped, from the region's base. A
    /// region's mapping only grows, in whole [`STEP`]s, and is never
    /// unmapped: a range that was committed once stays readable (as zeros
    /// after its records are dropped).
    static MAPPED: [AtomicUsize; REGIONS] = [const { AtomicUsize::new(0) }; REGIONS];

    /// Serializes the growth of the regions' mappings.
    static GROWING: Mutex<()> = Mutex::new(());

    /// The address space a region's mapping grows by: mapping and later
    /// unmapping a gigabyte costs the kernel a few microseconds, and a
    /// region of a large program needs a handful of steps.
    const STEP: usize = 1 << 30;

    /// Nothing is mapped ahead of use: see [`commit`].
    pub(super) fn reserve(_start: usize, _bytes: usize) -> bool {
        true
    }

    /// Back a range of one region with memory before it is read or written:
    /// the region's mapping grows to cover it.
    #[inline]
    pub(super) fn commit(start: usize, bytes: usize) {
        if bytes == 0 {
            return;
        }
        let offset = start - FIRST_REGION;
        let region = offset / REGION_BYTES;
        let end = offset % REGION_BYTES + bytes;
        if end > MAPPED[region].load(Ordering::Acquire) {
            grow(region, end);
        }
    }

    /// Map region `region` through its first `end` bytes.
    #[cold]
    #[inline(never)]
    fn grow(region: usize, end: usize) {
        assert!(
            end <= REGION_BYTES,
            "a store range past the end of its region"
        );
        let _growing = GROWING.lock().unwrap_or_else(PoisonError::into_inner);
        let mapped = MAPPED[region].load(Ordering::Relaxed);
        if end <= mapped {
            return;
        }
        let target = end.next_multiple_of(STEP).min(REGION_BYTES);
        let start = FIRST_REGION + region * REGION_BYTES + mapped;
        let bytes = target - mapped;
        #[cfg(target_os = "linux")]
        let placement = libc::MAP_FIXED_NOREPLACE;
        #[cfg(not(target_os = "linux"))]
        let placement = 0;
        // SAFETY: an anonymous private mapping at a hint address in this
        // region, right behind what the region has mapped; a kernel that
        // cannot honor the hint returns another address or fails, and
        // nothing has been placed in the range yet.
        let got = unsafe {
            libc::mmap(
                start as *mut libc::c_void,
                bytes,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANON | libc::MAP_NORESERVE | placement,
                -1,
                0,
            )
        };
        assert!(
            got as usize == start,
            "could not map {bytes} bytes of a store at {start:#x}: {}",
            std::io::Error::last_os_error()
        );
        MAPPED[region].store(target, Ordering::Release);
    }

    /// A private mapping of `bytes`, anywhere.
    pub(super) fn map(bytes: usize) -> *mut u8 {
        // SAFETY: an anonymous private mapping with no placement request.
        let mapped = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                bytes,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANON,
                -1,
                0,
            )
        };
        assert!(
            mapped != libc::MAP_FAILED,
            "could not map {bytes} bytes for a list: {}",
            std::io::Error::last_os_error()
        );
        mapped.cast()
    }

    /// Release a mapping made by [`map`].
    pub(super) fn unmap(start: *mut u8, bytes: usize) {
        // SAFETY: the mapping was made by `map` with these bounds and is not
        // used past this point.
        let failed = unsafe { libc::munmap(start.cast(), bytes) } != 0;
        assert!(
            !failed,
            "could not unmap a list's {bytes} bytes: {}",
            std::io::Error::last_os_error()
        );
    }

    /// Give whole pages back; they read as zeros from here on. Pages past
    /// the region's mapping were never touched and stay unmapped.
    pub(super) fn reset(start: usize, bytes: usize) {
        let offset = start - FIRST_REGION;
        let begin = offset % REGION_BYTES;
        let mapped = MAPPED[offset / REGION_BYTES].load(Ordering::Acquire);
        if begin >= mapped {
            return;
        }
        let bytes = bytes.min(mapped - begin);
        if bytes == 0 {
            return;
        }
        // SAFETY: remapping whole pages of a region's mapping over
        // themselves gives zero pages back and returns the old ones; only
        // this range's owner touches them.
        let mapped = unsafe {
            libc::mmap(
                start as *mut libc::c_void,
                bytes,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANON | libc::MAP_NORESERVE | libc::MAP_FIXED,
                -1,
                0,
            )
        };
        assert!(
            mapped as usize == start,
            "could not return a dropped store range: {}",
            std::io::Error::last_os_error()
        );
    }
}

/// The system's side of the reservation on Windows, which commits memory
/// before it is touched.
#[cfg(windows)]
mod system {
    use std::ffi::c_void;

    const MEM_COMMIT: u32 = 0x1000;
    const MEM_RESERVE: u32 = 0x2000;
    const MEM_RELEASE: u32 = 0x8000;
    const PAGE_READWRITE: u32 = 0x04;

    #[link(name = "kernel32")]
    extern "system" {
        fn VirtualAlloc(address: *mut c_void, size: usize, kind: u32, protect: u32) -> *mut c_void;
        fn VirtualFree(address: *mut c_void, size: usize, kind: u32) -> i32;
    }

    /// Reserve `bytes` at the fixed address `start`.
    pub(super) fn reserve(start: usize, bytes: usize) -> bool {
        // SAFETY: a reservation of free address space; nothing has been
        // placed in the range yet.
        let reserved =
            unsafe { VirtualAlloc(start as *mut c_void, bytes, MEM_RESERVE, PAGE_READWRITE) };
        reserved as usize == start
    }

    /// Back a range with memory before it is written.
    pub(super) fn commit(start: usize, bytes: usize) {
        if bytes == 0 {
            return;
        }
        // SAFETY: a range of the reservation; committing committed pages
        // leaves them as they are.
        let committed =
            unsafe { VirtualAlloc(start as *mut c_void, bytes, MEM_COMMIT, PAGE_READWRITE) };
        assert!(
            !committed.is_null(),
            "could not commit {bytes} bytes of a store: {}",
            std::io::Error::last_os_error()
        );
    }

    /// Zero whole pages, which stay committed: a decommitted page would fault
    /// on a stale identity instead of reading as zeros.
    pub(super) fn reset(start: usize, bytes: usize) {
        // SAFETY: the pages are committed, and only this range's owner
        // touches them.
        unsafe { std::ptr::write_bytes(start as *mut u8, 0, bytes) };
    }

    /// A committed mapping of `bytes`, anywhere.
    pub(super) fn map(bytes: usize) -> *mut u8 {
        // SAFETY: a fresh mapping with no placement request.
        let mapped = unsafe {
            VirtualAlloc(
                std::ptr::null_mut(),
                bytes,
                MEM_RESERVE | MEM_COMMIT,
                PAGE_READWRITE,
            )
        };
        assert!(
            !mapped.is_null(),
            "could not map {bytes} bytes for a list: {}",
            std::io::Error::last_os_error()
        );
        mapped.cast()
    }

    /// Release a mapping made by [`map`].
    pub(super) fn unmap(start: *mut u8, _bytes: usize) {
        // SAFETY: the mapping was made by `map` and is not used past this
        // point; a release names the whole mapping with size zero.
        let failed = unsafe { VirtualFree(start.cast(), 0, MEM_RELEASE) } == 0;
        assert!(
            !failed,
            "could not unmap a list: {}",
            std::io::Error::last_os_error()
        );
    }
}

/// From this many bytes a list's buffer is a mapping of its own rather than
/// an allocation: a parsed tree's lists are freed together when the tree
/// moves to the store, and pages that only ever held one buffer go back to
/// the system at once, while an allocator keeps a page until every object
/// in it is gone.
const MAPPED_THRESHOLD: usize = 64 << 10;

/// Where a list's records are.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Placement {
    /// A heap allocation, or no buffer at all.
    Heap,
    /// A mapping of the list's own (see [`MAPPED_THRESHOLD`]).
    Mapped,
    /// Slots of the store the list fills, leased before its records were
    /// counted; it moves out if it outgrows them.
    Claimed,
    /// Slots of the store, at the list's identities, final.
    Published,
}

/// A list of records that starts on the heap and may move to a store.
///
/// A `Vec` until [`publish`](Self::publish), and a fixed slice of the store
/// after it; the slice is reachable by identity from any thread (see
/// [`get`]). Reading a list costs what reading a `Vec` does: only growing
/// and dropping ask where the records are. A clone is a private copy.
pub struct StoreVec<T: StoreRecord> {
    records: NonNull<T>,
    len: usize,
    /// The buffer's room; a published list has none to spare.
    capacity: usize,
    placement: Placement,
}

// SAFETY: a list owns its records as a `Vec` does, whichever memory holds
// them.
unsafe impl<T: StoreRecord + Send> Send for StoreVec<T> {}
// SAFETY: as above; `&StoreVec` only gives out `&T`.
unsafe impl<T: StoreRecord + Sync> Sync for StoreVec<T> {}

impl<T: StoreRecord> StoreVec<T> {
    pub const fn new() -> Self {
        Self {
            records: NonNull::dangling(),
            len: 0,
            capacity: 0,
            placement: Placement::Heap,
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let mut list = Self::new();
        list.grow_to(capacity);
        list
    }

    /// The `len` slots from `position` as a published list of zeroed records,
    /// for tables indexed by the identities of a published tree: final, as a
    /// published list is. The caller's lease owns the slots, and each is
    /// claimed once per lease.
    pub fn claim(position: u32, len: u32) -> Self {
        let mut list = Self::claim_room(position, len);
        // Every slot of the table is read as a zeroed record from here on,
        // so all of them are looked at (a room's are not: see `claim_room`).
        debug_assert!(
            // SAFETY: the claimed slots, committed by `claim_room`.
            unsafe {
                reads_as_zeros(
                    address::<T>(position),
                    len as usize * std::mem::size_of::<T>(),
                )
            },
            "a store range claimed twice"
        );
        list.len = len as usize;
        list.placement = Placement::Published;
        list
    }

    /// An empty list that fills `room` slots of the store from `position`,
    /// for records built directly at identities leased before they are
    /// counted. The caller's lease owns the slots, and each is claimed once
    /// per lease; a list that outgrows its room moves out of the store.
    pub fn claim_room(position: u32, room: u32) -> Self {
        reserve();
        assert!(
            u64::from(position) + u64::from(room) <= 1 << 32,
            "a store range past the end of its region"
        );
        let bytes = room as usize * std::mem::size_of::<T>();
        system::commit(address::<T>(position), bytes);
        // Only the head of the room is read. A room is address space to grow
        // into (a checker's type room is 2 GiB) and its records start at its
        // first slot, so a room that is still held, or was dropped without
        // being scrubbed, shows there. Reading all of it backs every page of
        // it with memory on a system that gives a read of untouched anonymous
        // memory a page of its own (macOS): 2 GiB for each type room a
        // process ever held at once, which took the debug test binaries that
        // hold many checkers to a hundred gigabytes.
        debug_assert!(
            // SAFETY: the head of the claimed slots, committed above.
            unsafe { reads_as_zeros(address::<T>(position), bytes.min(ROOM_HEAD_BYTES)) },
            "a store range claimed twice"
        );
        Self {
            // SAFETY: an address in the reservation is not null.
            records: unsafe { NonNull::new_unchecked(address::<T>(position) as *mut T) },
            len: 0,
            capacity: room as usize,
            placement: Placement::Claimed,
        }
    }

    /// Whether the records live in the store.
    #[inline]
    pub fn is_published(&self) -> bool {
        matches!(self.placement, Placement::Claimed | Placement::Published)
    }

    /// The records' allocated room.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Requested bytes in the main allocator. Published/claimed records
    /// and separately mapped spill buffers belong to the store budget.
    pub fn heap_capacity_bytes(&self) -> usize {
        if self.placement == Placement::Heap {
            self.capacity * std::mem::size_of::<T>()
        } else {
            0
        }
    }

    /// The record at `index`, an index below the list's length: a list whose
    /// indices are its records' identities (a checker's types) reads a
    /// record it pushed with one load, as [`get`] reads a published record,
    /// and no bound. `index` must name a record the list holds.
    #[inline(always)]
    pub fn record(&self, index: u32) -> &T {
        debug_assert!(
            (index as usize) < self.len,
            "a store list index past its length"
        );
        // SAFETY: the caller's contract puts `index` below `len` (checked in
        // debug builds), so the slot is inside the buffer and holds a record
        // this list wrote.
        unsafe { &*self.records.as_ptr().add(index as usize) }
    }

    /// Add a record; a published list is final.
    #[inline]
    pub fn push(&mut self, record: T) {
        if self.len == self.capacity {
            self.grow_one();
        }
        // SAFETY: the slot at `len` is inside the buffer's capacity.
        unsafe { self.records.as_ptr().add(self.len).write(record) };
        self.len += 1;
    }

    /// Add records (see [`push`](Self::push)).
    #[inline]
    pub fn extend_from_slice(&mut self, records: &[T])
    where
        T: Copy,
    {
        if self.capacity - self.len < records.len() {
            self.reserve(records.len());
        }
        // SAFETY: as in `push`, for each of the slots; the source cannot
        // overlap the unused room of this list, which `&mut self` owns.
        unsafe {
            std::ptr::copy_nonoverlapping(
                records.as_ptr(),
                self.records.as_ptr().add(self.len),
                records.len(),
            );
        }
        self.len += records.len();
    }

    /// Make room for `additional` more records.
    pub fn reserve(&mut self, additional: usize) {
        let needed = self
            .len
            .checked_add(additional)
            .expect("a store list exceeds usize");
        if needed > self.capacity {
            self.grow_to(needed.max(self.capacity * 2));
        }
    }

    /// Lengthen the list to `len` records, filling the new slots with
    /// `record`; a list never shortens.
    pub fn resize(&mut self, len: usize, record: T)
    where
        T: Clone,
    {
        assert!(len >= self.len, "a store list does not shorten");
        self.reserve(len - self.len);
        while self.len < len {
            self.push(record.clone());
        }
    }

    #[cold]
    #[inline(never)]
    fn grow_one(&mut self) {
        self.grow_to((self.capacity * 2).max(4));
    }

    /// Give the list room for `capacity` records in a buffer of its own: a
    /// larger allocation or mapping, or the mapping a list of the store
    /// moves to when it outgrows its slots (which are zeroed again).
    fn grow_to(&mut self, capacity: usize) {
        if capacity <= self.capacity {
            return;
        }
        assert!(
            self.placement != Placement::Published,
            "a published store list is final"
        );
        let bytes = capacity
            .checked_mul(std::mem::size_of::<T>())
            .expect("a store list exceeds usize");
        let (records, capacity, placement) = if bytes >= MAPPED_THRESHOLD {
            let bytes = bytes.next_multiple_of(RETURN_GRANULE);
            (
                system::map(bytes).cast::<T>(),
                bytes / std::mem::size_of::<T>(),
                Placement::Mapped,
            )
        } else {
            let mut list = ManuallyDrop::new(Vec::<T>::with_capacity(capacity));
            (list.as_mut_ptr(), list.capacity(), Placement::Heap)
        };
        // SAFETY: the new buffer holds `capacity` slots, more than `len`, and
        // cannot overlap the old one, whose records are then forgotten (the
        // copy is the move).
        unsafe {
            std::ptr::copy_nonoverlapping(self.records.as_ptr(), records, self.len);
        }
        self.release_buffer();
        // SAFETY: neither an allocation nor a mapping is null.
        self.records = unsafe { NonNull::new_unchecked(records) };
        self.capacity = capacity;
        self.placement = placement;
    }

    /// Give the buffer back without dropping the records, which were moved
    /// out or dropped already.
    fn release_buffer(&mut self) {
        match self.placement {
            Placement::Heap => {
                // SAFETY: the parts of a heap list are a `Vec`'s; with length
                // zero it drops no record.
                drop(unsafe { Vec::from_raw_parts(self.records.as_ptr(), 0, self.capacity) });
            }
            Placement::Mapped => {
                system::unmap(
                    self.records.as_ptr().cast(),
                    self.capacity * std::mem::size_of::<T>(),
                );
            }
            Placement::Claimed | Placement::Published => {
                scrub(
                    self.records.as_ptr() as usize,
                    self.len * std::mem::size_of::<T>(),
                );
            }
        }
        self.records = NonNull::dangling();
        self.capacity = 0;
        self.placement = Placement::Heap;
    }

    /// Move the records to the slots from `position`, which the caller's
    /// identity lease owns, and free the buffer. A list built in those slots
    /// (see [`claim_room`](Self::claim_room)) is final from here on.
    pub fn publish(&mut self, position: u32) {
        let slots = address::<T>(position) as *mut T;
        if self.records.as_ptr() == slots {
            assert!(
                self.placement == Placement::Claimed,
                "a store list is published once"
            );
            self.capacity = self.len;
            self.placement = Placement::Published;
            return;
        }
        assert!(
            self.placement != Placement::Published,
            "a store list is published once"
        );
        reserve();
        let len = u32::try_from(self.len).expect("a store list exceeds u32");
        assert!(
            u64::from(position) + u64::from(len) <= 1 << 32,
            "a store range past the end of its region"
        );
        system::commit(slots as usize, self.len * std::mem::size_of::<T>());
        // SAFETY: the lease gives this list the exclusive use of the slots
        // (see the crate documentation), which are mapped; the source is a
        // valid buffer that cannot overlap them (a buffer of the list's own,
        // or slots of another live lease), and it is released without
        // dropping the records it moved out. The address in the reservation
        // is not null.
        unsafe {
            std::ptr::copy_nonoverlapping(self.records.as_ptr(), slots, self.len);
            self.release_buffer();
            self.records = NonNull::new_unchecked(slots);
        }
        self.capacity = self.len;
        self.placement = Placement::Published;
    }

    /// [`publish`](Self::publish) with each record passed through `map` on
    /// its way to the slots from `position`: one read and one write per
    /// record, where mapping the buffer in place and then copying it reads
    /// and writes each twice. The list stays as it was until the returned
    /// [`PendingPublish`] is committed; dropped (or aborted), it scrubs the
    /// slots it wrote, so a caller can still refuse what `map` produced.
    pub fn publish_mapped(
        &mut self,
        position: u32,
        mut map: impl FnMut(T) -> T,
    ) -> PendingPublish<'_, T>
    where
        T: Copy,
    {
        assert!(
            self.placement != Placement::Published && self.placement != Placement::Claimed,
            "a mapped publication moves a private list"
        );
        reserve();
        let len = u32::try_from(self.len).expect("a store list exceeds u32");
        assert!(
            u64::from(position) + u64::from(len) <= 1 << 32,
            "a store range past the end of its region"
        );
        let slots = address::<T>(position) as *mut T;
        system::commit(slots as usize, self.len * std::mem::size_of::<T>());
        let mut pending = PendingPublish {
            list: self,
            slots,
            written: 0,
        };
        let source = pending.list.records.as_ptr();
        for index in 0..pending.list.len {
            // SAFETY: `index` is below the list's length, so the source read is
            // of an initialized record of its own buffer; the lease gives this
            // list the exclusive use of the mapped slots, which do not overlap
            // the buffer (see `publish`). `written` counts the slots to scrub
            // should `map` unwind.
            unsafe {
                let record = map(source.add(index).read());
                slots.add(index).write(record);
            }
            pending.written = index + 1;
        }
        pending
    }
}

/// A list mapped into its slots by [`StoreVec::publish_mapped`] and not yet
/// published there: [`commit`](Self::commit) publishes it, dropping it
/// scrubs the slots and leaves the list as it was.
pub struct PendingPublish<'a, T: StoreRecord> {
    list: &'a mut StoreVec<T>,
    slots: *mut T,
    written: usize,
}

impl<T: StoreRecord> PendingPublish<'_, T> {
    /// Publish the mapped records: the list's buffer is freed and the list
    /// is the slots' from here on, as after [`StoreVec::publish`].
    pub fn commit(mut self) {
        debug_assert_eq!(self.written, self.list.len);
        self.list.release_buffer();
        // SAFETY: the slots hold the list's `len` mapped records (written
        // above), and the address in the reservation is not null.
        self.list.records = unsafe { NonNull::new_unchecked(self.slots) };
        self.list.capacity = self.list.len;
        self.list.placement = Placement::Published;
        self.written = 0;
    }
}

impl<T: StoreRecord> Drop for PendingPublish<'_, T> {
    fn drop(&mut self) {
        if self.written > 0 {
            scrub(self.slots as usize, self.written * std::mem::size_of::<T>());
        }
    }
}

impl<T: StoreRecord> Default for StoreVec<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: StoreRecord> Deref for StoreVec<T> {
    type Target = [T];

    #[inline]
    fn deref(&self) -> &[T] {
        // SAFETY: `len` records start at `records`, wherever the buffer is,
        // for as long as this list is.
        unsafe { std::slice::from_raw_parts(self.records.as_ptr(), self.len) }
    }
}

impl<T: StoreRecord> DerefMut for StoreVec<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut [T] {
        // SAFETY: as in `deref`; this list owns its records and is borrowed
        // exclusively, and a document is only shared after its last edit.
        unsafe { std::slice::from_raw_parts_mut(self.records.as_ptr(), self.len) }
    }
}

impl<T: StoreRecord> Drop for StoreVec<T> {
    fn drop(&mut self) {
        // SAFETY: this list owns its `len` records; nothing reads published
        // ones once their document is gone.
        unsafe {
            std::ptr::drop_in_place(std::ptr::slice_from_raw_parts_mut(
                self.records.as_ptr(),
                self.len,
            ));
        }
        self.release_buffer();
    }
}

impl<T: StoreRecord + Clone> Clone for StoreVec<T> {
    fn clone(&self) -> Self {
        let mut list = Self::with_capacity(self.len);
        for record in self.iter() {
            list.push(record.clone());
        }
        list
    }
}

/// How many four-byte records a [`SmallList`] holds without a heap buffer.
const SMALL_INLINE: usize = 2;

/// A short list of four-byte records in two words: up to two inline, longer
/// ones in a heap buffer. All-zero bytes are an empty list, so a record that
/// holds one can live in a store.
pub struct SmallList<T: Copy> {
    records: SmallRecords<T>,
    len: u32,
    /// The heap buffer's room; zero while the records are inline.
    capacity: u32,
}

union SmallRecords<T: Copy> {
    inline: [MaybeUninit<T>; SMALL_INLINE],
    heap: *mut T,
}

// SAFETY: a list owns its records as a `Vec` does.
unsafe impl<T: Copy + Send> Send for SmallList<T> {}
// SAFETY: as above; `&SmallList` only gives out `&T`.
unsafe impl<T: Copy + Sync> Sync for SmallList<T> {}

impl<T: Copy> SmallList<T> {
    const FOUR_BYTES: () = assert!(
        std::mem::size_of::<T>() == 4 && std::mem::align_of::<T>() == 4,
        "a small list holds four-byte records"
    );

    pub const fn new() -> Self {
        let () = Self::FOUR_BYTES;
        Self {
            records: SmallRecords {
                heap: std::ptr::null_mut(),
            },
            len: 0,
            capacity: 0,
        }
    }

    /// Bytes this list owns on the heap, for memory accounting.
    pub fn heap_bytes(&self) -> usize {
        self.capacity as usize * std::mem::size_of::<T>()
    }

    fn first(&self) -> *const T {
        if self.capacity == 0 {
            std::ptr::addr_of!(self.records.inline).cast()
        } else {
            // SAFETY: a list with room holds a heap buffer.
            unsafe { self.records.heap }
        }
    }

    pub fn push(&mut self, record: T) {
        if self.capacity == 0 && (self.len as usize) < SMALL_INLINE {
            // SAFETY: the inline field has a slot at `len`.
            unsafe { self.records.inline[self.len as usize] = MaybeUninit::new(record) };
            self.len += 1;
            return;
        }
        let mut list = self.take_heap();
        list.push(record);
        *self = Self::from(list);
    }

    /// The records in a `Vec`, leaving this list empty.
    fn take_heap(&mut self) -> Vec<T> {
        let list = if self.capacity == 0 {
            self.to_vec()
        } else {
            // SAFETY: the parts of a list with room are a `Vec`'s, and this
            // list forgets them below.
            unsafe {
                Vec::from_raw_parts(self.records.heap, self.len as usize, self.capacity as usize)
            }
        };
        self.len = 0;
        self.capacity = 0;
        list
    }
}

impl<T: Copy> From<Vec<T>> for SmallList<T> {
    fn from(list: Vec<T>) -> Self {
        let mut small = Self::new();
        if list.len() <= SMALL_INLINE {
            for record in list {
                small.push(record);
            }
            return small;
        }
        let mut list = ManuallyDrop::new(list);
        small.records.heap = list.as_mut_ptr();
        small.len = u32::try_from(list.len()).expect("a small list exceeds u32");
        small.capacity = u32::try_from(list.capacity()).expect("a small list exceeds u32");
        small
    }
}

impl<T: Copy> Default for SmallList<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy> Deref for SmallList<T> {
    type Target = [T];

    #[inline]
    fn deref(&self) -> &[T] {
        // SAFETY: `len` initialized records start at `first`.
        unsafe { std::slice::from_raw_parts(self.first(), self.len as usize) }
    }
}

impl<T: Copy> DerefMut for SmallList<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut [T] {
        // SAFETY: as in `deref`; the list is borrowed exclusively.
        unsafe { std::slice::from_raw_parts_mut(self.first().cast_mut(), self.len as usize) }
    }
}

impl<T: Copy> Drop for SmallList<T> {
    fn drop(&mut self) {
        if self.capacity != 0 {
            drop(self.take_heap());
        }
    }
}

impl<T: Copy> Clone for SmallList<T> {
    /// An inline list is copied as its words (no allocation: instantiating
    /// a symbol clones its one or two declarations); a heap list gets a
    /// buffer of its own.
    #[inline]
    fn clone(&self) -> Self {
        if self.capacity == 0 {
            Self {
                records: SmallRecords {
                    // SAFETY: a list without room keeps its records in the
                    // inline field; `MaybeUninit` records accept any bytes,
                    // including those of an empty list's null pointer.
                    inline: unsafe { self.records.inline },
                },
                len: self.len,
                capacity: 0,
            }
        } else {
            Self::from(self.to_vec())
        }
    }
}

/// The records of a [`SmallList`] by value, in order.
pub struct SmallListIter<T: Copy> {
    list: SmallList<T>,
    front: u32,
    back: u32,
}

impl<T: Copy> Iterator for SmallListIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        (self.front < self.back).then(|| {
            let record = self.list[self.front as usize];
            self.front += 1;
            record
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = (self.back - self.front) as usize;
        (left, Some(left))
    }
}

impl<T: Copy> DoubleEndedIterator for SmallListIter<T> {
    fn next_back(&mut self) -> Option<T> {
        (self.front < self.back).then(|| {
            self.back -= 1;
            self.list[self.back as usize]
        })
    }
}

impl<T: Copy> ExactSizeIterator for SmallListIter<T> {}

impl<T: Copy> IntoIterator for SmallList<T> {
    type Item = T;
    type IntoIter = SmallListIter<T>;

    fn into_iter(self) -> SmallListIter<T> {
        SmallListIter {
            back: self.len,
            front: 0,
            list: self,
        }
    }
}

impl<'a, T: Copy> IntoIterator for &'a SmallList<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<T: StoreRecord + std::fmt::Debug> std::fmt::Debug for StoreVec<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.deref().fmt(formatter)
    }
}

impl<T: StoreRecord + PartialEq> PartialEq for StoreVec<T> {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl<'a, T: StoreRecord> IntoIterator for &'a StoreVec<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, T: StoreRecord> IntoIterator for &'a mut StoreVec<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib/tests.rs"]
mod tests;

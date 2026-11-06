//! Source texts in their own allocator heap.
//!
//! A program's source texts live as long as the program, and the loader
//! allocates each one while it parses that file. In the general heap a text
//! shared its size class's pages with the file's heap-backed parse outputs,
//! which the adopt step relocates and frees, and the pages then stayed
//! committed for the texts alone (VS Code, one thread: 438 MiB of free slots
//! after adopt, 293 MiB at the footprint peak, with the texts the top live
//! site in them). Texts allocated in their own heap share no page with other
//! allocations (mimalloc v3: "Heaps keep allocations in separate pages from
//! each other (but share the arena's and free'd pages)"), so the freed parse
//! outputs leave whole pages that the bind and the check reuse.
//!
//! Every text of a [`SharedText`] lives in the text heap: a program's
//! sources, an edited document's new text (a live server's edit and every
//! rebuild construct their snapshots afresh), package.json and transient
//! texts alike. One block holds a text: its reference count, then its bytes.
//! A replaced text is freed when its last holder drops it, and its slot
//! serves the next text of its size bin.
//!
//! The heap is never deleted or destroyed (texts handed out as `&str` must
//! stay valid for the process). [`collect_text_heap`] (`custom/runGC`)
//! returns the free pages the calling thread holds in it; pages another
//! thread holds wait until that thread allocates texts again.
//!
//! Where mimalloc is not v3 (a v2 heap allocates only on the thread that
//! created it) or cannot create the heap, texts go to the calling thread's
//! default heap. Under Miri and on targets without mimalloc (WASM) the same
//! blocks come from `std::alloc`.

use std::fmt;
use std::ops::Deref;
use std::ptr::NonNull;
use std::sync::atomic::{fence, AtomicUsize, Ordering};
use std::sync::Arc;

/// The bytes before a text's first byte in its block: its reference count.
const HEADER: usize = std::mem::size_of::<AtomicUsize>();
const ALIGN: usize = std::mem::align_of::<AtomicUsize>();

/// A shared, immutable text in the text heap: a 16-byte handle whose data
/// pointer and length are inline (a read is what reading an `Arc<str>`
/// costs). Its block holds the reference count before the bytes, so a text
/// is one allocation. Its holders are per file (snapshots and position
/// indexes), not per node.
pub struct SharedText {
    /// The text's first byte, `HEADER` bytes into its block.
    ptr: NonNull<u8>,
    len: usize,
}

const _: () = assert!(std::mem::size_of::<SharedText>() == 16);
const _: () = assert!(std::mem::size_of::<Option<SharedText>>() == 16);

// SAFETY: the bytes are written once, before the handle exists, and never
// change; the count is atomic, and the last holder frees the block on any
// thread (mimalloc's `mi_free` and `std::alloc::dealloc` accept any thread).
unsafe impl Send for SharedText {}
// SAFETY: as above; `&SharedText` only reads.
unsafe impl Sync for SharedText {}

/// The block's size for a text of `len` bytes: the count, then at least one
/// byte, so that every live text (an empty one too) starts at an address of
/// its own (the printer's token-start memo and the source map's fast path
/// key on the address).
fn block_size(len: usize) -> usize {
    HEADER + len.max(1)
}

fn block_layout(size: usize) -> std::alloc::Layout {
    std::alloc::Layout::from_size_align(size, ALIGN).expect("text block layout")
}

#[cfg(not(any(miri, target_family = "wasm")))]
mod heap {
    use std::ptr::NonNull;
    use std::sync::OnceLock;

    use libmimalloc_sys::mi_heap_t;

    // The text heap is a v3 first-class heap: the loader's workers allocate
    // texts, and under v2 a heap allocates only on the thread that created
    // it. `mi_theap_t` exists only without libmimalloc-sys's `v2` feature, so
    // this fails to compile under v2.
    const _: Option<NonNull<libmimalloc_sys::mi_theap_t>> = None;

    unsafe extern "C" {
        /// v3: whether `p` lies in `heap`'s pages (not in the 0.1.49
        /// bindings).
        fn mi_heap_contains(heap: *const mi_heap_t, p: *const std::ffi::c_void) -> bool;
    }

    struct Heap(Option<NonNull<mi_heap_t>>);
    // SAFETY: a v3 heap allocates and frees on every thread.
    unsafe impl Send for Heap {}
    // SAFETY: as above.
    unsafe impl Sync for Heap {}

    static HEAP: OnceLock<Heap> = OnceLock::new();

    /// The process's text heap, created on first use and never deleted; none
    /// where mimalloc is not v3 or could not create one.
    fn text_heap() -> Option<NonNull<mi_heap_t>> {
        HEAP.get_or_init(|| {
            // SAFETY: querying the version has no preconditions.
            if unsafe { libmimalloc_sys::mi_version() } < 30000 {
                return Heap(None);
            }
            // SAFETY: creating a heap has no preconditions.
            Heap(NonNull::new(unsafe { libmimalloc_sys::mi_heap_new() }))
        })
        .0
    }

    /// # Safety
    /// `size` is not zero.
    pub(super) unsafe fn allocate(size: usize) -> *mut u8 {
        match text_heap() {
            // SAFETY: the heap lives for the process.
            Some(heap) => unsafe { libmimalloc_sys::mi_heap_malloc(heap.as_ptr(), size) }.cast(),
            // SAFETY: an allocation in the calling thread's default heap.
            None => unsafe { libmimalloc_sys::mi_malloc(size) }.cast(),
        }
    }

    /// # Safety
    /// `block` came from [`allocate`] and is freed once.
    pub(super) unsafe fn deallocate(block: NonNull<u8>, _size: usize) {
        // SAFETY: `mi_free` frees a block of any mimalloc heap, on any thread.
        unsafe { libmimalloc_sys::mi_free(block.as_ptr().cast()) }
    }

    /// Whether `block` is in the text heap.
    pub(super) fn contains(block: *const u8) -> bool {
        // SAFETY: a lookup in the heap's page map; it reads no block.
        text_heap().is_some_and(|heap| unsafe { mi_heap_contains(heap.as_ptr(), block.cast()) })
    }

    /// Returns the text heap's free memory the calling thread holds.
    pub(super) fn collect() {
        if let Some(Heap(Some(heap))) = HEAP.get() {
            // SAFETY: collects the calling thread's pages of a live heap.
            unsafe { libmimalloc_sys::mi_heap_collect(heap.as_ptr(), true) }
        }
    }

    /// Bytes of the text heap's live blocks (tests only: mimalloc's walk
    /// assumes no thread uses the heap meanwhile).
    pub(super) fn live_bytes() -> usize {
        unsafe extern "C" fn area(
            _heap: *const mi_heap_t,
            area: *const libmimalloc_sys::mi_heap_area_t,
            _block: *mut std::ffi::c_void,
            _block_size: usize,
            total: *mut std::ffi::c_void,
        ) -> bool {
            // SAFETY: the synchronous walk passes our counter and its area.
            unsafe {
                let area = &*area;
                *total.cast::<usize>() += area.used * area.block_size;
            }
            true
        }
        let Some(heap) = text_heap() else {
            return 0;
        };
        let mut total = 0usize;
        // SAFETY: areas only; the callback neither allocates nor frees.
        unsafe {
            libmimalloc_sys::mi_heap_visit_blocks(
                heap.as_ptr(),
                false,
                Some(area),
                std::ptr::from_mut(&mut total).cast(),
            );
        }
        total
    }
}

#[cfg(any(miri, target_family = "wasm"))]
mod heap {
    use std::ptr::NonNull;

    /// # Safety
    /// `size` is not zero.
    pub(super) unsafe fn allocate(size: usize) -> *mut u8 {
        // SAFETY: a non-zero layout.
        unsafe { std::alloc::alloc(super::block_layout(size)) }
    }

    /// # Safety
    /// `block` came from `allocate(size)` and is freed once.
    pub(super) unsafe fn deallocate(block: NonNull<u8>, size: usize) {
        // SAFETY: as the caller guarantees.
        unsafe { std::alloc::dealloc(block.as_ptr(), super::block_layout(size)) }
    }

    pub(super) fn contains(_block: *const u8) -> bool {
        true
    }

    pub(super) fn collect() {}

    pub(super) fn live_bytes() -> usize {
        0
    }
}

/// tsgo `RunGC` for the text heap: returns the free pages the calling
/// thread holds in it, and purges the arenas' free memory.
pub fn collect_text_heap() {
    heap::collect();
}

/// Bytes of the text heap's live blocks, for tests: mimalloc's walk assumes
/// no other thread uses the heap meanwhile. Zero where texts do not go to a
/// mimalloc text heap.
#[doc(hidden)]
pub fn text_heap_live_bytes() -> usize {
    heap::live_bytes()
}

impl SharedText {
    /// `text` copied into the text heap.
    pub fn new(text: &str) -> Self {
        let size = block_size(text.len());
        // SAFETY: `size` is at least `HEADER + 1`.
        let Some(block) = NonNull::new(unsafe { heap::allocate(size) }) else {
            std::alloc::handle_alloc_error(block_layout(size));
        };
        // mimalloc's blocks are word aligned; the count needs that.
        assert!(
            block.as_ptr().addr().is_multiple_of(ALIGN),
            "text block misaligned"
        );
        // SAFETY: `block` holds `size = HEADER + max(len, 1)` writable bytes,
        // aligned for the count, that overlap nothing; the text's bytes start
        // `HEADER` bytes in. Nothing between the allocation and the handle
        // can panic.
        unsafe {
            block.cast::<AtomicUsize>().write(AtomicUsize::new(1));
            let ptr = block.add(HEADER);
            std::ptr::copy_nonoverlapping(text.as_ptr(), ptr.as_ptr(), text.len());
            Self {
                ptr,
                len: text.len(),
            }
        }
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        // SAFETY: the `len` bytes at `ptr` were copied from a `&str`, never
        // change, and the block lives while `self` holds a count.
        unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(self.ptr.as_ptr(), self.len))
        }
    }

    /// Whether both share one block.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        self.ptr == other.ptr
    }

    /// Whether this text's block is in the text heap (tests; always true
    /// where blocks come from `std::alloc`).
    #[doc(hidden)]
    pub fn in_text_heap(&self) -> bool {
        heap::contains(self.block().as_ptr())
    }

    fn block(&self) -> NonNull<u8> {
        // SAFETY: `ptr` is `HEADER` bytes into the block it came from.
        unsafe { self.ptr.sub(HEADER) }
    }

    fn count(&self) -> &AtomicUsize {
        // SAFETY: the block starts with the count, aligned and initialized,
        // and lives while `self` holds a count.
        unsafe { self.block().cast::<AtomicUsize>().as_ref() }
    }

    /// The number of holders (tests).
    #[cfg(test)]
    fn holders(&self) -> usize {
        self.count().load(Ordering::Acquire)
    }
}

impl Clone for SharedText {
    /// As `Arc::clone`: a relaxed increment, aborting before the count could
    /// overflow.
    fn clone(&self) -> Self {
        if self.count().fetch_add(1, Ordering::Relaxed) > isize::MAX as usize {
            std::process::abort();
        }
        Self {
            ptr: self.ptr,
            len: self.len,
        }
    }
}

impl Drop for SharedText {
    /// As `Arc`'s drop: a release decrement, and an acquire fence before the
    /// last holder frees the block.
    fn drop(&mut self) {
        if self.count().fetch_sub(1, Ordering::Release) != 1 {
            return;
        }
        fence(Ordering::Acquire);
        // SAFETY: the last holder frees the block it came from, once.
        unsafe { heap::deallocate(self.block(), block_size(self.len)) }
    }
}

impl From<&str> for SharedText {
    fn from(text: &str) -> Self {
        Self::new(text)
    }
}

impl From<String> for SharedText {
    fn from(text: String) -> Self {
        Self::new(&text)
    }
}

impl From<Arc<str>> for SharedText {
    /// A copy: every text lives in the text heap.
    fn from(text: Arc<str>) -> Self {
        Self::new(&text)
    }
}

impl Deref for SharedText {
    type Target = str;

    #[inline]
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for SharedText {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Debug for SharedText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), formatter)
    }
}

impl PartialEq for SharedText {
    fn eq(&self, other: &Self) -> bool {
        self.ptr_eq(other) || self.as_str() == other.as_str()
    }
}

impl Eq for SharedText {}

#[cfg(test)]
#[path = "../tests/unit/shared_text/tests.rs"]
mod tests;

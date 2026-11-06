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
//! texts alike. A replaced text is freed when its last holder drops it, and
//! its slot serves the next text of its size bin.
//!
//! The heap is never deleted or destroyed (texts handed out as `&str` must
//! stay valid for the process), and `custom/runGC`'s `mi_collect` does not
//! reach it: it collects only the calling thread's default heap.
//!
//! Under Miri and on targets without mimalloc (WASM) the same type allocates
//! its block through `std::alloc`.

use std::fmt;
use std::ops::Deref;
use std::ptr::NonNull;
use std::sync::Arc;

/// A shared, immutable text in the text heap: a 24-byte handle whose data
/// pointer and length are inline (a read is what reading an `Arc<str>`
/// costs), owning its block through a reference count.
#[derive(Clone)]
pub struct SharedText {
    ptr: NonNull<u8>,
    len: usize,
    block: Arc<TextBlock>,
}

/// The block a text's bytes live in: at least one byte, so that every live
/// text (an empty one too) starts at an address of its own (the printer's
/// token-start memo and the source map's fast path key on the address).
struct TextBlock {
    ptr: NonNull<u8>,
    size: usize,
}

// SAFETY: the bytes are written once, before the block is shared, and never
// change; the block is freed once, by its last owner, on any thread (mimalloc
// v3's `mi_free` and `std::alloc::dealloc` both accept any thread).
unsafe impl Send for TextBlock {}
// SAFETY: as above; `&TextBlock` only reads.
unsafe impl Sync for TextBlock {}
// SAFETY: a `SharedText` is a read-only view of a block it keeps alive.
unsafe impl Send for SharedText {}
// SAFETY: as above.
unsafe impl Sync for SharedText {}

impl Drop for TextBlock {
    fn drop(&mut self) {
        // SAFETY: the block came from `allocate(self.size)` and is freed once.
        unsafe { deallocate(self.ptr, self.size) }
    }
}

#[cfg(not(any(miri, target_family = "wasm")))]
mod heap {
    use std::ptr::NonNull;

    // The text heap must be a v3 first-class heap: under v2 a heap allocates
    // only on the thread that created it, and the loader's workers allocate
    // texts. `mi_theap_t` exists only without libmimalloc-sys's `v2`
    // feature, so this fails to compile under v2.
    const _: Option<NonNull<libmimalloc_sys::mi_theap_t>> = None;

    unsafe extern "C" {
        /// v3: whether `p` lies in `heap`'s pages (not in the 0.1.49
        /// bindings). Tests only.
        fn mi_heap_contains(
            heap: *const libmimalloc_sys::mi_heap_t,
            p: *const std::ffi::c_void,
        ) -> bool;
    }

    /// The process's text heap, created on first use; never deleted.
    pub(super) fn text_heap() -> *mut libmimalloc_sys::mi_heap_t {
        struct Heap(*mut libmimalloc_sys::mi_heap_t);
        // SAFETY: a v3 heap is used from every thread.
        unsafe impl Send for Heap {}
        // SAFETY: as above.
        unsafe impl Sync for Heap {}
        static HEAP: std::sync::OnceLock<Heap> = std::sync::OnceLock::new();
        HEAP.get_or_init(|| {
            // SAFETY: querying the version has no preconditions.
            let version = unsafe { libmimalloc_sys::mi_version() };
            assert!(
                version >= 30000,
                "the text heap needs mimalloc v3, found {version}"
            );
            // SAFETY: creating a heap has no preconditions.
            let heap = unsafe { libmimalloc_sys::mi_heap_new() };
            assert!(!heap.is_null(), "mimalloc could not create the text heap");
            Heap(heap)
        })
        .0
    }

    /// # Safety
    /// `size` is at least one byte.
    pub(super) unsafe fn allocate(size: usize) -> *mut u8 {
        // SAFETY: the heap lives for the process.
        unsafe { libmimalloc_sys::mi_heap_malloc(text_heap(), size) }.cast()
    }

    /// # Safety
    /// `ptr` is a block of the text heap, freed once.
    pub(super) unsafe fn deallocate(ptr: NonNull<u8>, _size: usize) {
        // SAFETY: as the caller guarantees.
        unsafe { libmimalloc_sys::mi_free(ptr.as_ptr().cast()) }
    }

    /// Whether `ptr` is a block of the text heap (tests).
    pub(super) fn contains(ptr: *const u8) -> bool {
        // SAFETY: a lookup in the heap's page map; it reads no block.
        unsafe { mi_heap_contains(text_heap(), ptr.cast()) }
    }

    /// Bytes of the text heap's live blocks (tests only: mimalloc's walk
    /// assumes no thread uses the heap meanwhile).
    pub(super) fn live_bytes() -> usize {
        unsafe extern "C" fn area(
            _heap: *const libmimalloc_sys::mi_heap_t,
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
        let mut total = 0usize;
        // SAFETY: areas only; the callback neither allocates nor frees.
        unsafe {
            libmimalloc_sys::mi_heap_visit_blocks(
                text_heap(),
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

    fn layout(size: usize) -> std::alloc::Layout {
        std::alloc::Layout::from_size_align(size, 1).expect("text layout")
    }

    /// # Safety
    /// `size` is at least one byte.
    pub(super) unsafe fn allocate(size: usize) -> *mut u8 {
        // SAFETY: a non-zero layout.
        unsafe { std::alloc::alloc(layout(size)) }
    }

    /// # Safety
    /// `ptr` came from `allocate(size)` and is freed once.
    pub(super) unsafe fn deallocate(ptr: NonNull<u8>, size: usize) {
        // SAFETY: as the caller guarantees.
        unsafe { std::alloc::dealloc(ptr.as_ptr(), layout(size)) }
    }

    pub(super) fn contains(_ptr: *const u8) -> bool {
        true
    }

    pub(super) fn live_bytes() -> usize {
        0
    }
}

use heap::{allocate, deallocate};

/// Bytes of the text heap's live blocks, for tests: mimalloc's walk assumes
/// no other thread uses the heap meanwhile. Zero where the type falls back to
/// `std::alloc`.
#[doc(hidden)]
pub fn text_heap_live_bytes() -> usize {
    heap::live_bytes()
}

impl SharedText {
    /// `text` copied into the text heap.
    pub fn new(text: &str) -> Self {
        let size = text.len().max(1);
        // SAFETY: `size` is at least one byte.
        let block = unsafe { allocate(size) };
        let Some(ptr) = NonNull::new(block) else {
            std::alloc::handle_alloc_error(
                std::alloc::Layout::from_size_align(size, 1).expect("text layout"),
            );
        };
        // SAFETY: `ptr` holds `size >= text.len()` writable bytes that overlap
        // nothing; nothing between the allocation and the `Arc` can panic.
        unsafe { std::ptr::copy_nonoverlapping(text.as_ptr(), ptr.as_ptr(), text.len()) };
        Self {
            ptr,
            len: text.len(),
            block: Arc::new(TextBlock { ptr, size }),
        }
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        // SAFETY: the `len` bytes at `ptr` were copied from a `&str`, never
        // change, and the block lives while `self` holds it.
        unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(self.ptr.as_ptr(), self.len))
        }
    }

    /// Whether both share one block.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.block, &other.block)
    }

    /// Whether this text's block is in the text heap (tests; always true
    /// where the type falls back to `std::alloc`).
    #[doc(hidden)]
    pub fn in_text_heap(&self) -> bool {
        heap::contains(self.block.ptr.as_ptr())
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

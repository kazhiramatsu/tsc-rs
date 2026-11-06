//! Source texts in their own allocator heap.
//!
//! A program's source texts live as long as the program, and the loader
//! allocates each one while it parses that file: in the general heap a text
//! shares its size class's pages with the file's parse outputs, which the
//! adopt step frees, and the pages then stay committed for the texts alone
//! (VS Code: 136 MiB of texts holding ≈ 200 MiB of otherwise free pages).
//! Texts allocated in their own heap share no page with other allocations
//! (mimalloc v3: "Heaps keep allocations in separate pages from each other
//! (but share the arena's and free'd pages)"), so freed parse outputs leave
//! whole pages that the bind and the check reuse.

use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

/// A shared, immutable text: a source text in the text heap, or a text
/// shared as `Arc<str>` (an edited document's).
#[derive(Clone)]
pub struct SharedText(Repr);

#[derive(Clone)]
enum Repr {
    #[cfg(not(target_family = "wasm"))]
    Heap(Arc<HeapText>),
    Shared(Arc<str>),
}

/// The bytes of a text in the text heap (valid UTF-8, never empty).
#[cfg(not(target_family = "wasm"))]
struct HeapText {
    ptr: std::ptr::NonNull<u8>,
    len: usize,
}

// SAFETY: the bytes are immutable after construction and owned by the
// `HeapText` alone; mimalloc frees a block from any thread.
#[cfg(not(target_family = "wasm"))]
unsafe impl Send for HeapText {}
// SAFETY: as above; `&HeapText` only reads the bytes.
#[cfg(not(target_family = "wasm"))]
unsafe impl Sync for HeapText {}

#[cfg(not(target_family = "wasm"))]
impl Drop for HeapText {
    fn drop(&mut self) {
        // SAFETY: the block came from `mi_heap_malloc` and is freed once.
        unsafe { libmimalloc_sys::mi_free(self.ptr.as_ptr().cast()) }
    }
}

/// The process's text heap, created on first use. A first-class mimalloc v3
/// heap: any thread allocates from it and frees into it.
#[cfg(not(target_family = "wasm"))]
fn text_heap() -> *mut libmimalloc_sys::mi_heap_t {
    struct Heap(*mut libmimalloc_sys::mi_heap_t);
    // SAFETY: a v3 heap is usable from every thread.
    unsafe impl Send for Heap {}
    // SAFETY: as above.
    unsafe impl Sync for Heap {}
    static HEAP: std::sync::OnceLock<Heap> = std::sync::OnceLock::new();
    // SAFETY: creating a heap has no preconditions; it lives for the process.
    HEAP.get_or_init(|| Heap(unsafe { libmimalloc_sys::mi_heap_new() }))
        .0
}

impl SharedText {
    /// `text` copied into the text heap.
    pub fn in_text_heap(text: &str) -> Self {
        #[cfg(not(target_family = "wasm"))]
        if !text.is_empty() {
            // SAFETY: a fresh block of `text.len()` bytes from the text heap.
            let block =
                unsafe { libmimalloc_sys::mi_heap_malloc(text_heap(), text.len()) }.cast::<u8>();
            let Some(ptr) = std::ptr::NonNull::new(block) else {
                std::alloc::handle_alloc_error(
                    std::alloc::Layout::from_size_align(text.len(), 1).expect("text layout"),
                );
            };
            // SAFETY: `ptr` holds `text.len()` writable bytes that overlap nothing.
            unsafe { std::ptr::copy_nonoverlapping(text.as_ptr(), ptr.as_ptr(), text.len()) };
            return Self(Repr::Heap(Arc::new(HeapText {
                ptr,
                len: text.len(),
            })));
        }
        Self(Repr::Shared(Arc::from(text)))
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        match &self.0 {
            #[cfg(not(target_family = "wasm"))]
            // SAFETY: the bytes were copied from a `&str` and never change.
            Repr::Heap(text) => unsafe {
                std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                    text.ptr.as_ptr(),
                    text.len,
                ))
            },
            Repr::Shared(text) => text,
        }
    }

    /// Whether both share one allocation.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            #[cfg(not(target_family = "wasm"))]
            (Repr::Heap(left), Repr::Heap(right)) => Arc::ptr_eq(left, right),
            (Repr::Shared(left), Repr::Shared(right)) => Arc::ptr_eq(left, right),
            #[cfg(not(target_family = "wasm"))]
            _ => false,
        }
    }
}

impl From<Arc<str>> for SharedText {
    fn from(text: Arc<str>) -> Self {
        Self(Repr::Shared(text))
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

//! The CLI's mimalloc global allocator.
use std::alloc::{GlobalAlloc, Layout};

/// Mimalloc with its plain entry points for naturally aligned allocations.
/// Library users continue to choose their own global allocator.
pub struct MiMalloc;

// Bundled mimalloc: include/mimalloc/types.h (MI_MAX_ALIGN_SIZE),
// src/page-queue.c (size classes) and src/alloc.c (block alignment). Below
// 16 bytes the size class supplies the alignment; conservatively require
// align <= size as well. This uses no stronger size-class alignments.
const MI_MAX_ALIGN_SIZE: usize = 16;

#[inline]
fn naturally_aligned(size: usize, align: usize) -> bool {
    align <= MI_MAX_ALIGN_SIZE && align <= size
}

#[inline]
fn checked_alignment(ptr: *mut u8, align: usize) -> *mut u8 {
    debug_assert!(ptr.is_null() || (ptr as usize) & (align - 1) == 0);
    ptr
}

// SAFETY: every entry point belongs to the same mimalloc instance. The plain
// entry points satisfy the requested alignment under naturally_aligned's
// conservative rule; the aligned entry points handle every other layout.
unsafe impl GlobalAlloc for MiMalloc {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: GlobalAlloc supplies a nonzero size and a valid alignment.
        let ptr = unsafe {
            if naturally_aligned(layout.size(), layout.align()) {
                libmimalloc_sys::mi_malloc(layout.size())
            } else {
                libmimalloc_sys::mi_malloc_aligned(layout.size(), layout.align())
            }
        };
        checked_alignment(ptr.cast(), layout.align())
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: as in alloc; both entry points zero the requested bytes.
        let ptr = unsafe {
            if naturally_aligned(layout.size(), layout.align()) {
                libmimalloc_sys::mi_zalloc(layout.size())
            } else {
                libmimalloc_sys::mi_zalloc_aligned(layout.size(), layout.align())
            }
        };
        checked_alignment(ptr.cast(), layout.align())
    }

    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: ptr is a live mimalloc allocation. If realloc moves it,
        // malloc's guarantee for NEW size must satisfy the old alignment;
        // if it keeps it, its alignment already does. Both variants retain
        // min(old size, new size) bytes and leave ptr live on failure.
        let ptr = unsafe {
            if naturally_aligned(new_size, layout.align()) {
                libmimalloc_sys::mi_realloc(ptr.cast(), new_size)
            } else {
                libmimalloc_sys::mi_realloc_aligned(ptr.cast(), new_size, layout.align())
            }
        };
        checked_alignment(ptr.cast(), layout.align())
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        // SAFETY: all allocation variants above use mimalloc, whose free
        // accepts their original pointer without a size or alignment.
        unsafe { libmimalloc_sys::mi_free(ptr.cast()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_size_classes_and_overaligned_allocations() {
        for size in 1..=256 {
            for align in [1, 2, 4, 8, 16, 32, 64, 256, 4096] {
                let layout = Layout::from_size_align(size, align).unwrap();
                // SAFETY: each allocation is accessed within its layout and
                // freed once with the same layout.
                unsafe {
                    let ptr = MiMalloc.alloc_zeroed(layout);
                    assert!(!ptr.is_null());
                    assert_eq!(ptr as usize % align, 0);
                    assert!(std::slice::from_raw_parts(ptr, size)
                        .iter()
                        .all(|&b| b == 0));
                    std::ptr::write_bytes(ptr, 0xa5, size);
                    MiMalloc.dealloc(ptr, layout);
                    let ptr = MiMalloc.alloc(layout);
                    assert!(!ptr.is_null());
                    assert_eq!(ptr as usize % align, 0);
                    MiMalloc.dealloc(ptr, layout);
                }
            }
        }
    }

    #[test]
    fn realloc_preserves_alignment_and_contents_across_size_boundaries() {
        for align in [1, 2, 4, 8, 16, 32, 256, 4096] {
            for old_size in [1, 7, 8, 15, 16, 17, 127, 1024] {
                for new_size in [1, 7, 8, 15, 16, 17, 257, 65536] {
                    let layout = Layout::from_size_align(old_size, align).unwrap();
                    // SAFETY: ptr comes from this allocator, realloc takes
                    // its live layout, and the retained prefix is initialized.
                    unsafe {
                        let ptr = MiMalloc.alloc(layout);
                        assert!(!ptr.is_null());
                        std::ptr::write_bytes(ptr, 0x5a, old_size);
                        let ptr = MiMalloc.realloc(ptr, layout, new_size);
                        assert!(!ptr.is_null());
                        assert_eq!(ptr as usize % align, 0);
                        assert!(std::slice::from_raw_parts(ptr, old_size.min(new_size))
                            .iter()
                            .all(|&b| b == 0x5a));
                        MiMalloc.dealloc(ptr, Layout::from_size_align(new_size, align).unwrap());
                    }
                }
            }
        }
    }
}

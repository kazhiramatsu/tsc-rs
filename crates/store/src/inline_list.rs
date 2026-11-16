//! Short, owned lists whose inline storage occupies a Vec's pointer/capacity words.

use std::fmt;
use std::mem::{ManuallyDrop, MaybeUninit};
use std::num::NonZeroUsize;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;

const HEAP: usize = 1 << (usize::BITS - 1);

#[repr(C)]
struct Allocation<T> {
    pointer: NonNull<T>,
    capacity: usize,
}

union Records<T: Copy, const N: usize> {
    inline: [MaybeUninit<T>; N],
    heap: ManuallyDrop<Allocation<T>>,
}

/// A list of Copy records with up to N elements stored in its own header.
///
/// The inline payload must fit in sixteen bytes, with alignment at most eight.
/// On 64-bit targets both this list and its Option occupy 24 bytes. Length plus
/// one provides the Option niche; its high bit selects an owned allocation.
/// A spill preserves element order. Clearing a spilled list retains its buffer.
pub struct InlineList<T: Copy, const N: usize> {
    records: Records<T, N>,
    length: NonZeroUsize,
}

const _: () = {
    assert!(std::mem::size_of::<InlineList<u32, 4>>() == 24);
    assert!(std::mem::size_of::<Option<InlineList<u32, 4>>>() == 24);
    assert!(std::mem::size_of::<InlineList<u64, 2>>() == 24);
    assert!(std::mem::size_of::<Option<InlineList<u64, 2>>>() == 24);
};

// SAFETY: the pointer, when selected, owns its allocation exclusively. Inline
// elements have the same thread requirements as the equivalent owned Vec.
unsafe impl<T: Copy + Send, const N: usize> Send for InlineList<T, N> {}
// SAFETY: shared reads expose only shared references to T.
unsafe impl<T: Copy + Sync, const N: usize> Sync for InlineList<T, N> {}

impl<T: Copy, const N: usize> InlineList<T, N> {
    pub const fn new() -> Self {
        assert!(N > 0 && std::mem::size_of::<T>() > 0);
        assert!(std::mem::size_of::<[T; N]>() <= 16);
        assert!(std::mem::align_of::<T>() <= 8);
        Self {
            records: Records {
                inline: [MaybeUninit::uninit(); N],
            },
            length: NonZeroUsize::MIN,
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        if capacity <= N {
            Self::new()
        } else {
            Vec::with_capacity(capacity).into()
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        (self.length.get() & !HEAP) - 1
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    fn is_heap(&self) -> bool {
        self.length.get() & HEAP != 0
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        if self.is_heap() {
            // SAFETY: the length tag selects the initialized union field.
            unsafe { self.records.heap.capacity }
        } else {
            N
        }
    }

    /// Only separately allocated backing; the inline payload is in the header.
    pub fn heap_capacity_bytes(&self) -> usize {
        if self.is_heap() {
            self.capacity() * std::mem::size_of::<T>()
        } else {
            0
        }
    }

    #[inline]
    fn pointer(&self) -> *const T {
        if self.is_heap() {
            // SAFETY: the heap field is initialized while the tag is set.
            unsafe { self.records.heap.pointer.as_ptr() }
        } else {
            // SAFETY: taking the inline field's address does not read its bytes.
            unsafe { self.records.inline.as_ptr().cast() }
        }
    }

    #[inline]
    fn pointer_mut(&mut self) -> *mut T {
        if self.is_heap() {
            // SAFETY: selected field, exclusively borrowed with the list.
            unsafe { self.records.heap.pointer.as_ptr() }
        } else {
            // SAFETY: the inline field is selected; uninitialized tail is not read.
            unsafe { self.records.inline.as_mut_ptr().cast() }
        }
    }

    #[inline]
    fn set_len(&mut self, length: usize) {
        assert!(length < HEAP - 1, "inline list length overflow");
        self.length = NonZeroUsize::new((self.length.get() & HEAP) | (length + 1)).unwrap();
    }

    #[inline]
    pub fn push(&mut self, value: T) {
        let length = self.len();
        if length == self.capacity() {
            self.reserve(1);
        }
        // SAFETY: reserve supplies room, length names the first uninitialized
        // element, and the exclusive borrow prevents views into the old storage.
        unsafe { self.pointer_mut().add(length).write(value) };
        self.set_len(length + 1);
    }

    pub fn reserve(&mut self, additional: usize) {
        let needed = self
            .len()
            .checked_add(additional)
            .expect("inline list length overflow");
        if needed <= self.capacity() {
            return;
        }
        let values = if self.is_heap() {
            let mut values = std::mem::take(self).into_vec();
            values.reserve(additional);
            values
        } else {
            let mut values = Vec::with_capacity(needed.max(N * 2));
            values.extend_from_slice(self);
            values
        };
        *self = Self::from_heap(values);
    }

    pub fn clear(&mut self) {
        self.set_len(0);
    }

    pub fn extend_from_slice(&mut self, values: &[T]) {
        self.reserve(values.len());
        let length = self.len();
        // SAFETY: the source cannot alias our exclusive borrow. Reserve covers
        // the destination, and both ranges contain Copy elements.
        unsafe {
            std::ptr::copy_nonoverlapping(
                values.as_ptr(),
                self.pointer_mut().add(length),
                values.len(),
            )
        };
        self.set_len(length + values.len());
    }

    pub fn as_slice(&self) -> &[T] {
        self
    }

    pub fn into_vec(self) -> Vec<T> {
        if !self.is_heap() {
            return self.as_slice().to_vec();
        }
        let this = ManuallyDrop::new(self);
        // SAFETY: the selected heap field owns exactly this initialized prefix
        // and capacity. ManuallyDrop transfers the allocation to the Vec once.
        unsafe {
            Vec::from_raw_parts(
                this.records.heap.pointer.as_ptr(),
                this.len(),
                this.records.heap.capacity,
            )
        }
    }

    fn from_heap(values: Vec<T>) -> Self {
        let _ = Self::new();
        let mut values = ManuallyDrop::new(values);
        assert!(values.len() < HEAP - 1, "inline list length overflow");
        Self {
            records: Records {
                heap: ManuallyDrop::new(Allocation {
                    pointer: NonNull::new(values.as_mut_ptr()).unwrap(),
                    capacity: values.capacity(),
                }),
            },
            length: NonZeroUsize::new(HEAP | (values.len() + 1)).unwrap(),
        }
    }
}

impl<T: Copy, const N: usize> Default for InlineList<T, N> {
    fn default() -> Self {
        Self::new()
    }
}
impl<T: Copy, const N: usize> From<Vec<T>> for InlineList<T, N> {
    fn from(values: Vec<T>) -> Self {
        if values.capacity() > N {
            return Self::from_heap(values);
        }
        let mut list = Self::new();
        list.extend_from_slice(&values);
        list
    }
}
impl<T: Copy, const N: usize> FromIterator<T> for InlineList<T, N> {
    fn from_iter<I: IntoIterator<Item = T>>(values: I) -> Self {
        let values = values.into_iter();
        let mut list = Self::with_capacity(values.size_hint().0);
        list.extend(values);
        list
    }
}
impl<T: Copy, const N: usize> Extend<T> for InlineList<T, N> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, values: I) {
        for value in values {
            self.push(value);
        }
    }
}
impl<T: Copy, const N: usize> Clone for InlineList<T, N> {
    fn clone(&self) -> Self {
        if !self.is_heap() {
            Self {
                // SAFETY: inline is selected. Copying MaybeUninit preserves
                // the initialized prefix without reading the unused T values.
                records: Records {
                    inline: unsafe { self.records.inline },
                },
                length: self.length,
            }
        } else {
            let mut clone = Self::with_capacity(self.len());
            clone.extend_from_slice(self);
            clone
        }
    }
    fn clone_from(&mut self, source: &Self) {
        if source.len() <= N {
            *self = source.clone();
        } else {
            self.clear();
            self.extend_from_slice(source);
        }
    }
}
impl<T: Copy + fmt::Debug, const N: usize> fmt::Debug for InlineList<T, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_slice().fmt(f)
    }
}
impl<T: Copy + PartialEq, const N: usize> PartialEq for InlineList<T, N> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}
impl<T: Copy + Eq, const N: usize> Eq for InlineList<T, N> {}
impl<T: Copy, const N: usize> Deref for InlineList<T, N> {
    type Target = [T];
    #[inline]
    fn deref(&self) -> &[T] {
        // SAFETY: exactly len elements are initialized in the selected storage.
        unsafe { std::slice::from_raw_parts(self.pointer(), self.len()) }
    }
}
impl<T: Copy, const N: usize> DerefMut for InlineList<T, N> {
    #[inline]
    fn deref_mut(&mut self) -> &mut [T] {
        // SAFETY: the exclusive borrow covers exactly the initialized prefix.
        unsafe { std::slice::from_raw_parts_mut(self.pointer_mut(), self.len()) }
    }
}
impl<'a, T: Copy, const N: usize> IntoIterator for &'a InlineList<T, N> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl<T: Copy, const N: usize> IntoIterator for InlineList<T, N> {
    type Item = T;
    type IntoIter = IntoIter<T, N>;
    fn into_iter(self) -> Self::IntoIter {
        IntoIter {
            list: self,
            index: 0,
        }
    }
}
/// An owning iterator which also keeps inline elements inline.
pub struct IntoIter<T: Copy, const N: usize> {
    list: InlineList<T, N>,
    index: usize,
}
impl<T: Copy, const N: usize> Iterator for IntoIter<T, N> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        let value = self.list.get(self.index).copied()?;
        self.index += 1;
        Some(value)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = self.list.len() - self.index;
        (left, Some(left))
    }
}
impl<T: Copy, const N: usize> ExactSizeIterator for IntoIter<T, N> {}
impl<T: Copy, const N: usize> Drop for InlineList<T, N> {
    fn drop(&mut self) {
        if self.is_heap() {
            // SAFETY: this is the unique owner of the selected allocation.
            // Copy elements have no destructors; only the allocation is freed.
            unsafe {
                drop(Vec::from_raw_parts(
                    self.records.heap.pointer.as_ptr(),
                    self.len(),
                    self.records.heap.capacity,
                ))
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::InlineList;

    #[test]
    fn layout_and_option_presence() {
        assert_eq!(std::mem::size_of::<InlineList<u64, 2>>(), 24);
        assert_eq!(std::mem::size_of::<Option<InlineList<u64, 2>>>(), 24);
        assert_eq!(std::mem::size_of::<InlineList<u32, 4>>(), 24);
        assert_eq!(std::mem::size_of::<Option<InlineList<u32, 4>>>(), 24);
        let present = Some(InlineList::<u32, 4>::new());
        assert!(present.is_some_and(|values| values.is_empty()));
    }

    #[test]
    fn spill_clone_clear_and_transfer_keep_order() {
        let mut list = InlineList::<u32, 4>::new();
        list.extend(0..4);
        assert_eq!(list.heap_capacity_bytes(), 0);
        list.push(4);
        assert_eq!(&*list, &[0, 1, 2, 3, 4]);
        list[1] = 9;
        let clone = list.clone();
        list.clear();
        assert!(list.heap_capacity_bytes() >= 20);
        list.extend_from_slice(&[8, 7]);
        assert_eq!(list.into_vec(), [8, 7]);
        assert_eq!(clone.into_iter().collect::<Vec<_>>(), [0, 9, 2, 3, 4]);
        let mut destination = InlineList::<u32, 4>::with_capacity(20);
        destination.extend(0..10);
        let short: InlineList<u32, 4> = [8, 6].into_iter().collect();
        destination.clone_from(&short);
        assert_eq!(destination.heap_capacity_bytes(), 0);
        assert_eq!(&*destination, &[8, 6]);
        let long: InlineList<u32, 4> = (4..12).collect();
        destination.clone_from(&long);
        destination[0] = 99;
        assert_eq!(&*long, &[4, 5, 6, 7, 8, 9, 10, 11]);
        let mut list: InlineList<u64, 2> = [5, 6].into_iter().collect();
        list.push(7);
        assert_eq!(list.into_vec(), [5, 6, 7]);
    }
}

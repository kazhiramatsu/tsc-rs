//! Small owners for the hot lists and optional cold parts of checker records.
use std::fmt;
use std::mem::ManuallyDrop;
use std::ops::{Deref, DerefMut};

const ADDRESS: usize = (1usize << 48) - 1;
const EMPTY_SLICE: usize = 8;
/// A one-word boxed slice owner. Large lengths and addresses use a full
/// descriptor; no element identity or supported length is truncated.
#[repr(transparent)]
pub struct ThinBoxSlice<T> {
    word: *mut T,
}
// SAFETY: ownership and borrowing match Box<[T]>.
unsafe impl<T: Send> Send for ThinBoxSlice<T> {}
// SAFETY: shared references only expose shared elements.
unsafe impl<T: Sync> Sync for ThinBoxSlice<T> {}
impl<T> ThinBoxSlice<T> {
    const VALID: () = assert!(size_of::<T>() > 0 && align_of::<T>() <= 8);
    pub const fn empty() -> Self {
        let () = Self::VALID;
        Self {
            word: std::ptr::without_provenance_mut(EMPTY_SLICE),
        }
    }
    #[inline]
    pub fn len(&self) -> usize {
        if self.full() {
            // SAFETY: tag one owns the full descriptor throughout this borrow.
            unsafe { (&*self.word.map_addr(|a| a & !7).cast::<Box<[T]>>()).len() }
        } else {
            self.word.addr() >> 48
        }
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        // Empty slices have one canonical word; full descriptors are created
        // only for nonempty lists, so this query never follows their pointer.
        self.word.addr() == EMPTY_SLICE
    }
    pub fn into_vec(self) -> Vec<T> {
        self.into_boxed_slice().into_vec()
    }
    pub fn new(list: Box<[T]>) -> Self {
        assert!(size_of::<T>() > 0 && align_of::<T>() <= 8);
        let len = list.len();
        if len == 0 {
            return Self {
                word: std::ptr::without_provenance_mut(EMPTY_SLICE),
            };
        }
        let ptr = list.as_ptr();
        if len <= u16::MAX as usize && ptr.addr() <= ADDRESS && ptr.addr() & 7 == 0 {
            let ptr = Box::into_raw(list).cast::<T>();
            Self {
                word: ptr.map_addr(|a| a | len << 48),
            }
        } else {
            let ptr = Box::into_raw(Box::new(list)).cast::<T>();
            Self {
                word: ptr.map_addr(|a| a | 1),
            }
        }
    }
    #[inline]
    fn full(&self) -> bool {
        self.word.addr() & 1 != 0
    }
    pub fn heap_bytes(&self) -> usize {
        self.len() * size_of::<T>()
            + if self.full() {
                size_of::<Box<[T]>>()
            } else {
                0
            }
    }
    pub fn into_boxed_slice(self) -> Box<[T]> {
        let this = ManuallyDrop::new(self);
        if this.full() {
            // SAFETY: tag one owns a Box containing the original fat Box pointer.
            unsafe { *Box::from_raw(this.word.map_addr(|a| a & !7).cast::<Box<[T]>>()) }
        } else {
            let len = this.word.addr() >> 48;
            if len == 0 {
                return Box::new([]);
            }
            // SAFETY: the original pointer's provenance was retained by map_addr.
            unsafe {
                Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                    this.word.map_addr(|a| a & ADDRESS),
                    len,
                ))
            }
        }
    }
}
impl<T> Deref for ThinBoxSlice<T> {
    type Target = [T];
    #[inline]
    fn deref(&self) -> &[T] {
        if self.full() {
            // SAFETY: the full descriptor is live and borrowed from this owner.
            unsafe { &*self.word.map_addr(|a| a & !7).cast::<Box<[T]>>() }
        } else {
            let len = self.word.addr() >> 48;
            if len == 0 {
                return &[];
            }
            // SAFETY: the encoded length and original slice pointer belong to this owner.
            unsafe { std::slice::from_raw_parts(self.word.map_addr(|a| a & ADDRESS), len) }
        }
    }
}
impl<T: Clone> Clone for ThinBoxSlice<T> {
    fn clone(&self) -> Self {
        Self::new(self.to_vec().into_boxed_slice())
    }
}
impl<T> Drop for ThinBoxSlice<T> {
    fn drop(&mut self) {
        let word = std::mem::replace(
            &mut self.word,
            std::ptr::without_provenance_mut(EMPTY_SLICE),
        );
        let owner = ManuallyDrop::new(Self { word });
        if owner.full() {
            // SAFETY: tag one uniquely owns the full slice descriptor.
            unsafe {
                drop(Box::from_raw(word.map_addr(|a| a & !7).cast::<Box<[T]>>()));
            }
        } else if word.addr() >> 48 != 0 {
            // SAFETY: the compressed pointer and length are the original Box parts.
            unsafe {
                drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                    word.map_addr(|a| a & ADDRESS),
                    word.addr() >> 48,
                )));
            }
        }
    }
}
impl<T: fmt::Debug> fmt::Debug for ThinBoxSlice<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.deref().fmt(f)
    }
}

impl<T> Default for ThinBoxSlice<T> {
    fn default() -> Self {
        Self::empty()
    }
}
impl<T> From<Box<[T]>> for ThinBoxSlice<T> {
    fn from(list: Box<[T]>) -> Self {
        Self::new(list)
    }
}
impl<T, const N: usize> From<Box<[T; N]>> for ThinBoxSlice<T> {
    fn from(list: Box<[T; N]>) -> Self {
        let list: Box<[T]> = list;
        Self::new(list)
    }
}
impl<T> From<Vec<T>> for ThinBoxSlice<T> {
    fn from(list: Vec<T>) -> Self {
        Self::new(list.into_boxed_slice())
    }
}
impl<T> DerefMut for ThinBoxSlice<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut [T] {
        if self.full() {
            // SAFETY: this owner exclusively borrows the full descriptor.
            unsafe { &mut *self.word.map_addr(|a| a & !7).cast::<Box<[T]>>() }
        } else {
            let len = self.word.addr() >> 48;
            if len == 0 {
                return &mut [];
            }
            // SAFETY: the pointer/length are this owner's original Box parts.
            unsafe { std::slice::from_raw_parts_mut(self.word.map_addr(|a| a & ADDRESS), len) }
        }
    }
}
pub struct ThinBoxIter<T: Copy> {
    owner: ThinBoxSlice<T>,
    front: usize,
    back: usize,
}
impl<T: Copy> Iterator for ThinBoxIter<T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        if self.front == self.back {
            None
        } else {
            let value = self.owner[self.front];
            self.front += 1;
            Some(value)
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.back - self.front;
        (len, Some(len))
    }
}
impl<T: Copy> DoubleEndedIterator for ThinBoxIter<T> {
    fn next_back(&mut self) -> Option<T> {
        if self.front == self.back {
            None
        } else {
            self.back -= 1;
            Some(self.owner[self.back])
        }
    }
}
impl<T: Copy> ExactSizeIterator for ThinBoxIter<T> {}
impl<T: Copy> IntoIterator for ThinBoxSlice<T> {
    type Item = T;
    type IntoIter = ThinBoxIter<T>;
    fn into_iter(self) -> Self::IntoIter {
        ThinBoxIter {
            back: self.len(),
            front: 0,
            owner: self,
        }
    }
}
impl<'a, T> IntoIterator for &'a ThinBoxSlice<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl<'a, T> IntoIterator for &'a mut ThinBoxSlice<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

/// A nullable one-word slice owner. Present-empty is different from absent.
#[repr(transparent)]
pub struct OptionalBoxSlice<T> {
    word: *mut T,
}
// SAFETY: the active value is an owned Box slice, or no value.
unsafe impl<T: Send> Send for OptionalBoxSlice<T> {}
// SAFETY: shared access exposes only shared element references.
unsafe impl<T: Sync> Sync for OptionalBoxSlice<T> {}
impl<T> OptionalBoxSlice<T> {
    pub const fn new() -> Self {
        Self {
            word: std::ptr::null_mut(),
        }
    }
    #[inline]
    pub fn is_some(&self) -> bool {
        !self.word.is_null()
    }
    #[inline]
    pub fn is_none(&self) -> bool {
        self.word.is_null()
    }
    #[inline]
    pub fn as_deref(&self) -> Option<&[T]> {
        if self.word.is_null() {
            None
        } else {
            // SAFETY: a present word is exactly a live ThinBoxSlice's transparent
            // representation; this view borrows it without taking ownership.
            Some(unsafe { &*std::ptr::addr_of!(self.word).cast::<ThinBoxSlice<T>>() })
        }
    }
    pub fn set(&mut self, value: Option<Box<[T]>>) {
        *self = Self::from(value);
    }
    pub fn take(&mut self) -> Option<Box<[T]>> {
        let word = std::mem::replace(&mut self.word, std::ptr::null_mut());
        (!word.is_null()).then(|| ThinBoxSlice { word }.into_boxed_slice())
    }
    pub fn heap_bytes(&self) -> usize {
        if self.word.is_null() {
            0
        } else {
            // SAFETY: as in as_deref, the owner representation is transparent.
            unsafe { (&*std::ptr::addr_of!(self.word).cast::<ThinBoxSlice<T>>()).heap_bytes() }
        }
    }
}
impl<T> Default for OptionalBoxSlice<T> {
    fn default() -> Self {
        Self::new()
    }
}
impl<T> From<Option<Box<[T]>>> for OptionalBoxSlice<T> {
    fn from(value: Option<Box<[T]>>) -> Self {
        Self {
            word: value.map_or(std::ptr::null_mut(), |list| {
                let owner = ManuallyDrop::new(ThinBoxSlice::new(list));
                owner.word
            }),
        }
    }
}
impl<T: Clone> Clone for OptionalBoxSlice<T> {
    fn clone(&self) -> Self {
        Self::from(self.as_deref().map(|list| list.to_vec().into_boxed_slice()))
    }
}
impl<T> Drop for OptionalBoxSlice<T> {
    fn drop(&mut self) {
        let word = std::mem::replace(&mut self.word, std::ptr::null_mut());
        if !word.is_null() {
            drop(ThinBoxSlice { word });
        }
    }
}
impl<T: fmt::Debug> fmt::Debug for OptionalBoxSlice<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_deref().fmt(f)
    }
}

#[repr(C, align(16))]
struct Aligned<T>(T);
#[repr(C, align(16))]
struct Full<T> {
    payload: Option<Box<Aligned<T>>>,
    count: u32,
    kind: u8,
}
/// One optional cold owner with an inline count, two-bit kind and three
/// independent flags. Unusually large counts or addresses use a descriptor
/// containing their complete values. Payload addresses retain provenance.
pub struct PackedBox<T> {
    word: *mut Aligned<T>,
}
// SAFETY: this owner transfers its uniquely owned payload as Box<T> does.
unsafe impl<T: Send> Send for PackedBox<T> {}
// SAFETY: shared access only borrows the payload.
unsafe impl<T: Sync> Sync for PackedBox<T> {}
impl<T> PackedBox<T> {
    pub const fn new() -> Self {
        Self {
            word: std::ptr::null_mut(),
        }
    }
    #[inline]
    fn full(&self) -> bool {
        self.word.addr() & 8 != 0
    }
    #[inline]
    fn pointer(&self) -> *mut Aligned<T> {
        self.word.map_addr(|a| a & ADDRESS & !15)
    }
    fn descriptor(&self) -> &Full<T> {
        // SAFETY: called only for a tagged, live full descriptor.
        unsafe { &*self.word.map_addr(|a| a & !15).cast::<Full<T>>() }
    }
    fn descriptor_mut(&mut self) -> &mut Full<T> {
        // SAFETY: the owner is borrowed exclusively and owns the descriptor.
        unsafe { &mut *self.word.map_addr(|a| a & !15).cast::<Full<T>>() }
    }
    #[inline]
    pub fn tag(&self, bit: u8) -> bool {
        assert!(bit < 3);
        self.word.addr() & (1usize << bit) != 0
    }
    #[inline]
    pub fn set_tag(&mut self, bit: u8, value: bool) {
        assert!(bit < 3);
        let mask = 1usize << bit;
        self.word = self
            .word
            .map_addr(|a| if value { a | mask } else { a & !mask });
    }
    #[inline]
    pub fn count(&self) -> u32 {
        if self.full() {
            self.descriptor().count
        } else {
            (self.word.addr() >> 48 & 0x3fff) as u32
        }
    }
    #[inline]
    pub fn kind(&self) -> u8 {
        if self.full() {
            self.descriptor().kind
        } else {
            (self.word.addr() >> 62) as u8
        }
    }
    #[inline]
    pub fn set_count(&mut self, count: u32) {
        if !self.full() && count <= 0x3fff {
            self.word = self
                .word
                .map_addr(|a| (a & !(0x3fffusize << 48)) | ((count as usize) << 48));
        } else {
            self.promote();
            self.descriptor_mut().count = count;
        }
    }
    #[inline]
    pub fn set_kind(&mut self, kind: u8) {
        assert!(kind < 4);
        if self.full() {
            self.descriptor_mut().kind = kind;
        } else {
            self.word = self
                .word
                .map_addr(|a| (a & !(3usize << 62)) | ((kind as usize) << 62));
        }
    }
    fn promote(&mut self) {
        if self.full() {
            return;
        }
        let count = self.count();
        let kind = self.kind();
        let tags = self.word.addr() & 7;
        let ptr = self.pointer();
        let payload = if ptr.is_null() {
            None
        } else {
            // SAFETY: the compressed pointer uniquely owns an Aligned<T>.
            Some(unsafe { Box::from_raw(ptr) })
        };
        let ptr = Box::into_raw(Box::new(Full {
            payload,
            count,
            kind,
        }))
        .cast::<Aligned<T>>();
        self.word = ptr.map_addr(|a| a | 8 | tags);
    }
    #[inline]
    pub fn get(&self) -> Option<&T> {
        if self.full() {
            return self.descriptor().payload.as_ref().map(|v| &v.0);
        }
        let ptr = self.pointer();
        if ptr.is_null() {
            None
        } else {
            // SAFETY: the original aligned payload pointer is borrowed from its owner.
            Some(unsafe { &(*ptr).0 })
        }
    }
    #[inline]
    pub fn get_mut(&mut self) -> Option<&mut T> {
        if self.full() {
            return self.descriptor_mut().payload.as_mut().map(|v| &mut v.0);
        }
        let ptr = self.pointer();
        if ptr.is_null() {
            None
        } else {
            // SAFETY: this owner exclusively borrows its payload.
            Some(unsafe { &mut (*ptr).0 })
        }
    }
    pub fn ensure_with(&mut self, make: impl FnOnce() -> T) -> &mut T {
        if self.get().is_none() {
            // Construct before changing the owner, so a panic preserves its metadata.
            let payload = Box::new(Aligned(make()));
            if self.full() {
                self.descriptor_mut().payload = Some(payload);
            } else {
                let ptr = Box::into_raw(payload);
                let metadata = self.word.addr();
                if ptr.addr() <= ADDRESS {
                    self.word = ptr.map_addr(|a| a | metadata);
                } else {
                    // SAFETY: this is the just-created box, not yet published elsewhere.
                    let payload = Some(unsafe { Box::from_raw(ptr) });
                    let count = self.count();
                    let kind = self.kind();
                    let tags = metadata & 7;
                    self.word = Box::into_raw(Box::new(Full {
                        payload,
                        count,
                        kind,
                    }))
                    .cast::<Aligned<T>>()
                    .map_addr(|a| a | 8 | tags);
                }
            }
        }
        self.get_mut().expect("a cold payload was initialized")
    }
    pub fn clear(&mut self) {
        if self.full() {
            self.descriptor_mut().payload = None;
            return;
        }
        let ptr = self.pointer();
        let metadata = self.word.addr() & !(ADDRESS & !15);
        self.word = std::ptr::without_provenance_mut(metadata);
        if !ptr.is_null() {
            // SAFETY: the old payload is detached and uniquely owned here.
            unsafe {
                drop(Box::from_raw(ptr));
            }
        }
    }
    pub fn heap_bytes(&self) -> usize {
        usize::from(self.full()) * size_of::<Full<T>>()
            + usize::from(self.get().is_some()) * size_of::<Aligned<T>>()
    }
}
impl<T> Default for PackedBox<T> {
    fn default() -> Self {
        Self::new()
    }
}
impl<T: Clone> Clone for PackedBox<T> {
    fn clone(&self) -> Self {
        let mut result = Self::new();
        result.set_count(self.count());
        result.set_kind(self.kind());
        for bit in 0..3 {
            result.set_tag(bit, self.tag(bit));
        }
        if let Some(value) = self.get() {
            result.ensure_with(|| value.clone());
        }
        result
    }
}
impl<T> Drop for PackedBox<T> {
    fn drop(&mut self) {
        if self.full() {
            // SAFETY: a full owner uniquely owns its descriptor and the nested payload.
            unsafe {
                drop(Box::from_raw(
                    self.word.map_addr(|a| a & !15).cast::<Full<T>>(),
                ));
            }
        } else {
            self.clear();
        }
    }
}
impl<T: fmt::Debug> fmt::Debug for PackedBox<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.get().fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    #[test]
    fn thin_lists_keep_their_allocation_and_full_lengths() {
        assert_eq!(size_of::<ThinBoxSlice<u32>>(), 8);
        for n in [0, 1, 2, 65535, 65536] {
            let values: Box<[u32]> = (0..n).collect();
            let pointer = values.as_ptr();
            let mut list = ThinBoxSlice::new(values);
            let copy = list.clone();
            assert_eq!(list.len(), n as usize);
            assert_eq!(list.is_empty(), n == 0);
            if n > 0 {
                list[0] = u32::MAX;
                assert_eq!(copy[0], 0);
            }
            let boxed = list.into_boxed_slice();
            if n > 0 {
                assert_eq!(boxed.as_ptr(), pointer);
            }
            assert_eq!(
                copy.into_iter().rev().collect::<Vec<_>>(),
                (0..n).rev().collect::<Vec<_>>()
            );
        }
    }
    #[derive(Clone)]
    struct CountDrop(Arc<AtomicUsize>);
    impl Drop for CountDrop {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    #[test]
    fn thin_lists_drop_each_element_once_in_both_representations() {
        for n in [0, 1, 65535, 65536] {
            let count = Arc::new(AtomicUsize::new(0));
            let list: Box<[_]> = (0..n).map(|_| CountDrop(count.clone())).collect();
            let thin = ThinBoxSlice::new(list);
            let copy = thin.clone();
            drop(thin);
            assert_eq!(count.load(Ordering::Relaxed), n);
            drop(copy);
            assert_eq!(count.load(Ordering::Relaxed), 2 * n);
        }
    }
    #[test]
    fn optional_lists_keep_absent_empty_and_long_values_distinct() {
        assert_eq!(size_of::<OptionalBoxSlice<u32>>(), 8);
        let mut list = OptionalBoxSlice::new();
        assert!(list.is_none());
        list.set(Some(Box::new([])));
        assert!(list.is_some());
        assert_eq!(list.as_deref(), Some([].as_slice()));
        assert_eq!(list.take().as_deref(), Some([].as_slice()));
        assert!(list.is_none());
        let values: Box<[u32]> = (0..65536).collect();
        let pointer = values.as_ptr();
        list.set(Some(values));
        let copy = list.clone();
        assert_eq!(list.as_deref().unwrap().len(), 65536);
        assert_eq!(list.as_deref().unwrap().as_ptr(), pointer);
        list.set(None);
        assert!(list.is_none());
        assert_eq!(copy.as_deref().unwrap()[65535], 65535);
    }
    #[test]
    fn cold_owners_keep_metadata_across_payload_and_full_descriptor_changes() {
        assert_eq!(size_of::<PackedBox<u32>>(), 8);
        let mut owner = PackedBox::new();
        owner.set_count(16383);
        owner.set_kind(2);
        owner.set_tag(0, true);
        owner.set_tag(2, true);
        owner.ensure_with(|| 17u32);
        assert_eq!(owner.get(), Some(&17));
        owner.set_count(u32::MAX);
        assert!(owner.full());
        assert_eq!(owner.count(), u32::MAX);
        assert_eq!(owner.kind(), 2);
        assert!(owner.tag(0) && owner.tag(2));
        *owner.get_mut().unwrap() = 19;
        let copy = owner.clone();
        owner.clear();
        assert!(owner.get().is_none());
        assert_eq!(owner.count(), u32::MAX);
        assert_eq!(copy.get(), Some(&19));
        owner.set_count(0);
        owner.set_kind(1);
        owner.ensure_with(|| 23);
        assert_eq!(owner.get(), Some(&23));
        let mut forced = PackedBox::new();
        forced.set_count(7);
        forced.ensure_with(|| 29);
        forced.promote();
        assert_eq!(forced.get(), Some(&29));
        assert_eq!(forced.count(), 7);
        forced.clear();
        assert_eq!(forced.count(), 7);
    }
    #[test]
    fn a_panicking_payload_constructor_preserves_metadata_and_ownership() {
        let mut owner = PackedBox::<u32>::new();
        owner.set_count(u32::MAX);
        owner.set_kind(2);
        owner.set_tag(1, true);
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            owner.ensure_with(|| panic!("constructor"));
        }))
        .is_err());
        assert_eq!(owner.count(), u32::MAX);
        assert_eq!(owner.kind(), 2);
        assert!(owner.tag(1));
        assert!(owner.get().is_none());
    }
}

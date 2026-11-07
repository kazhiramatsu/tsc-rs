use std::cmp::Ordering;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::num::NonZeroU32;

use crate::{owner_of, Record};

/// Compact identity, with caller-managed liveness (see the crate contract).
#[repr(transparent)]
pub struct Ref<T: Record>(NonZeroU32, PhantomData<&'static T>);

impl<T: Record> Ref<T> {
    /// The caller must supply an issued allocation address: aligned, in the
    /// handle range, and nonzero after encoding. Only arena allocation uses it.
    #[inline(always)]
    pub(crate) unsafe fn from_allocated(pointer: usize) -> Self {
        let raw = crate::memory::raw(pointer);
        debug_assert_ne!(raw, 0);
        // SAFETY: the caller establishes the allocation's nonzero handle.
        Self(unsafe { NonZeroU32::new_unchecked(raw) }, PhantomData)
    }
    pub const fn raw(self) -> u32 {
        self.0.get()
    }
    /// Decode a stored handle. Nonzero alone does not prove allocation, type,
    /// bounds or liveness. Only dereference handles issued for the expected T.
    pub const fn from_raw(raw: u32) -> Option<Self> {
        match NonZeroU32::new(raw) {
            Some(raw) => Some(Self(raw, PhantomData)),
            None => None,
        }
    }
    /// Read a live issued record. The arena holder keeps every use of this
    /// view within the crate's lifetime, publication and aliasing contract.
    #[inline(always)]
    pub fn get(self) -> &'static T {
        debug_assert!(
            owner_of(self.raw()).is_some(),
            "arena handle names a free or unissued block"
        );
        // SAFETY: caller's holder contract establishes a live, aligned, issued
        // T and excludes conflicting writes. Record establishes value validity.
        unsafe { &*(crate::memory::address(self.raw()) as *const T) }
    }
}
impl<T: Record> Copy for Ref<T> {}
impl<T: Record> Clone for Ref<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: Record> PartialEq for Ref<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl<T: Record> Eq for Ref<T> {}
impl<T: Record> PartialOrd for Ref<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl<T: Record> Ord for Ref<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}
impl<T: Record> Hash for Ref<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}
impl<T: Record> std::fmt::Debug for Ref<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Ref").field(&self.raw()).finish()
    }
}
// SAFETY: the niche represents zero as None and all other u32 values as a
// handle. PhantomData adds no storage. Dereferencing still requires the holder contract.
unsafe impl<T: Record> Record for Option<Ref<T>> {}

/// Eight-byte span. The stored start is a raw word so zero/empty is valid POD.
/// Bare `Ref<T>` would violate Record's all-bit-pattern validity at zero.
#[repr(C)]
pub struct Span<T: Record> {
    pub(crate) start: u32,
    pub(crate) len: u32,
    marker: PhantomData<&'static T>,
}
impl<T: Record> Span<T> {
    pub(crate) fn new(start: u32, len: u32) -> Self {
        Self {
            start,
            len,
            marker: PhantomData,
        }
    }
    pub const fn empty() -> Self {
        Self {
            start: 0,
            len: 0,
            marker: PhantomData,
        }
    }
    pub fn start(self) -> Option<Ref<T>> {
        Ref::from_raw(self.start)
    }
    pub fn len(self) -> usize {
        self.len as usize
    }
    pub fn is_empty(self) -> bool {
        self.len == 0
    }
    pub fn as_slice(self) -> &'static [T] {
        if self.len == 0 {
            return &[];
        }
        let start = self.start().expect("nonempty span has zero start");
        let pointer = start.get() as *const T;
        // SAFETY: issued spans describe a single initialized allocation. The
        // holder must not use arbitrary/stale lengths or outlive that allocation.
        unsafe { std::slice::from_raw_parts(pointer, self.len as usize) }
    }
}
impl<T: Record> Copy for Span<T> {}
impl<T: Record> Clone for Span<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: Record> Default for Span<T> {
    fn default() -> Self {
        Self::empty()
    }
}
// SAFETY: two unrestricted u32 words and zero-sized marker; all bits valid.
unsafe impl<T: Record> Record for Span<T> {}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ListHeader {
    pub len: u32,
    pub extra: u32,
}
// SAFETY: C layout, two unrestricted integers, no Drop.
unsafe impl Record for ListHeader {}

/// Marker for a ListHeader followed immediately by its T elements.
#[repr(transparent)]
pub struct List<T: Record>(PhantomData<T>);
// SAFETY: the marker is zero-sized and has no validity constraints or Drop.
unsafe impl<T: Record> Record for List<T> {}
impl<T: Record> Ref<List<T>> {
    pub fn header(self) -> &'static ListHeader {
        Ref::<ListHeader>::from_raw(self.raw()).unwrap().get()
    }
    pub fn items(self) -> &'static [T] {
        let len = self.header().len as usize;
        // SAFETY: issued list allocation has its header followed by len Ts;
        // T's alignment is <= 8. Same holder contract as Ref::get.
        unsafe {
            std::slice::from_raw_parts((crate::memory::address(self.raw()) + 8) as *const T, len)
        }
    }
}

/// Marker for a ListHeader followed by literal bytes.
pub struct Bytes;
// SAFETY: zero-sized marker, no invalid bit patterns or Drop.
unsafe impl Record for Bytes {}
impl Ref<Bytes> {
    pub fn as_bytes(self) -> &'static [u8] {
        Ref::<List<u8>>::from_raw(self.raw()).unwrap().items()
    }
}

const _: () = assert!(std::mem::size_of::<Ref<u64>>() == 4);
const _: () = assert!(std::mem::size_of::<Option<Ref<u64>>>() == 4);
const _: () = assert!(std::mem::size_of::<Span<u64>>() == 8);

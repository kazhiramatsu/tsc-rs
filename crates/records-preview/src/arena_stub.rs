//! Safe, Vec-backed shape stub. No address arithmetic, leaked storage or unsafe blocks.
//! Reads borrow the arena explicitly; see STATUS.md for integration differences.
use std::any::Any;
use std::cell::{Cell, Ref as Borrow, RefCell};
use std::marker::PhantomData;
use std::num::NonZeroU32;

/// # Safety
/// Implement only for repr(C)/transparent, reference-free, drop-free records
/// with alignment <= 8 and no invalid bit patterns (including zero).
pub unsafe trait Record: 'static {}

#[repr(transparent)]
pub struct Ref<T: Record>(NonZeroU32, PhantomData<&'static T>);
impl<T: Record> Copy for Ref<T> {}
impl<T: Record> Clone for Ref<T> { fn clone(&self) -> Self { *self } }
impl<T: Record> PartialEq for Ref<T> { fn eq(&self, b: &Self) -> bool { self.raw() == b.raw() } }
impl<T: Record> Eq for Ref<T> {}
impl<T: Record> PartialOrd for Ref<T> { fn partial_cmp(&self, b: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(b)) } }
impl<T: Record> Ord for Ref<T> { fn cmp(&self, b: &Self) -> std::cmp::Ordering { self.raw().cmp(&b.raw()) } }
impl<T: Record> std::hash::Hash for Ref<T> { fn hash<H: std::hash::Hasher>(&self, h: &mut H) { self.raw().hash(h); } }
impl<T: Record> std::fmt::Debug for Ref<T> { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { self.raw().fmt(f) } }
impl<T: Record> Ref<T> {
    pub fn raw(self) -> u32 { self.0.get() }
    pub fn from_raw(raw: u32) -> Option<Self> { NonZeroU32::new(raw).map(|r| Self(r, PhantomData)) }
    pub fn get(self, arena: &Arena) -> Borrow<'_, T> { arena.get(self) }
}

/// Raw start is nullable so every bit pattern, including zero, is valid.
/// Unlike SPEC-arena's NonZero start, Option<Span<T>> would be 12 bytes.
#[repr(C)]
pub struct Span<T: Record> { pub start: u32, pub len: u32, marker: PhantomData<&'static T> }
impl<T: Record> Copy for Span<T> {}
impl<T: Record> Clone for Span<T> { fn clone(&self) -> Self { *self } }
impl<T: Record> Default for Span<T> { fn default() -> Self { Self::from_raw(0, 0) } }
impl<T: Record> std::fmt::Debug for Span<T> { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { (self.start, self.len).fmt(f) } }
impl<T: Record> PartialEq for Span<T> { fn eq(&self, b: &Self) -> bool { (self.start, self.len) == (b.start, b.len) } }
impl<T: Record> Eq for Span<T> {}
unsafe impl<T: Record> Record for Span<T> {}
impl<T: Record> Span<T> {
    pub const fn from_raw(start: u32, len: u32) -> Self { Self { start, len, marker: PhantomData } }
    pub fn as_slice(self, arena: &Arena) -> Borrow<'_, [T]> {
        Borrow::map(arena.values.borrow(), |v| {
            if self.len == 0 { return &[] as &[T]; }
            &v[(self.start - 1) as usize].as_ref().expect("span start").downcast_ref::<Vec<T>>().expect("span type")[..self.len as usize]
        })
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ListHeader { pub len: u32, pub extra: u32 }
unsafe impl Record for ListHeader {}
#[repr(C)]
pub struct List<T: Record>(PhantomData<&'static T>);
unsafe impl<T: Record> Record for List<T> {}
struct ListValue<T> { header: ListHeader, values: Vec<T> }
impl<T: Record> Ref<List<T>> {
    pub fn items(self, arena: &Arena) -> Borrow<'_, [T]> {
        Borrow::map(arena.values.borrow(), |v| &v[(self.raw() - 1) as usize].as_ref().expect("record start").downcast_ref::<ListValue<T>>().expect("list type").values[..])
    }
    pub fn header(self, arena: &Arena) -> Borrow<'_, ListHeader> {
        Borrow::map(arena.values.borrow(), |v| &v[(self.raw() - 1) as usize].as_ref().expect("record start").downcast_ref::<ListValue<T>>().expect("list type").header)
    }
}
#[repr(C)]
pub struct Bytes;
unsafe impl Record for Bytes {}
impl Ref<Bytes> {
    pub fn as_bytes(self, arena: &Arena) -> Borrow<'_, [u8]> {
        Borrow::map(arena.values.borrow(), |v| &v[(self.raw() - 1) as usize].as_ref().expect("record start").downcast_ref::<Vec<u8>>().expect("bytes type")[..])
    }
}
#[repr(transparent)]
#[derive(Debug, Default)]
pub struct ArenaCell<T: Copy>(Cell<T>);
impl<T: Copy> ArenaCell<T> {
    pub const fn new(value: T) -> Self { Self(Cell::new(value)) }
    #[inline] pub fn get(&self) -> T { self.0.get() }
    #[inline] pub fn set(&self, value: T) { self.0.set(value); }
}
unsafe impl<T: Copy + Record> Record for ArenaCell<T> {}
macro_rules! primitives { ($($t:ty),*) => { $(unsafe impl Record for $t {})* }; }
primitives!(u8, u16, u32, i32, u64, f64);
unsafe impl<T: Record, const N: usize> Record for [T; N] {}
unsafe impl<T: Record> Record for Option<Ref<T>> {}

#[derive(Default)]
pub struct Arena { values: RefCell<Vec<Option<Box<dyn Any>>>>, used: Cell<usize>, owner: u32 }
impl Arena {
    pub fn new(owner: u32) -> Self { Self { owner, ..Self::default() } }
    pub fn owner(&self) -> u32 { self.owner }
    fn push<T: 'static>(&self, value: T, bytes: usize) -> u32 {
        let mut values = self.values.borrow_mut();
        let raw = u32::try_from(values.len() + 1).expect("stub handle overflow");
        let charged = bytes.next_multiple_of(8).max(8);
        let next = values.len() + charged / 8;
        values.push(Some(Box::new(value)));
        values.resize_with(next, || None);
        self.used.set(self.used.get() + charged);
        raw
    }
    pub fn alloc<T: Record>(&self, value: T) -> Ref<T> {
        Ref::from_raw(self.push(value, std::mem::size_of::<T>())).unwrap()
    }
    pub fn alloc_slice<T: Record + Copy>(&self, values: &[T]) -> Span<T> {
        if values.is_empty() { return Span::default(); }
        let raw = self.push(values.to_vec(), std::mem::size_of_val(values));
        Span::from_raw(raw, values.len().try_into().unwrap())
    }
    pub fn alloc_list<T: Record + Copy>(&self, values: &[T]) -> Ref<List<T>> {
        let value = ListValue { header: ListHeader { len: values.len().try_into().unwrap(), extra: 0 }, values: values.to_vec() };
        Ref::from_raw(self.push(value, 8 + std::mem::size_of_val(values))).unwrap()
    }
    pub fn alloc_bytes(&self, bytes: &[u8]) -> Ref<Bytes> {
        Ref::from_raw(self.push(bytes.to_vec(), 8 + bytes.len())).unwrap()
    }
    pub fn get<T: Record>(&self, r: Ref<T>) -> Borrow<'_, T> {
        Borrow::map(self.values.borrow(), |v| v[(r.raw() - 1) as usize].as_ref().expect("record start").downcast_ref().expect("record type"))
    }
    pub fn get_mut<T: Record>(&mut self, r: Ref<T>) -> &mut T {
        self.values.get_mut()[(r.raw() - 1) as usize].as_mut().expect("record start").downcast_mut().expect("record type")
    }
    pub fn freeze(self) -> FrozenArena { FrozenArena(self) }
    pub fn used_bytes(&self) -> usize { self.used.get() }
    pub fn committed_bytes(&self) -> usize { self.used_bytes() }
    pub fn adopt_current_thread(&self) {}
}
pub type LocalArena = Arena;
pub struct SharedArena(Arena);
impl SharedArena {
    pub fn new(owner: u32) -> Self { Self(Arena::new(owner)) }
    pub fn alloc<T: Record + Sync>(&self, value: T) -> Ref<T> { self.0.alloc(value) }
    pub fn alloc_slice<T: Record + Copy + Sync>(&self, values: &[T]) -> Span<T> { self.0.alloc_slice(values) }
    pub fn alloc_list<T: Record + Copy + Sync>(&self, values: &[T]) -> Ref<List<T>> { self.0.alloc_list(values) }
    pub fn alloc_bytes(&self, bytes: &[u8]) -> Ref<Bytes> { self.0.alloc_bytes(bytes) }
    pub fn get<T: Record>(&self, r: Ref<T>) -> Borrow<'_, T> { self.0.get(r) }
    pub fn get_mut<T: Record>(&mut self, r: Ref<T>) -> &mut T { self.0.get_mut(r) }
    pub fn list_items<T: Record>(&self, r: Ref<List<T>>) -> Borrow<'_, [T]> { r.items(&self.0) }
    pub fn list_header<T: Record>(&self, r: Ref<List<T>>) -> Borrow<'_, ListHeader> { r.header(&self.0) }
    pub fn bytes(&self, r: Ref<Bytes>) -> Borrow<'_, [u8]> { r.as_bytes(&self.0) }
    pub fn freeze(self) -> FrozenArena { self.0.freeze() }
    pub fn used_bytes(&self) -> usize { self.0.used_bytes() }
    pub fn committed_bytes(&self) -> usize { self.0.committed_bytes() }
}
pub struct FrozenArena(Arena);
impl FrozenArena {
    pub fn list_items<T: Record>(&self, r: Ref<List<T>>) -> Borrow<'_, [T]> { r.items(&self.0) }
    pub fn bytes(&self, r: Ref<Bytes>) -> Borrow<'_, [u8]> { r.as_bytes(&self.0) }
    pub fn get<T: Record>(&self, r: Ref<T>) -> Borrow<'_, T> { self.0.get(r) }
    pub fn used_bytes(&self) -> usize { self.0.used_bytes() }
    pub fn committed_bytes(&self) -> usize { self.0.committed_bytes() }
    pub fn release(self) {}
}
const _: () = assert!(std::mem::size_of::<Option<Ref<Bytes>>>() == 4);
const _: () = assert!(std::mem::size_of::<Span<u32>>() == 8);

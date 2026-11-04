//! Final-placement rooms for zero-valid Copy record headers. An issued
//! position never moves, and there is no publish step or heap fallback.
//!
//! Existing regions 3 (Symbol) and 7 (Type) use `with_room_source` to retain
//! their existing identity leases. The default allocator only serves the new
//! B-3 regions, so it cannot mint overlapping legacy identities.
//!
//! A proved record cannot survive its owner:
//! ```compile_fail
//! use tsc_store::claimed::ClaimedRecords;
//! use tsc_store::StoreRecord;
//! #[derive(Clone, Copy)] struct Header(u32);
//! impl StoreRecord for Header { const REGION: usize = 15; }
//! let proof = {
//!     let mut owner = ClaimedRecords::new();
//!     let id = owner.push(Header(1));
//!     owner.prove(id).unwrap()
//! };
//! let _ = proof.get();
//! ```

use std::marker::PhantomData;
use std::mem::size_of;

use crate::rooms::Room;
use crate::{StorageStats, StoreRecord, StoreVec};

/// B-3's typed region range, immediately after the frozen element room.
pub const FIRST_RECORD_REGION: usize = crate::frozen::ELEMENT_REGION + 1;
/// A symbol's declaration list (S1a-2): `Tagged<NodeId, 37>` spans.
pub const DECLARATION_ITEM_REGION: usize = FIRST_RECORD_REGION + 22;
/// Flow nodes (S10), 16-byte records in 32-record chunks.
pub const FLOW_NODE_REGION: usize = FIRST_RECORD_REGION + 23;
/// A flow label's antecedents (S10): `Tagged<FlowId, 39>` spans.
pub const FLOW_ITEM_REGION: usize = FIRST_RECORD_REGION + 24;
pub const RECORD_REGION_END: usize = FIRST_RECORD_REGION + 25;

/// A `T` stored in region `R` as well as (or instead of) its own: a second
/// list of the same zero-valid record type, read back as `&[T]` through
/// [`slice_tagged`]. `repr(transparent)`, so its bytes are a `T`'s.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(transparent)]
pub struct Tagged<T, const R: usize>(pub T);

impl<T: StoreRecord, const R: usize> StoreRecord for Tagged<T, R> {
    const REGION: usize = R;
}

/// The `len` records of region `R` from `position`, as `T`s. The contract of
/// [`crate::slice`]: the positions were issued by a live owner (a span of
/// [`ClaimedRecords::push_slice`]); unissued slots read as zeros.
#[inline(always)]
pub fn slice_tagged<T: StoreRecord, const R: usize>(position: u32, len: u32) -> &'static [T] {
    let tagged = crate::slice::<Tagged<T, R>>(position, len);
    // SAFETY: Tagged<T, R> is repr(transparent) over T: a slice of it is a
    // slice of T with the same address, length and validity.
    unsafe { std::slice::from_raw_parts(tagged.as_ptr().cast::<T>(), tagged.len()) }
}

/// An empty final-placement StoreVec room and its actual identity lease.
/// StoreVec's existing claim/lease preconditions apply. The owner consumes
/// both; the records are scrubbed before the lease can return their IDs.
pub struct LeasedRoom<T: StoreRecord> {
    records: StoreVec<T>,
    _lease: Box<dyn std::any::Any + Send + Sync>,
    start: u32,
}

impl<T: StoreRecord> LeasedRoom<T> {
    /// Transfer a claimed empty room and the lease that owns its complete
    /// capacity. This does not publish, move or copy the records.
    pub fn new<L: Send + Sync + 'static>(records: StoreVec<T>, lease: L) -> Self {
        assert!(
            records.placement == crate::Placement::Claimed
                && records.len == 0
                && records.capacity != 0,
            "expected an empty claimed room"
        );
        let size = size_of::<T>();
        assert!(size != 0, "a zero-sized record");
        let offset = (records.records.as_ptr() as usize)
            .checked_sub(crate::base::<T>())
            .expect("room outside its region");
        assert!(offset.is_multiple_of(size), "misaligned record position");
        let start = u32::try_from(offset / size).expect("room position exceeds u32");
        Self {
            records,
            _lease: Box::new(lease),
            start,
        }
    }
}

enum RecordRoom<T: StoreRecord> {
    Allocated(Room<T>),
    Leased(LeasedRoom<T>),
}

impl<T: StoreRecord + Copy> RecordRoom<T> {
    fn start(&self) -> u32 {
        match self {
            Self::Allocated(room) => room.start,
            Self::Leased(room) => room.start,
        }
    }
    fn capacity(&self) -> u32 {
        match self {
            Self::Allocated(room) => room.capacity,
            Self::Leased(room) => {
                u32::try_from(room.records.capacity).expect("room capacity exceeds u32")
            }
        }
    }
    fn dirty(&self) -> u32 {
        match self {
            Self::Allocated(room) => room.dirty,
            Self::Leased(room) => room.records.len as u32,
        }
    }
    fn push(&mut self, offset: u32, record: T) {
        match self {
            Self::Allocated(room) => {
                room.initialize_through(offset + 1);
                // SAFETY: offset is the next unexposed slot, below capacity.
                unsafe {
                    room.pointer(offset).write(record);
                }
            }
            Self::Leased(room) => {
                assert_eq!(room.records.len, offset as usize);
                assert!(
                    room.records.len < room.records.capacity,
                    "a final room cannot grow"
                );
                room.records.push(record);
            }
        }
    }
    fn push_slice(&mut self, offset: u32, records: &[T]) {
        let end = offset + records.len() as u32;
        match self {
            Self::Allocated(room) => {
                room.initialize_through(end);
                // SAFETY: [offset, end) are the next unexposed slots, below
                // capacity (initialize_through asserts end <= capacity).
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        records.as_ptr(),
                        room.pointer(offset),
                        records.len(),
                    );
                }
            }
            Self::Leased(_) => {
                for (index, &record) in records.iter().enumerate() {
                    self.push(offset + index as u32, record);
                }
            }
        }
    }
}

struct Chunk<T: StoreRecord> {
    room: RecordRoom<T>,
    len: u32,
}

/// Nonmoving record headers, owned until program/checker teardown. `T` is a
/// zero-valid `Copy` codec under [`StoreRecord`]'s contract. No zeroed/free
/// record is decoded by this API; only positions issued by `push` are readable.
/// Mutable access requires an exclusive borrow of the owner.
pub struct ClaimedRecords<T: StoreRecord + Copy> {
    chunks: Vec<Chunk<T>>,
    chunk_capacity: u32,
    len: usize,
    source: Option<Box<dyn FnMut(u32) -> LeasedRoom<T> + Send + Sync>>,
}

impl<T: StoreRecord + Copy> ClaimedRecords<T> {
    pub fn new() -> Self {
        Self::with_chunk_capacity(1 << 16)
    }

    /// Room capacity is a growth estimate, not a record-count limit.
    pub fn with_chunk_capacity(capacity: u32) -> Self {
        assert!(capacity != 0, "an empty record chunk");
        assert!(
            (FIRST_RECORD_REGION..crate::REGIONS).contains(&T::REGION)
                && T::REGION != crate::recycle::SCRATCH_WORD_REGION,
            "record region is legacy or reserved for scratch words"
        );
        Self {
            chunks: Vec::new(),
            chunk_capacity: capacity,
            len: 0,
            source: None,
        }
    }

    /// Grow by obtaining empty rooms from the existing identity domain. The
    /// callback must transfer the actual lease into each LeasedRoom; Symbols
    /// from binder/program and checker use the same domain and region 3.
    /// Types retain region 7. No second legacy allocator is installed.
    pub fn with_room_source<F>(capacity: u32, source: F) -> Self
    where
        F: FnMut(u32) -> LeasedRoom<T> + Send + Sync + 'static,
    {
        assert!(capacity != 0, "an empty record chunk");
        assert!(
            matches!(T::REGION, 3 | 7),
            "external room sources are for the existing Symbol/Type domains"
        );
        Self {
            chunks: Vec::new(),
            chunk_capacity: capacity,
            len: 0,
            source: Some(Box::new(source)),
        }
    }

    /// Initialize directly at the returned global position. Exhaustion adds
    /// another room, leaving all earlier addresses and identities unchanged.
    pub fn push(&mut self, record: T) -> u32 {
        if self
            .chunks
            .last()
            .is_none_or(|chunk| chunk.len == chunk.room.capacity())
        {
            self.add_room(self.chunk_capacity);
        }
        let chunk = self.chunks.last_mut().unwrap();
        let position = chunk.room.start() + chunk.len;
        chunk.room.push(chunk.len, record);
        chunk.len += 1;
        self.len += 1;
        position
    }

    /// Initialize `records` at consecutive global positions of one room and
    /// return the first, so that [`crate::slice`] (or [`slice_tagged`]) reads
    /// them back as one slice. A room that cannot hold all of them is left
    /// with its tail unused and the next room starts; a slice longer than the
    /// chunk capacity gets a room of its own size. An empty slice claims
    /// nothing and returns 0, which no room issues.
    pub fn push_slice(&mut self, records: &[T]) -> u32 {
        let len = u32::try_from(records.len()).expect("record slice length exceeds u32");
        if len == 0 {
            return 0;
        }
        if self
            .chunks
            .last()
            .is_none_or(|chunk| chunk.room.capacity() - chunk.len < len)
        {
            self.add_room(self.chunk_capacity.max(len));
        }
        let chunk = self.chunks.last_mut().unwrap();
        let position = chunk.room.start() + chunk.len;
        chunk.room.push_slice(chunk.len, records);
        chunk.len += len;
        self.len += records.len();
        position
    }

    fn add_room(&mut self, capacity: u32) {
        let room = if let Some(source) = &mut self.source {
            RecordRoom::Leased(source(capacity))
        } else {
            RecordRoom::Allocated(Room::new(capacity))
        };
        let start = room.start();
        let end = u64::from(start) + u64::from(room.capacity());
        assert!(
            self.chunks
                .iter()
                .all(|chunk| end <= u64::from(chunk.room.start())
                    || u64::from(start)
                        >= u64::from(chunk.room.start()) + u64::from(chunk.room.capacity())),
            "overlapping record rooms"
        );
        self.chunks.push(Chunk { room, len: 0 });
    }

    /// Prove initialization/ownership once, in all builds. The resulting
    /// capability keeps the owner borrowed and permits a constant-base load.
    pub fn prove(&self, position: u32) -> Option<RecordRef<'_, T>> {
        self.owns(position).then_some(RecordRef {
            position,
            marker: PhantomData,
            #[cfg(debug_assertions)]
            owner: self,
        })
    }

    /// Checked owner-scoped lookup for an untrusted/raw record ID.
    #[inline]
    pub fn get(&self, position: u32) -> Option<&T> {
        self.prove(position).map(RecordRef::get)
    }

    /// Short exclusive edit. No record view or proved capability can overlap
    /// this edit through safe code.
    pub fn get_mut(&mut self, position: u32) -> Option<&mut T> {
        if !self.owns(position) {
            return None;
        }
        // SAFETY: owns proves a live initialized slot, and &mut self excludes
        // every shared/exclusive payload borrow issued through this owner.
        Some(unsafe { &mut *(crate::address::<T>(position) as *mut T) })
    }

    /// Debug facade for the per-room owner check; absent in release builds.
    #[cfg(debug_assertions)]
    pub fn debug_owns(&self, position: u32) -> bool {
        self.owns(position)
    }

    fn owns(&self, position: u32) -> bool {
        // Reused ranges need not be in address order.
        self.chunks.iter().any(|chunk| {
            position >= chunk.room.start() && position - chunk.room.start() < chunk.len
        })
    }

    pub fn stats(&self) -> StorageStats {
        StorageStats {
            rooms: self.chunks.len(),
            live_bytes: self.len * size_of::<T>(),
            reserved_bytes: self
                .chunks
                .iter()
                .map(|c| c.room.capacity() as usize * size_of::<T>())
                .sum(),
            touched_bytes: self
                .chunks
                .iter()
                .map(|c| c.room.dirty() as usize * size_of::<T>())
                .sum(),
            metadata_bytes: self.chunks.capacity() * size_of::<Chunk<T>>()
                + self
                    .source
                    .as_ref()
                    .map_or(0, |source| std::mem::size_of_val(source.as_ref()))
                + self
                    .chunks
                    .iter()
                    .map(|chunk| match &chunk.room {
                        RecordRoom::Leased(room) => std::mem::size_of_val(room._lease.as_ref()),
                        RecordRoom::Allocated(_) => 0,
                    })
                    .sum::<usize>(),
            created: self.len as u64,
            peak_live: self.len,
            ..StorageStats::default()
        }
    }
}

impl<T: StoreRecord + Copy> Default for ClaimedRecords<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// An initialized-position proof borrowed from its actual owner. Copying the
/// capability keeps the same borrow. In release it holds just the position;
/// debug also carries the owner for a per-room assertion on each access.
#[derive(Clone, Copy)]
pub struct RecordRef<'a, T: StoreRecord + Copy> {
    position: u32,
    marker: PhantomData<&'a T>,
    #[cfg(debug_assertions)]
    owner: &'a ClaimedRecords<T>,
}

impl<'a, T: StoreRecord + Copy> RecordRef<'a, T> {
    pub fn position(self) -> u32 {
        self.position
    }

    /// One constant-base address calculation and record load after proof;
    /// the owner membership assertion is compiled only in debug builds.
    #[inline(always)]
    pub fn get(self) -> &'a T {
        #[cfg(debug_assertions)]
        debug_assert!(
            self.owner.debug_owns(self.position),
            "record proof owner mismatch"
        );
        // SAFETY: only prove constructs this capability, after checking an
        // initialized slot. Its lifetime prevents owner drop or mutation.
        unsafe { &*(crate::address::<T>(self.position) as *const T) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy)]
    #[repr(C)]
    struct Header {
        flags: u32,
        target: u32,
    }
    impl StoreRecord for Header {
        const REGION: usize = RECORD_REGION_END; // unit-test-only region
    }
    const _: () = {
        assert!(size_of::<Header>() == 8);
        assert!(std::mem::offset_of!(Header, target) == 4);
    };

    /// A slice lands on consecutive positions of one room: one that does not
    /// fit the current room starts the next, one longer than the chunk
    /// capacity gets a room of its own, and the tagged read returns the
    /// inner records.
    #[test]
    fn a_pushed_slice_is_contiguous_and_reads_back_through_its_tag() {
        type Item = Tagged<Header, DECLARATION_ITEM_REGION>;
        let item = |flags| Tagged(Header { flags, target: 0 });
        let mut owner = ClaimedRecords::<Item>::with_chunk_capacity(4);
        assert_eq!(owner.push_slice(&[]), 0);
        let first = owner.push_slice(&[item(1), item(2), item(3)]);
        let second = owner.push_slice(&[item(4), item(5)]);
        let long: Vec<Item> = (10..20).map(item).collect();
        let third = owner.push_slice(&long);
        let fourth = owner.push_slice(&[item(6)]);
        assert_eq!(owner.stats().rooms, 4);
        let flags = |start, len| -> Vec<u32> {
            slice_tagged::<Header, DECLARATION_ITEM_REGION>(start, len)
                .iter()
                .map(|header| header.flags)
                .collect()
        };
        assert_eq!(flags(first, 3), [1, 2, 3]);
        assert_eq!(flags(second, 2), [4, 5]);
        assert_eq!(flags(third, 10), (10..20).collect::<Vec<_>>());
        assert_eq!(flags(fourth, 1), [6]);
        assert_eq!(owner.get(second + 1).unwrap().0.flags, 5);
    }

    #[test]
    fn final_placement_grows_without_moving_or_publishing() {
        let mut owner = ClaimedRecords::with_chunk_capacity(2);
        let first = owner.push(Header {
            flags: 1,
            target: 0,
        });
        let address = owner.get(first).unwrap() as *const Header;
        for flags in 2..12 {
            owner.push(Header { flags, target: 0 });
        }
        assert_eq!(owner.get(first).unwrap().flags, 1);
        assert_eq!(owner.get(first).unwrap() as *const Header, address);
        assert_eq!(owner.prove(first).unwrap().get().flags, 1);
        owner.get_mut(first).unwrap().target = 7;
        assert_eq!(owner.get(first).unwrap().target, 7);
        assert_eq!(owner.stats().rooms, 6);
        assert!(owner.get(0).is_none());
        let mut other = ClaimedRecords::with_chunk_capacity(2);
        let other_id = other.push(Header {
            flags: 2,
            target: 0,
        });
        assert!(owner.get(other_id).is_none());
        assert!(other.get(first).is_none());
        assert!(owner.get(other_id + 1).is_none()); // reserved but unwritten
    }

    #[test]
    fn teardown_scrubs_and_reuses_only_after_owner_drop() {
        let mut owner = ClaimedRecords::with_chunk_capacity(3);
        let id = owner.push(Header {
            flags: 81,
            target: 42,
        });
        drop(owner);
        // No payload reference survived drop; inspect valid raw codec bytes.
        assert_eq!(
            unsafe { (*(crate::address::<Header>(id) as *const Header)).flags },
            0
        );
        let mut next = ClaimedRecords::with_chunk_capacity(3);
        let id = next.push(Header {
            flags: 19,
            target: 0,
        });
        assert_eq!(next.get(id).unwrap().flags, 19);
    }

    #[test]
    fn source_growth_retains_each_lease_until_its_records_are_scrubbed() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        #[derive(Clone, Copy)]
        struct LegacySymbol {
            flags: u32,
        }
        impl StoreRecord for LegacySymbol {
            const REGION: usize = 3;
        }
        struct Lease {
            start: u32,
            drops: Arc<AtomicUsize>,
        }
        impl Drop for Lease {
            fn drop(&mut self) {
                // This callback runs after the backing StoreVec was dropped.
                assert_eq!(
                    unsafe {
                        (*(crate::address::<LegacySymbol>(self.start) as *const LegacySymbol)).flags
                    },
                    0
                );
                self.drops.fetch_add(1, Ordering::Relaxed);
            }
        }
        let drops = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&drops);
        let mut next = 12_000_000;
        let mut owner = ClaimedRecords::with_room_source(2, move |capacity| {
            let start = next;
            next += capacity;
            let records = StoreVec::claim_room(start, capacity);
            LeasedRoom::new(
                records,
                Lease {
                    start,
                    drops: Arc::clone(&observed),
                },
            )
        });
        let first = owner.push(LegacySymbol { flags: 1 });
        let pointer = owner.get(first).unwrap() as *const LegacySymbol;
        for flags in 2..8 {
            owner.push(LegacySymbol { flags });
        }
        assert_eq!(drops.load(Ordering::Relaxed), 0);
        assert_eq!(owner.get(first).unwrap() as *const LegacySymbol, pointer);
        assert_eq!(owner.stats().rooms, 4);
        drop(owner);
        assert_eq!(drops.load(Ordering::Relaxed), 4);
    }

    #[test]
    fn room_sources_cannot_compete_with_new_region_allocators() {
        let source = std::panic::catch_unwind(|| {
            ClaimedRecords::<Header>::with_room_source(2, |_| panic!("source must not be called"))
        });
        assert!(source.is_err());
        #[derive(Clone, Copy)]
        struct ScratchWord {
            _value: u32,
        }
        impl StoreRecord for ScratchWord {
            const REGION: usize = crate::recycle::SCRATCH_WORD_REGION;
        }
        assert!(std::panic::catch_unwind(ClaimedRecords::<ScratchWord>::new).is_err());
    }

    #[test]
    fn owner_check_cost() {
        #[cfg(debug_assertions)]
        {
            use std::hint::black_box;
            use std::io::Write;
            use std::time::Instant;
            let mut owner = ClaimedRecords::with_chunk_capacity(2);
            let id = owner.push(Header {
                flags: 17,
                target: 0,
            });
            let proof = owner.prove(id).unwrap();
            let count = 200_000;
            let start = Instant::now();
            for _ in 0..count {
                black_box(black_box(proof).get().flags);
            }
            let checked = start.elapsed();
            let value = proof.get();
            let start = Instant::now();
            for _ in 0..count {
                black_box(black_box(value).flags);
            }
            let baseline = start.elapsed();
            // Direct writer bypasses libtest's capture, retaining this small
            // unit-test measurement in the authorized cargo test transcript.
            writeln!(std::io::stderr(), "owner-check sample: {count} reads, one room, checked={} ns, borrowed={} ns, delta={:.3} ns/read (debug, uncalibrated)", checked.as_nanos(), baseline.as_nanos(), (checked.as_nanos() as f64 - baseline.as_nanos() as f64) / count as f64).unwrap();
            assert!(owner.debug_owns(id));
        }
    }
}

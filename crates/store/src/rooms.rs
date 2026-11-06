//! Private range allocation for the new owners. Legacy StoreVec leases are
//! intentionally not allocated here; migrating a region requires one allocator.

use std::any::TypeId;
use std::sync::Mutex;

use crate::{address, reserve, scrub, system, StoreRecord, REGIONS};

#[derive(Default)]
struct Allocator {
    record: Option<TypeId>,
    next: u64,
    free: Vec<(u32, u32)>,
}

static ALLOCATORS: Mutex<Vec<Allocator>> = Mutex::new(Vec::new());

/// A unique range. Only its initialized prefix may have physical backing.
pub(super) struct Room<T: StoreRecord> {
    pub start: u32,
    pub capacity: u32,
    pub dirty: u32,
    marker: std::marker::PhantomData<T>,
}

impl<T: StoreRecord> Room<T> {
    pub fn new(capacity: u32) -> Self {
        assert!(capacity != 0, "an empty room");
        assert!(std::mem::size_of::<T>() != 0, "a zero-sized store record");
        // Compile-time layout checks and a region-wide runtime type check.
        let _ = address::<T>(0);
        assert!(
            T::REGION >= crate::frozen::ELEMENT_REGION,
            "legacy regions need their identity lease adapter"
        );
        reserve();
        let mut allocators = ALLOCATORS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        allocators.resize_with(REGIONS, Allocator::default);
        let allocator = &mut allocators[T::REGION];
        let record = allocator.record.get_or_insert(TypeId::of::<T>());
        assert_eq!(
            *record,
            TypeId::of::<T>(),
            "two record layouts in one region"
        );
        // Frozen positions are not reused: a bare one-word list ID must not
        // resolve through another owner after the original owner is gone.
        let reusable = T::REGION != crate::frozen::ELEMENT_REGION;
        let found = reusable
            .then(|| allocator.free.iter().position(|&(_, len)| len >= capacity))
            .flatten();
        let start = if let Some(index) = found {
            let (start, len) = allocator.free[index];
            if len == capacity {
                allocator.free.remove(index);
            } else {
                allocator.free[index] = (start + capacity, len - capacity);
            }
            start
        } else {
            let start = allocator.next.max(16);
            let end = start
                .checked_add(u64::from(capacity))
                .expect("room position overflow");
            // Keep an exclusive end representable as u32 too.
            assert!(end <= u64::from(u32::MAX), "the room region is exhausted");
            allocator.next = end;
            start as u32
        };
        crate::census::note(
            Self::census_site(),
            1,
            capacity as usize * std::mem::size_of::<T>(),
            0,
        );
        Self {
            start,
            capacity,
            dirty: 0,
            marker: std::marker::PhantomData,
        }
    }

    fn census_site() -> crate::census::Site {
        if T::REGION == crate::frozen::ELEMENT_REGION {
            crate::census::Site::FrozenRoom
        } else if T::REGION == crate::recycle::SCRATCH_WORD_REGION {
            crate::census::Site::RecycleRoom
        } else {
            crate::census::Site::ClaimedRoom
        }
    }
    pub fn pointer(&self, offset: u32) -> *mut T {
        assert!(offset <= self.capacity);
        address::<T>(self.start + offset) as *mut T
    }

    /// Commit only the prefix touched, not the room's virtual estimate.
    pub fn initialize_through(&mut self, end: u32) {
        assert!(end <= self.capacity);
        if end > self.dirty {
            system::commit(
                address::<T>(self.start + self.dirty),
                (end - self.dirty) as usize * std::mem::size_of::<T>(),
            );
            crate::census::note(
                Self::census_site(),
                0,
                0,
                (end - self.dirty) as usize * std::mem::size_of::<T>(),
            );
            self.dirty = end;
        }
    }
}

impl<T: StoreRecord> Drop for Room<T> {
    fn drop(&mut self) {
        scrub(
            address::<T>(self.start),
            self.dirty as usize * std::mem::size_of::<T>(),
        );
        let mut allocators = ALLOCATORS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let allocator = &mut allocators[T::REGION];
        allocator.free.push((self.start, self.capacity));
        allocator.free.sort_unstable();
        let mut index = 0;
        while index + 1 < allocator.free.len() {
            let (start, len) = allocator.free[index];
            let (next, count) = allocator.free[index + 1];
            if start + len == next {
                allocator.free[index].1 += count;
                allocator.free.remove(index + 1);
            } else {
                index += 1;
            }
        }
        // Adjacent small returns can together cross the return threshold.
        // Reset only whole pages strictly inside free intervals. Windows'
        // existing reset zeros committed pages; untouched slack is uncommitted.
        #[cfg(unix)]
        if let Some(&(start, len)) = allocator
            .free
            .iter()
            .find(|&&(start, len)| self.start >= start && self.start < start + len)
        {
            let first_byte = address::<T>(start);
            let bytes = len as usize * std::mem::size_of::<T>();
            if bytes >= crate::RETURN_THRESHOLD {
                let first = first_byte.next_multiple_of(crate::RETURN_GRANULE);
                let last = (first_byte + bytes) / crate::RETURN_GRANULE * crate::RETURN_GRANULE;
                if first < last {
                    system::reset(first, last - first);
                }
            }
        }
    }
}

/// Owner-local logical memory, separate from process RSS/committed-page data.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StorageStats {
    /// Number of nonmoving rooms owned.
    pub rooms: usize,
    /// Bytes in initialized live payloads (including frozen length prefixes).
    pub live_bytes: usize,
    /// Bytes reserved by this owner's rooms; not RSS.
    pub reserved_bytes: usize,
    /// Highest initialized prefix per room; a logical upper bound, not RSS.
    pub touched_bytes: usize,
    /// Owner directories, allocation entries, logs, edges and free-run capacity.
    pub metadata_bytes: usize,
    /// Total allocation events; never decreases on reset.
    pub created: u64,
    /// Currently live allocation units marked escaped.
    pub escaped: usize,
    /// Total reclaimed allocation units.
    pub recycled: u64,
    /// Highest simultaneously live unit count.
    pub peak_live: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy)]
    #[repr(transparent)]
    struct TestWord(u32);
    impl StoreRecord for TestWord {
        const REGION: usize = crate::claimed::RECORD_REGION_END + 1; // test only
    }

    #[test]
    fn adjacent_small_returns_coalesce_without_resetting_live_partial_pages() {
        let mut before = Room::<TestWord>::new(16);
        let words = (crate::RETURN_THRESHOLD / 4 / 2) as u32;
        let mut first = Room::<TestWord>::new(words);
        let mut second = Room::<TestWord>::new(words);
        let mut after = Room::<TestWord>::new(16);
        assert_eq!(first.start + words, second.start);
        for room in [&mut before, &mut first, &mut second, &mut after] {
            room.initialize_through(room.capacity);
            // SAFETY: uniquely owned committed room, no live typed views.
            unsafe {
                std::ptr::write_bytes(room.pointer(0), 0x11, room.capacity as usize);
            }
        }
        let start = first.start;
        drop(first);
        drop(second);
        // Each dropped allocation was only 512 KiB; their free union crosses
        // the threshold. Neither neighboring live partial page was reset.
        assert_eq!(unsafe { (*before.pointer(0)).0 }, 0x1111_1111);
        assert_eq!(unsafe { (*after.pointer(0)).0 }, 0x1111_1111);
        assert_eq!(
            unsafe { (*(address::<TestWord>(start) as *const TestWord)).0 },
            0
        );
        assert_eq!(
            unsafe { (*(address::<TestWord>(start + words * 2 - 1) as *const TestWord)).0 },
            0
        );
        let allocators = ALLOCATORS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(allocators[TestWord::REGION]
            .free
            .iter()
            .any(|&(position, count)| position <= start && position + count >= start + words * 2));
    }

    #[test]
    fn exhaustion_does_not_wrap_or_advance_the_cursor() {
        let result = std::panic::catch_unwind(|| Room::<TestWord>::new(u32::MAX));
        assert!(result.is_err());
        let mut room = Room::<TestWord>::new(16);
        room.initialize_through(1);
        unsafe {
            room.pointer(0).write(TestWord(3));
        }
        assert_eq!(unsafe { (*room.pointer(0)).0 }, 3);
    }
}

/// One new region's allocator state. Position counts are virtual identity
/// budgets; free positions do not imply an equal number of resident pages.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegionStats {
    pub region: usize,
    pub high_water_positions: u64,
    pub free_positions: u64,
    pub reuses_positions: bool,
}

/// Process-wide room allocator overhead, counted once across all owners.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AllocatorStats {
    pub regions: Vec<RegionStats>,
    pub metadata_bytes: usize,
}

/// Snapshot the new region allocators. Existing IdentityDomain and tails
/// allocators are separate ledgers and must be reported by their consumers.
pub fn allocator_stats() -> AllocatorStats {
    let allocators = ALLOCATORS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    AllocatorStats {
        regions: allocators
            .iter()
            .enumerate()
            .filter(|(_, allocator)| allocator.record.is_some())
            .map(|(region, allocator)| RegionStats {
                region,
                high_water_positions: allocator.next,
                free_positions: allocator.free.iter().map(|&(_, len)| u64::from(len)).sum(),
                reuses_positions: region != crate::frozen::ELEMENT_REGION,
            })
            .collect(),
        metadata_bytes: allocators.capacity() * std::mem::size_of::<Allocator>()
            + allocators
                .iter()
                .map(|allocator| allocator.free.capacity() * std::mem::size_of::<(u32, u32)>())
                .sum::<usize>(),
    }
}

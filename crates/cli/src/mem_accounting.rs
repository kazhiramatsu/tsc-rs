//! mimalloc 3, statistics ABI version 5 (libmimalloc-sys 0.1.49).
//! The C API rejects a size/version mismatch before writing the buffer.
//! This fixed stack buffer avoids the allocating JSON statistics API.
use tsc_types::accounting::Account;
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Count {
    total: i64,
    peak: i64,
    current: i64,
}
#[repr(C)]
struct Stats {
    size: usize,
    version: usize,
    pages: Count,
    reserved: Count,
    committed: Count,
    reset: i64,
    purged: i64,
    page_committed: Count,
    pages_abandoned: Count,
    threads: Count,
    malloc_normal: Count,
    malloc_huge: Count,
    malloc_requested: Count,
    mmap_calls: i64,
    commit_calls: i64,
    reset_calls: i64,
    purge_calls: i64,
    arena_count: i64,
    malloc_normal_count: i64,
    malloc_huge_count: i64,
    malloc_guarded_count: i64,
    arena_rollback_count: i64,
    arena_purges: i64,
    pages_extended: i64,
    pages_retire: i64,
    page_searches: i64,
    page_searches_count: i64,
    segments: Count,
    segments_abandoned: Count,
    segments_cache: Count,
    _segments_reserved: Count,
    heaps: Count,
    theaps: Count,
    pages_reclaim_on_alloc: i64,
    pages_reclaim_on_free: i64,
    pages_reabandon_full: i64,
    pages_unabandon_busy_wait: i64,
    heaps_delete_wait: i64,

    reserved_extension: [Count; 4],
    counter_extension: [i64; 4],
    malloc_bins: [Count; 74],
    page_bins: [Count; 74],
    chunk_bins: [Count; 6],
}
unsafe extern "C" {
    fn mi_stats_get(stats: *mut Stats) -> bool;
}

pub(super) fn sample(a: &mut Account) {
    // SAFETY: Stats contains only integer values, so all-zero is valid.
    let mut stats: Stats = unsafe { std::mem::zeroed() };
    stats.size = std::mem::size_of::<Stats>();
    stats.version = 5;
    a.metric(
        "observer.fixed_working_bytes",
        Some(stats.size + std::mem::size_of::<Account>() + 256 + 2048),
    );
    a.metric("allocator.retained_free", None);
    // SAFETY: the version-5 API checks the complete size and version before
    // writing. The linked v3 library provides this API; no Rust layouts are
    // inferred from private allocator internals.
    if unsafe { mi_stats_get(&mut stats) } {
        for (name, value) in [
            ("allocator.reserved", stats.reserved.current),
            ("allocator.committed", stats.committed.current),
            ("allocator.page_committed", stats.page_committed.current),
        ] {
            a.metric(name, usize::try_from(value).ok());
        }
        // An upper bound includes metadata, slab slack and live rounding.
        // It is not an exact count of reusable or purgeable free pages.
        let usable = if stats.malloc_normal.total != 0 {
            usize::try_from(stats.malloc_normal.current + stats.malloc_huge.current).ok()
        } else {
            None
        };
        a.metric("allocator.live_usable", usable);
        a.metric(
            "allocator.live_requested",
            if stats.malloc_requested.total != 0 {
                usize::try_from(stats.malloc_requested.current).ok()
            } else {
                None
            },
        );
        a.metric(
            "allocator.retained_free_upper_bound",
            usize::try_from(stats.committed.current)
                .ok()
                .map(|n| n.saturating_sub(usable.unwrap_or(0))),
        );
    } else {
        a.metric("allocator.reserved", None);
        a.metric("allocator.committed", None);
        a.metric("allocator.retained_free_upper_bound", None);
    }
    let sample = super::memory_sample();
    a.metric("mi_process_info.current_rss", Some(sample.resident));
    a.metric("mi_process_info.peak_rss", Some(sample.peak_resident));
    a.metric("mi_process_info.current_commit", Some(sample.committed));
    a.metric("mi_process_info.peak_commit", Some(sample.peak_committed));
}

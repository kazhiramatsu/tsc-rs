//! Diagnostic counters (never merged).
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
pub static COND_TRUE_CALLS: AtomicU64 = AtomicU64::new(0);
pub static COND_TRUE_HITS: AtomicU64 = AtomicU64::new(0);
pub static COND_FALSE_CALLS: AtomicU64 = AtomicU64::new(0);
pub static COND_FALSE_HITS: AtomicU64 = AtomicU64::new(0);
pub static NORMALIZED_CALLS: AtomicU64 = AtomicU64::new(0);
pub static SCRATCH_RETURNS: AtomicU64 = AtomicU64::new(0);
pub static SCRATCH_SET_CAPACITY_CLEARED: AtomicU64 = AtomicU64::new(0);
pub static SCRATCH_SET_MAX_CAPACITY: AtomicU64 = AtomicU64::new(0);
pub static UNION_MEMBERS_CALLS: AtomicU64 = AtomicU64::new(0);
pub fn bump(c: &AtomicU64) { c.fetch_add(1, Relaxed); }
pub fn report() -> String {
    format!("cond_true {}/{} hits, cond_false {}/{} hits, normalized {}, scratch returns {} (set capacity cleared {}, max {}), union_members {}",
        COND_TRUE_HITS.load(Relaxed), COND_TRUE_CALLS.load(Relaxed), COND_FALSE_HITS.load(Relaxed), COND_FALSE_CALLS.load(Relaxed),
        NORMALIZED_CALLS.load(Relaxed), SCRATCH_RETURNS.load(Relaxed), SCRATCH_SET_CAPACITY_CLEARED.load(Relaxed),
        SCRATCH_SET_MAX_CAPACITY.load(Relaxed), UNION_MEMBERS_CALLS.load(Relaxed))
}

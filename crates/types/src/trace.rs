//! Opt-in phase timing for the command-line compiler: with `TSRS_PHASE_TRACE`
//! set in the environment, every pipeline phase prints its wall time to
//! stderr as `[phase] <name>: <ms>`, followed by the process memory when the
//! binary registered a [`set_memory_probe`]. Off by default; the check is
//! one relaxed load after the first call.

use std::sync::OnceLock;
use std::time::Instant;

/// On-disk ids. Append new ids; never renumber an existing phase.
#[repr(u16)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    ConfigDriver = 1,
    FsLoader = 2,
    ScanParse = 3,
    ModuleRequests = 4,
    Resolution = 5,
    PublicationAdopt = 6,
    Bind = 7,
    CheckerInit = 8,
    SemanticCheck = 9,
    Diagnostics = 10,
    Probe = 11,
    OtherInParent = 12,
    Publication = 13,
    GraphFinish = 14,
    ParallelLoad = 15,
    ShardPartition = 16,
    ShardSetup = 17,
    ShardFiles = 18,
    ShardBusy = 19,
}

#[cfg(feature = "phase-ledger")]
pub mod ledger;
#[cfg(feature = "phase-ledger")]
pub use ledger::{begin, end, execution_budget, shard, warm_thread, ScopeToken, ShardToken};

#[cfg(not(feature = "phase-ledger"))]
pub struct ScopeToken;
// Keep the token's RAII interface in both feature states; callers may end
// scopes explicitly with drop even when observation is compiled out.
#[cfg(not(feature = "phase-ledger"))]
impl Drop for ScopeToken {
    fn drop(&mut self) {}
}
#[cfg(not(feature = "phase-ledger"))]
pub struct ShardToken;
#[cfg(not(feature = "phase-ledger"))]
#[inline(always)]
pub fn begin(_: Phase) -> ScopeToken {
    ScopeToken
}
#[cfg(not(feature = "phase-ledger"))]
#[inline(always)]
pub fn end(_: ScopeToken) {}
#[cfg(not(feature = "phase-ledger"))]
#[inline(always)]
pub fn execution_budget(_: usize, _: usize) {}
#[cfg(not(feature = "phase-ledger"))]
#[inline(always)]
pub fn shard(_: usize) -> ShardToken {
    ShardToken
}
#[cfg(not(feature = "phase-ledger"))]
#[inline(always)]
pub fn warm_thread() {}

/// RAII closes intervals on every return, `?`, and unwind. Explicit `end`
/// remains available for boundaries in the middle of a function.
#[inline(always)]
pub fn scope(phase: Phase) -> ScopeToken {
    begin(phase)
}

static ENABLED: OnceLock<bool> = OnceLock::new();
static EPOCH: OnceLock<Instant> = OnceLock::new();
static MEMORY_PROBE: OnceLock<fn() -> MemorySample> = OnceLock::new();

/// The process memory at a phase mark, in bytes.
#[derive(Clone, Copy, Debug, Default)]
pub struct MemorySample {
    pub resident: usize,
    pub peak_resident: usize,
    /// Memory the allocator has committed from the OS.
    pub committed: usize,
    pub peak_committed: usize,
}

/// Register how the binary measures its memory; the first registration wins.
pub fn set_memory_probe(probe: fn() -> MemorySample) {
    let _ = MEMORY_PROBE.set(probe);
}

/// Whether phase tracing was requested through `TSRS_PHASE_TRACE`.
pub fn enabled() -> bool {
    *ENABLED.get_or_init(|| {
        EPOCH.get_or_init(Instant::now);
        std::env::var_os("TSRS_PHASE_TRACE").is_some()
    })
}

/// Milliseconds since tracing was first consulted (the process start, for
/// a command-line run): the timeline position printed with every mark.
pub fn since_epoch_ms() -> f64 {
    EPOCH.get_or_init(Instant::now).elapsed().as_secs_f64() * 1e3
}

/// Print the wall time since `started` under `name` when tracing is enabled.
pub fn mark(name: &str, started: Instant) {
    if enabled() {
        let memory = MEMORY_PROBE.get().map_or_else(String::new, |probe| {
            let sample = probe();
            let mib = |bytes: usize| bytes >> 20;
            format!(
                "; rss {} MiB, peak {} MiB; committed {} MiB, peak {} MiB",
                mib(sample.resident),
                mib(sample.peak_resident),
                mib(sample.committed),
                mib(sample.peak_committed)
            )
        });
        eprintln!(
            "[phase] {name}: {:.3} ms (at {:.1} ms{memory})",
            started.elapsed().as_secs_f64() * 1e3,
            since_epoch_ms()
        );
    }
}

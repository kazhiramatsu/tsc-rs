//! Opt-in phase timing for the command-line compiler: with `TSRS_PHASE_TRACE`
//! set in the environment, every pipeline phase prints its wall time to
//! stderr as `[phase] <name>: <ms>`, followed by the process memory when the
//! binary registered a [`set_memory_probe`]. Off by default; the check is
//! one relaxed load after the first call.
//! On macOS marks also report cumulative process instructions; [`Timer`]
//! reports consecutive interval deltas. Attribute counters only at one worker
//! and one checker, since they include every thread in the process.

use std::sync::OnceLock;
use std::time::Instant;

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
        print_mark(name, started, None);
    }
}

fn print_mark(name: &str, started: Instant, start_instructions: Option<u64>) {
    // Sample before formatting or memory accounting. These are process
    // counters: only a one-worker/one-checker run attributes serial work.
    let elapsed_ms = started.elapsed().as_secs_f64() * 1e3;
    let at_ms = since_epoch_ms();
    let instructions = tsc_store::process_instructions();
    let counters = instructions.map_or_else(String::new, |end| {
        let delta = start_instructions.and_then(|start| end.checked_sub(start));
        match delta {
            Some(delta) => format!("; instructions {end}; interval instructions {delta}"),
            None => format!("; instructions {end}"),
        }
    });
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
        "[phase] {name}: {:.3} ms (at {:.1} ms{memory}{counters})",
        elapsed_ms, at_ms
    );
}

/// Consecutive serial intervals, with no clock or OS probe when tracing is
/// disabled. Each mark excludes the preceding mark's formatting and I/O.
pub struct Timer {
    started: Option<(Instant, Option<u64>)>,
}

impl Timer {
    pub fn new() -> Self {
        Self {
            started: enabled().then(|| (Instant::now(), tsc_store::process_instructions())),
        }
    }

    pub fn mark(&mut self, name: &str) {
        if let Some((started, instructions)) = self.started {
            print_mark(name, started, instructions);
            self.started = Some((Instant::now(), tsc_store::process_instructions()));
        }
    }
}

impl Default for Timer {
    fn default() -> Self {
        Self::new()
    }
}

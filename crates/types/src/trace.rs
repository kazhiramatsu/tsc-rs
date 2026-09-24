//! Opt-in phase timing for the command-line compiler: with `TSRS_PHASE_TRACE`
//! set in the environment, every pipeline phase prints its wall time to
//! stderr as `[phase] <name>: <ms>`. Off by default; the check is one
//! relaxed load after the first call.

use std::sync::OnceLock;
use std::time::Instant;

static ENABLED: OnceLock<bool> = OnceLock::new();
static EPOCH: OnceLock<Instant> = OnceLock::new();

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
        eprintln!(
            "[phase] {name}: {:.3} ms (at {:.1} ms)",
            started.elapsed().as_secs_f64() * 1e3,
            since_epoch_ms()
        );
    }
}

//! The executable owns the platform ABI; tsc-types receives only a callback.
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use tsc_types::trace::ledger::{self, CounterSample, Failure, ProbeError};

#[cfg(target_os = "macos")]
use super::phase_ledger_abi as libc;

static CALIBRATION: AtomicUsize = AtomicUsize::new(0);

#[cfg(target_os = "macos")]
fn sample() -> Result<CounterSample, ProbeError> {
    use std::sync::OnceLock;
    use std::time::Instant;
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    let epoch = EPOCH.get_or_init(Instant::now);
    // libc's SDK-derived V4 layout, checked against this host's resource.h.
    // The type annotations and const checks reject a narrower counter ABI.
    const _: () = assert!(std::mem::size_of::<libc::rusage_info_v4>() == 296);
    const _: () = assert!(std::mem::size_of::<u64>() == 8);
    let mut info = std::mem::MaybeUninit::<libc::rusage_info_v4>::zeroed();
    // SAFETY: V4 writes sizeof(rusage_info_v4) bytes to our aligned buffer.
    // The API's pointer-to-rusage_info_t is the SDK's untyped output buffer.
    let result = unsafe {
        libc::proc_pid_rusage(
            libc::getpid(),
            libc::RUSAGE_INFO_V4,
            info.as_mut_ptr().cast(),
        )
    };
    if result != 0 {
        return Err(ProbeError {
            reason: Failure::Syscall,
            errno: std::io::Error::last_os_error().raw_os_error().unwrap_or(0),
        });
    }
    // SAFETY: a successful V4 call initialized the entire zeroed structure.
    let info = unsafe { info.assume_init() };
    let instructions: u64 = info.ri_instructions;
    let cycles: u64 = info.ri_cycles;
    Ok(CounterSample {
        instructions,
        cycles,
        physical_bytes: info.ri_phys_footprint,
        peak_physical_bytes: info.ri_lifetime_max_phys_footprint,
        wall_ns: epoch.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64,
    })
}

#[cfg(not(target_os = "macos"))]
fn sample() -> Result<CounterSample, ProbeError> {
    Err(ProbeError {
        reason: Failure::Unsupported,
        errno: 0,
    })
}

/// Sample on entry to main, including when tracing is off; no buffer or
/// callback is installed without TSRS_PHASE_LEDGER. This one startup probe
/// is disclosed separately from the off-path single-branch boundary cost.
pub fn entry() -> Result<CounterSample, ProbeError> {
    sample()
}

fn setting(name: &str, default: usize) -> Result<usize, ProbeError> {
    match std::env::var(name) {
        Ok(value) => value.parse().map_err(|_| ProbeError {
            reason: Failure::Capacity,
            errno: 0,
        }),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(_) => Err(ProbeError {
            reason: Failure::Capacity,
            errno: 0,
        }),
    }
}

pub fn install(entry: Result<CounterSample, ProbeError>) -> Option<PathBuf> {
    let path = std::env::var_os("TSRS_PHASE_LEDGER")?;
    let settings = setting("TSRS_PHASE_LEDGER_CAPACITY", 1 << 16).and_then(|capacity| {
        setting("TSRS_PHASE_LEDGER_CALIBRATE", 0).map(|count| (capacity, count))
    });
    match settings {
        Ok((capacity, count)) => {
            ledger::install(sample, capacity, entry);
            ledger::warm_thread();
            ledger::set_coarse(
                std::env::var_os("TSRS_PHASE_LEDGER_COARSE").is_some_and(|value| value != "0"),
            );
            CALIBRATION.store(count, Ordering::Relaxed);
        }
        Err(error) => ledger::install(sample, 0, Err(error)),
    }
    Some(PathBuf::from(path))
}

/// Run after the CLI installs its ordinary scheduling/allocator settings,
/// before compiler work, so calibration uses the compiler's requested QoS.
pub fn calibrate() {
    let count = CALIBRATION.load(Ordering::Relaxed);
    if count != 0 {
        ledger::calibrate(count);
    }
}

/// Freeze compiler work before opening the sidecar; flush samples pre-exit
/// after bulk observer I/O and before its small final metadata write. A sidecar
/// I/O failure must never alter the compiler's streams or exit code.
pub fn flush(path: Option<&std::path::Path>) {
    if let Some(path) = path {
        let end = ledger::work_end();
        if let Ok(file) = std::fs::File::create(path) {
            let _ = ledger::flush(&mut std::io::BufWriter::new(file), end);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "macos")]
    fn sdk_probe_is_checked_without_assuming_host_counter_support() {
        match (sample(), sample()) {
            (Ok(first), Ok(second)) => {
                assert!(second.wall_ns >= first.wall_ns);
                // Host qualification is a run-time ledger decision, not a
                // unit-test prerequisite (CI may not expose PMCs).
                assert_eq!(std::mem::size_of_val(&first.instructions), 8);
            }
            (Err(error), _) | (_, Err(error)) => assert!(matches!(error.reason, Failure::Syscall)),
        }
    }
}

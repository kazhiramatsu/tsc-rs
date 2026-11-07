//! Platform probe and explicit flush before the CLI's `_exit` paths.
use std::path::PathBuf;
use tsc_types::phase_ledger::{self as ledger, Phase, ProbeError, Sample};

#[cfg(target_os = "macos")]
fn sample() -> Result<Sample, ProbeError> {
    use std::sync::OnceLock;
    use std::time::Instant;
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    let epoch = EPOCH.get_or_init(Instant::now);
    const _: () = assert!(std::mem::size_of::<libc::rusage_info_v4>() == 296);
    let mut info = std::mem::MaybeUninit::<libc::rusage_info_v4>::zeroed();
    // SAFETY: the SDK V4 call writes a full, correctly aligned V4 buffer.
    let result = unsafe {
        libc::proc_pid_rusage(
            libc::getpid(),
            libc::RUSAGE_INFO_V4,
            info.as_mut_ptr().cast(),
        )
    };
    if result != 0 {
        return Err(ProbeError {
            reason: "proc_pid_rusage failed",
            errno: std::io::Error::last_os_error().raw_os_error().unwrap_or(0),
        });
    }
    // SAFETY: the successful call initialized the zeroed V4 buffer.
    let info = unsafe { info.assume_init() };
    Ok(Sample {
        ri_instructions: info.ri_instructions,
        ri_user_time: info.ri_user_time,
        ri_system_time: info.ri_system_time,
        wall_ns: epoch.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64,
        ri_lifetime_max_phys_footprint: info.ri_lifetime_max_phys_footprint,
    })
}

#[cfg(not(target_os = "macos"))]
fn sample() -> Result<Sample, ProbeError> {
    Err(ProbeError {
        reason: "RUSAGE_INFO_V4 requires macOS",
        errno: 0,
    })
}

pub struct Session {
    path: PathBuf,
    calibration: Vec<u8>,
}

pub fn install() -> Option<Session> {
    let path = PathBuf::from(std::env::var_os("TSRS_PHASE_LEDGER")?);
    ledger::install(sample);
    ledger::switch(Phase::Config);
    Some(Session {
        path,
        calibration: Vec::new(),
    })
}

pub fn calibrate(session: Option<&mut Session>) {
    let Some(session) = session else { return };
    if let Some(count) = std::env::var("TSRS_PHASE_LEDGER_CALIBRATE")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|&count| count > 0)
    {
        ledger::stop();
        let _ = ledger::calibrate(count, &mut session.calibration);
        ledger::switch(Phase::Config);
    }
}

pub fn finish(session: Option<Session>) {
    use std::io::Write;
    if let Some(session) = session {
        ledger::switch(Phase::Finalization);
        ledger::stop();
        if let Ok(file) = std::fs::File::create(session.path) {
            let mut out = std::io::BufWriter::new(file);
            let _ = out.write_all(&session.calibration);
            let _ = ledger::finish(&mut out);
        }
    }
}

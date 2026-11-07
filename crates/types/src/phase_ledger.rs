//! Coarse process-counter ledger, compiled only with `phase-ledger`.
//!
//! The executable supplies the platform probe. Boundaries buffer flat,
//! disjoint intervals; a scoped child suspends its parent, so adding rows
//! implements inclusive-parent minus direct children without double counting.
//! No per-file or nested events are emitted. Repeated phases are summed by
//! the reducer. Counters are process-wide: attribution requires serialized
//! loading AND checking. A mutex protects the small boundary buffer across
//! the main/worker-stack handoff; it is never used in a compiler inner loop.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Config,
    Load,
    Bind,
    CheckerInit,
    Semantic,
    Finalization,
}

impl Phase {
    fn name(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Load => "load",
            Self::Bind => "bind",
            Self::CheckerInit => "checker-init",
            Self::Semantic => "semantic",
            Self::Finalization => "finalization",
        }
    }
}

/// Raw cumulative counters; CPU times and wall time are nanoseconds, memory
/// is bytes. Lifetime peak footprint is a gauge, never an additive delta.
#[derive(Clone, Copy, Debug, Default)]
pub struct Sample {
    pub ri_instructions: u64,
    pub ri_user_time: u64,
    pub ri_system_time: u64,
    pub wall_ns: u64,
    pub ri_lifetime_max_phys_footprint: u64,
}

impl Sample {
    fn follows(self, previous: Self) -> bool {
        self.ri_instructions >= previous.ri_instructions
            && self.ri_user_time >= previous.ri_user_time
            && self.ri_system_time >= previous.ri_system_time
            && self.wall_ns >= previous.wall_ns
            && self.ri_lifetime_max_phys_footprint >= previous.ri_lifetime_max_phys_footprint
    }

    fn write(self, out: &mut impl Write) -> std::io::Result<()> {
        write!(out, "\"ri_instructions\":{},\"ri_user_time\":{},\"ri_system_time\":{},\"wall_ns\":{},\"ri_lifetime_max_phys_footprint\":{}",
            self.ri_instructions, self.ri_user_time, self.ri_system_time,
            self.wall_ns, self.ri_lifetime_max_phys_footprint)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ProbeError {
    pub reason: &'static str,
    pub errno: i32,
}

pub type Probe = fn() -> Result<Sample, ProbeError>;

struct Boundary {
    phase: Phase,
    start: Sample,
    end: Sample,
}

struct Ledger {
    probe: Probe,
    previous: Sample,
    current: Option<Phase>,
    start: Sample,
    rows: Vec<Boundary>,
    error: Option<ProbeError>,
    workers: usize,
    checkers: usize,
}

impl Ledger {
    fn sample(&mut self) -> Option<Sample> {
        if self.error.is_some() {
            return None;
        }
        match (self.probe)() {
            Ok(sample) if sample.follows(self.previous) && sample.ri_instructions != 0 => {
                self.previous = sample;
                Some(sample)
            }
            Ok(_) => {
                self.error = Some(ProbeError {
                    reason: "non-monotonic or unavailable process counters",
                    errno: 0,
                });
                None
            }
            Err(error) => {
                self.error = Some(error);
                None
            }
        }
    }

    fn switch(&mut self, next: Option<Phase>) {
        if next == self.current || self.error.is_some() {
            return;
        }
        let Some(end) = self.sample() else { return };
        if let Some(phase) = self.current {
            if self.rows.len() == self.rows.capacity() {
                self.error = Some(ProbeError {
                    reason: "boundary buffer overflow",
                    errno: 0,
                });
                return;
            }
            self.rows.push(Boundary {
                phase,
                start: self.start,
                end,
            });
        }
        self.current = next;
        // Exclude the observed probe/buffering interval from phase work.
        // Its exact cost stays in total - sum(phases); no estimated correction.
        if let Some(start) = self.sample() {
            self.start = start;
        }
    }
}

static ACTIVE: AtomicBool = AtomicBool::new(false);
static LEDGER: OnceLock<Mutex<Ledger>> = OnceLock::new();

/// Install once at executable entry, only when a sidecar was requested.
/// Allocation and probe qualification precede config work.
pub fn install(probe: Probe) {
    let ledger = Ledger {
        probe,
        previous: Sample::default(),
        current: None,
        start: Sample::default(),
        rows: Vec::with_capacity(1024),
        error: None,
        workers: 1,
        checkers: 1,
    };
    if LEDGER.set(Mutex::new(ledger)).is_ok() {
        ACTIVE.store(true, Ordering::Relaxed);
    }
}

fn with_ledger<T>(work: impl FnOnce(&mut Ledger) -> T) -> Option<T> {
    if !ACTIVE.load(Ordering::Relaxed) {
        return None;
    }
    let mut ledger = LEDGER.get()?.lock().ok()?;
    Some(work(&mut ledger))
}

pub fn switch(phase: Phase) {
    let _ = with_ledger(|ledger| ledger.switch(Some(phase)));
}

/// Freeze phase work before any sidecar I/O; total is sampled after that I/O.
pub fn stop() {
    let _ = with_ledger(|ledger| ledger.switch(None));
}

/// Record actual budgets after command-line overrides. Never changes them.
pub fn budgets(workers: usize, checkers: usize) {
    let _ = with_ledger(|ledger| {
        ledger.workers = ledger.workers.max(workers);
        ledger.checkers = ledger.checkers.max(checkers);
    });
}

pub fn start_load(workers: usize, checkers: usize) {
    budgets(workers, checkers);
    switch(Phase::Load);
}

pub struct Scope(Option<Option<Phase>>);

pub fn scope(phase: Phase) -> Scope {
    Scope(with_ledger(|ledger| {
        let parent = ledger.current;
        ledger.switch(Some(phase));
        parent
    }))
}

impl Drop for Scope {
    fn drop(&mut self) {
        if let Some(parent) = self.0 {
            let _ = with_ledger(|ledger| ledger.switch(parent));
        }
    }
}

/// Empty paired probes outside compiler work. The whole envelope / (2*N)
/// is an upper per-boundary calibration, not a correction to the rows.
pub fn calibrate(count: usize, out: &mut impl Write) -> std::io::Result<()> {
    let Some((start, saved_rows)) =
        with_ledger(|ledger| ledger.sample().map(|sample| (sample, ledger.rows.len()))).flatten()
    else {
        return Ok(());
    };
    for _ in 0..count {
        // Exercise real boundaries, including both SDK calls, locking and
        // buffering. Cleanup is extra calibration work, so the envelope is
        // an upper estimate. The temporary empty phase is never emitted.
        switch(Phase::Config);
        stop();
        let valid = with_ledger(|ledger| {
            ledger.rows.truncate(saved_rows);
            ledger.error.is_none()
        })
        .unwrap_or(false);
        if !valid {
            break;
        }
    }
    if let Some(end) = with_ledger(Ledger::sample).flatten() {
        writeln!(
            out,
            "{{\"kind\":\"calibration\",\"boundaries\":{},\"instructions\":{},\"wall_ns\":{}}}",
            count.saturating_mul(2),
            end.ri_instructions - start.ri_instructions,
            end.wall_ns - start.wall_ns
        )?;
    }
    Ok(())
}

/// Close work, flush buffered rows, then sample total immediately before
/// the final small write. `total.ri_instructions` is the process cumulative
/// value (including pre-main startup), suitable for native reconciliation.
pub fn finish(out: &mut impl Write) -> std::io::Result<()> {
    with_ledger(|ledger| {
        ledger.switch(None);
        writeln!(out, "{{\"kind\":\"metadata\",\"version\":1,\"workers\":{},\"checkers\":{},\"serialized\":{}}}",
            ledger.workers, ledger.checkers, ledger.workers == 1 && ledger.checkers == 1)?;
        for row in &ledger.rows {
            write!(out, "{{\"phase\":\"{}\",\"start\":{{", row.phase.name())?;
            row.start.write(out)?;
            write!(out, "}},")?;
            row.end.write(out)?;
            writeln!(out, "}}")?;
        }
        out.flush()?;
        if let Some(total) = ledger.sample() {
            write!(out, "{{\"phase\":\"total\",")?;
            total.write(out)?;
            writeln!(out, "}}")?;
        }
        if let Some(error) = ledger.error {
            writeln!(out, "{{\"error\":\"{}\",\"errno\":{}}}", error.reason, error.errno)?;
        }
        out.flush()
    }).unwrap_or(Ok(()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    static TICK: AtomicU64 = AtomicU64::new(100);
    fn probe() -> Result<Sample, ProbeError> {
        let tick = TICK.fetch_add(10, Ordering::Relaxed);
        Ok(Sample {
            ri_instructions: tick,
            wall_ns: tick,
            ..Sample::default()
        })
    }

    fn ledger() -> Ledger {
        Ledger {
            probe,
            previous: Sample::default(),
            current: None,
            start: Sample::default(),
            rows: Vec::with_capacity(8),
            error: None,
            workers: 1,
            checkers: 1,
        }
    }

    #[test]
    fn child_intervals_are_excluded_from_parent() {
        let mut ledger = ledger();
        ledger.switch(Some(Phase::Load));
        ledger.switch(Some(Phase::Bind));
        ledger.switch(Some(Phase::Bind)); // same-phase nesting emits nothing
        ledger.switch(Some(Phase::Load));
        ledger.switch(None);
        assert_eq!(ledger.rows.len(), 3);
        assert_eq!(ledger.rows[1].phase, Phase::Bind);
        let work: u64 = ledger
            .rows
            .iter()
            .map(|row| row.end.ri_instructions - row.start.ri_instructions)
            .sum();
        let envelope = ledger.rows[2].end.ri_instructions - ledger.rows[0].start.ri_instructions;
        assert_eq!(envelope - work, 20); // two measured observer gaps
    }

    #[test]
    fn decreasing_counters_and_overflow_disable_the_run() {
        let mut ledger = ledger();
        ledger.previous.ri_instructions = u64::MAX;
        assert!(ledger.sample().is_none());
        assert!(ledger.error.is_some());
        let mut ledger = self::ledger();
        ledger.rows = Vec::new();
        ledger.switch(Some(Phase::Load));
        ledger.switch(None);
        assert_eq!(ledger.error.unwrap().reason, "boundary buffer overflow");
    }

    #[test]
    fn probe_failure_preserves_errno() {
        let mut ledger = ledger();
        ledger.probe = || {
            Err(ProbeError {
                reason: "proc_pid_rusage failed",
                errno: 5,
            })
        };
        assert!(ledger.sample().is_none());
        assert_eq!(ledger.error.unwrap().errno, 5);
    }
}

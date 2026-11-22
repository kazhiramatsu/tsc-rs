//! Opt-in phase timing for the command-line compiler: with `TSRS_PHASE_TRACE`
//! set in the environment, every pipeline phase prints its wall time to
//! stderr as `[phase] <name>: <ms>`, followed by the process memory when the
//! binary registered a [`set_memory_probe`]. Off by default; the check is
//! one relaxed load after the first call.
//! On macOS marks also report cumulative process instructions; [`Timer`]
//! reports consecutive interval deltas. Attribute counters only at one worker
//! and one checker, since they include every thread in the process.
//!
//! `TSRS_PHASE_TRACE=light` takes the same marks and nothing else: no memory
//! probe, no instruction count, no detail lines ([`detail`] is false), and
//! nothing is written until [`flush`] prints the marks at the run's end. A
//! mark of the full trace costs its phase a probe, a counter read and an
//! unbuffered write (ten to twenty microseconds), and the detail lines sit
//! inside the phases they describe (a line a stripe, a census a step): a
//! program of a few thousand files prints hundreds of them, and a step of
//! the load was once read as three milliseconds that were its own lines. The
//! light form is for reading how long the phases take; the full form for
//! reading what they did.

use std::sync::OnceLock;
use std::time::Instant;

/// How `TSRS_PHASE_TRACE` asked for the trace.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Off,
    /// Marks only, kept until [`flush`].
    Light,
    /// Marks with the process's memory and instructions, and detail lines,
    /// printed as they are taken.
    Full,
}

static MODE: OnceLock<Mode> = OnceLock::new();
static EPOCH: OnceLock<Instant> = OnceLock::new();
/// The light trace's marks in the order they were taken: name, the phase's
/// milliseconds, and the milliseconds since the epoch at the mark.
static KEPT: std::sync::Mutex<Vec<(String, f64, f64)>> = std::sync::Mutex::new(Vec::new());
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

/// What a value of `TSRS_PHASE_TRACE` asks for: unset, no trace; `light`,
/// the light form; any other value, the full form (as every value did
/// before there were two).
fn requested_mode(value: Option<&std::ffi::OsStr>) -> Mode {
    match value {
        None => Mode::Off,
        Some(value) if value == "light" => Mode::Light,
        Some(_) => Mode::Full,
    }
}

fn mode() -> Mode {
    *MODE.get_or_init(|| {
        EPOCH.get_or_init(Instant::now);
        requested_mode(std::env::var_os("TSRS_PHASE_TRACE").as_deref())
    })
}

/// Whether phase tracing was requested through `TSRS_PHASE_TRACE`, in either
/// form: marks are taken.
pub fn enabled() -> bool {
    mode() != Mode::Off
}

/// Whether the trace also prints what the phases did (the lines beside the
/// marks: a stripe's statistics, a census of the load): the full form only.
/// A detail line is written inside the phase it describes, so the light
/// form, which is read for the phases' times, leaves them out.
pub fn detail() -> bool {
    mode() == Mode::Full
}

/// Print the marks the light trace kept, in the order they were taken, and
/// forget them. The command calls it once, after its last mark; the full
/// trace has printed its marks already and this does nothing.
pub fn flush() {
    if mode() != Mode::Light {
        return;
    }
    let kept = std::mem::take(
        &mut *KEPT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    eprint!("{}", kept_lines(&kept));
}

/// The kept marks as lines, one a mark, in the form the full trace begins
/// its lines with (the name, the phase's milliseconds, the position on the
/// timeline): a reader of one form's marks reads the other's.
fn kept_lines(kept: &[(String, f64, f64)]) -> String {
    use std::fmt::Write as _;
    let mut text = String::with_capacity(kept.len() * 64);
    for (name, elapsed_ms, at_ms) in kept {
        let _ = writeln!(
            text,
            "[phase] {name}: {elapsed_ms:.3} ms (at {at_ms:.1} ms)"
        );
    }
    text
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
    if mode() == Mode::Light {
        KEPT.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((name.to_owned(), elapsed_ms, at_ms));
        return;
    }
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
            started: enabled().then(Self::start),
        }
    }

    /// The start of an interval: the clock, and under the full trace the
    /// process's instruction count.
    fn start() -> (Instant, Option<u64>) {
        let instructions = if detail() {
            tsc_store::process_instructions()
        } else {
            None
        };
        (Instant::now(), instructions)
    }

    pub fn mark(&mut self, name: &str) {
        if let Some((started, instructions)) = self.started {
            print_mark(name, started, instructions);
            self.started = Some(Self::start());
        }
    }
}

impl Default for Timer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{kept_lines, requested_mode, Mode};
    use std::ffi::OsStr;

    /// The variable's value selects the form: `light` alone asks for the
    /// light one, every other value for the full trace, none for no trace.
    #[test]
    fn the_variables_value_selects_the_trace_form() {
        assert_eq!(requested_mode(None), Mode::Off);
        assert_eq!(requested_mode(Some(OsStr::new("light"))), Mode::Light);
        for full in ["1", "", "0", "full", "LIGHT", "light "] {
            assert_eq!(
                requested_mode(Some(OsStr::new(full))),
                Mode::Full,
                "{full:?}"
            );
        }
    }

    /// The light form's lines begin as the full trace's do, so one reader
    /// reads both: `[phase] <name>: <ms> ms (at <ms> ms)`, a mark a line in
    /// the order the marks were taken.
    #[test]
    fn kept_marks_print_as_the_full_traces_marks_begin() {
        let kept = [
            ("load: read-ahead".to_owned(), 15.1294, 15.94),
            ("shard 1: check (5 types, 7 files)".to_owned(), 0.0, 148.0),
        ];
        assert_eq!(
            kept_lines(&kept),
            "[phase] load: read-ahead: 15.129 ms (at 15.9 ms)\n\
             [phase] shard 1: check (5 types, 7 files): 0.000 ms (at 148.0 ms)\n"
        );
        assert_eq!(kept_lines(&[]), "");
    }
}

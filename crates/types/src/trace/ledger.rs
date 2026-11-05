//! Bounded observation storage. Platform sampling belongs to the executable.
//!
//! Serialized work uses one process stack (a token may cross threads). Once
//! either execution budget exceeds one, new scopes use thread-local stacks;
//! their process counters are observations, never concurrent attribution.
//! Slots are reserved atomically and published once; flush reads only published
//! slots. No allocation, formatting, registry lock or I/O occurs at a boundary.

use super::Phase;
use std::cell::Cell;
use std::io::{self, Seek, SeekFrom, Write};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicUsize, Ordering};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, Default)]
pub struct CounterSample {
    pub instructions: u64,
    pub cycles: u64,
    pub physical_bytes: u64,
    pub peak_physical_bytes: u64,
    pub wall_ns: u64,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug)]
pub enum Failure {
    Syscall = 1,
    Unsupported = 2,
    CounterUnavailable = 3,
    NonMonotonic = 4,
    Stack = 5,
    Capacity = 6,
}

#[derive(Clone, Copy, Debug)]
pub struct ProbeError {
    pub reason: Failure,
    pub errno: i32,
}

pub type Probe = fn() -> Result<CounterSample, ProbeError>;

#[derive(Clone, Copy, Debug)]
struct Event {
    sequence: u64,
    span: u64,
    parent: u64,
    phase: u16,
    depth: u16,
    begin: bool,
    thread: u64,
    shard: u32,
    sample: CounterSample,
    observer_end: CounterSample,
}

struct Slot {
    ready: AtomicBool,
    words: [AtomicU64; 18],
}

impl Slot {
    fn new() -> Self {
        Self {
            ready: AtomicBool::new(false),
            words: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    fn write(&self, event: Event) {
        let words = [
            event.sequence,
            event.span,
            event.parent,
            u64::from(event.phase),
            u64::from(event.depth),
            u64::from(event.begin),
            event.thread,
            u64::from(event.shard),
            event.sample.instructions,
            event.sample.cycles,
            event.sample.physical_bytes,
            event.sample.peak_physical_bytes,
            event.sample.wall_ns,
        ];
        for (target, value) in self.words.iter().zip(words) {
            target.store(value, Ordering::Relaxed);
        }
    }

    fn publish(&self, sample: CounterSample) {
        let words = [
            sample.instructions,
            sample.cycles,
            sample.physical_bytes,
            sample.peak_physical_bytes,
            sample.wall_ns,
        ];
        for (target, value) in self.words[13..].iter().zip(words) {
            target.store(value, Ordering::Relaxed);
        }
        self.ready.store(true, Ordering::Release);
    }

    fn read(&self) -> Option<Event> {
        if !self.ready.load(Ordering::Acquire) {
            return None;
        }
        let w = self
            .words
            .each_ref()
            .map(|word| word.load(Ordering::Relaxed));
        Some(Event {
            sequence: w[0],
            span: w[1],
            parent: w[2],
            phase: w[3] as u16,
            depth: w[4] as u16,
            begin: w[5] != 0,
            thread: w[6],
            shard: w[7] as u32,
            sample: CounterSample {
                instructions: w[8],
                cycles: w[9],
                physical_bytes: w[10],
                peak_physical_bytes: w[11],
                wall_ns: w[12],
            },
            observer_end: CounterSample {
                instructions: w[13],
                cycles: w[14],
                physical_bytes: w[15],
                peak_physical_bytes: w[16],
                wall_ns: w[17],
            },
        })
    }
}

struct Ledger {
    probe: Probe,
    entry: CounterSample,
    slots: Box<[Slot]>,
    next: AtomicUsize,
    dropped: AtomicUsize,
    failure: AtomicUsize,
    errno: AtomicI32,
    concurrent: AtomicBool,
    workers: AtomicUsize,
    checkers: AtomicUsize,
    head: AtomicU64,
    last_instructions: AtomicU64,
    last_cycles: AtomicU64,
    last_wall: AtomicU64,
    last_peak: AtomicU64,
    wall_valid: AtomicBool,
    invalid_fields: AtomicUsize,
    invalid_before: [AtomicU64; 5],
    invalid_after: [AtomicU64; 5],
}

static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Coarse mode (`TSRS_PHASE_LEDGER_COARSE=1`): the per-call phases (scan/parse, module requests,
/// resolution, per-file bind) are not recorded, so their probe cost does not land in the work rows;
/// the loader envelope then carries them as one integrated row and the first open Bind scope is the
/// bind envelope. Decided with the comparator side on 2026-11-05 after calibrating the probe.
static COARSE: AtomicBool = AtomicBool::new(false);
static COARSE_BIND_OPEN: AtomicBool = AtomicBool::new(false);

pub fn set_coarse(on: bool) {
    COARSE.store(on, Ordering::Release);
}
static INSTALLED: OnceLock<&'static Ledger> = OnceLock::new();
static NEXT_THREAD: AtomicU64 = AtomicU64::new(1);
const ID_MASK: u64 = (1 << 48) - 1;
const NO_SHARD: u32 = u32::MAX;

thread_local! {
    static HEAD: Cell<u64> = const { Cell::new(0) };
    static SHARD: Cell<u32> = const { Cell::new(NO_SHARD) };
    static THREAD: Cell<u64> = const { Cell::new(0) };
    static LAST: Cell<CounterSample> = const { Cell::new(CounterSample {
        instructions: 0, cycles: 0, physical_bytes: 0, peak_physical_bytes: 0, wall_ns: 0,
    }) };
}

fn thread_id() -> u64 {
    THREAD.with(|id| {
        if id.get() == 0 {
            id.set(NEXT_THREAD.fetch_add(1, Ordering::Relaxed));
        }
        id.get()
    })
}

/// Touch TLS before compiler work, from the executable/worker start hook.
/// Const, destructor-free TLS has no Rust heap-backed registry.
pub fn warm_thread() {
    if !ACTIVE.load(Ordering::Acquire) {
        return;
    }
    thread_id();
    HEAD.with(|_| {});
    SHARD.with(|_| {});
    LAST.with(|_| {});
}

fn monotonic(before: CounterSample, after: CounterSample) -> bool {
    invalid_fields(before, after) == 0
}

/// Samples whose current footprint exceeded the lifetime peak: the peak is updated lazily by
/// the kernel, so the gauge can transiently exceed it. The gauges are informational (never
/// summed), so this is counted and reported in the header, not treated as a failure.
static FOOTPRINT_ABOVE_PEAK: AtomicU64 = AtomicU64::new(0);

// Bits: instructions, cycles, wall, lifetime peak.
fn invalid_fields(before: CounterSample, after: CounterSample) -> usize {
    if after.physical_bytes > after.peak_physical_bytes {
        FOOTPRINT_ABOVE_PEAK.fetch_add(1, Ordering::Relaxed);
    }
    usize::from(after.instructions < before.instructions)
        | (usize::from(after.cycles < before.cycles) << 1)
        | (usize::from(after.wall_ns < before.wall_ns) << 2)
        | (usize::from(after.peak_physical_bytes < before.peak_physical_bytes) << 3)
}

fn sample_words(sample: CounterSample) -> [u64; 5] {
    [
        sample.instructions,
        sample.cycles,
        sample.physical_bytes,
        sample.peak_physical_bytes,
        sample.wall_ns,
    ]
}

fn read_sample(words: &[AtomicU64; 5]) -> CounterSample {
    let words = words.each_ref().map(|word| word.load(Ordering::Relaxed));
    CounterSample {
        instructions: words[0],
        cycles: words[1],
        physical_bytes: words[2],
        peak_physical_bytes: words[3],
        wall_ns: words[4],
    }
}

impl Ledger {
    fn new(probe: Probe, capacity: usize, entry: CounterSample) -> Self {
        let mut slots = Vec::new();
        let reserved = slots.try_reserve_exact(capacity).is_ok();
        if reserved {
            slots.extend((0..capacity).map(|_| Slot::new()));
        }
        Self {
            probe,
            entry,
            slots: slots.into_boxed_slice(),
            next: AtomicUsize::new(0),
            dropped: AtomicUsize::new(0),
            failure: AtomicUsize::new(if reserved {
                0
            } else {
                Failure::Capacity as usize
            }),
            errno: AtomicI32::new(0),
            concurrent: AtomicBool::new(false),
            workers: AtomicUsize::new(0),
            checkers: AtomicUsize::new(0),
            head: AtomicU64::new(0),
            last_instructions: AtomicU64::new(entry.instructions),
            last_cycles: AtomicU64::new(entry.cycles),
            last_wall: AtomicU64::new(entry.wall_ns),
            last_peak: AtomicU64::new(entry.peak_physical_bytes),
            wall_valid: AtomicBool::new(true),
            invalid_fields: AtomicUsize::new(0),
            invalid_before: std::array::from_fn(|_| AtomicU64::new(0)),
            invalid_after: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    fn fail(&self, error: ProbeError) {
        self.wall_valid.store(false, Ordering::Relaxed);
        if self
            .failure
            .compare_exchange(
                0,
                error.reason as usize,
                Ordering::Relaxed,
                Ordering::Relaxed,
            )
            .is_ok()
        {
            self.errno.store(error.errno, Ordering::Relaxed);
        }
        ACTIVE.store(false, Ordering::Release);
    }

    fn sample(&self) -> Option<CounterSample> {
        let failure = self.failure.load(Ordering::Relaxed);
        if failure != 0
            && !(failure == Failure::NonMonotonic as usize
                && self.concurrent.load(Ordering::Relaxed)
                && self.wall_valid.load(Ordering::Relaxed))
        {
            return None;
        }
        match (self.probe)() {
            Ok(sample) => {
                // In serialized mode also compare against entry. Per-thread
                // LAST avoids falsely rejecting probes that finish out of
                // order on concurrent threads. Flush checks the global serial
                // sequence, including a handoff between threads.
                let (previous, mut invalid) = LAST.with(|last| {
                    let previous = last.get();
                    let invalid =
                        invalid_fields(previous, sample) | invalid_fields(self.entry, sample);
                    last.set(sample);
                    (previous, invalid)
                });
                if !self.concurrent.load(Ordering::Relaxed) {
                    invalid |= usize::from(
                        sample.instructions
                            < self
                                .last_instructions
                                .fetch_max(sample.instructions, Ordering::Relaxed),
                    );
                    invalid |= usize::from(
                        sample.cycles
                            < self.last_cycles.fetch_max(sample.cycles, Ordering::Relaxed),
                    ) << 1;
                    invalid |= usize::from(
                        sample.wall_ns
                            < self.last_wall.fetch_max(sample.wall_ns, Ordering::Relaxed),
                    ) << 2;
                    invalid |= usize::from(
                        sample.peak_physical_bytes
                            < self
                                .last_peak
                                .fetch_max(sample.peak_physical_bytes, Ordering::Relaxed),
                    ) << 3;
                }
                if invalid == 0 {
                    Some(sample)
                } else {
                    if self
                        .invalid_fields
                        .compare_exchange(0, invalid, Ordering::Relaxed, Ordering::Relaxed)
                        .is_ok()
                    {
                        for (target, value) in
                            self.invalid_before.iter().zip(sample_words(previous))
                        {
                            target.store(value, Ordering::Relaxed);
                        }
                        for (target, value) in self.invalid_after.iter().zip(sample_words(sample)) {
                            target.store(value, Ordering::Relaxed);
                        }
                    }
                    // M6/addendum: process counters have no concurrent
                    // attribution. Disable the instruction ledger, but retain
                    // paired wall observations if the wall clock is valid.
                    if self.concurrent.load(Ordering::Relaxed) && invalid & 4 == 0 {
                        let _ = self.failure.compare_exchange(
                            0,
                            Failure::NonMonotonic as usize,
                            Ordering::Relaxed,
                            Ordering::Relaxed,
                        );
                        return Some(sample);
                    }
                    self.fail(ProbeError {
                        reason: Failure::NonMonotonic,
                        errno: 0,
                    });
                    None
                }
            }
            Err(error) => {
                self.fail(error);
                None
            }
        }
    }

    fn push(&self, mut event: Event) -> u64 {
        let index = self.next.fetch_add(1, Ordering::Relaxed);
        let sequence = index as u64 + 1;
        event.sequence = sequence;
        if event.begin {
            event.span = sequence;
        }
        if let Some(slot) = self.slots.get(index) {
            slot.write(event);
        } else {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
        sequence
    }

    fn events(&self) -> impl Iterator<Item = Event> + '_ {
        self.slots.iter().filter_map(Slot::read)
    }

    fn publish(&self, sequence: u64) {
        if let Some(sample) = self.sample() {
            if let Some(slot) = self.slots.get(sequence as usize - 1) {
                slot.publish(sample);
            }
        }
    }
}

/// Install once, before compiler work. `entry` is sampled by the host at the
/// first line of main, before environment parsing or buffer allocation.
pub fn install(probe: Probe, capacity: usize, entry: Result<CounterSample, ProbeError>) {
    if INSTALLED.get().is_some() {
        return;
    }
    let ledger: &'static Ledger = Box::leak(Box::new(Ledger::new(
        probe,
        capacity,
        entry.unwrap_or_default(),
    )));
    if INSTALLED.set(ledger).is_err() {
        return;
    }
    if ledger.failure.load(Ordering::Relaxed) != 0 {
        return;
    }
    match entry {
        Err(error) => ledger.fail(error),
        Ok(first) => match probe() {
            Err(error) => ledger.fail(error),
            Ok(next)
                if first.instructions == 0
                    || next.instructions <= first.instructions
                    || next.cycles <= first.cycles =>
            {
                ledger.fail(ProbeError {
                    reason: Failure::CounterUnavailable,
                    errno: 0,
                })
            }
            Ok(next) if !monotonic(first, next) => ledger.fail(ProbeError {
                reason: Failure::NonMonotonic,
                errno: 0,
            }),
            Ok(_) => ACTIVE.store(true, Ordering::Release),
        },
    }
}

/// A movable single-use token. Its previous head is the nesting stack link;
/// no heap stack is needed. Drop closes a live token, including on early exit.
pub struct ScopeToken {
    ledger: Option<&'static Ledger>,
    frame: u64,
    previous: u64,
    phase: Phase,
    shard: u32,
    serial: bool,
}

impl ScopeToken {
    const fn off() -> Self {
        Self {
            ledger: None,
            frame: 0,
            previous: 0,
            phase: Phase::OtherInParent,
            shard: NO_SHARD,
            serial: true,
        }
    }
}

#[inline(always)]
pub fn begin(phase: Phase) -> ScopeToken {
    if !ACTIVE.load(Ordering::Acquire) {
        return ScopeToken::off();
    }
    if COARSE.load(Ordering::Relaxed) {
        match phase {
            Phase::ScanParse | Phase::ModuleRequests | Phase::Resolution => {
                return ScopeToken::off()
            }
            Phase::Bind => {
                if COARSE_BIND_OPEN.swap(true, Ordering::Relaxed) {
                    return ScopeToken::off();
                }
            }
            _ => {}
        }
    }
    begin_active(INSTALLED.get().expect("active ledger is installed"), phase)
}

#[inline(never)]
fn begin_active(ledger: &'static Ledger, phase: Phase) -> ScopeToken {
    // First action inside the active begin boundary is the counter snapshot.
    let Some(sample) = ledger.sample() else {
        return ScopeToken::off();
    };
    let serial = !ledger.concurrent.load(Ordering::Relaxed);
    let previous = if serial {
        ledger.head.load(Ordering::Relaxed)
    } else {
        HEAD.with(Cell::get)
    };
    let depth = (previous >> 48) as u16;
    if depth == u16::MAX || ledger.next.load(Ordering::Relaxed) as u64 >= ID_MASK {
        ledger.fail(ProbeError {
            reason: Failure::Stack,
            errno: 0,
        });
        return ScopeToken::off();
    }
    let shard = SHARD.with(Cell::get);
    let span = ledger.push(Event {
        sequence: 0,
        span: 0,
        parent: previous & ID_MASK,
        phase: phase as u16,
        depth,
        begin: true,
        thread: thread_id(),
        shard,
        sample,
        observer_end: CounterSample::default(),
    });
    let frame = span | (u64::from(depth + 1) << 48);
    if serial {
        ledger.head.store(frame, Ordering::Relaxed);
    } else {
        HEAD.with(|head| head.set(frame));
    }
    // A second snapshot brackets the observer's own logging. The reducer
    // assigns these measured intervals to Probe, not to compiler work.
    // Publishing these final five words remains a calibrated residual.
    ledger.publish(span);
    ScopeToken {
        ledger: Some(ledger),
        frame,
        previous,
        phase,
        shard,
        serial,
    }
}

#[inline(always)]
pub fn end(token: ScopeToken) {
    drop(token);
}

impl Drop for ScopeToken {
    #[inline(always)]
    fn drop(&mut self) {
        if self.ledger.is_some()
            && matches!(self.phase, Phase::Bind)
            && COARSE.load(Ordering::Relaxed)
        {
            COARSE_BIND_OPEN.store(false, Ordering::Relaxed);
        }
        if let Some(ledger) = self.ledger {
            end_active(ledger, self);
        }
    }
}

#[inline(never)]
fn end_active(ledger: &Ledger, token: &ScopeToken) {
    // Sample before touching the end log or popping the nesting link.
    let Some(sample) = ledger.sample() else {
        return;
    };
    let valid = if token.serial {
        ledger
            .head
            .compare_exchange(
                token.frame,
                token.previous,
                Ordering::Relaxed,
                Ordering::Relaxed,
            )
            .is_ok()
    } else {
        HEAD.with(|head| {
            let valid = head.get() == token.frame;
            head.set(token.previous);
            valid
        })
    };
    if !valid {
        ledger.fail(ProbeError {
            reason: Failure::Stack,
            errno: 0,
        });
        return;
    }
    let sequence = ledger.push(Event {
        sequence: 0,
        span: token.frame & ID_MASK,
        parent: token.previous & ID_MASK,
        phase: token.phase as u16,
        depth: ((token.frame >> 48) - 1) as u16,
        begin: false,
        thread: thread_id(),
        shard: token.shard,
        sample,
        observer_end: CounterSample::default(),
    });
    ledger.publish(sequence);
}

#[inline]
pub fn execution_budget(workers: usize, checkers: usize) {
    if !ACTIVE.load(Ordering::Acquire) {
        return;
    }
    let ledger = INSTALLED.get().expect("active ledger is installed");
    ledger.workers.fetch_max(workers, Ordering::Relaxed);
    ledger.checkers.fetch_max(checkers, Ordering::Relaxed);
    if workers > 1 || checkers > 1 {
        ledger.concurrent.store(true, Ordering::Relaxed);
    }
}

pub struct ShardToken(Option<u32>);
impl Drop for ShardToken {
    fn drop(&mut self) {
        if let Some(previous) = self.0 {
            SHARD.with(|shard| shard.set(previous));
        }
    }
}
#[inline]
pub fn shard(index: usize) -> ShardToken {
    if !ACTIVE.load(Ordering::Acquire) {
        return ShardToken(None);
    }
    ShardToken(Some(SHARD.with(|shard| shard.replace(index as u32))))
}

/// The calibration envelope is entry-to-exit, so the tool can compare its
/// complete cost with the sum of N empty paired intervals.
pub fn calibrate(count: usize) {
    if !ACTIVE.load(Ordering::Acquire) {
        return;
    }
    calibrate_with(count, begin);
}

fn calibrate_with(count: usize, begin: impl Fn(Phase) -> ScopeToken) {
    let envelope = begin(Phase::Probe);
    for _ in 0..count {
        end(begin(Phase::Probe));
    }
    end(envelope);
}

fn write_sample(writer: &mut impl Write, sample: CounterSample) -> io::Result<()> {
    write!(writer, "\"instructions\":{},\"cycles\":{},\"physical_bytes\":{},\"peak_physical_bytes\":{},\"wall_ns\":{}",
        sample.instructions, sample.cycles, sample.physical_bytes, sample.peak_physical_bytes, sample.wall_ns)
}

/// Freeze compiler recording after its threads join and output streams flush.
/// This work-end sample precedes sidecar I/O; flush takes the final pre-exit
/// sample after bulk serialization, so observer I/O has its own measured row.
pub fn work_end() -> Option<CounterSample> {
    let ledger = INSTALLED.get()?;
    let exit = ledger.sample();
    ACTIVE.store(false, Ordering::Release);
    exit
}

pub fn flush(writer: &mut (impl Write + Seek), work_end: Option<CounterSample>) -> io::Result<()> {
    let Some(ledger) = INSTALLED.get() else {
        return Ok(());
    };
    flush_active(ledger, writer, work_end)
}

fn flush_active(
    ledger: &Ledger,
    writer: &mut (impl Write + Seek),
    work_end: Option<CounterSample>,
) -> io::Result<()> {
    if !ledger.concurrent.load(Ordering::Relaxed) {
        let mut previous = ledger.entry;
        for event in ledger.events() {
            if !monotonic(previous, event.sample) {
                ledger.fail(ProbeError {
                    reason: Failure::NonMonotonic,
                    errno: 0,
                });
                break;
            }
            if !monotonic(event.sample, event.observer_end) {
                ledger.fail(ProbeError {
                    reason: Failure::NonMonotonic,
                    errno: 0,
                });
                break;
            }
            previous = event.observer_end;
        }
        if work_end.is_some_and(|end| !monotonic(previous, end)) {
            ledger.fail(ProbeError {
                reason: Failure::NonMonotonic,
                errno: 0,
            });
        }
    }
    // Reserve a padded first line. Rewriting its bounded metadata after the
    // final probe lets a post-I/O probe failure still appear in the header.
    // Only this fixed header and the small final marker remain unsampled.
    const HEADER_BYTES: usize = 4096;
    let mut header = vec![b' '; HEADER_BYTES];
    header[HEADER_BYTES - 1] = b'\n';
    writer.write_all(&header)?;
    write!(writer, "{{\"type\":\"entry\",")?;
    write_sample(writer, ledger.entry)?;
    writeln!(writer, "}}")?;
    for event in ledger.events() {
        write!(writer, "{{\"type\":\"event\",\"sequence\":{},\"span\":{},\"parent\":{},\"phase\":{},\"depth\":{},\"kind\":\"{}\",\"thread\":{},\"shard\":{},",
            event.sequence, event.span, event.parent, event.phase, event.depth,
            if event.begin { "begin" } else { "end" }, event.thread,
            if event.shard == NO_SHARD { -1i64 } else { i64::from(event.shard) })?;
        write_sample(writer, event.sample)?;
        write!(writer, ",\"observer_end\":{{")?;
        write_sample(writer, event.observer_end)?;
        write!(writer, "}}")?;
        writeln!(writer, "}}")?;
    }
    if let Some(end) = work_end {
        write!(writer, "{{\"type\":\"work_end\",")?;
        write_sample(writer, end)?;
        writeln!(writer, "}}")?;
    }
    writer.flush()?;
    let exit = ledger.sample();
    // Capacity and pages were warmed before the final sample; formatting the
    // bounded header never grows it with the current fixed schema.
    header.clear();
    write_header(ledger, &mut header)?;
    let _ = header.pop(); // newline: pad the single JSON line before restoring it
    if header.len() >= HEADER_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ledger header exceeds reservation",
        ));
    }
    header.resize(HEADER_BYTES - 1, b' ');
    header.push(b'\n');
    writer.seek(SeekFrom::Start(0))?;
    writer.write_all(&header)?;
    writer.seek(SeekFrom::End(0))?;
    if let Some(exit) = exit {
        write!(writer, "{{\"type\":\"pre_exit\",")?;
        write_sample(writer, exit)?;
        writeln!(writer, "}}")?;
    }
    writer.flush()
}

fn write_header(ledger: &Ledger, writer: &mut impl Write) -> io::Result<()> {
    let failure = ledger.failure.load(Ordering::Relaxed);
    let reason = match failure {
        0 => "ok",
        1 => "proc_pid_rusage failed",
        2 => "RUSAGE_INFO_V4 unsupported on this platform",
        3 => "64-bit instructions/cycles unavailable or not advancing",
        4 => "non-monotonic counter sample",
        5 => "scope stack violation",
        6 => "invalid capacity or calibration setting",
        _ => "unknown failure",
    };
    write!(writer, "{{\"type\":\"header\",\"version\":1,\"counter_bits\":64,\"coarse\":{},\"footprint_above_peak\":{},\"enabled\":{},\"reason\":\"{}\",\"errno\":{},\"capacity\":{},\"dropped\":{},\"attempted_events\":{},\"concurrent\":{},\"workers\":{},\"checkers\":{},\"wall_valid\":{},\"invalid_fields\":{},\"invalid_before\":{{",
        COARSE.load(Ordering::Relaxed), FOOTPRINT_ABOVE_PEAK.load(Ordering::Relaxed), failure == 0, reason, ledger.errno.load(Ordering::Relaxed), ledger.slots.len(),
        ledger.dropped.load(Ordering::Relaxed), ledger.next.load(Ordering::Relaxed), ledger.concurrent.load(Ordering::Relaxed),
        ledger.workers.load(Ordering::Relaxed), ledger.checkers.load(Ordering::Relaxed),
        ledger.wall_valid.load(Ordering::Relaxed), ledger.invalid_fields.load(Ordering::Relaxed))?;
    write_sample(writer, read_sample(&ledger.invalid_before))?;
    write!(writer, "}},\"invalid_after\":{{")?;
    write_sample(writer, read_sample(&ledger.invalid_after))?;
    writeln!(writer, "}}}}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    static SAMPLE: AtomicU64 = AtomicU64::new(100);
    fn fake() -> Result<CounterSample, ProbeError> {
        let value = SAMPLE.fetch_add(10, Ordering::Relaxed);
        Ok(CounterSample {
            instructions: value,
            cycles: value,
            wall_ns: value,
            ..CounterSample::default()
        })
    }
    fn reset() {
        SAMPLE.store(100, Ordering::Relaxed);
        LAST.with(|last| last.set(CounterSample::default()));
    }

    #[test]
    fn overflow_counts_every_dropped_boundary_without_growing() {
        reset();
        let ledger = Box::leak(Box::new(Ledger::new(fake, 2, CounterSample::default())));
        for _ in 0..4 {
            end(begin_active(ledger, Phase::ScanParse));
        }
        assert_eq!(ledger.slots.len(), 2);
        assert_eq!(ledger.events().count(), 2);
        assert_eq!(ledger.next.load(Ordering::Relaxed), 8);
        assert_eq!(ledger.dropped.load(Ordering::Relaxed), 6);
        assert_eq!(ledger.head.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn decreasing_fake_sample_disables_the_run() {
        reset();
        let ledger = Box::leak(Box::new(Ledger::new(fake, 8, CounterSample::default())));
        let token = begin_active(ledger, Phase::Bind);
        SAMPLE.store(1, Ordering::Relaxed);
        end(token);
        assert_eq!(
            ledger.failure.load(Ordering::Relaxed),
            Failure::NonMonotonic as usize
        );
        assert!(ledger.sample().is_none());
        assert_eq!(ledger.events().count(), 1);
    }

    #[test]
    fn syscall_error_preserves_errno_and_disables() {
        fn denied() -> Result<CounterSample, ProbeError> {
            Err(ProbeError {
                reason: Failure::Syscall,
                errno: 13,
            })
        }
        let ledger = Ledger::new(denied, 2, CounterSample::default());
        assert!(ledger.sample().is_none());
        assert_eq!(ledger.errno.load(Ordering::Relaxed), 13);
        assert_eq!(
            ledger.failure.load(Ordering::Relaxed),
            Failure::Syscall as usize
        );
    }

    #[test]
    fn calibration_pairs_have_only_the_envelope_as_parent() {
        reset();
        let ledger = Box::leak(Box::new(Ledger::new(fake, 16, CounterSample::default())));
        calibrate_with(3, |phase| begin_active(ledger, phase));
        let events: Vec<_> = ledger.events().collect();
        assert_eq!(events.len(), 8);
        assert!(events
            .iter()
            .all(|event| event.phase == Phase::Probe as u16));
        assert!(events[1..7]
            .iter()
            .all(|event| event.parent == 1 && event.depth == 1));
        assert_eq!(ledger.head.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn serialized_token_can_close_on_the_next_thread() {
        reset();
        let ledger = Box::leak(Box::new(Ledger::new(fake, 8, CounterSample::default())));
        let token = begin_active(ledger, Phase::FsLoader);
        std::thread::spawn(move || end(token)).join().unwrap();
        let events: Vec<_> = ledger.events().collect();
        assert_eq!(events.len(), 2);
        assert_ne!(events[0].thread, events[1].thread);
        assert_eq!(events[0].span, events[1].span);
        assert_eq!(ledger.head.load(Ordering::Relaxed), 0);
        assert_eq!(ledger.failure.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn concurrent_slots_preserve_thread_shard_and_parent_links() {
        reset();
        let ledger: &'static Ledger =
            Box::leak(Box::new(Ledger::new(fake, 256, CounterSample::default())));
        ledger.concurrent.store(true, Ordering::Relaxed);
        std::thread::scope(|threads| {
            for shard in 0..2 {
                threads.spawn(move || {
                    SHARD.with(|slot| slot.set(shard));
                    for _ in 0..20 {
                        let parent = begin_active(ledger, Phase::ShardBusy);
                        end(begin_active(ledger, Phase::ShardSetup));
                        end(parent);
                    }
                });
            }
        });
        let events: Vec<_> = ledger.events().collect();
        assert_eq!(events.len(), 160);
        assert_eq!(ledger.dropped.load(Ordering::Relaxed), 0);
        assert_eq!(ledger.failure.load(Ordering::Relaxed), 0);
        for (index, event) in events.iter().enumerate() {
            assert_eq!(event.sequence as usize, index + 1);
            assert!(event.sample.instructions < event.observer_end.instructions);
            if event.depth == 1 {
                let parent = events[(event.parent - 1) as usize];
                assert_eq!(parent.thread, event.thread);
                assert_eq!(parent.shard, event.shard);
            }
        }
    }

    #[test]
    fn concurrent_non_monotonic_counters_disable_instructions_but_retain_wall() {
        static CLOCK: AtomicU64 = AtomicU64::new(0);
        fn approximate() -> Result<CounterSample, ProbeError> {
            let mut sample = fake()?;
            sample.wall_ns = CLOCK.fetch_add(10, Ordering::Relaxed);
            Ok(sample)
        }
        reset();
        let ledger = Ledger::new(approximate, 2, CounterSample::default());
        ledger.concurrent.store(true, Ordering::Relaxed);
        let _ = ledger.sample().unwrap();
        SAMPLE.store(1, Ordering::Relaxed);
        assert!(ledger.sample().is_some());
        assert_eq!(
            ledger.failure.load(Ordering::Relaxed),
            Failure::NonMonotonic as usize
        );
        assert_eq!(ledger.invalid_fields.load(Ordering::Relaxed), 3);
        assert!(ledger.wall_valid.load(Ordering::Relaxed));
        assert!(ledger.sample().is_some());
    }

    #[test]
    fn final_probe_after_bulk_output_can_disable_the_first_header() {
        reset();
        let ledger = Box::leak(Box::new(Ledger::new(fake, 16, CounterSample::default())));
        calibrate_with(3, |phase| begin_active(ledger, phase));
        let end = ledger.sample();
        SAMPLE.store(1, Ordering::Relaxed);
        let mut file = io::Cursor::new(Vec::new());
        flush_active(ledger, &mut file, end).unwrap();
        let text = String::from_utf8(file.into_inner()).unwrap();
        let header = text.lines().next().unwrap();
        assert!(header.contains("\"enabled\":false"));
        assert!(header.contains("non-monotonic counter sample"));
        assert!(text.contains("\"type\":\"work_end\""));
        assert!(!text.contains("\"type\":\"pre_exit\""));
    }
}

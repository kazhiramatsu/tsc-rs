//! Observation-only mimalloc wrapper. Hooks use const TLS, fixed atomic
//! counters, a bounded table and the qualified frame-chain reader. No heap
//! allocation, formatting, platform call or unwinding library in capture.
mod unwind;
use std::alloc::{GlobalAlloc, Layout};
use std::cell::Cell;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::sync::OnceLock;
use tsc_types::perf::census::{self, Site};

const THREADS: usize = 64;
const PHASES: usize = 20;
const SIZES: usize = 9;
const ALIGNS: usize = 8;
const BINS: usize = SIZES * ALIGNS;
const FIELDS: usize = 11;
const ALLOC: usize = 0;
const ZEROED: usize = 1;
const REALLOC: usize = 2;
const DEALLOC: usize = 3;
const FAIL: usize = 4;
const REQUESTED: usize = 5;
const OLD: usize = 6;
const NEW: usize = 7;
const FREED: usize = 8;
const MOVED: usize = 9;
const COPIED: usize = 10;
const WORDS: usize = 15;
// Pack physical release into the existing end-kind word; no larger sample.
const END_KIND_MASK: u64 = 0xff;
const BLOCK_RELEASED: u64 = 1 << 8;
struct Sample {
    words: [AtomicU64; WORDS],
    ready: AtomicBool,
    pcs: [AtomicU64; unwind::DEPTH],
    pc_count: AtomicUsize,
    // Frozen at M7's after_check boundary, independent of pre-exit lifetime.
    // 0 = outside snapshot, 1 = live (from table), 2 = ended, 3 = lost, 4 = failed.
    peak_state: AtomicUsize,
}
impl Sample {
    const fn new() -> Self {
        Self {
            words: [const { AtomicU64::new(0) }; WORDS],
            ready: AtomicBool::new(false),
            pcs: [const { AtomicU64::new(0) }; unwind::DEPTH],
            pc_count: AtomicUsize::new(0),
            peak_state: AtomicUsize::new(0),
        }
    }
}
struct Entry {
    pointer: AtomicUsize,
    sample: AtomicUsize,
}
struct Data {
    samples: Box<[Sample]>,
    table: Box<[Entry]>,
    next: AtomicUsize,
    dropped: AtomicU64,
    table_loss: AtomicU64,
    occupancy: AtomicUsize,
    table_peak: AtomicUsize,
    lock: AtomicBool,
    rate: usize,
    peak_count: AtomicUsize,
    peak_limit: AtomicUsize,
    peak_live: AtomicI64,
    peak_serial: AtomicBool,
}
static DATA: OnceLock<Data> = OnceLock::new();
static NEXT_THREAD: AtomicUsize = AtomicUsize::new(0);
static SLOT_LIMIT: AtomicUsize = AtomicUsize::new(THREADS);
static SLOT_OVERFLOW: AtomicU64 = AtomicU64::new(0);
static PREINSTALL: AtomicU64 = AtomicU64::new(0);
static RECURSION: AtomicU64 = AtomicU64::new(0);
static FLUSHING: AtomicBool = AtomicBool::new(false);
static CONFIG_ERROR: AtomicBool = AtomicBool::new(false);
static UNWIND_EMPTY: AtomicU64 = AtomicU64::new(0);
static IMAGE: OnceLock<(usize, isize)> = OnceLock::new();

static STACK_LOW: [AtomicUsize; THREADS] = [const { AtomicUsize::new(0) }; THREADS];
static STACK_HIGH: [AtomicUsize; THREADS] = [const { AtomicUsize::new(0) }; THREADS];
pub fn warm_thread() {
    let thread = slot();
    if thread < THREADS {
        let (low, high) = unwind::stack_bounds();
        STACK_LOW[thread].store(low, Relaxed);
        STACK_HIGH[thread].store(high, Relaxed);
    }
}

#[cfg(test)]
thread_local! { static HOOK_ENTRIES: Cell<u64> = const { Cell::new(0) }; }
#[cfg(test)]
pub(super) fn hook_entries() -> u64 {
    HOOK_ENTRIES.with(Cell::get)
}

struct Counts {
    values: [AtomicU64; FIELDS],
    peak: AtomicI64,
}
impl Counts {
    const fn new() -> Self {
        Self {
            values: [const { AtomicU64::new(0) }; FIELDS],
            peak: AtomicI64::new(0),
        }
    }
    fn add(&self, kind: usize, size: usize, old: usize, success: bool) {
        self.values[kind].fetch_add(1, Relaxed);
        if !success {
            self.values[FAIL].fetch_add(1, Relaxed);
        }
        if kind != DEALLOC {
            self.values[REQUESTED].fetch_add(size as u64, Relaxed);
        }
        if kind == REALLOC {
            self.values[OLD].fetch_add(old as u64, Relaxed);
            self.values[NEW].fetch_add(size as u64, Relaxed);
        }
        if kind == DEALLOC || (kind == REALLOC && success) {
            self.values[FREED].fetch_add(old as u64, Relaxed);
        }
    }
    fn read(&self) -> [u64; FIELDS] {
        self.values.each_ref().map(|n| n.load(Relaxed))
    }
}
static ROWS: [Counts; (THREADS + 1) * PHASES * BINS] =
    [const { Counts::new() }; (THREADS + 1) * PHASES * BINS];
static OBSERVER: Counts = Counts::new();
static RECURSIVE: Counts = Counts::new();
static ALL_LIVE: AtomicI64 = AtomicI64::new(0);
static ALL_PEAK: AtomicI64 = AtomicI64::new(0);
static LIVE: AtomicI64 = AtomicI64::new(0);
static PEAK: AtomicI64 = AtomicI64::new(0);
static BIN_LIVE: [AtomicI64; SIZES] = [const { AtomicI64::new(0) }; SIZES];
// site x phase x length bucket, each cell = calls, length/reserved, payload/used
static SITES: [[AtomicU64; 3]; 13 * PHASES * 8] =
    [const { [const { AtomicU64::new(0) }; 3] }; 13 * PHASES * 8];
thread_local! {
    static SLOT: Cell<usize> = const { Cell::new(usize::MAX) };
    static RNG: Cell<u64> = const { Cell::new(0) };
    static IN_HOOK: Cell<bool> = const { Cell::new(false) };
    static OBSERVING: Cell<bool> = const { Cell::new(false) };
}
fn slot() -> usize {
    SLOT.try_with(|slot| {
        if slot.get() == usize::MAX {
            let id = NEXT_THREAD.fetch_add(1, Relaxed);
            slot.set(if id < SLOT_LIMIT.load(Relaxed) {
                id
            } else {
                THREADS
            });
            RNG.with(|rng| rng.set((id as u64 + 1).wrapping_mul(0x9e3779b97f4a7c15)));
        }
        slot.get()
    })
    .unwrap_or(THREADS)
}
fn size_bin(size: usize) -> usize {
    match size {
        0..=16 => 0,
        17..=32 => 1,
        33..=64 => 2,
        65..=128 => 3,
        129..=256 => 4,
        257..=1024 => 5,
        1025..=4096 => 6,
        4097..=65536 => 7,
        _ => 8,
    }
}
fn align_bin(align: usize) -> usize {
    (align.trailing_zeros() as usize).min(ALIGNS - 1)
}
fn length_bin(len: usize) -> usize {
    match len {
        0 => 0,
        1 => 1,
        2..=8 => 2,
        9..=12 => 3,
        13..=20 => 4,
        21..=64 => 5,
        65..=256 => 6,
        _ => 7,
    }
}
fn phase() -> (usize, u64) {
    let (phase, span) = tsc_types::trace::ledger::current_frame();
    (usize::from(phase).min(PHASES - 1), span)
}
struct Observer(bool);
impl Observer {
    fn enter() -> Self {
        Self(OBSERVING.with(|value| value.replace(true)))
    }
}
impl Drop for Observer {
    fn drop(&mut self) {
        OBSERVING.with(|value| value.set(self.0));
    }
}

pub struct CountingAllocator;
// SAFETY: all pointers and layouts are passed unchanged to the same mimalloc
// GlobalAlloc implementation. Observation never changes allocator results.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { operation(ALLOC, std::ptr::null_mut(), layout, layout.size()) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        unsafe { operation(ZEROED, std::ptr::null_mut(), layout, layout.size()) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        unsafe { operation(REALLOC, ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe {
            operation(DEALLOC, ptr, layout, 0);
        }
    }
}
// The recursion guard covers the backend too. Teardown TLS unavailability
// goes to the explicit recursive bucket, without touching TLS a second time.
unsafe fn operation(kind: usize, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
    #[cfg(test)]
    HOOK_ENTRIES.with(|count| count.set(count.get() + 1));
    let nested = IN_HOOK
        .try_with(|guard| guard.replace(true))
        .unwrap_or(true);
    let observing = FLUSHING.load(Relaxed) || OBSERVING.try_with(Cell::get).unwrap_or(true);
    let thread = if nested || observing { THREADS } else { slot() };
    let (phase, span) = if nested || observing { (0, 0) } else { phase() };
    let old = if kind == REALLOC || kind == DEALLOC {
        layout.size()
    } else {
        0
    };
    // Remove before releasing the address to mimalloc; address reuse cannot
    // attach an old sample to a new block, even across threads.
    let previous = if !nested && (kind == REALLOC || kind == DEALLOC) {
        DATA.get().and_then(|data| data.remove(ptr as usize))
    } else {
        None
    };
    if kind == DEALLOC {
        ALL_LIVE.fetch_sub(old as i64, Relaxed);
    }
    if kind == DEALLOC && !nested && !observing {
        live_change(-((old) as i64), size_bin(old), -(old as i64));
    }
    let result = unsafe {
        match kind {
            ALLOC => mimalloc::MiMalloc.alloc(layout),
            ZEROED => mimalloc::MiMalloc.alloc_zeroed(layout),
            REALLOC => mimalloc::MiMalloc.realloc(ptr, layout, size),
            _ => {
                mimalloc::MiMalloc.dealloc(ptr, layout);
                std::ptr::null_mut()
            }
        }
    };
    let success = kind == DEALLOC || !result.is_null();
    if success && kind != DEALLOC {
        let now = ALL_LIVE.fetch_add(size as i64 - old as i64, Relaxed) + size as i64 - old as i64;
        ALL_PEAK.fetch_max(now, Relaxed);
    }
    if let Some(index) = previous {
        let data = DATA.get().unwrap();
        if success {
            data.retire(
                index,
                phase,
                span,
                thread,
                kind,
                kind == DEALLOC || result != ptr,
            );
        } else {
            data.insert(ptr as usize, index);
        }
    }
    if success && kind == REALLOC && result != ptr {
        let target = if nested {
            &RECURSIVE
        } else if observing {
            &OBSERVER
        } else {
            &ROWS[(thread * PHASES + phase) * BINS
                + size_bin(size) * ALIGNS
                + align_bin(layout.align())]
        };
        target.values[MOVED].fetch_add(1, Relaxed);
        target.values[COPIED].fetch_add(old.min(size) as u64, Relaxed);
    }
    if nested {
        RECURSION.fetch_add(1, Relaxed);
        RECURSIVE.add(kind, size, old, success);
    } else if observing {
        OBSERVER.add(kind, size, old, success);
    } else {
        if thread == THREADS {
            SLOT_OVERFLOW.fetch_add(1, Relaxed);
        }
        let bin = size_bin(if kind == DEALLOC { old } else { size });
        let row =
            &ROWS[(thread * PHASES + phase) * BINS + bin * ALIGNS + align_bin(layout.align())];
        row.add(kind, size, old, success);
        let bin_now = if success && kind != DEALLOC {
            if kind == REALLOC {
                BIN_LIVE[size_bin(old)].fetch_sub(old as i64, Relaxed);
            }
            live_change(size as i64 - old as i64, bin, size as i64)
        } else {
            BIN_LIVE[bin].load(Relaxed)
        };
        row.peak.fetch_max(bin_now, Relaxed);
        if kind != DEALLOC {
            if let Some(data) = DATA.get() {
                if choose(data.rate) {
                    data.sample(
                        thread,
                        phase,
                        span,
                        kind,
                        size,
                        old,
                        layout.align(),
                        success,
                        result as usize,
                    );
                }
            } else {
                PREINSTALL.fetch_add(1, Relaxed);
            }
        }
    }
    if !nested {
        let _ = IN_HOOK.try_with(|guard| guard.set(false));
    }
    result
}
fn live_change(delta: i64, bin: usize, bin_delta: i64) -> i64 {
    let now = LIVE.fetch_add(delta, Relaxed) + delta;
    PEAK.fetch_max(now, Relaxed);
    BIN_LIVE[bin].fetch_add(bin_delta, Relaxed) + bin_delta
}
fn choose(rate: usize) -> bool {
    RNG.try_with(|rng| {
        let mut x = rng.get();
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        rng.set(x);
        (x.wrapping_mul(0x2545f4914f6cdd1d) as usize & (rate - 1)) == 0
    })
    .unwrap_or(false)
}
struct TableGuard<'a>(&'a AtomicBool);
impl Drop for TableGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::Release);
    }
}
impl Data {
    fn lock(&self) -> TableGuard<'_> {
        while self
            .lock
            .compare_exchange_weak(false, true, std::sync::atomic::Ordering::Acquire, Relaxed)
            .is_err()
        {
            std::hint::spin_loop();
        }
        TableGuard(&self.lock)
    }
    fn hash(&self, ptr: usize) -> usize {
        (ptr >> 4).wrapping_mul(0x9e3779b97f4a7c15usize) % self.table.len()
    }
    fn remove(&self, ptr: usize) -> Option<usize> {
        if self.table.is_empty() {
            return None;
        }
        let _guard = self.lock();
        let start = self.hash(ptr);
        for offset in 0..self.table.len().min(128) {
            let entry = &self.table[(start + offset) % self.table.len()];
            let key = entry.pointer.load(Relaxed);
            if key == 0 {
                break;
            }
            if key == ptr {
                entry.pointer.store(1, Relaxed); // tombstone preserves probe chains
                self.occupancy.fetch_sub(1, Relaxed);
                return Some(entry.sample.load(Relaxed));
            }
        }
        None
    }
    fn insert(&self, ptr: usize, index: usize) {
        let _guard = self.lock();
        if !self.table.is_empty() {
            let start = self.hash(ptr);
            for offset in 0..self.table.len().min(128) {
                let entry = &self.table[(start + offset) % self.table.len()];
                if entry.pointer.load(Relaxed) <= 1 {
                    entry.sample.store(index, Relaxed);
                    entry.pointer.store(ptr, Relaxed);
                    let count = self.occupancy.fetch_add(1, Relaxed) + 1;
                    self.table_peak.fetch_max(count, Relaxed);
                    return;
                }
            }
        }
        self.table_loss.fetch_add(1, Relaxed);
        self.samples[index].words[7].store(3, Relaxed); // unknown lifetime, never "retained"
    }
    #[allow(clippy::too_many_arguments)]
    fn sample(
        &self,
        thread: usize,
        phase: usize,
        span: u64,
        kind: usize,
        size: usize,
        old: usize,
        align: usize,
        success: bool,
        ptr: usize,
    ) {
        let index = self.next.fetch_add(1, Relaxed);
        let Some(sample) = self.samples.get(index) else {
            self.dropped.fetch_add(1, Relaxed);
            return;
        };
        let words = [
            thread as u64,
            phase as u64,
            span,
            kind as u64,
            size as u64,
            old as u64,
            align as u64,
            if success { 0 } else { 4 },
            0,
            0,
            0,
            0,
            u64::from(success),
            index as u64,
            census::current_site() as u64,
        ];
        for (target, value) in sample.words.iter().zip(words) {
            target.store(value, Relaxed);
        }
        let mut pcs = [0; unwind::DEPTH];
        let bounds = if thread < THREADS {
            (
                STACK_LOW[thread].load(Relaxed),
                STACK_HIGH[thread].load(Relaxed),
            )
        } else {
            (0, 0)
        };
        let count = unwind::capture(&mut pcs, bounds);
        if count == 0 {
            UNWIND_EMPTY.fetch_add(1, Relaxed);
        }
        for (target, pc) in sample.pcs.iter().zip(pcs) {
            target.store(pc as u64, Relaxed);
        }
        sample.pc_count.store(count, Relaxed);
        sample
            .ready
            .store(true, std::sync::atomic::Ordering::Release);
        if success {
            self.insert(ptr, index);
        }
    }
    fn snapshot_peak(&self) {
        if self.peak_count.fetch_add(1, Relaxed) != 0 {
            return;
        }
        let _guard = self.lock();
        self.peak_serial
            .store(tsc_types::trace::ledger::census_serial(), Relaxed);
        self.peak_live.store(LIVE.load(Relaxed), Relaxed);
        let limit = self.next.load(Relaxed).min(self.samples.len());
        self.peak_limit.store(limit, Relaxed);
        for sample in self.samples.iter().take(limit) {
            let state = match sample.words[7].load(Relaxed) {
                1 | 2 => 2,
                3 => 3,
                4 => 4,
                _ => 0,
            };
            sample.peak_state.store(state, Relaxed);
        }
        for entry in &self.table {
            if entry.pointer.load(Relaxed) > 1 {
                self.samples[entry.sample.load(Relaxed)]
                    .peak_state
                    .store(1, Relaxed);
            }
        }
    }
    fn retire(
        &self,
        index: usize,
        phase: usize,
        span: u64,
        thread: usize,
        kind: usize,
        released: bool,
    ) {
        let sample = &self.samples[index];
        sample.words[8].store(phase as u64, Relaxed);
        sample.words[9].store(span, Relaxed);
        sample.words[10].store(thread as u64, Relaxed);
        sample.words[11].store(
            kind as u64 | if released { BLOCK_RELEASED } else { 0 },
            Relaxed,
        );
        sample.words[7].store(
            if sample.words[1].load(Relaxed) == phase as u64
                && sample.words[2].load(Relaxed) == span
            {
                1
            } else {
                2
            },
            Relaxed,
        );
    }
}
fn site_note(site: Site, calls: u64, reserved: usize, used: usize) {
    if FLUSHING.load(Relaxed) {
        return;
    }
    let (phase, _) = phase();
    let bin = if (site as usize) < census::BACKING_SITES {
        0
    } else {
        length_bin(reserved)
    };
    let row = &SITES[((site as usize) * PHASES + phase) * 8 + bin];
    row[0].fetch_add(calls, Relaxed);
    row[1].fetch_add(reserved as u64, Relaxed);
    row[2].fetch_add(used as u64, Relaxed);
}
fn setting(name: &str, default: usize, maximum: usize, power_of_two: bool) -> usize {
    match std::env::var(name) {
        Err(std::env::VarError::NotPresent) => default,
        Ok(value) => match value.parse::<usize>() {
            Ok(n) if n > 0 && n <= maximum && (!power_of_two || n.is_power_of_two()) => n,
            _ => {
                CONFIG_ERROR.store(true, Relaxed);
                default
            }
        },
        Err(_) => {
            CONFIG_ERROR.store(true, Relaxed);
            default
        }
    }
}
pub fn observe<T>(work: impl FnOnce() -> T) -> T {
    let _observer = Observer::enter();
    work()
}
pub fn install() -> Option<PathBuf> {
    let _observer = Observer::enter();
    let path = std::env::var_os("TSRS_ALLOC_CENSUS")?;
    warm_thread();
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn _dyld_get_image_header(index: u32) -> *const libc::c_void;
            fn _dyld_get_image_vmaddr_slide(index: u32) -> isize;
        }
        // SAFETY: image 0 is the executable; queried once outside all hooks.
        let image = unsafe {
            (
                _dyld_get_image_header(0) as usize,
                _dyld_get_image_vmaddr_slide(0),
            )
        };
        let _ = IMAGE.set(image);
    }
    let slots = setting("TSRS_ALLOC_CENSUS_SLOTS", THREADS, THREADS, false);
    // Existing slots remain valid if the startup thread already owns one.
    SLOT_LIMIT.store(slots, Relaxed);
    let bytes = setting("TSRS_ALLOC_CENSUS_SAMPLE_BYTES", 16 << 20, 256 << 20, false);
    let entries = setting("TSRS_ALLOC_CENSUS_POINTERS", 65536, 1 << 20, false);
    let rate = setting("TSRS_ALLOC_CENSUS_RATE", 1024, 1 << 30, true);
    let mut samples = Vec::new();
    let mut table = Vec::new();
    let count = bytes / std::mem::size_of::<Sample>();
    let allocated = count > 0
        && samples.try_reserve_exact(count).is_ok()
        && table.try_reserve_exact(entries).is_ok();
    if allocated {
        samples.extend((0..count).map(|_| Sample::new()));
        table.extend((0..entries).map(|_| Entry {
            pointer: AtomicUsize::new(0),
            sample: AtomicUsize::new(0),
        }));
    } else {
        CONFIG_ERROR.store(true, Relaxed);
    }
    let _ = DATA.set(Data {
        samples: samples.into_boxed_slice(),
        table: table.into_boxed_slice(),
        next: AtomicUsize::new(0),
        dropped: AtomicU64::new(0),
        table_loss: AtomicU64::new(0),
        occupancy: AtomicUsize::new(0),
        table_peak: AtomicUsize::new(0),
        lock: AtomicBool::new(false),
        rate,
        peak_count: AtomicUsize::new(0),
        peak_limit: AtomicUsize::new(0),
        peak_live: AtomicI64::new(0),
        peak_serial: AtomicBool::new(false),
    });
    census::install(site_note);
    census::install_thread(warm_thread);
    census::install_peak(|| {
        if let Some(data) = DATA.get() {
            data.snapshot_peak();
        }
    });
    Some(PathBuf::from(path))
}
fn write_counts(writer: &mut impl Write, values: [u64; FIELDS]) -> io::Result<()> {
    let [alloc, alloc_zeroed, realloc, dealloc, failures, requested_bytes, realloc_old_bytes, realloc_new_bytes, freed_bytes, realloc_moved, realloc_copy_bytes] =
        values;
    write!(writer, "\"alloc\":{alloc},\"alloc_zeroed\":{alloc_zeroed},\"realloc\":{realloc},\"dealloc\":{dealloc},\"failures\":{failures},\"requested_bytes\":{requested_bytes},\"realloc_old_bytes\":{realloc_old_bytes},\"realloc_new_bytes\":{realloc_new_bytes},\"freed_bytes\":{freed_bytes},\"realloc_moved\":{realloc_moved},\"realloc_copy_bytes\":{realloc_copy_bytes}")
}
pub fn flush(path: Option<&Path>) {
    let Some(path) = path else {
        return;
    };
    FLUSHING.store(true, Relaxed);
    let _observer = Observer::enter();
    // Observation failure never changes compiler streams or its exit status.
    let _ = write_sidecar(path);
}
fn write_sidecar(path: &Path) -> io::Result<()> {
    let Some(data) = DATA.get() else {
        return Ok(());
    };
    let mut writer = BufWriter::new(std::fs::File::create(path)?);
    writeln!(writer, "{{\"type\":\"header\",\"format\":\"tsrs-alloc-census\",\"version\":1,\"alloc_census\":true,\"phase_ledger\":true,\"peak_schema\":1,\"perf_counters\":{},\"sampling_rate\":{},\"slot_count\":{},\"overflow_slot\":{},\"size_bins\":9,\"alignment_bins\":8,\"sample_capacity\":{},\"sample_bytes\":{},\"pointer_capacity\":{},\"sample_attempts\":{},\"dropped_samples\":{},\"table_losses\":{},\"table_peak\":{},\"slot_overflow_events\":{},\"preinstall_unsampled_events\":{},\"recursion_events\":{},\"config_error\":{},\"unwinder\":\"{}\",\"unwind_empty_samples\":{},\"load_address\":\"0x{:x}\",\"image_slide\":{},\"live_peak_semantics\":\"maximum process size-bin event-balance observed in this phase; max across slots and alignments\"}}",
        tsc_types::perf::enabled(), data.rate, SLOT_LIMIT.load(Relaxed), THREADS,
        data.samples.len(), data.samples.len() * std::mem::size_of::<Sample>(), data.table.len(),
        data.next.load(Relaxed), data.dropped.load(Relaxed), data.table_loss.load(Relaxed), data.table_peak.load(Relaxed),
        SLOT_OVERFLOW.load(Relaxed), PREINSTALL.load(Relaxed), RECURSION.load(Relaxed), CONFIG_ERROR.load(Relaxed),
        if unwind::QUALIFIED { "darwin-arm64-frame-pointer-v1" } else { "unsupported" },
        UNWIND_EMPTY.load(Relaxed), IMAGE.get().copied().unwrap_or_default().0,
        IMAGE.get().copied().unwrap_or_default().1)?;
    let live_samples = data
        .samples
        .iter()
        .filter(|s| s.peak_state.load(Relaxed) == 1)
        .count();
    writeln!(writer, "{{\"type\":\"peak\",\"instant\":\"after_check\",\"snapshots\":{},\"serial\":{},\"sample_limit\":{},\"live_samples\":{},\"live_requested_bytes\":{}}}",
        data.peak_count.load(Relaxed), data.peak_serial.load(Relaxed), data.peak_limit.load(Relaxed), live_samples, data.peak_live.load(Relaxed))?;
    let mut totals = [0u64; FIELDS];
    for (index, row) in ROWS.iter().enumerate() {
        let counts = row.read();
        if counts.iter().all(|&n| n == 0) {
            continue;
        }
        for (total, value) in totals.iter_mut().zip(counts) {
            *total += value;
        }
        let thread = index / (PHASES * BINS);
        let phase = index / BINS % PHASES;
        let bin = index % BINS;
        write!(writer, "{{\"type\":\"block\",\"thread\":{thread},\"phase\":{phase},\"size_bin\":{},\"alignment_bin\":{},\"live_peak\":{},", bin / ALIGNS, bin % ALIGNS, row.peak.load(Relaxed))?;
        write_counts(&mut writer, counts)?;
        writeln!(writer, "}}")?;
    }
    write!(writer, "{{\"type\":\"totals\",\"level\":\"block\",\"live_requested_bytes\":{},\"peak_live_requested_bytes\":{},", LIVE.load(Relaxed), PEAK.load(Relaxed))?;
    write_counts(&mut writer, totals)?;
    writeln!(writer, "}}")?;
    for site in 0..census::NAMES.len() {
        for phase in 0..PHASES {
            for bin in 0..8 {
                let values = SITES[(site * PHASES + phase) * 8 + bin]
                    .each_ref()
                    .map(|n| n.load(Relaxed));
                if values == [0; 3] {
                    continue;
                }
                writeln!(writer, "{{\"type\":\"site\",\"level\":\"{}\",\"site\":\"{}\",\"phase\":{},\"length_bin\":{},\"count\":{},\"reserved_or_length\":{},\"used_bytes\":{}}}",
                    if site < census::BACKING_SITES { "backing" } else { "logical" }, census::NAMES[site], phase, bin, values[0], values[1], values[2])?;
            }
        }
    }
    for sample in data.samples.iter().take(data.next.load(Relaxed)) {
        if !sample.ready.load(std::sync::atomic::Ordering::Acquire) {
            continue;
        }
        write_sample(&mut writer, sample)?;
    }
    for (name, value) in tsc_types::perf::snapshot() {
        if let Some(value) = value {
            writeln!(
                writer,
                "{{\"type\":\"perf\",\"name\":\"{name}\",\"value\":{value}}}"
            )?;
        }
    }
    writer.flush()?;
    for (name, counts) in [("observer", &OBSERVER), ("recursive", &RECURSIVE)] {
        write!(writer, "{{\"type\":\"{name}\",")?;
        write_counts(&mut writer, counts.read())?;
        writeln!(writer, "}}")?;
    }
    let (available, instructions, cycles) = match crate::phase_ledger::entry() {
        Ok(sample) => (sample.instructions != 0, sample.instructions, sample.cycles),
        Err(_) => (false, 0, 0),
    };
    writeln!(writer, "{{\"type\":\"pre_exit\",\"all_live_requested_bytes\":{},\"all_peak_live_requested_bytes\":{},\"counters_available\":{},\"instructions\":{},\"cycles\":{}}}", ALL_LIVE.load(Relaxed), ALL_PEAK.load(Relaxed), available, instructions, cycles)?;
    writeln!(writer, "{{\"type\":\"complete\",\"version\":1}}")?;
    writer.flush()
}

fn write_sample(writer: &mut impl Write, sample: &Sample) -> io::Result<()> {
    let w = sample.words.each_ref().map(|n| n.load(Relaxed));
    write!(writer, "{{\"type\":\"sample\",\"thread\":{},\"phase\":{},\"span\":{},\"kind\":{},\"size\":{},\"old_size\":{},\"alignment\":{},\"lifetime\":{},\"end_phase\":{},\"end_span\":{},\"end_thread\":{},\"end_kind\":{},\"success\":{},\"sequence\":{},\"site\":\"{}\",\"peak_state\":{},\"end_released\":{},\"pcs\":[", w[0], w[1], w[2], w[3], w[4], w[5], w[6], w[7], w[8], w[9], w[10], w[11] & END_KIND_MASK, w[12] != 0, w[13], w[14].checked_sub(1).and_then(|site| census::NAMES.get(site as usize)).copied().unwrap_or("unknown"), sample.peak_state.load(Relaxed), w[11] & BLOCK_RELEASED != 0)?;
    for (index, pc) in sample
        .pcs
        .iter()
        .take(sample.pc_count.load(Relaxed))
        .enumerate()
    {
        if index != 0 {
            write!(writer, ",")?;
        }
        write!(writer, "\"0x{:x}\"", pc.load(Relaxed))?;
    }
    writeln!(writer, "]}}")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bins_keep_empty_and_singleton_separate() {
        assert_eq!((length_bin(0), length_bin(1), length_bin(2)), (0, 1, 2));
        assert_eq!(
            [16, 32, 64, 128, 256, 1024, 4096, 65536, 65537].map(size_bin),
            [0, 1, 2, 3, 4, 5, 6, 7, 8]
        );
        assert_eq!(
            [1, 2, 4, 8, 16, 32, 64, 128, 4096].map(align_bin),
            [0, 1, 2, 3, 4, 5, 6, 7, 7]
        );
    }
    fn data(capacity: usize) -> Data {
        Data {
            samples: (0..8).map(|_| Sample::new()).collect(),
            table: (0..capacity)
                .map(|_| Entry {
                    pointer: AtomicUsize::new(0),
                    sample: AtomicUsize::new(0),
                })
                .collect(),
            next: AtomicUsize::new(0),
            dropped: AtomicU64::new(0),
            table_loss: AtomicU64::new(0),
            occupancy: AtomicUsize::new(0),
            table_peak: AtomicUsize::new(0),
            lock: AtomicBool::new(false),
            rate: 1024,
            peak_count: AtomicUsize::new(0),
            peak_limit: AtomicUsize::new(0),
            peak_live: AtomicI64::new(0),
            peak_serial: AtomicBool::new(false),
        }
    }
    #[test]
    fn cross_thread_free_and_table_loss_are_explicit() {
        let data = data(1);
        data.sample(0, 9, 10, ALLOC, 32, 0, 8, true, 0x1000);
        data.sample(1, 9, 10, ALLOC, 32, 0, 8, true, 0x2000);
        assert_eq!(data.table_loss.load(Relaxed), 1);
        assert_eq!(data.samples[1].words[7].load(Relaxed), 3);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let id = data.remove(0x1000).unwrap();
                data.retire(id, 10, 11, 1, DEALLOC, true);
            });
        });
        assert_eq!(data.samples[0].words[7].load(Relaxed), 2);
        assert_eq!(data.samples[0].words[10].load(Relaxed), 1);
        data.insert(0x3000, 2);
        assert_eq!(data.remove(0x3000), Some(2));
    }
    #[test]
    fn failed_realloc_preserves_sample_and_success_ends_old_lifetime() {
        let data = data(4);
        data.sample(0, 9, 1, ALLOC, 32, 0, 8, true, 0x1000);
        let index = data.remove(0x1000).unwrap();
        data.insert(0x1000, index);
        assert_eq!(data.samples[index].words[7].load(Relaxed), 0);
        data.retire(data.remove(0x1000).unwrap(), 9, 1, 0, REALLOC, false);
        assert_eq!(data.samples[index].words[7].load(Relaxed), 1);
        data.sample(0, 9, 1, REALLOC, 64, 32, 8, false, 0);
        assert_eq!(data.samples[1].words[7].load(Relaxed), 4);
    }
    #[test]
    fn in_place_realloc_is_not_reported_as_a_released_block() {
        let data = data(16);
        for (index, (kind, released)) in [(REALLOC, false), (REALLOC, true), (DEALLOC, true)]
            .into_iter()
            .enumerate()
        {
            let pointer = 0x1000 + index * 0x1000;
            data.sample(0, 9, 1, ALLOC, 32, 0, 8, true, pointer);
            data.retire(data.remove(pointer).unwrap(), 9, 1, 0, kind, released);
            let mut bytes = Vec::new();
            write_sample(&mut bytes, &data.samples[index]).unwrap();
            let row: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(row["end_kind"], kind);
            assert_eq!(row["end_released"], released);
        }
        data.snapshot_peak();
        assert!(data.samples[..3]
            .iter()
            .all(|s| s.peak_state.load(Relaxed) == 2));
    }
    #[test]
    fn sample_serializes_as_one_complete_json_line_with_its_pcs() {
        let sample = Sample::new();
        sample.words[4].store(4096, Relaxed);
        sample.peak_state.store(1, Relaxed);
        sample.pc_count.store(2, Relaxed);
        sample.pcs[0].store(0x1004, Relaxed);
        sample.pcs[1].store(0x2004, Relaxed);
        let mut bytes = Vec::new();
        write_sample(&mut bytes, &sample).unwrap();
        assert_eq!(bytes.iter().filter(|&&byte| byte == b'\n').count(), 1);
        let row: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(row["pcs"], serde_json::json!(["0x1004", "0x2004"]));
        assert_eq!(row["size"], 4096);
        assert_eq!(row["peak_state"], 1);
        bytes.clear();
        sample.pc_count.store(0, Relaxed);
        write_sample(&mut bytes, &sample).unwrap();
        let row: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(row["pcs"], serde_json::json!([]));
    }
    #[test]
    fn peak_is_from_pointer_table_and_survives_later_free_and_reuse() {
        let data = data(16);
        data.sample(0, 9, 1, ALLOC, 4096, 0, 8, true, 0x1000);
        data.sample(0, 2, 2, ALLOC, 1024, 0, 8, true, 0x2000);
        data.retire(data.remove(0x2000).unwrap(), 9, 1, 0, DEALLOC, true);
        data.sample(0, 9, 1, ALLOC, 32, 0, 8, false, 0);
        data.snapshot_peak();
        assert_eq!(data.peak_limit.load(Relaxed), 3);
        assert_eq!(data.samples[0].peak_state.load(Relaxed), 1);
        assert_eq!(data.samples[1].peak_state.load(Relaxed), 2);
        assert_eq!(data.samples[2].peak_state.load(Relaxed), 4);
        data.retire(data.remove(0x1000).unwrap(), 10, 3, 0, DEALLOC, true);
        data.sample(0, 10, 3, ALLOC, 8192, 0, 8, true, 0x1000);
        data.snapshot_peak(); // first checkpoint stays frozen; repeat invalidates qualification
        assert_eq!(data.peak_count.load(Relaxed), 2);
        assert_eq!(data.samples[0].peak_state.load(Relaxed), 1);
        assert_eq!(data.samples[3].peak_state.load(Relaxed), 0);
    }
    #[test]
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    fn sampled_event_contains_the_bounded_return_pcs() {
        warm_thread();
        let data = data(16);
        data.sample(slot(), 9, 1, ALLOC, 4096, 0, 8, true, 0x1000);
        let sample = &data.samples[0];
        let count = sample.pc_count.load(Relaxed);
        assert!((1..=unwind::DEPTH).contains(&count));
        assert!(sample.pcs[..count].iter().all(|pc| pc.load(Relaxed) != 0));
    }
    #[test]
    fn peak_never_calls_a_lost_pointer_live() {
        let data = data(1);
        data.sample(0, 9, 1, ALLOC, 4096, 0, 8, true, 0x1000);
        data.sample(0, 9, 1, ALLOC, 1024, 0, 8, true, 0x2000);
        data.snapshot_peak();
        assert_eq!(data.samples[0].peak_state.load(Relaxed), 1);
        assert_eq!(data.samples[1].peak_state.load(Relaxed), 3);
    }
    #[test]
    fn wrapper_preserves_zeroing_alignment_and_realloc_payload() {
        let allocator = CountingAllocator;
        let layout = Layout::from_size_align(33, 128).unwrap();
        // SAFETY: layouts are valid; each successful pointer remains owned
        // by this test until its matching realloc/dealloc.
        unsafe {
            let pointer = allocator.alloc_zeroed(layout);
            assert!(!pointer.is_null());
            assert_eq!(pointer as usize % 128, 0);
            assert!((0..33).all(|i| pointer.add(i).read() == 0));
            pointer.write_bytes(0x5a, 33);
            let grown = allocator.realloc(pointer, layout, 96);
            assert!(!grown.is_null());
            assert_eq!(grown as usize % 128, 0);
            assert!((0..33).all(|i| grown.add(i).read() == 0x5a));
            allocator.dealloc(grown, Layout::from_size_align(96, 128).unwrap());
        }
    }
    #[test]
    fn recursion_is_counted_and_observer_work_is_separate() {
        let allocator = CountingAllocator;
        let layout = Layout::from_size_align(17, 8).unwrap();
        let before = RECURSION.load(Relaxed);
        IN_HOOK.with(|guard| guard.set(true));
        // SAFETY: valid layout and matching allocation/deallocation.
        unsafe {
            let pointer = allocator.alloc(layout);
            assert!(!pointer.is_null());
            allocator.dealloc(pointer, layout);
        }
        IN_HOOK.with(|guard| guard.set(false));
        assert_eq!(RECURSION.load(Relaxed) - before, 2);
        let before = OBSERVER.values[ALLOC].load(Relaxed);
        observe(|| {
            // SAFETY: valid layout and matching allocation/deallocation.
            unsafe {
                let pointer = allocator.alloc(layout);
                allocator.dealloc(pointer, layout);
            }
        });
        assert_eq!(OBSERVER.values[ALLOC].load(Relaxed) - before, 1);
    }
}

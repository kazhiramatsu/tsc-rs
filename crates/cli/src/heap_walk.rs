//! Observation only (m8 heap walk, never merged): with `TSRS_HEAP_WALK` set (one worker and one checker), the
//! allocator's areas are walked at the checker's named instants (after the bind, after the check), and the free
//! slots inside partly used pages are printed per heap view and per size class. Areas only, never blocks; the
//! callbacks do not allocate. Modelled on the M7b census (`handover/perf-tsrs/m7b-lane-record/patches/01-heap-census.patch`).

use libmimalloc_sys::{mi_block_visit_fun, mi_heap_area_t, mi_heap_t, mi_theap_t};
use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, Ordering};

#[repr(C)]
#[derive(Clone, Copy)]
struct SubprocId {
    id: *mut c_void,
}

// mimalloc v3 public APIs that the Rust bindings do not declare.
unsafe extern "C" {
    fn mi_subproc_current() -> SubprocId;
    fn mi_subproc_visit_heaps(
        subproc: SubprocId,
        visitor: unsafe extern "C" fn(*mut mi_heap_t, *mut c_void) -> bool,
        arg: *mut c_void,
    ) -> bool;
    fn mi_theap_get_default() -> *mut mi_theap_t;
    fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut c_void) -> i32;
    fn getpid() -> i32;
    fn mi_theap_visit_blocks(
        theap: *const mi_theap_t,
        visit_blocks: bool,
        visitor: mi_block_visit_fun,
        arg: *mut c_void,
    ) -> bool;
}

static MAIN_THEAP: AtomicPtr<mi_theap_t> = AtomicPtr::new(std::ptr::null_mut());
const MI_OPTION_VISIT_ABANDONED: libmimalloc_sys::mi_option_t = 28;
const CLASSES: usize = 256;
const SEEN: usize = 1 << 17;

/// Install the observer when asked for (on the process main thread, before any worker).
pub(super) fn install() {
    if std::env::var_os("TSRS_HEAP_WALK").is_none() {
        return;
    }
    // SAFETY: public APIs that set an allocator option and read this thread's theap.
    unsafe {
        libmimalloc_sys::mi_option_set(MI_OPTION_VISIT_ABANDONED, 1);
        MAIN_THEAP.store(mi_theap_get_default(), Ordering::Relaxed);
    }
    tsc_types::trace::set_observer(observe);
}

#[derive(Clone, Copy, Default)]
struct Class {
    block_size: usize,
    pages: usize,
    committed: usize,
    live: usize,
}

struct Walk {
    seen: Vec<usize>,
    classes: [Class; CLASSES],
    by_source: [(usize, usize); 3],
    source: usize,
    areas: usize,
    duplicates: usize,
}

unsafe extern "C" fn area(
    _heap: *const mi_heap_t,
    area: *const mi_heap_area_t,
    _block: *mut c_void,
    _block_size: usize,
    arg: *mut c_void,
) -> bool {
    // SAFETY: the synchronous walk passes our Walk and the API's area descriptor.
    let (walk, area) = unsafe { (&mut *arg.cast::<Walk>(), &*area) };
    // Deduplicate the views by the area's block start (open addressing, preallocated).
    let key = area.blocks as usize;
    let mut slot = (key >> 4).wrapping_mul(0x9E37_79B9_7F4A_7C15) & (SEEN - 1);
    loop {
        match walk.seen[slot] {
            0 => {
                walk.seen[slot] = key;
                break;
            }
            seen if seen == key => {
                walk.duplicates += 1;
                return true;
            }
            _ => slot = (slot + 1) & (SEEN - 1),
        }
    }
    walk.areas += 1;
    let live = area.used * area.full_block_size;
    let free = area.committed.saturating_sub(live);
    walk.by_source[walk.source].0 += area.committed;
    walk.by_source[walk.source].1 += free;
    for class in walk.classes.iter_mut() {
        if class.block_size == area.block_size || class.block_size == 0 {
            class.block_size = area.block_size;
            class.pages += 1;
            class.committed += area.committed;
            class.live += live;
            break;
        }
    }
    true
}

unsafe extern "C" fn heap(heap: *mut mi_heap_t, arg: *mut c_void) -> bool {
    // SAFETY: the heap comes from the locked subprocess iterator; one worker, one checker.
    unsafe { libmimalloc_sys::mi_heap_visit_blocks(heap, false, Some(area), arg) }
}

fn observe(instant: &str) {
    if MAIN_THEAP.load(Ordering::Relaxed).is_null() {
        return;
    }
    let mut walk = Box::new(Walk {
        seen: vec![0; SEEN],
        classes: [Class::default(); CLASSES],
        by_source: [(0, 0); 3],
        source: 0,
        areas: 0,
        duplicates: 0,
    });
    let arg = std::ptr::from_mut(&mut *walk).cast::<c_void>();
    // SAFETY: callbacks do not allocate or free; all contexts live for the synchronous walks.
    let complete = unsafe {
        walk.source = 0;
        let heaps = mi_subproc_visit_heaps(mi_subproc_current(), heap, arg);
        let current = mi_theap_get_default();
        (*arg.cast::<Walk>()).source = 1;
        let calling = mi_theap_visit_blocks(current, false, Some(area), arg);
        let main = MAIN_THEAP.load(Ordering::Relaxed);
        (*arg.cast::<Walk>()).source = 2;
        let parked = main == current || mi_theap_visit_blocks(main, false, Some(area), arg);
        heaps && calling && parked
    };
    let mib = |bytes: usize| bytes as f64 / (1 << 20) as f64;
    let (committed, live) = walk.classes.iter().fold((0, 0), |(c, l), class| {
        (c + class.committed, l + class.live)
    });
    eprintln!(
        "[heap walk] {instant}: complete {complete}, areas {} (+{} seen twice), committed {:.1} MiB, live {:.1} MiB, free slots {:.1} MiB; by first view (heap / calling / parked): free {:.1} / {:.1} / {:.1} MiB",
        walk.areas,
        walk.duplicates,
        mib(committed),
        mib(live),
        mib(committed - live),
        mib(walk.by_source[0].1),
        mib(walk.by_source[1].1),
        mib(walk.by_source[2].1)
    );
    // The process's physical footprint now and its lifetime maximum (proc_pid_rusage, RUSAGE_INFO_V4: words 9 and 30).
    let mut usage = [0u64; 64];
    // SAFETY: a public libSystem call that fills the caller's buffer (rusage_info_v4 is 37 words).
    let ok = unsafe { proc_pid_rusage(getpid(), 4, usage.as_mut_ptr().cast::<c_void>()) } == 0;
    if ok {
        eprintln!(
            "[heap walk] {instant}: footprint {:.1} MiB, lifetime max {:.1} MiB",
            mib(usage[9] as usize),
            mib(usage[30] as usize)
        );
    }
    // Then purge every freed page and arena slice now (mi_collect(force)) and read the footprint again: the difference
    // is freed memory the allocator had not purged yet.
    // SAFETY: a public mimalloc API; the walk's own allocations are dropped before it returns.
    unsafe { libmimalloc_sys::mi_collect(true) };
    // SAFETY: as above.
    if unsafe { proc_pid_rusage(getpid(), 4, usage.as_mut_ptr().cast::<c_void>()) } == 0 {
        eprintln!(
            "[heap walk] {instant}: footprint after a forced purge {:.1} MiB",
            mib(usage[9] as usize)
        );
    }
    let mut classes = walk
        .classes
        .iter()
        .filter(|class| class.block_size != 0)
        .copied()
        .collect::<Vec<_>>();
    classes.sort_by_key(|class| std::cmp::Reverse(class.committed - class.live));
    // The 80-128 B classes always, then the rest of the top 16 by free slots.
    let small = |class: &&Class| (80..=128).contains(&class.block_size);
    let listed = classes
        .iter()
        .filter(small)
        .chain(classes.iter().filter(|class| !small(class)).take(12));
    for class in listed {
        eprintln!(
            "[heap walk] {instant}:   block {:>9} B: pages {:>5}, committed {:>7.1} MiB, live {:>7.1} MiB, free {:>6.1} MiB",
            class.block_size,
            class.pages,
            mib(class.committed),
            mib(class.live),
            mib(class.committed - class.live)
        );
    }
}

//! Process-lifetime mappings and block leases. Released mappings stay readable.

#[cfg(debug_assertions)]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, Once, OnceLock};

pub const BLOCK_BYTES: usize = 16 * 1024;
pub const RESERVE_BYTES: u64 = 32 * 1024 * 1024 * 1024;
const FREE: u32 = u32::MAX;
#[cfg(unix)]
const FIRST_REGION: usize = 0x6000_0000_0000;
#[cfg(target_arch = "wasm32")]
const FIRST_REGION: usize = 0;
#[cfg(unix)]
const BLOCKS: usize = RESERVE_BYTES as usize / BLOCK_BYTES;
#[cfg(target_arch = "wasm32")]
const BLOCKS: usize = ((1u64 << 32) / BLOCK_BYTES as u64) as usize;
#[cfg(unix)]
const OWNER_BASE: usize = FIRST_REGION + RESERVE_BYTES as usize;
#[cfg(unix)]
const OWNER_BYTES: usize = BLOCKS * size_of::<AtomicU32>();
#[cfg(all(unix, debug_assertions))]
const THREAD_BASE: usize = OWNER_BASE + OWNER_BYTES;
#[cfg(unix)]
const TABLE_BYTES: usize = OWNER_BYTES
    + if cfg!(debug_assertions) {
        BLOCKS * 8
    } else {
        0
    };
#[cfg(unix)]
const MAPPING_BYTES: usize = RESERVE_BYTES as usize + TABLE_BYTES;

// wasm has no virtual-address reservation. Static tables have link-time
// addresses in linear memory; block storage comes from alloc_zeroed.
#[cfg(target_arch = "wasm32")]
static OWNERS: [AtomicU32; BLOCKS] = [const { AtomicU32::new(0) }; BLOCKS];
#[cfg(all(target_arch = "wasm32", debug_assertions))]
static THREADS: [AtomicU64; BLOCKS] = [const { AtomicU64::new(0) }; BLOCKS];
static RESERVE: Once = Once::new();

#[inline(always)]
pub(crate) const fn address(raw: u32) -> usize {
    FIRST_REGION + ((raw as usize) << 3)
}

#[inline(always)]
pub(crate) const fn raw(pointer: usize) -> u32 {
    ((pointer - FIRST_REGION) >> 3) as u32
}

/// Reserve payload and metadata together, before any arena allocation or lookup.
/// Native placement never replaces an existing mapping. The payload starts
/// PROT_NONE; the adjacent owner and debug-thread tables start zero-filled RW.
pub(crate) fn reserve() {
    RESERVE.call_once(|| {
        #[cfg(unix)]
        {
            reserve_native();
            // SAFETY: these metadata pages are within our newly owned mapping.
            let result = unsafe {
                libc::mprotect(
                    OWNER_BASE as *mut _,
                    TABLE_BYTES,
                    libc::PROT_READ | libc::PROT_WRITE,
                )
            };
            assert_eq!(
                result,
                0,
                "arena metadata mprotect: {}",
                std::io::Error::last_os_error()
            );
        }
    });
}

#[cfg(unix)]
fn reserve_native() {
    #[cfg(target_os = "linux")]
    let placement = libc::MAP_FIXED_NOREPLACE;
    #[cfg(not(target_os = "linux"))]
    let placement = 0;
    // SAFETY: NOREPLACE (Linux), or a non-destructive hint (other Unix).
    // Exact placement is verified before any fixed-address access is possible.
    let mapped = unsafe {
        libc::mmap(
            FIRST_REGION as *mut _,
            MAPPING_BYTES,
            libc::PROT_NONE,
            libc::MAP_PRIVATE | libc::MAP_ANON | placement,
            -1,
            0,
        )
    };
    if mapped as usize != FIRST_REGION {
        let error = std::io::Error::last_os_error();
        if mapped != libc::MAP_FAILED {
            // SAFETY: a successful hint mapping at the wrong address is ours
            // to discard. The existing mapping at FIRST_REGION is untouched.
            unsafe {
                libc::munmap(mapped, MAPPING_BYTES);
            }
            panic!(
                "arena needs {MAPPING_BYTES} bytes at {FIRST_REGION:#x}; mmap returned {mapped:p}"
            );
        }
        panic!("arena could not reserve {MAPPING_BYTES} bytes at {FIRST_REGION:#x}: {error}");
    }
}

/// The process-wide reservation, created lazily by arena construction or global().
pub struct Reservation;
impl Reservation {
    pub fn global() -> &'static Self {
        static INSTANCE: Reservation = Reservation;
        reserve();
        &INSTANCE
    }
}

#[inline(always)]
fn owner_entry(index: usize) -> &'static AtomicU32 {
    debug_assert!(RESERVE.is_completed(), "arena lookup before reserve");
    debug_assert!(index < BLOCKS);
    #[cfg(unix)]
    // SAFETY: reserve initialized the fixed table; callers bound the index.
    unsafe {
        &*((OWNER_BASE as *const AtomicU32).add(index))
    }
    #[cfg(target_arch = "wasm32")]
    // SAFETY: the static table is initialized and the caller bounds the index.
    unsafe {
        OWNERS.get_unchecked(index)
    }
}

/// One owner-table load. None denotes unissued or released storage. Construct an
/// arena or call Reservation::global() before looking up handles; debug asserts
/// this initialization precondition without adding a release-mode Once check.
#[inline(always)]
pub fn owner_of(raw: u32) -> Option<u32> {
    #[cfg(target_arch = "wasm32")]
    if raw as usize >= (1usize << 29) {
        return None;
    }
    let owner = owner_entry((raw as usize) >> 11).load(Ordering::Acquire);
    if owner == 0 || owner == FREE {
        None
    } else {
        Some(owner - 1)
    }
}

#[cfg(debug_assertions)]
#[inline(always)]
fn thread_entry(index: usize) -> &'static AtomicU64 {
    debug_assert!(index < BLOCKS);
    #[cfg(unix)]
    // SAFETY: the zero-filled debug table is initialized with reserve.
    unsafe {
        &*((THREAD_BASE as *const AtomicU64).add(index))
    }
    #[cfg(target_arch = "wasm32")]
    // SAFETY: the caller bounded the index into the static debug table.
    unsafe {
        THREADS.get_unchecked(index)
    }
}

#[cfg(debug_assertions)]
#[inline(always)]
pub(crate) fn current_thread_id() -> u64 {
    // ThreadId::as_u64 is unstable on the lane's rustc. Allocate an id once per
    // thread; subsequent cell checks read TLS and one block-table entry.
    static NEXT_THREAD: AtomicU64 = AtomicU64::new(1);
    thread_local! {
        static ID: u64 = NEXT_THREAD.try_update(Ordering::Relaxed, Ordering::Relaxed,
            |id| (id < u64::MAX - 1).then_some(id + 1)).expect("arena thread ids exhausted");
    }
    ID.with(|id| *id)
}

#[cfg(debug_assertions)]
#[inline(always)]
pub(crate) fn assert_thread(pointer: usize) {
    #[cfg(unix)]
    let index = {
        let offset = pointer.wrapping_sub(FIRST_REGION);
        // An ArenaCell can be constructed on the stack before allocation.
        if offset >= RESERVE_BYTES as usize {
            return;
        }
        offset / BLOCK_BYTES
    };
    #[cfg(target_arch = "wasm32")]
    let index = pointer / BLOCK_BYTES;
    let owner = thread_entry(index).load(Ordering::Relaxed);
    // Zero means a non-local block (or ordinary wasm stack/heap storage).
    // Released blocks are marked MAX, so a retained cell reference diagnoses it.
    if owner != 0 {
        assert_eq!(
            owner,
            current_thread_id(),
            "ArenaCell used before adopting current thread or after release"
        );
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Run {
    pub first: usize,
    pub count: usize,
}
impl Run {
    pub fn address(self) -> usize {
        FIRST_REGION + self.first * BLOCK_BYTES
    }
    pub fn bytes(self) -> usize {
        self.count * BLOCK_BYTES
    }
    #[cfg(debug_assertions)]
    pub fn set_thread(self, thread: u64) {
        debug_assert!(RESERVE.is_completed(), "arena thread update before reserve");
        for index in self.first..self.first + self.count {
            thread_entry(index).store(thread, Ordering::Relaxed);
        }
    }
}

struct State {
    free: Vec<Run>,
    #[cfg(all(unix, feature = "arena-shuffle"))]
    seed: u32,
}

/// Process-wide block allocator, reusing released runs before issuing new blocks.
pub struct BlockAllocator {
    bump: AtomicU32,
    state: Mutex<State>,
}

impl BlockAllocator {
    pub fn global() -> &'static Self {
        static INSTANCE: OnceLock<BlockAllocator> = OnceLock::new();
        INSTANCE.get_or_init(|| Self {
            // Block zero is reserved so even the first block's first byte has
            // a nonzero handle. Its owner entry stays unissued.
            bump: AtomicU32::new(1),
            state: Mutex::new(State {
                free: Vec::new(),
                #[cfg(all(unix, feature = "arena-shuffle"))]
                seed: std::env::var("TSRS_ARENA_SHUFFLE_SEED")
                    .map(|s| s.parse().expect("invalid TSRS_ARENA_SHUFFLE_SEED"))
                    .unwrap_or(1),
            }),
        })
    }

    pub(crate) fn allocate(&self, count: usize, owner: u32) -> Run {
        assert!(
            owner < u32::MAX - 1,
            "owner collides with reserved owner-table values"
        );
        assert!(
            count > 0 && count < BLOCKS,
            "arena allocation exceeds address space"
        );
        reserve();
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(i) = state.free.iter().position(|run| run.count >= count) {
            let run = Run {
                first: state.free[i].first,
                count,
            };
            state.free[i].first += count;
            state.free[i].count -= count;
            if state.free[i].count == 0 {
                state.free.remove(i);
            }
            #[cfg(debug_assertions)]
            run.set_thread(0);
            for index in run.first..run.first + count {
                owner_entry(index).store(owner + 1, Ordering::Release);
            }
            return run;
        }
        #[cfg(all(unix, not(feature = "arena-shuffle")))]
        let run = {
            let first = self
                .bump
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |first| {
                    (first as usize + count <= BLOCKS).then_some(first + count as u32)
                })
                .expect("arena reservation exhausted") as usize;
            Run { first, count }
        };
        #[cfg(all(unix, feature = "arena-shuffle"))]
        let run = loop {
            let logical = self.bump.fetch_add(1, Ordering::Relaxed);
            assert!(
                (logical as usize) < BLOCKS * 2,
                "shuffled reservation exhausted"
            );
            let first = shuffled(logical, state.seed) as usize;
            if first != 0
                && first + count <= BLOCKS
                && (first..first + count).all(|i| owner_entry(i).load(Ordering::Relaxed) == 0)
            {
                break Run { first, count };
            }
        };
        #[cfg(target_arch = "wasm32")]
        let run = {
            use std::alloc::{alloc_zeroed, handle_alloc_error, Layout};
            let layout = Layout::from_size_align(count * BLOCK_BYTES, BLOCK_BYTES).unwrap();
            // SAFETY: valid nonzero layout. Mappings are deliberately retained
            // for process lifetime; released allocations go to our free list.
            let pointer = unsafe { alloc_zeroed(layout) };
            if pointer.is_null() {
                handle_alloc_error(layout);
            }
            self.bump.fetch_add(count as u32, Ordering::Relaxed);
            Run {
                first: pointer as usize / BLOCK_BYTES,
                count,
            }
        };
        #[cfg(unix)]
        {
            // SAFETY: the complete run is an unissued part of our reservation.
            let result = unsafe {
                libc::mprotect(
                    run.address() as *mut _,
                    run.bytes(),
                    libc::PROT_READ | libc::PROT_WRITE,
                )
            };
            assert_eq!(result, 0, "mprotect: {}", std::io::Error::last_os_error());
        }
        for index in run.first..run.first + count {
            owner_entry(index).store(owner + 1, Ordering::Release);
        }
        run
    }

    pub(crate) fn release(&self, run: Run) {
        #[cfg(unix)]
        zero_run(run, remap_run);
        #[cfg(target_arch = "wasm32")]
        zero_run(run, |_| false);
        #[cfg(debug_assertions)]
        run.set_thread(u64::MAX);
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        for index in run.first..run.first + run.count {
            owner_entry(index).store(FREE, Ordering::Release);
        }
        state.free.push(run);
        // Only mmap-backed reservation runs may be coalesced. Separate wasm
        // allocator objects must retain their original allocation boundaries.
        #[cfg(unix)]
        {
            state.free.sort_unstable_by_key(|r| r.first);
            let mut i = 0;
            while i + 1 < state.free.len() {
                if state.free[i].first + state.free[i].count == state.free[i + 1].first {
                    let next = state.free.remove(i + 1);
                    state.free[i].count += next.count;
                } else {
                    i += 1;
                }
            }
        }
    }
}

#[cfg(all(unix, feature = "arena-shuffle"))]
fn shuffled(logical: u32, seed: u32) -> u32 {
    // An odd affine permutation followed by invertible xor shifts is a
    // bijection on the power-of-two block domain: no single-block collisions.
    let mask = BLOCKS as u32 - 1;
    let mut x = logical
        .wrapping_mul(seed.wrapping_mul(2) | 1)
        .wrapping_add(seed.rotate_left(9))
        & mask;
    x ^= x >> 7;
    x = x.wrapping_mul(0x45d9f3b) & mask;
    (x ^ (x >> 11)) & mask
}

fn zero_run(run: Run, remap: impl FnOnce(Run) -> bool) {
    if !remap(run) {
        // SAFETY: the arena exclusively owns these writable blocks. No payload
        // access survives its drop under the holder contract. wasm and mmap
        // failures preserve the explicit-zero fallback.
        unsafe {
            std::ptr::write_bytes(run.address() as *mut u8, 0, run.bytes());
        }
    }
}

#[cfg(unix)]
fn remap_run(run: Run) -> bool {
    // SAFETY: only replace this arena's complete, aligned run, never metadata
    // or another live owner. The reservation keeps its address, with fresh zero
    // pages. One mmap per run replaces the old per-byte zeroing on success.
    let mapped = unsafe {
        libc::mmap(
            run.address() as *mut _,
            run.bytes(),
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_FIXED | libc::MAP_PRIVATE | libc::MAP_ANON,
            -1,
            0,
        )
    };
    if mapped == libc::MAP_FAILED {
        return false;
    }
    assert_eq!(
        mapped as usize,
        run.address(),
        "MAP_FIXED returned a different address"
    );
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn released_blocks_are_zero_before_reissue() {
        let allocator = BlockAllocator::global();
        let run = allocator.allocate(3, 101);
        // SAFETY: exclusively owned committed run, no typed views exist.
        unsafe {
            std::ptr::write_bytes(run.address() as *mut u8, 0xA7, run.bytes());
        }
        let first = run.first;
        allocator.release(run);
        assert_eq!(owner_of((first * BLOCK_BYTES / 8) as u32), None);
        let reused = allocator.allocate(3, 102);
        assert_eq!(reused.first, first);
        // SAFETY: complete reissued, initialized, still exclusively held run.
        assert!(unsafe {
            std::slice::from_raw_parts(reused.address() as *const u8, reused.bytes())
        }
        .iter()
        .all(|&b| b == 0));
        allocator.release(reused);
    }

    #[cfg(unix)]
    #[test]
    fn constant_addresses_and_remap_preserve_other_owners() {
        let allocator = BlockAllocator::global();
        let a = allocator.allocate(2, 110);
        let b = allocator.allocate(1, 111);
        assert_eq!(address((a.first * BLOCK_BYTES / 8) as u32), a.address());
        assert_eq!(owner_entry(0) as *const _ as usize, OWNER_BASE);
        #[cfg(debug_assertions)]
        assert_eq!(thread_entry(0) as *const _ as usize, THREAD_BASE);
        // SAFETY: distinct committed runs owned by this test.
        unsafe {
            std::ptr::write_bytes(a.address() as *mut u8, 0xA7, a.bytes());
            std::ptr::write_bytes(b.address() as *mut u8, 0xB8, b.bytes());
        }
        assert!(remap_run(a));
        // SAFETY: remapping retains a's address and does not touch b.
        unsafe {
            assert!(
                std::slice::from_raw_parts(a.address() as *const u8, a.bytes())
                    .iter()
                    .all(|&v| v == 0)
            );
            assert!(
                std::slice::from_raw_parts(b.address() as *const u8, b.bytes())
                    .iter()
                    .all(|&v| v == 0xB8)
            );
        }
        // Exercise the explicit-zero fallback without provoking OS exhaustion.
        zero_run(b, |_| false);
        assert!(
            unsafe { std::slice::from_raw_parts(b.address() as *const u8, b.bytes()) }
                .iter()
                .all(|&v| v == 0)
        );
        allocator.release(a);
        allocator.release(b);
    }

    #[cfg(unix)]
    #[test]
    fn reservation_collision_is_loud_and_preserves_mapping() {
        const PROBE: &str = "TSRS_ARENA_RESERVATION_COLLISION_PROBE";
        if std::env::var_os(PROBE).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "memory::tests::reservation_collision_is_loud_and_preserves_mapping",
                    "--test-threads=1",
                ])
                .env(PROBE, "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        // Fresh child process: occupy the requested base before reserve runs.
        // SAFETY: a non-destructive hint; verify placement before using it.
        let mapped = unsafe {
            libc::mmap(
                FIRST_REGION as *mut _,
                BLOCK_BYTES,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANON,
                -1,
                0,
            )
        };
        assert_eq!(mapped as usize, FIRST_REGION);
        unsafe {
            (mapped as *mut u64).write_volatile(0x12345678);
        }
        assert!(std::panic::catch_unwind(reserve).is_err());
        // A failed fixed reservation must not replace the pre-existing page.
        assert_eq!(
            unsafe { (mapped as *const u64).read_volatile() },
            0x12345678
        );
        assert_eq!(unsafe { libc::munmap(mapped, BLOCK_BYTES) }, 0);
    }
}

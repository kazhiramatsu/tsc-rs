//! A bounded Darwin arm64 frame-chain reader. Bounds are obtained OUTSIDE
//! allocator hooks. Capture calls no OS/unwind/allocator API, takes no lock,
//! and uses only supplied bounds plus a caller-owned 16-PC array.

pub(super) const DEPTH: usize = 16;
pub(super) const QUALIFIED: bool = cfg!(all(target_os = "macos", target_arch = "aarch64"));
pub(super) fn stack_bounds() -> (usize, usize) {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        // SAFETY: these Darwin APIs inspect the calling pthread's descriptor;
        // called before work, never from capture/the allocator hook.
        let (top, size) = unsafe {
            let thread = libc::pthread_self();
            (
                libc::pthread_get_stackaddr_np(thread) as usize,
                libc::pthread_get_stacksize_np(thread),
            )
        };
        (top.saturating_sub(size), top)
    }
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    {
        (0, 0)
    }
}

/// Every read is two machine words wholly within the current thread's stack.
/// A frame must be 16-byte aligned and strictly ascend; bad links terminate.
/// The chain is not trusted to terminate: the hard 16-step cap always applies.
///
/// SAFETY: [low, high) must be readable storage on this thread's live stack;
/// read must return the two words at a previously checked frame address.
#[cfg_attr(
    not(all(target_os = "macos", target_arch = "aarch64")),
    allow(dead_code)
)]
#[inline(always)]
unsafe fn walk(
    mut frame: usize,
    low: usize,
    high: usize,
    pcs: &mut [usize; DEPTH],
    mut read: impl FnMut(usize) -> (usize, usize),
) -> usize {
    let mut count = 0;
    for pc in pcs.iter_mut() {
        if frame == 0
            || frame & 15 != 0
            || frame < low
            || frame
                .checked_add(2 * std::mem::size_of::<usize>())
                .is_none_or(|end| end > high)
        {
            break;
        }
        let (next, address) = read(frame);
        let address = strip_pac(address);
        if address == 0 {
            break;
        }
        *pc = address;
        count += 1;
        if next <= frame {
            break;
        }
        frame = next;
    }
    count
}

#[inline(always)]
#[cfg_attr(
    not(all(target_os = "macos", target_arch = "aarch64")),
    allow(dead_code)
)]
fn strip_pac(mut address: usize) -> usize {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    // XPACLRI (HINT #7) strips instruction PAC from x30, including signed
    // arm64e system-frame LRs. It is a NOP on hardware without PAC. Declare
    // x30 clobbered so the compiler preserves this function's own return LR.
    // SAFETY: operates only on registers, without dereferencing the address.
    unsafe {
        std::arch::asm!("mov x30, {pc}", "hint #7", "mov {pc}, x30",
            pc = inout(reg) address, out("x30") _,
            options(nomem, nostack, preserves_flags));
    }
    // Keep the same signature on other architectures.
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    {
        let _ = &mut address;
    }
    address
}

#[inline(never)]
pub(super) fn capture(pcs: &mut [usize; DEPTH], bounds: (usize, usize)) -> usize {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        let (low, high) = bounds;
        let frame: usize;
        // SAFETY: read x29 without changing registers, stack or memory.
        unsafe {
            std::arch::asm!("mov {}, x29", out(reg) frame,
            options(nomem, nostack, preserves_flags));
        }
        // SAFETY: stack_bounds obtained this thread's stack bounds. Each read
        // is checked by walk before dereference. Older frames on this same
        // thread remain live until capture returns. No signal-stack walking.
        unsafe {
            walk(frame, low, high, pcs, |frame| {
                let words = frame as *const usize;
                (words.read(), words.add(1).read())
            })
        }
    }
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    {
        let _ = (pcs, bounds);
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_frames_are_never_read() {
        for (frame, low, high) in [
            (0, 0, 64),
            (17, 0, 64),
            (16, 32, 64),
            (64, 0, 64),
            (16, 0, 31),
            (usize::MAX - 15, 0, usize::MAX),
            (32, 64, 16),
        ] {
            let mut pcs = [0; DEPTH];
            // SAFETY: reader never dereferences, and must never be called.
            let n = unsafe { walk(frame, low, high, &mut pcs, |_| panic!("invalid read")) };
            assert_eq!(n, 0);
        }
    }
    #[test]
    fn monotonicity_and_depth_bound_all_reads() {
        for next in [0, 16, 8, 17, 0x10000] {
            let mut reads = 0;
            let mut pcs = [0; DEPTH];
            let n = unsafe {
                walk(16, 16, 0x10000, &mut pcs, |_| {
                    reads += 1;
                    (next, 0x1000)
                })
            };
            assert_eq!((n, reads), (1, 1));
        }
        let mut reads = 0;
        let mut pcs = [0; DEPTH];
        let n = unsafe {
            walk(16, 16, usize::MAX, &mut pcs, |p| {
                reads += 1;
                (p + 16, 0x2000)
            })
        };
        assert_eq!((n, reads), (DEPTH, DEPTH));
        assert!(pcs.iter().all(|&pc| pc == 0x2000));
        let n = unsafe { walk(16, 16, 32, &mut pcs, |_| (32, 0)) };
        assert_eq!(n, 0);
    }
    #[test]
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    fn main_and_worker_stacks_capture_without_allocator_operations() {
        fn check() {
            let bounds = stack_bounds();
            let mut pcs = [0; DEPTH];
            // Count this thread's allocator entries, including observer and
            // recursive buckets. There are no foreign calls in capture, so
            // C-runtime allocation is excluded structurally as well.
            let before = crate::alloc_census::hook_entries();
            for _ in 0..1000 {
                let count = capture(&mut pcs, bounds);
                assert!((1..=DEPTH).contains(&count));
                assert!(pcs[..count].iter().all(|&pc| pc > 0 && pc >> 48 == 0));
            }
            assert_eq!(before, crate::alloc_census::hook_entries());
        }
        check();
        std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(check)
            .unwrap()
            .join()
            .unwrap();
    }
    #[test]
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    fn strips_a_signed_instruction_return_address() {
        let original = capture as *const () as usize;
        let mut signed = original;
        // SAFETY: PACIASP/HINT #25 signs only the scratch LR; the declared
        // clobber makes the compiler preserve the test's actual return LR.
        // On pre-PAC hardware this is a NOP, as is XPACLRI.
        unsafe {
            std::arch::asm!("mov x30, {pc}", "hint #25", "mov {pc}, x30",
                pc = inout(reg) signed, out("x30") _,
                options(nomem, nostack, preserves_flags));
        }
        assert_eq!(strip_pac(signed), original);
    }
    #[test]
    fn unprepared_thread_returns_no_frames() {
        std::thread::spawn(|| {
            let mut pcs = [0; DEPTH];
            assert_eq!(capture(&mut pcs, (0, 0)), 0);
        })
        .join()
        .unwrap();
    }
}

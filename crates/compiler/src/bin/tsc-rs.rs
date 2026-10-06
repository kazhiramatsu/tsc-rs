/// The CLI binary uses mimalloc: the compiler allocates and frees many small
/// arena/table records, and the system allocator's free path dominated the
/// sampled self time. Library consumers keep their own global allocator.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// `mi_option_purge_delay` (mimalloc v2.3 `mi_option_e`, ordinal 15): memory
/// purging is delayed by N ms, -1 disables it. The bundled binding names the
/// neighbouring options (`mi_option_eager_commit_delay` = 14,
/// `mi_option_use_numa_nodes` = 16) but not this one.
const MI_OPTION_PURGE_DELAY: libmimalloc_sys::mi_option_t = 15;

/// Ask the OS to schedule the calling thread as interactive work. On macOS
/// that steers the thread to the performance cores for the rest of the
/// compile (a default-class thread may be placed on an efficiency core,
/// where the same parse, bind or check takes about twice as long); on other
/// platforms this is a no-op.
fn prefer_interactive_scheduling() {
    #[cfg(target_os = "macos")]
    // SAFETY: the call only changes the calling thread's QoS class and has
    // no other effect; a refused request leaves the class unchanged.
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0);
    }
}

/// The process memory `TSRS_PHASE_TRACE` prints, from mimalloc's accounting.
fn memory_sample() -> tsc_types::trace::MemorySample {
    let (mut elapsed, mut user, mut system, mut faults) = (0, 0, 0, 0);
    let mut sample = tsc_types::trace::MemorySample::default();
    // SAFETY: mi_process_info only writes the eight counters it is given.
    unsafe {
        libmimalloc_sys::mi_process_info(
            &mut elapsed,
            &mut user,
            &mut system,
            &mut sample.resident,
            &mut sample.peak_resident,
            &mut sample.committed,
            &mut sample.peak_committed,
            &mut faults,
        );
    }
    sample
}

fn main() {
    // Return freed pages to the OS 10 ms after they empty (mimalloc v2's
    // default, pinned here). A large program's arenas and tables outgrow
    // their buffers and leave the old ones free; keeping every freed page
    // mapped until exit added up to 7 % to the peak footprint (280 MiB on
    // VS Code, 40 MiB on the TypeScript compiler's full build), while
    // purging after this delay costs no measurable time.
    // SAFETY: mi_option_set only writes a process-global option value and
    // has no other preconditions.
    unsafe { libmimalloc_sys::mi_option_set(MI_OPTION_PURGE_DELAY, 10) };
    tsc_types::trace::set_memory_probe(memory_sample);
    // Every worker thread and checker shard starts with the same request;
    // the main thread reads, coordinates and runs the first shard itself.
    tsc_program::set_thread_start_hook(prefer_interactive_scheduling);
    prefer_interactive_scheduling();
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    // The command's work runs on a thread with the compiler's stack
    // reservation (`WORKER_STACK_BYTES`, the same as every worker and
    // checker shard). The compiler recurses in proportion to the nesting
    // depth of a source, and the main thread's stack (8 MiB on macOS and
    // Linux) is the one size the process cannot choose. A refused thread
    // leaves the work on the main thread.
    let output = std::thread::scope(|scope| {
        match std::thread::Builder::new()
            .name("tsc-rs-main".to_owned())
            .stack_size(tsc_program::WORKER_STACK_BYTES)
            .spawn_scoped(scope, || {
                tsc_program::run_thread_start_hook();
                tsc_compiler::run_cli(&arguments)
            }) {
            Ok(handle) => handle
                .join()
                .unwrap_or_else(|payload| std::panic::resume_unwind(payload)),
            Err(_) => tsc_compiler::run_cli(&arguments),
        }
    });
    {
        use std::io::Write;
        let mut stdout = std::io::stdout().lock();
        let _ = stdout.write_all(output.stdout().as_bytes());
        let _ = stdout.flush();
        let mut stderr = std::io::stderr().lock();
        let _ = stderr.write_all(output.stderr().as_bytes());
        let _ = stderr.flush();
    }
    // Leave without the C runtime's exit chain: the CLI keeps its arenas
    // until the end on purpose, and mimalloc's exit hook would otherwise walk
    // and unmap every segment it still holds (milliseconds for a large
    // Program) before the kernel reclaims the address space anyway. Every
    // artifact was written and closed by the sink; both streams are flushed
    // above.
    // SAFETY: _exit terminates the process immediately; nothing runs after
    // it, and no other thread holds work the process still needs.
    unsafe { libc::_exit(output.exit_code()) }
}

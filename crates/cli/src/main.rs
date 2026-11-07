/// The CLI binary uses mimalloc: the compiler allocates and frees many small
/// arena/table records, and the system allocator's free path dominated the
/// sampled self time. Library consumers keep their own global allocator; only
/// source texts go to mimalloc's text heap in any process
/// (`tsc_store::SharedText`).
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

/// tsgo `RunGC` (the language server's `custom/runGC`): mimalloc returns the
/// memory the process no longer uses, the source texts' heap included.
fn collect_memory() {
    // SAFETY: mi_collect only releases free memory mimalloc holds.
    unsafe { libmimalloc_sys::mi_collect(true) };
    tsc_store::shared_text::collect_text_heap();
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

/// Runs `work` on a thread with the compiler's stack reservation
/// (`WORKER_STACK_BYTES`, the same as every worker and checker shard). The
/// compiler recurses in proportion to the nesting depth of a source, and the
/// main thread's stack (8 MiB on macOS and Linux) is the one size the
/// process cannot choose. A refused thread leaves the work on the main
/// thread.
fn run_on_worker_stack<T: Send>(work: impl Fn() -> T + Sync) -> T {
    std::thread::scope(|scope| {
        match std::thread::Builder::new()
            .name("tsc-rs-main".to_owned())
            .stack_size(tsc_program::WORKER_STACK_BYTES)
            .spawn_scoped(scope, || {
                tsc_program::run_thread_start_hook();
                work()
            }) {
            Ok(handle) => handle
                .join()
                .unwrap_or_else(|payload| std::panic::resume_unwind(payload)),
            Err(_) => work(),
        }
    })
}

/// tsgo `isProcessAlive` (cmd/tsc/isprocessalive_unix.go): signal 0
/// reaches the process, or it exists but belongs to another user.
#[cfg(unix)]
fn is_process_alive(pid: i32) -> bool {
    // SAFETY: kill with signal 0 sends nothing; it only checks that the
    // process exists and may be signalled.
    if unsafe { libc::kill(pid, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// The parent process watchdog's check, where the platform has one.
#[cfg(unix)]
const PROCESS_ALIVE: Option<fn(i32) -> bool> = Some(is_process_alive);
#[cfg(not(unix))]
const PROCESS_ALIVE: Option<fn(i32) -> bool> = None;

fn main() {
    #[cfg(feature = "phase-ledger")]
    let mut ledger = phase_ledger::install(); // Before allocator/scheduling setup.
                                              // Return freed pages to the OS 10 ms after they empty (mimalloc v2's
                                              // default, pinned here). Large arenas and tables outgrow their buffers
                                              // and leave old ones free. Delayed purging returns those pages while
                                              // preserving the allocator's ordinary reuse window.
                                              // SAFETY: mi_option_set only writes a process-global option value and
                                              // has no other preconditions.
    unsafe { libmimalloc_sys::mi_option_set(MI_OPTION_PURGE_DELAY, 10) };
    tsc_types::trace::set_memory_probe(memory_sample);
    tsc_compiler::pprof::set_gc(collect_memory);
    // Every worker thread and checker shard starts with the same request;
    // the main thread reads, coordinates and runs the first shard itself.
    tsc_program::set_thread_start_hook(prefer_interactive_scheduling);
    prefer_interactive_scheduling();
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    #[cfg(feature = "phase-ledger")]
    phase_ledger::calibrate(ledger.as_mut()); // After ordinary scheduling setup.
    match arguments.first().map(String::as_str) {
        Some("--lsp") => {
            let code =
                run_on_worker_stack(|| tsc_lsp::cli::run_lsp(&arguments[1..], PROCESS_ALIVE));
            #[cfg(feature = "phase-ledger")]
            phase_ledger::finish(ledger);
            unsafe { libc::_exit(code) } // SAFETY: the server flushed its messages.
        }
        Some("--api") => {
            let code = run_on_worker_stack(|| tsc_api::server::run_api(&arguments[1..]));
            #[cfg(feature = "phase-ledger")]
            phase_ledger::finish(ledger);
            unsafe { libc::_exit(code) } // SAFETY: the server flushed its responses.
        }
        _ => {}
    }
    let output = run_on_worker_stack(|| tsc_compiler::run_cli(&arguments));
    {
        use std::io::Write;
        let mut stdout = std::io::stdout().lock();
        let _ = stdout.write_all(output.stdout().as_bytes());
        let _ = stdout.flush();
        let mut stderr = std::io::stderr().lock();
        let _ = stderr.write_all(output.stderr().as_bytes());
        let _ = stderr.flush();
    }
    #[cfg(feature = "phase-ledger")]
    phase_ledger::finish(ledger);
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        // PGO flushes through C runtime exit.
        std::process::exit(output.exit_code());
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

#[cfg(test)]
#[path = "../tests/unit/main/tests.rs"]
mod tests;

#[cfg(feature = "phase-ledger")]
mod phase_ledger;

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

fn main() {
    // A one-shot compile frees large arenas only at the end; returning their
    // pages to the OS while still running cost ~5 % of the sampled ticks in
    // madvise. Keep freed pages mapped until the process exits.
    // SAFETY: mi_option_set only writes a process-global option value and
    // has no other preconditions.
    unsafe { libmimalloc_sys::mi_option_set(MI_OPTION_PURGE_DELAY, -1) };
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let output = tsc_compiler::run_cli(&arguments);
    print!("{}", output.stdout());
    eprint!("{}", output.stderr());
    std::process::exit(output.exit_code());
}

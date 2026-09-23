//! Source read-ahead contract: the loader may read root sources ahead of its
//! sequential discovery only for hosts that declare
//! `CompilerHost::permits_source_read_ahead` AND a parallel `WorkerBudget`
//! in the load limits. A retained result (bytes, absence, error) stands in
//! for the host call at the root's original visit position; retained
//! payloads count against the source-count and byte limits together with the
//! admitted sources, a payload that does not fit is never retained (the visit
//! reads again), and retained payloads are evicted when an admitted
//! dependency would break that joint bound. Hosts that keep the trait
//! default, and serial budgets, observe the unchanged sequential read trace
//! and first-error precedence.
//!
//! Every test that exercises the parallel path pins an explicit budget of
//! four workers, and the serial control pins one, so the results do not
//! depend on the runner's CPU count.

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../host/tests/support/scalar_query_bridge.rs"
));

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use tsc_host::{CompilerHost, HostError, HostErrorKind, HostOperation, MemoryCompilerHost};
use tsc_program::{
    load_no_lib_program, plan_source_requests_retaining_syntax, CompilerOptions, PreparedProgram,
    PreparedSourceFile, ProgramLoadError, ProgramLoadLimit, ProgramLoadLimits, ProgramOptions,
    ProgramPath, WorkerBudget,
};

const GENEROUS_LIMIT: usize = 1_024;

/// A memory-backed host that records every `read_file` call in order, can
/// answer a queued fault on the n-th read of a path, and reports the
/// read-ahead capability the test selects. With `read_ahead == false` it is
/// exactly the kind of order-observing host the trait default protects.
struct TracedHost {
    inner: MemoryCompilerHost,
    read_ahead: bool,
    reads: RefCell<Vec<String>>,
    faults: RefCell<BTreeMap<PathBuf, VecDeque<HostError>>>,
}

impl TracedHost {
    fn new(inner: MemoryCompilerHost, read_ahead: bool) -> Self {
        Self {
            inner,
            read_ahead,
            reads: RefCell::new(Vec::new()),
            faults: RefCell::new(BTreeMap::new()),
        }
    }

    /// Fail the next read of `path` with `error`; later reads succeed.
    fn fault_next_read(self, path: &str, error: HostError) -> Self {
        self.faults
            .borrow_mut()
            .entry(PathBuf::from(path))
            .or_default()
            .push_back(error);
        self
    }

    fn reads(&self) -> Vec<String> {
        self.reads.borrow().clone()
    }
}

impl CompilerHost for TracedHost {
    scalar_host_query_bridge!();

    fn current_directory(&self) -> Result<PathBuf, HostError> {
        self.inner.current_directory()
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.inner.use_case_sensitive_file_names()
    }

    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError> {
        self.reads
            .borrow_mut()
            .push(path.to_string_lossy().into_owned());
        if let Some(queued) = self.faults.borrow_mut().get_mut(path) {
            if let Some(error) = queued.pop_front() {
                return Err(error);
            }
        }
        self.inner.read_file(path)
    }

    fn file_exists(&self, path: &Path) -> Result<bool, HostError> {
        self.inner.file_exists(path)
    }

    fn directory_exists(&self, path: &Path) -> Result<bool, HostError> {
        self.inner.directory_exists(path)
    }

    fn read_directory(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        self.inner.read_directory(path)
    }

    fn realpath(&self, path: &Path) -> Result<Option<PathBuf>, HostError> {
        self.inner.realpath(path)
    }

    fn permits_source_read_ahead(&self) -> bool {
        self.read_ahead
    }
}

fn compiler_options() -> CompilerOptions {
    CompilerOptions {
        no_emit: Some(true),
        ..CompilerOptions::default()
    }
}

fn program_options() -> ProgramOptions {
    ProgramOptions::default()
        .with_no_lib(true)
        .with_types(Vec::new())
}

/// Four workers: the explicit parallel path, independent of the runner.
fn parallel() -> WorkerBudget {
    WorkerBudget::new(NonZeroUsize::new(4).expect("nonzero"))
}

fn limits(
    max_source_files: usize,
    max_source_file_bytes: usize,
    max_total_source_bytes: usize,
    workers: WorkerBudget,
) -> ProgramLoadLimits {
    ProgramLoadLimits::new(
        max_source_files,
        GENEROUS_LIMIT,
        GENEROUS_LIMIT,
        max_source_file_bytes,
        max_total_source_bytes,
    )
    .with_workers(workers)
}

/// Generous limits with the explicit four-worker budget.
fn parallel_limits() -> ProgramLoadLimits {
    limits(GENEROUS_LIMIT, GENEROUS_LIMIT, GENEROUS_LIMIT, parallel())
}

/// Generous limits with the serial budget: the exact pre-concurrency loader.
fn serial_limits() -> ProgramLoadLimits {
    limits(
        GENEROUS_LIMIT,
        GENEROUS_LIMIT,
        GENEROUS_LIMIT,
        WorkerBudget::serial(),
    )
}

fn load(
    host: &dyn CompilerHost,
    roots: &[&str],
    options: CompilerOptions,
    limits: ProgramLoadLimits,
) -> Result<PreparedProgram, ProgramLoadError> {
    let roots = roots.iter().map(PathBuf::from).collect::<Vec<_>>();
    load_no_lib_program(host, &roots, options, program_options(), limits)
}

fn source_paths(program: &PreparedProgram) -> Vec<String> {
    program
        .source_files()
        .iter()
        .map(|source| {
            source
                .path()
                .display()
                .as_str()
                .expect("scalar test path")
                .to_owned()
        })
        .collect()
}

fn read_error(path: &str) -> HostError {
    HostError::new(
        HostErrorKind::PermissionDenied,
        HostOperation::ReadFile,
        Some(PathBuf::from(path)),
        "transient read failure",
    )
}

const A_IMPORTS_DEP: &[u8] = b"import './dep';\nexport const a = 1;\n";
const DEP: &[u8] = b"export const dep = 1;\n";
const B: &[u8] = b"export const b = 1;\n";

/// `a.ts` imports `dep.ts`; `b.ts` is an independent second root.
fn dependency_host() -> MemoryCompilerHost {
    MemoryCompilerHost::builder("/work")
        .file("/work/a.ts", A_IMPORTS_DEP.to_vec())
        .file("/work/dep.ts", DEP.to_vec())
        .file("/work/b.ts", B.to_vec())
        .build()
        .expect("build dependency host")
}

#[test]
fn order_observing_hosts_keep_the_sequential_read_trace_and_first_error() {
    // Success: dependencies interleave with roots, exactly as tsc reads them,
    // even under a parallel budget, because the host keeps the default.
    let host = TracedHost::new(dependency_host(), false);
    let program = load(
        &host,
        &["/work/a.ts", "/work/b.ts"],
        compiler_options(),
        parallel_limits(),
    )
    .expect("sequential load succeeds");
    assert_eq!(host.reads(), ["/work/a.ts", "/work/dep.ts", "/work/b.ts"]);
    assert_eq!(
        source_paths(&program),
        ["/work/dep.ts", "/work/a.ts", "/work/b.ts"]
    );

    // First error wins and nothing after it is read: b.ts is never touched
    // even though a second read of a.ts would have succeeded.
    let host = TracedHost::new(dependency_host(), false)
        .fault_next_read("/work/a.ts", read_error("/work/a.ts"));
    let error = load(
        &host,
        &["/work/a.ts", "/work/b.ts"],
        compiler_options(),
        parallel_limits(),
    )
    .expect_err("the first root read fails");
    assert_eq!(host.reads(), ["/work/a.ts"]);
    assert_eq!(error.path(), Some(Path::new("/work/a.ts")));
    let ProgramLoadError::Host { source, .. } = &error else {
        panic!("expected the host read error, got {error:?}");
    };
    assert_eq!(source.as_ref(), &read_error("/work/a.ts"));
}

#[test]
fn serial_budget_keeps_the_sequential_trace_for_read_ahead_hosts() {
    // The serial control: a host that permits read-ahead still sees the
    // sequential trace when the caller's budget is serial, and the program
    // equals the parallel load's.
    let serial_host = TracedHost::new(dependency_host(), true);
    let serial = load(
        &serial_host,
        &["/work/a.ts", "/work/b.ts"],
        compiler_options(),
        serial_limits(),
    )
    .expect("serial load succeeds");
    assert_eq!(
        serial_host.reads(),
        ["/work/a.ts", "/work/dep.ts", "/work/b.ts"]
    );

    let parallel_host = TracedHost::new(dependency_host(), true);
    let parallel = load(
        &parallel_host,
        &["/work/a.ts", "/work/b.ts"],
        compiler_options(),
        parallel_limits(),
    )
    .expect("parallel load succeeds");
    assert_eq!(
        parallel_host.reads(),
        ["/work/a.ts", "/work/b.ts", "/work/dep.ts"]
    );
    assert_eq!(serial, parallel);
    for program in [&serial, &parallel] {
        assert!(program
            .source_files()
            .iter()
            .all(|source| source.preparsed_syntax().is_available()));
    }
}

#[test]
fn read_ahead_hosts_retain_the_first_error_instead_of_retrying() {
    let sequential_host = TracedHost::new(dependency_host(), false)
        .fault_next_read("/work/a.ts", read_error("/work/a.ts"));
    let sequential = load(
        &sequential_host,
        &["/work/a.ts", "/work/b.ts"],
        compiler_options(),
        parallel_limits(),
    )
    .expect_err("sequential load reports the read error");

    let host = TracedHost::new(dependency_host(), true)
        .fault_next_read("/work/a.ts", read_error("/work/a.ts"));
    let error = load(
        &host,
        &["/work/a.ts", "/work/b.ts"],
        compiler_options(),
        parallel_limits(),
    )
    .expect_err("the retained read error is reported at a.ts's visit");
    // Read-ahead stops at the failed root: b.ts is never read, the failed
    // read of a.ts is retained and reported at its visit, never repeated, so
    // the queued second (valid) answer is never consumed and dep.ts is never
    // requested.
    assert_eq!(host.reads(), ["/work/a.ts"]);
    assert_eq!(error, sequential);
}

#[test]
fn read_ahead_programs_equal_sequential_programs() {
    let sequential_host = TracedHost::new(dependency_host(), false);
    let sequential = load(
        &sequential_host,
        &["/work/a.ts", "/work/b.ts"],
        compiler_options(),
        parallel_limits(),
    )
    .expect("sequential load succeeds");

    let host = TracedHost::new(dependency_host(), true);
    let program = load(
        &host,
        &["/work/a.ts", "/work/b.ts"],
        compiler_options(),
        parallel_limits(),
    )
    .expect("read-ahead load succeeds");
    // Roots are read ahead in root order; the dependency is discovered by the
    // sequential walk as before. Every file is read exactly once.
    assert_eq!(host.reads(), ["/work/a.ts", "/work/b.ts", "/work/dep.ts"]);
    assert_eq!(
        source_paths(&program),
        ["/work/dep.ts", "/work/a.ts", "/work/b.ts"]
    );
    assert_eq!(program, sequential);
    assert!(program
        .source_files()
        .iter()
        .all(|source| source.preparsed_syntax().is_available()));
}

/// Eighty small roots (`export const`), every eighth importing its successor.
fn many_root_host() -> (MemoryCompilerHost, Vec<String>) {
    let mut builder = MemoryCompilerHost::builder("/work");
    let mut roots = Vec::new();
    for index in 0..tsc_program::PARALLEL_READ_AHEAD_MIN_ROOTS + 16 {
        let path = format!("/work/f{index}.ts");
        let text = if index % 8 == 0 {
            format!(
                "import './f{}';\nexport const v{index} = {index};\n",
                index + 1
            )
        } else {
            format!("export const v{index} = {index};\n")
        };
        builder = builder.file(path.clone(), text.into_bytes());
        roots.push(path);
    }
    (builder.build().expect("build many-root host"), roots)
}

#[test]
fn many_roots_read_on_the_workers_load_the_sequential_program() {
    // From `PARALLEL_READ_AHEAD_MIN_ROOTS` roots on, a host with a parallel
    // source reader (the immutable memory host) has the parse workers read
    // the roots; the traced wrapper offers no such reader, so the loading
    // thread reads them in root order as before. Both loads must produce
    // the same program, and under a source-count bound below the root
    // count the same failure.
    let (host, roots) = many_root_host();
    let (traced_inner, _) = many_root_host();
    let traced = TracedHost::new(traced_inner, true);
    let roots = roots.iter().map(String::as_str).collect::<Vec<_>>();
    assert!(roots.len() >= tsc_program::PARALLEL_READ_AHEAD_MIN_ROOTS);
    let generous = || limits(GENEROUS_LIMIT, GENEROUS_LIMIT, 1 << 20, parallel());
    let parallel_program = load(&host, &roots, compiler_options(), generous())
        .expect("parallel read-ahead load succeeds");
    let sequential_program = load(&traced, &roots, compiler_options(), generous())
        .expect("sequential read-ahead load succeeds");
    assert_eq!(parallel_program, sequential_program);
    assert_eq!(traced.reads().len(), roots.len());
    assert!(parallel_program
        .source_files()
        .iter()
        .all(|source| source.preparsed_syntax().is_available()));

    let bounded = || limits(roots.len() / 2, GENEROUS_LIMIT, 1 << 20, parallel());
    let parallel_failure = load(&host, &roots, compiler_options(), bounded());
    let sequential_failure = load(&traced, &roots, compiler_options(), bounded());
    assert!(parallel_failure.is_err());
    assert_eq!(
        format!("{parallel_failure:?}"),
        format!("{sequential_failure:?}")
    );
}

#[test]
fn read_ahead_root_reached_first_as_a_dependency_is_not_re_read() {
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/a.ts",
            b"import './b';\nexport const a = 1;\n".to_vec(),
        )
        .file("/work/b.ts", B.to_vec())
        .build()
        .expect("build host");
    let sequential_host = TracedHost::new(host.clone(), false);
    let sequential = load(
        &sequential_host,
        &["/work/a.ts", "/work/b.ts"],
        compiler_options(),
        parallel_limits(),
    )
    .expect("sequential load succeeds");
    assert_eq!(sequential_host.reads(), ["/work/a.ts", "/work/b.ts"]);

    let read_ahead_host = TracedHost::new(host, true);
    let program = load(
        &read_ahead_host,
        &["/work/a.ts", "/work/b.ts"],
        compiler_options(),
        parallel_limits(),
    )
    .expect("read-ahead load succeeds");
    assert_eq!(read_ahead_host.reads(), ["/work/a.ts", "/work/b.ts"]);
    assert_eq!(source_paths(&program), ["/work/b.ts", "/work/a.ts"]);
    assert_eq!(program, sequential);
}

#[test]
fn read_ahead_leaves_skipped_missing_and_json_roots_on_the_sequential_path() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/notes.txt", b"not a source".to_vec())
        .file("/work/data.json", br#"{"value":1}"#.to_vec())
        .file("/work/a.ts", b"export const a = 1;\n".to_vec())
        .file("/work/b.ts", B.to_vec())
        .failure(HostError::new(
            HostErrorKind::Other,
            HostOperation::ReadFile,
            Some(PathBuf::from("/work/notes.txt")),
            "unsupported roots must be gated before readFile",
        ))
        .build()
        .expect("build host");
    let options = CompilerOptions {
        module: Some(1),
        module_resolution: Some(2),
        resolve_json_module: Some(true),
        ..compiler_options()
    };
    let roots = [
        "/work/notes.txt",
        "/work/missing.ts",
        "/work/data.json",
        "/work/a.ts",
        "/work/b.ts",
    ];

    let sequential_host = TracedHost::new(host.clone(), false);
    let sequential = load(&sequential_host, &roots, options.clone(), parallel_limits())
        .expect("sequential load succeeds");
    assert_eq!(
        sequential_host.reads(),
        [
            "/work/missing.ts",
            "/work/data.json",
            "/work/a.ts",
            "/work/b.ts"
        ]
    );

    let read_ahead_host = TracedHost::new(host, true);
    let program = load(&read_ahead_host, &roots, options, parallel_limits())
        .expect("read-ahead load succeeds");
    // The unsupported root is never read on either path; the JSON root is
    // read at its visit; the missing root's absence was retained.
    assert_eq!(
        read_ahead_host.reads(),
        [
            "/work/missing.ts",
            "/work/a.ts",
            "/work/b.ts",
            "/work/data.json"
        ]
    );
    assert_eq!(program, sequential);
    assert_eq!(
        program
            .diagnostics()
            .program()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        sequential
            .diagnostics()
            .program()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>()
    );
    assert_eq!(program.roots().len(), 5);
    assert!(program.roots()[0].source().is_none());
    assert!(program.roots()[1].source().is_none());
}

/// Three independent ten-byte roots for the limit cases.
fn ten_byte_roots_host() -> MemoryCompilerHost {
    let ten_bytes = b"let x = 1;".to_vec();
    assert_eq!(ten_bytes.len(), 10);
    MemoryCompilerHost::builder("/work")
        .file("/work/a.ts", ten_bytes.clone())
        .file("/work/b.ts", ten_bytes.clone())
        .file("/work/c.ts", ten_bytes)
        .build()
        .expect("build host")
}

#[test]
fn read_ahead_never_retains_a_payload_outside_the_load_limits() {
    let host = ten_byte_roots_host();
    let roots = ["/work/a.ts", "/work/b.ts", "/work/c.ts"];

    // Total-byte budget 15: a.ts (10) is retained; b.ts would bring the
    // retained total to 20, so its payload is dropped after the host's
    // one-call allocation, read-ahead stops, and the visit reads b.ts again
    // and rejects it with the sequential observed value. c.ts is never read.
    let sequential_host = TracedHost::new(host.clone(), false);
    let sequential = load(
        &sequential_host,
        &roots,
        compiler_options(),
        limits(GENEROUS_LIMIT, GENEROUS_LIMIT, 15, parallel()),
    )
    .expect_err("total byte limit");
    assert_eq!(sequential_host.reads(), ["/work/a.ts", "/work/b.ts"]);
    let read_ahead_host = TracedHost::new(host.clone(), true);
    let error = load(
        &read_ahead_host,
        &roots,
        compiler_options(),
        limits(GENEROUS_LIMIT, GENEROUS_LIMIT, 15, parallel()),
    )
    .expect_err("total byte limit");
    assert_eq!(
        read_ahead_host.reads(),
        ["/work/a.ts", "/work/b.ts", "/work/b.ts"]
    );
    assert_eq!(error, sequential);
    let exceeded = error.limit_exceeded().expect("limit evidence");
    assert_eq!(exceeded.limit(), ProgramLoadLimit::TotalSourceBytes);
    assert_eq!(exceeded.path(), Some(Path::new("/work/b.ts")));
    assert_eq!(exceeded.observed(), 20);

    // Source-count budget 1: read-ahead retains a.ts and stops before reading
    // the root that would exceed the joint count; the visit reads b.ts and
    // rejects it exactly as the sequential walk does.
    let sequential_host = TracedHost::new(host.clone(), false);
    let sequential = load(
        &sequential_host,
        &roots,
        compiler_options(),
        limits(1, GENEROUS_LIMIT, GENEROUS_LIMIT, parallel()),
    )
    .expect_err("source count limit");
    let read_ahead_host = TracedHost::new(host.clone(), true);
    let error = load(
        &read_ahead_host,
        &roots,
        compiler_options(),
        limits(1, GENEROUS_LIMIT, GENEROUS_LIMIT, parallel()),
    )
    .expect_err("source count limit");
    assert_eq!(sequential_host.reads(), ["/work/a.ts", "/work/b.ts"]);
    assert_eq!(read_ahead_host.reads(), ["/work/a.ts", "/work/b.ts"]);
    assert_eq!(error, sequential);
    let exceeded = error.limit_exceeded().expect("limit evidence");
    assert_eq!(exceeded.limit(), ProgramLoadLimit::SourceFiles);
    assert_eq!(exceeded.observed(), 2);

    // Per-file budget 5: the oversized root's payload is never retained
    // (its size is unknown until read); read-ahead stops, and the visit
    // reads a.ts again and reports the sequential limit error.
    let sequential_host = TracedHost::new(host.clone(), false);
    let sequential = load(
        &sequential_host,
        &roots,
        compiler_options(),
        limits(GENEROUS_LIMIT, 5, GENEROUS_LIMIT, parallel()),
    )
    .expect_err("per-file byte limit");
    let read_ahead_host = TracedHost::new(host, true);
    let error = load(
        &read_ahead_host,
        &roots,
        compiler_options(),
        limits(GENEROUS_LIMIT, 5, GENEROUS_LIMIT, parallel()),
    )
    .expect_err("per-file byte limit");
    assert_eq!(sequential_host.reads(), ["/work/a.ts"]);
    assert_eq!(read_ahead_host.reads(), ["/work/a.ts", "/work/a.ts"]);
    assert_eq!(error, sequential);
    let exceeded = error.limit_exceeded().expect("limit evidence");
    assert_eq!(exceeded.limit(), ProgramLoadLimit::SourceFileBytes);
    assert_eq!(exceeded.path(), Some(Path::new("/work/a.ts")));
    assert_eq!(exceeded.observed(), 10);
}

/// The integrator's F8 counterexample: `max_source_files = 2`, roots `a.ts`
/// and `b.ts`, `a.ts` imports `dep.ts`. Read-ahead retains both roots (two
/// of two); when the walk admits `dep.ts` the joint bound would reach three,
/// so the retained `b.ts` payload is evicted before `dep.ts` is decoded, and
/// `b.ts` is read again at its visit, where the sequential count limit
/// rejects it with the sequential observed value.
#[test]
fn admitted_dependencies_evict_retained_payloads_under_the_source_count_bound() {
    let sequential_host = TracedHost::new(dependency_host(), false);
    let sequential = load(
        &sequential_host,
        &["/work/a.ts", "/work/b.ts"],
        compiler_options(),
        limits(2, GENEROUS_LIMIT, GENEROUS_LIMIT, parallel()),
    )
    .expect_err("three sources under a two-source limit");
    assert_eq!(
        sequential_host.reads(),
        ["/work/a.ts", "/work/dep.ts", "/work/b.ts"]
    );

    let read_ahead_host = TracedHost::new(dependency_host(), true);
    let error = load(
        &read_ahead_host,
        &["/work/a.ts", "/work/b.ts"],
        compiler_options(),
        limits(2, GENEROUS_LIMIT, GENEROUS_LIMIT, parallel()),
    )
    .expect_err("three sources under a two-source limit");
    assert_eq!(
        read_ahead_host.reads(),
        ["/work/a.ts", "/work/b.ts", "/work/dep.ts", "/work/b.ts"]
    );
    assert_eq!(error, sequential);
    let exceeded = error.limit_exceeded().expect("limit evidence");
    assert_eq!(exceeded.limit(), ProgramLoadLimit::SourceFiles);
    assert_eq!(exceeded.path(), Some(Path::new("/work/b.ts")));
    assert_eq!(exceeded.observed(), 3);
}

/// The byte analogue: the total budget admits `a + b` and `a + dep` but not
/// all three, so the retained `b.ts` payload is evicted when `dep.ts` is
/// admitted and the walk fails at `b.ts` with the sequential total.
#[test]
fn admitted_dependencies_evict_retained_payloads_under_the_byte_bound() {
    let total = A_IMPORTS_DEP.len() + DEP.len() + B.len();
    let budget = total - 1;
    assert!(A_IMPORTS_DEP.len() + B.len() <= budget);
    assert!(A_IMPORTS_DEP.len() + DEP.len() <= budget);

    let sequential_host = TracedHost::new(dependency_host(), false);
    let sequential = load(
        &sequential_host,
        &["/work/a.ts", "/work/b.ts"],
        compiler_options(),
        limits(GENEROUS_LIMIT, GENEROUS_LIMIT, budget, parallel()),
    )
    .expect_err("total bytes exceed the budget at b.ts");
    assert_eq!(
        sequential_host.reads(),
        ["/work/a.ts", "/work/dep.ts", "/work/b.ts"]
    );

    let read_ahead_host = TracedHost::new(dependency_host(), true);
    let error = load(
        &read_ahead_host,
        &["/work/a.ts", "/work/b.ts"],
        compiler_options(),
        limits(GENEROUS_LIMIT, GENEROUS_LIMIT, budget, parallel()),
    )
    .expect_err("total bytes exceed the budget at b.ts");
    assert_eq!(
        read_ahead_host.reads(),
        ["/work/a.ts", "/work/b.ts", "/work/dep.ts", "/work/b.ts"]
    );
    assert_eq!(error, sequential);
    let exceeded = error.limit_exceeded().expect("limit evidence");
    assert_eq!(exceeded.limit(), ProgramLoadLimit::TotalSourceBytes);
    assert_eq!(exceeded.path(), Some(Path::new("/work/b.ts")));
    assert_eq!(exceeded.observed(), total);
}

#[test]
fn read_ahead_is_inert_for_a_single_root() {
    let host = TracedHost::new(dependency_host(), true);
    let program = load(
        &host,
        &["/work/a.ts"],
        compiler_options(),
        parallel_limits(),
    )
    .expect("single-root load succeeds");
    assert_eq!(host.reads(), ["/work/a.ts", "/work/dep.ts"]);
    assert_eq!(source_paths(&program), ["/work/dep.ts", "/work/a.ts"]);
}

#[test]
fn immutable_memory_host_declares_read_ahead_and_matches_a_traced_sequential_load() {
    let host = dependency_host();
    assert!(host.permits_source_read_ahead());
    let program = load(
        &host,
        &["/work/a.ts", "/work/b.ts"],
        compiler_options(),
        parallel_limits(),
    )
    .expect("memory host load succeeds");
    let sequential_host = TracedHost::new(dependency_host(), false);
    let sequential = load(
        &sequential_host,
        &["/work/a.ts", "/work/b.ts"],
        compiler_options(),
        parallel_limits(),
    )
    .expect("sequential load succeeds");
    assert_eq!(program, sequential);
}

/// Deep-input control for the worker stack budget: a nested array literal
/// and a long binary chain load identically through the read-ahead workers
/// and through the sequential (calling-thread) path. This exercises the
/// stated 16 MiB worker budget against an input the calling thread handles;
/// it is a control, not a proof of a universal depth bound.
#[test]
fn deep_inputs_load_identically_through_read_ahead_workers() {
    const NESTING: usize = 64;
    const CHAIN_TERMS: usize = 1_000;
    let mut deep = String::new();
    deep.push_str("export const nested = ");
    deep.push_str(&"[".repeat(NESTING));
    deep.push('1');
    deep.push_str(&"]".repeat(NESTING));
    deep.push_str(";\nexport const chain = ");
    deep.push_str(&"1 + ".repeat(CHAIN_TERMS - 1));
    deep.push_str("1;\n");
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/deep.ts", deep.into_bytes())
        .file("/work/b.ts", B.to_vec())
        .build()
        .expect("build host");
    let generous = limits(GENEROUS_LIMIT, 1 << 20, 1 << 20, parallel());
    let serial = limits(GENEROUS_LIMIT, 1 << 20, 1 << 20, WorkerBudget::serial());

    let sequential_host = TracedHost::new(host.clone(), true);
    let sequential = load(
        &sequential_host,
        &["/work/deep.ts", "/work/b.ts"],
        compiler_options(),
        serial,
    )
    .expect("sequential load succeeds");
    assert_eq!(sequential_host.reads(), ["/work/deep.ts", "/work/b.ts"]);

    let read_ahead_host = TracedHost::new(host, true);
    let program = load(
        &read_ahead_host,
        &["/work/deep.ts", "/work/b.ts"],
        compiler_options(),
        generous,
    )
    .expect("read-ahead load succeeds");
    assert_eq!(read_ahead_host.reads(), ["/work/deep.ts", "/work/b.ts"]);
    assert_eq!(program, sequential);
    assert!(program.diagnostics().program().is_empty());
}

#[test]
fn prepared_source_equality_ignores_the_preparsed_slot_in_every_state() {
    let path = ProgramPath::from_trusted_parts("/work/a.ts", "/work/a.ts").expect("trusted path");
    let text = "export const a = 1;\n";
    let absent = PreparedSourceFile::new(path.clone(), text);
    let (_plan, syntax) =
        plan_source_requests_retaining_syntax(&absent, &compiler_options()).expect("plan");
    // The planner parsed `absent`'s own snapshot, so the cache attaches to a
    // clone of that prepared source (same snapshot), as the loader does.
    let available = absent.clone().with_preparsed_syntax(syntax);
    assert!(!absent.preparsed_syntax().is_available());
    assert!(available.preparsed_syntax().is_available());
    assert_eq!(
        absent, available,
        "an attached cache is not program content"
    );

    // Clones share the take-once slot.
    let shared = available.clone();
    let taken = available
        .preparsed_syntax()
        .take()
        .expect("first take yields the planner's parse");
    assert_eq!(taken.parse_options().node_id_base, 0);
    assert_eq!(taken.parse_options().node_array_id_base, 0);
    assert_eq!(taken.source().file_name.as_str(), Some("/work/a.ts"));
    assert!(std::sync::Arc::ptr_eq(
        taken.source().snapshot(),
        available.snapshot()
    ));
    assert!(std::sync::Arc::ptr_eq(
        taken.source().snapshot(),
        absent.snapshot()
    ));
    assert!(!available.preparsed_syntax().is_available());
    assert!(!shared.preparsed_syntax().is_available());
    assert!(shared.preparsed_syntax().take().is_none());
    assert_eq!(absent, available, "a consumed cache is not program content");
    assert_eq!(available, shared);

    // Equal text on a distinct snapshot is still an equal prepared source;
    // the cache attached to one of them changes nothing. A different snapshot
    // is not adoptable, which the checker verifies by Arc identity, but that
    // is an adoption fact, not content equality.
    let same_text_other_snapshot = PreparedSourceFile::new(path.clone(), text);
    assert!(!std::sync::Arc::ptr_eq(
        same_text_other_snapshot.snapshot(),
        absent.snapshot()
    ));
    assert_eq!(same_text_other_snapshot, absent);
    let (_plan, other_syntax) =
        plan_source_requests_retaining_syntax(&same_text_other_snapshot, &compiler_options())
            .expect("plan");
    assert_eq!(
        same_text_other_snapshot
            .clone()
            .with_preparsed_syntax(other_syntax),
        available
    );

    // A distinct text is still a distinct prepared source.
    let other = PreparedSourceFile::new(path, "export const a = 2;\n");
    assert_ne!(absent, other);
}

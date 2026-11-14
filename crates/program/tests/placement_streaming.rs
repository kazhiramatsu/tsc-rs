include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../host/tests/support/scalar_query_bridge.rs"
));

use std::{
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::{Condvar, Mutex},
    time::Duration,
};
use tsc_diagnostics::JsStr;
use tsc_host::{CompilerHost, HostError, MemoryCompilerHost, ParallelSourceReader};
use tsc_program::{
    load_no_lib_program, load_program, CompilerOptions, LibraryCatalog, ProgramLoadLimits,
    ProgramOptions, WorkerBudget,
};

// The layout comparison needs isolated commands: domains share the
// process-wide stores' collision-safe numeric allocator.
static COMMAND: Mutex<()> = Mutex::new(());

struct State {
    completed: Vec<usize>,
    early_parsed: bool,
    libraries_read: usize,
}
struct StreamingHost {
    inner: MemoryCompilerHost,
    scramble: bool,
    observe: bool,
    state: Mutex<State>,
    changed: Condvar,
}
impl CompilerHost for StreamingHost {
    scalar_host_query_bridge!();
    fn current_directory(&self) -> Result<PathBuf, HostError> {
        self.inner.current_directory()
    }
    fn use_case_sensitive_file_names(&self) -> bool {
        true
    }
    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError> {
        let name = path.to_str().unwrap();
        if let Some(text) = library(name) {
            self.state.lock().unwrap().libraries_read += 1;
            return Ok(Some(text.as_bytes().to_vec()));
        }
        if let Some(index) = name
            .strip_prefix("/p/r")
            .and_then(|s| s.strip_suffix(".ts"))
            .filter(|_| self.observe)
        {
            let index: usize = index.parse().unwrap();
            let mut state = self.state.lock().unwrap();
            assert!(
                state.libraries_read >= 2,
                "config closure precedes root reads"
            );
            // The first three reads must complete 2, 1, 0, even though
            // admission and leases must still be in root order.
            if self.scramble && index < 2 {
                let (next, timeout) = self
                    .changed
                    .wait_timeout_while(state, Duration::from_secs(10), |s| {
                        !s.completed.contains(&(index + 1))
                    })
                    .unwrap();
                state = next;
                assert!(!timeout.timed_out(), "scrambled readers made no progress");
            }
            if index == 16 {
                // This read can finish only after stripe zero parsed and
                // resolved its import. A read-all-roots barrier fails here.
                let (next, timeout) = self
                    .changed
                    .wait_timeout_while(state, Duration::from_secs(10), |s| !s.early_parsed)
                    .unwrap();
                state = next;
                assert!(
                    !timeout.timed_out(),
                    "a later read blocked the first stripe's parse"
                );
            }
            state.completed.push(index);
            self.changed.notify_all();
        }
        self.inner.read_file(path)
    }
    fn file_exists(&self, path: &Path) -> Result<bool, HostError> {
        if path == Path::new("/p/early-dep.ts") {
            self.state.lock().unwrap().early_parsed = true;
            self.changed.notify_all();
        }
        if library(path.to_str().unwrap()).is_some() {
            return Ok(true);
        }
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
        true
    }
    fn parallel_source_reader(&self) -> Option<&(dyn ParallelSourceReader + Sync)> {
        Some(self)
    }
    fn parallel_resolution_host(&self) -> Option<&(dyn CompilerHost + Sync)> {
        Some(self)
    }
}
impl ParallelSourceReader for StreamingHost {
    fn read_source_js(&self, path: JsStr<'_>) -> Result<Option<Vec<u8>>, HostError> {
        self.read_file_js(path)
    }
}
fn library(name: &str) -> Option<&'static str> {
    match name {
        "bundled:///libs/lib.es5.d.ts" => {
            Some("/// <reference lib=\"es2015.core\"/>\ninterface Base {}")
        }
        "bundled:///libs/lib.es2015.core.d.ts" => Some("interface Core {}"),
        "bundled:///libs/lib.es2015.promise.d.ts" => Some("interface PromiseOnly {}"),
        _ => None,
    }
}

#[test]
fn task_binds_travel_with_the_parse_only_when_the_consumer_uses_them() {
    let _command = COMMAND.lock().unwrap();
    for workers in [1, 4, 16] {
        for (demand, list_only) in [(false, false), (true, false), (true, true)] {
            let host = StreamingHost {
                inner: MemoryCompilerHost::builder("/p")
                    .file(
                        "/p/root.ts",
                        b"/// <reference lib=\"es2015.promise\"/>\nexport const root = 1;".to_vec(),
                    )
                    .build()
                    .unwrap(),
                scramble: false,
                observe: false,
                state: Mutex::new(State {
                    completed: Vec::new(),
                    early_parsed: false,
                    libraries_read: 0,
                }),
                changed: Condvar::new(),
            };
            let domain = tsc_types::IdentityDomain::new();
            let mut program_options = ProgramOptions::default()
                .with_types(Vec::new())
                .with_identity_domain(domain.clone());
            if demand {
                program_options = program_options.with_source_binding();
            }
            let program = load_program(
                &host,
                &["/p/root.ts".into()],
                CompilerOptions {
                    no_emit: Some(true),
                    no_check: Some(true),
                    lib: Some(vec!["es5".into()]),
                    list_files_only: Some(list_only),
                    ..Default::default()
                },
                program_options,
                &LibraryCatalog::typescript_7_1("bundled:///libs"),
                ProgramLoadLimits::new(100, 1000, 100, 1 << 20, 4 << 20)
                    .with_workers(WorkerBudget::new(NonZeroUsize::new(workers).unwrap())),
            )
            .unwrap();
            let mut bound = 0;
            for prepared in program.source_files() {
                let Some(parsed) = prepared.preparsed_syntax().take() else {
                    continue;
                };
                let (reservation, data, mut tree) = parsed.into_bind_parts();
                let is_direct_library = matches!(
                    tree.file_name.as_str(),
                    Some("bundled:///libs/lib.es5.d.ts" | "bundled:///libs/lib.es2015.core.d.ts")
                );
                assert_eq!(
                    data.is_some(),
                    demand && !list_only && workers > 1 && is_direct_library,
                    "{:?}: workers={workers}, demand={demand}, list_only={list_only}",
                    tree.file_name,
                );
                if let Some(data) = data {
                    bound += 1;
                    assert!(data.identity_owned_by(&domain));
                    assert!(tree
                        .arena
                        .identity_leases()
                        .unwrap()
                        .nodes
                        .claim()
                        .is_none());
                    let expected = data.clone();
                    drop(data);
                    let reservation = reservation.unwrap();
                    let mut direct = tsc_binder::Binder::bind_reserved(
                        &mut tree,
                        reservation.symbols.range(),
                        reservation.serials.range().start(),
                    );
                    assert!(direct
                        .attach_reserved_leases(&domain, reservation.symbols, reservation.serials)
                        .unwrap());
                    assert_eq!(expected.symbols, direct.symbols);
                    assert_eq!(expected.node_symbol, direct.node_symbol);
                    assert_eq!(expected.locals, direct.locals);
                    assert_eq!(expected.node_flow, direct.node_flow);
                    assert_eq!(expected.bind_diagnostics, direct.bind_diagnostics);
                } else if tree.arena.is_published() {
                    // Parse-only, serial, listed and discovered inputs do
                    // not acquire a claim on published bind tables.
                    assert!(tree
                        .arena
                        .identity_leases()
                        .unwrap()
                        .nodes
                        .claim()
                        .is_some());
                }
            }
            assert_eq!(bound, usize::from(demand && !list_only && workers > 1) * 2);
        }
    }
}

#[test]
fn scrambled_reads_keep_root_ranges_and_parse_before_the_last_read() {
    let _command = COMMAND.lock().unwrap();
    let roots: Vec<_> = (0..17)
        .map(|i| PathBuf::from(format!("/p/r{i}.ts")))
        .collect();
    let mut expected = None;
    let mut expected_bind_ranges = None;
    for (run, workers) in [1, 4, 16, 4].into_iter().enumerate() {
        let mut inner = MemoryCompilerHost::builder("/p");
        for (i, path) in roots.iter().enumerate() {
            let text = if i == 0 {
                "/// <reference lib=\"es2015.promise\"/>\nimport './early-dep'; export const x = 0;"
                    .to_owned()
            } else {
                format!("export const x{i} = [{i}, 'r{i}'];")
            };
            inner = inner.file(path, text.into_bytes());
        }
        let host = StreamingHost {
            inner: inner
                .file("/p/early-dep.ts", b"export {};".to_vec())
                .build()
                .unwrap(),
            scramble: workers > 1,
            observe: true,
            state: Mutex::new(State {
                completed: Vec::new(),
                early_parsed: false,
                libraries_read: 0,
            }),
            changed: Condvar::new(),
        };
        let mut load_roots = roots.clone();
        if run == 3 {
            // Configuration membership wins even if a library also occurs
            // in the explicit root list. It must not be parsed twice.
            load_roots.push("bundled:///libs/lib.es5.d.ts".into());
        }
        let program = load_program(
            &host,
            &load_roots,
            CompilerOptions {
                no_emit: Some(true),
                lib: Some(vec!["es5".into()]),
                module: Some(199),
                module_resolution: Some(3),
                ..Default::default()
            },
            ProgramOptions::default()
                .with_types(Vec::new())
                .with_identity_domain(tsc_types::IdentityDomain::new())
                .with_source_binding(),
            &LibraryCatalog::typescript_7_1("bundled:///libs"),
            ProgramLoadLimits::new(100, 1000, 100, 1 << 20, 4 << 20)
                .with_workers(WorkerBudget::new(NonZeroUsize::new(workers).unwrap())),
        )
        .unwrap();
        let state = host.state.lock().unwrap();
        if host.scramble {
            let first: Vec<_> = state.completed.iter().copied().filter(|i| *i < 3).collect();
            assert_eq!(first, [2, 1, 0]);
        }
        assert!(state.early_parsed);
        drop(state);
        let layout = program.source_files()[0]
            .preparsed_syntax()
            .direct_layout()
            .unwrap();
        assert_eq!(layout.len(), 19, "two config libraries and seventeen roots");
        assert!(layout[..2]
            .iter()
            .all(|p| p.name.starts_with("bundled:///libs/")));
        assert!(!layout
            .iter()
            .any(|p| p.name.ends_with("lib.es2015.promise.d.ts")));
        let bases = layout[0].ranges().map(|r| r.start());
        let relative: Vec<_> = layout
            .iter()
            .map(|p| {
                let ranges = p.ranges();
                (
                    p.name.clone(),
                    std::array::from_fn::<_, 4, _>(|i| {
                        (ranges[i].start() - bases[i], ranges[i].len())
                    }),
                )
            })
            .collect();
        if let Some(expected) = &expected {
            assert_eq!(&relative, expected);
        } else {
            expected = Some(relative);
        }
        let mut bases = None;
        let mut bind_ranges = Vec::new();
        for placement in layout.iter() {
            let prepared = program
                .source_files()
                .iter()
                .find(|source| source.path().display() == placement.name.as_js())
                .unwrap();
            let (reservation, data, tree) = prepared
                .preparsed_syntax()
                .take()
                .unwrap()
                .into_bind_parts();
            let reservation = reservation.expect("each directly placed tree has a bind slice");
            let ranges = [reservation.symbols.range(), reservation.serials.range()];
            assert_eq!(ranges[0].len(), tree.arena.symbol_bound().max(1));
            assert_eq!(ranges[1].len() as usize, tree.arena.nodes().len());
            assert!(reservation.symbols.is_room_slice());
            let is_direct_library = matches!(
                tree.file_name.as_str(),
                Some("bundled:///libs/lib.es5.d.ts" | "bundled:///libs/lib.es2015.core.d.ts")
            );
            assert_eq!(data.is_some(), workers > 1 && is_direct_library);
            let claim = tree.arena.identity_leases().unwrap().nodes.claim();
            assert_eq!(claim.is_none(), data.is_some());
            drop(claim);
            let bases = *bases.get_or_insert(ranges.map(|range| range.start()));
            bind_ranges.push((
                placement.name.clone(),
                std::array::from_fn::<_, 2, _>(|i| (ranges[i].start() - bases[i], ranges[i].len())),
            ));
        }
        for pair in bind_ranges.windows(2) {
            for (&(start, count), &(next, _)) in pair[0].1.iter().zip(&pair[1].1) {
                assert_eq!(start + count, next);
            }
        }
        if let Some(expected) = &expected_bind_ranges {
            assert_eq!(&bind_ranges, expected);
        } else {
            expected_bind_ranges = Some(bind_ranges);
        }
        let late = program
            .source_files()
            .iter()
            .find(|s| s.path().display() == "bundled:///libs/lib.es2015.promise.d.ts")
            .unwrap();
        assert!(
            late.preparsed_syntax()
                .take()
                .is_none_or(|parsed| !parsed.source().arena.is_published()),
            "root-only library is ordinary (its full parse may be deferred)"
        );
    }
    check_interrupted_streams();
    check_empty_batches_and_unused_symbol_limits();
}

fn check_empty_batches_and_unused_symbol_limits() {
    let roots: Vec<_> = (0..17)
        .map(|i| PathBuf::from(format!("/p/r{i}.ts")))
        .collect();
    for workers in [1, 4, 16] {
        let mut host = MemoryCompilerHost::builder("/p");
        for root in &roots[8..] {
            host = host.file(root, b"export const value = 1;".to_vec());
        }
        let host = host.build().unwrap();
        let program = load_no_lib_program(
            &host,
            &roots,
            CompilerOptions {
                no_emit: Some(true),
                ..Default::default()
            },
            ProgramOptions::default()
                .with_no_lib(true)
                .with_types(Vec::new())
                .with_identity_domain(tsc_types::IdentityDomain::new())
                .with_source_binding(),
            ProgramLoadLimits::new(100, 1000, 100, 1 << 20, 4 << 20)
                .with_workers(WorkerBudget::new(NonZeroUsize::new(workers).unwrap())),
        )
        .unwrap();
        assert_eq!(program.source_files().len(), 9);
        for source in program.source_files() {
            assert!(source
                .preparsed_syntax()
                .take()
                .unwrap()
                .into_parts()
                .0
                .is_some());
        }
        let listed = load_no_lib_program(
            &host,
            &roots,
            CompilerOptions {
                list_files_only: Some(true),
                no_emit: Some(true),
                ..Default::default()
            },
            ProgramOptions::default()
                .with_no_lib(true)
                .with_types(Vec::new())
                .with_identity_domain(tsc_types::IdentityDomain::new())
                .with_source_binding(),
            ProgramLoadLimits::new(100, 1000, 100, 1 << 20, 4 << 20)
                .with_workers(WorkerBudget::new(NonZeroUsize::new(workers).unwrap())),
        )
        .unwrap();
        for source in listed.source_files() {
            let (reservation, tree) = source.preparsed_syntax().take().unwrap().into_parts();
            assert!(tree.arena.is_published());
            assert!(reservation.is_none());
        }
    }

    let domain = tsc_types::IdentityDomain::with_limits(tsc_types::IdentityLimits {
        persistent_symbol_end: 1,
        ..Default::default()
    })
    .unwrap();
    let host = MemoryCompilerHost::builder("/p")
        .file("/p/a.ts", b"export class A {}".to_vec())
        .build()
        .unwrap();
    let program = load_no_lib_program(
        &host,
        &[PathBuf::from("/p/a.ts")],
        CompilerOptions {
            no_emit: Some(true),
            ..Default::default()
        },
        ProgramOptions::default()
            .with_no_lib(true)
            .with_types(Vec::new())
            .with_identity_domain(domain.clone()),
        ProgramLoadLimits::new(100, 1000, 100, 1 << 20, 4 << 20),
    )
    .expect("a parse-only load does not require symbol space");
    let (reservation, tree) = program.source_files()[0]
        .preparsed_syntax()
        .take()
        .unwrap()
        .into_parts();
    assert!(tree.arena.is_published());
    assert!(reservation.is_none());
    assert_eq!(
        domain
            .stats()
            .unwrap()
            .space(tsc_types::IdentitySpace::Symbol)
            .active_values,
        0
    );
}

fn check_interrupted_streams() {
    let roots: Vec<_> = (0..17)
        .map(|i| PathBuf::from(format!("/p/r{i}.ts")))
        .collect();
    for workers in [1, 4, 16] {
        for bad_decode in [false, true] {
            let mut inner = MemoryCompilerHost::builder("/p");
            for (i, root) in roots.iter().enumerate() {
                if i == 1 {
                    continue;
                } // Missing input inside a stripe.
                let text = if i == 0 {
                    b"import './early-dep'; export {};".to_vec()
                } else if bad_decode && i == 9 {
                    vec![0xff, 0xfe, 0x00]
                } else {
                    format!("export const x{i} = {i};").into_bytes()
                };
                inner = inner.file(root, text);
            }
            let host = StreamingHost {
                inner: inner
                    .file("/p/early-dep.ts", b"export {};".to_vec())
                    .build()
                    .unwrap(),
                scramble: false,
                observe: false,
                state: Mutex::new(State {
                    completed: Vec::new(),
                    early_parsed: false,
                    libraries_read: 0,
                }),
                changed: Condvar::new(),
            };
            let options = CompilerOptions {
                no_emit: Some(true),
                lib: Some(vec!["es5".into()]),
                module: Some(199),
                module_resolution: Some(3),
                ..Default::default()
            };
            for (sources, bytes) in [
                (1, 4 << 20),
                (3, 4 << 20),
                (10, 4 << 20),
                (100, 100),
                (100, 4 << 20),
            ] {
                let limits = ProgramLoadLimits::new(sources, 1000, 100, 1 << 20, bytes)
                    .with_workers(WorkerBudget::new(NonZeroUsize::new(workers).unwrap()));
                let load = |finite| {
                    let program_options = ProgramOptions::default().with_types(Vec::new());
                    let program_options = if finite {
                        program_options
                            .with_identity_domain(tsc_types::IdentityDomain::new())
                            .with_source_binding()
                    } else {
                        program_options
                    };
                    load_program(
                        &host,
                        &roots,
                        options.clone(),
                        program_options,
                        &LibraryCatalog::typescript_7_1("bundled:///libs"),
                        limits,
                    )
                };
                let ordinary = load(false);
                let placed = load(true);
                match (ordinary, placed) {
                    (Err(a), Err(b)) => assert_eq!(
                        format!("{a:?}"),
                        format!("{b:?}"),
                        "workers={workers}, sources={sources}, bytes={bytes}, decode={bad_decode}"
                    ),
                    (Ok(a), Ok(b)) => {
                        let paths = |p: &tsc_program::PreparedProgram| {
                            p.source_files()
                                .iter()
                                .map(|s| s.path().display().to_owned())
                                .collect::<Vec<_>>()
                        };
                        assert_eq!(paths(&a), paths(&b));
                        assert_eq!(a.diagnostics(), b.diagnostics());
                    }
                    (a, b) => panic!("ordinary/direct outcome differs: {a:?} / {b:?}"),
                }
            }
        }
    }
}

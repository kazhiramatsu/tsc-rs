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
    load_program, CompilerOptions, LibraryCatalog, ProgramLoadLimits, ProgramOptions, WorkerBudget,
};

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
fn scrambled_reads_keep_root_ranges_and_parse_before_the_last_read() {
    let roots: Vec<_> = (0..17)
        .map(|i| PathBuf::from(format!("/p/r{i}.ts")))
        .collect();
    let mut expected = None;
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
                .with_identity_domain(tsc_types::IdentityDomain::new()),
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
                        program_options.with_identity_domain(tsc_types::IdentityDomain::new())
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

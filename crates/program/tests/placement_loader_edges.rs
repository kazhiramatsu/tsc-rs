include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../host/tests/support/scalar_query_bridge.rs"
));
use std::{
    num::NonZeroUsize,
    path::{Path, PathBuf},
};
use tsc_host::{CompilerHost, HostError, HostErrorKind, HostOperation, MemoryCompilerHost};
use tsc_program::{
    load_no_lib_program, CompilerOptions, ProgramLoadLimits, ProgramOptions, WorkerBudget,
};

fn options() -> CompilerOptions {
    CompilerOptions {
        no_emit: Some(true),
        module: Some(199),
        module_resolution: Some(3),
        ..Default::default()
    }
}
fn program_options() -> ProgramOptions {
    ProgramOptions::default()
        .with_no_lib(true)
        .with_types(Vec::new())
}
fn limits(workers: usize) -> ProgramLoadLimits {
    ProgramLoadLimits::new(100, 1000, 100, 1 << 20, 4 << 20)
        .with_workers(WorkerBudget::new(NonZeroUsize::new(workers).unwrap()))
}

struct NoAhead(MemoryCompilerHost);
impl CompilerHost for NoAhead {
    scalar_host_query_bridge!();
    fn current_directory(&self) -> Result<PathBuf, HostError> {
        self.0.current_directory()
    }
    fn use_case_sensitive_file_names(&self) -> bool {
        self.0.use_case_sensitive_file_names()
    }
    fn read_file(&self, p: &Path) -> Result<Option<Vec<u8>>, HostError> {
        self.0.read_file(p)
    }
    fn file_exists(&self, p: &Path) -> Result<bool, HostError> {
        self.0.file_exists(p)
    }
    fn directory_exists(&self, p: &Path) -> Result<bool, HostError> {
        self.0.directory_exists(p)
    }
    fn read_directory(&self, p: &Path) -> Result<Vec<PathBuf>, HostError> {
        self.0.read_directory(p)
    }
    fn realpath(&self, p: &Path) -> Result<Option<PathBuf>, HostError> {
        self.0.realpath(p)
    }
}

#[test]
fn no_read_ahead_and_trace_hosts_remain_private_at_every_budget() {
    let host = MemoryCompilerHost::builder("/p")
        .file("/p/a.ts", b"import './b'; export const a = 1;".to_vec())
        .file("/p/b.ts", b"export const b = 1;".to_vec())
        .build()
        .unwrap();
    let roots = [PathBuf::from("/p/a.ts"), PathBuf::from("/p/b.ts")];
    let no_ahead = NoAhead(host.clone());
    for workers in [1, 4, 16] {
        for trace in [false, true] {
            let mut opts = options();
            opts.trace_resolution = Some(trace);
            let host: &dyn CompilerHost = if trace { &host } else { &no_ahead };
            let program = load_no_lib_program(
                host,
                &roots,
                opts,
                program_options().with_identity_domain(tsc_types::IdentityDomain::new()),
                limits(workers),
            )
            .unwrap();
            for source in program.source_files() {
                let syntax = source.preparsed_syntax();
                assert!(!syntax.has_direct_layout());
                assert!(!syntax.take().unwrap().source().arena.is_published());
            }
        }
    }
}

#[test]
fn duplicate_roots_use_the_first_canonical_position_and_json_stays_ordinary() {
    let host = MemoryCompilerHost::builder("/p")
        .file("/p/a.ts", b"import './b'; export const a = 1;".to_vec())
        .file("/p/b.ts", b"export const b = 1;".to_vec())
        .file("/p/value.json", b"{\"value\":42}".to_vec())
        .build()
        .unwrap();
    let roots = ["/p/a.ts", "/p/b.ts", "/p/a.ts", "/p/value.json"].map(PathBuf::from);
    for workers in [1, 4, 16] {
        let mut opts = options();
        opts.resolve_json_module = Some(true);
        let program = load_no_lib_program(
            &host,
            &roots,
            opts,
            program_options().with_identity_domain(tsc_types::IdentityDomain::new()),
            limits(workers),
        )
        .unwrap();
        assert_eq!(program.source_files().len(), 3);
        let mut placed = Vec::new();
        for path in ["/p/a.ts", "/p/b.ts"] {
            let source = program
                .source_files()
                .iter()
                .find(|s| s.path().display() == path)
                .unwrap();
            let source = source.preparsed_syntax().take().unwrap().into_source();
            assert!(source.arena.is_published());
            placed.push(source);
        }
        assert_eq!(
            placed[0]
                .arena
                .identity_leases()
                .unwrap()
                .nodes
                .range()
                .end(),
            placed[1]
                .arena
                .identity_leases()
                .unwrap()
                .nodes
                .range()
                .start()
        );
        let json = program
            .source_files()
            .iter()
            .find(|s| s.path().display() == "/p/value.json")
            .unwrap();
        assert!(!json.preparsed_syntax().is_available());
    }
}

#[test]
fn failed_read_and_each_resource_stop_match_ordinary_error_precedence() {
    let base = MemoryCompilerHost::builder("/p")
        .file("/p/a.ts", b"import './dep'; export const a = 1;".to_vec())
        .file("/p/b.ts", b"export const b = 1;".to_vec())
        .file("/p/dep.ts", b"export const dep = 1;".to_vec());
    let host = base.clone().build().unwrap();
    let failed = base
        .failure(HostError::new(
            HostErrorKind::PermissionDenied,
            HostOperation::ReadFile,
            Some(PathBuf::from("/p/a.ts")),
            "fixed failure",
        ))
        .build()
        .unwrap();
    let roots = ["/p/a.ts", "/p/b.ts"].map(PathBuf::from);
    for workers in [1, 4, 16] {
        let worker = WorkerBudget::new(NonZeroUsize::new(workers).unwrap());
        for (host, limit) in [
            (
                &host,
                ProgramLoadLimits::new(1, 1000, 100, 1 << 20, 4 << 20),
            ),
            (&host, ProgramLoadLimits::new(100, 1000, 100, 10, 4 << 20)),
            (&host, ProgramLoadLimits::new(100, 1000, 100, 1 << 20, 30)),
            (&host, ProgramLoadLimits::new(100, 0, 100, 1 << 20, 4 << 20)),
            (
                &host,
                ProgramLoadLimits::new(100, 1000, 0, 1 << 20, 4 << 20),
            ),
            (&failed, limits(workers)),
        ] {
            let limit = limit.with_workers(worker);
            let normal =
                load_no_lib_program(host, &roots, options(), program_options(), limit).unwrap_err();
            let direct = load_no_lib_program(
                host,
                &roots,
                options(),
                program_options().with_identity_domain(tsc_types::IdentityDomain::new()),
                limit,
            )
            .unwrap_err();
            assert_eq!(
                format!("{normal:?}"),
                format!("{direct:?}"),
                "workers={workers}"
            );
        }
    }
}

#[test]
fn prefetched_scopes_keep_nested_package_metadata_and_probe_inventory() {
    let host = MemoryCompilerHost::builder("/p")
        .file(
            "/p/package.json",
            br#"{"name":"root","version":"1.0.0","type":"module"}"#.to_vec(),
        )
        .file(
            "/p/nested/package.json",
            br#"{"name":"nested","version":"2.0.0","type":"commonjs"}"#.to_vec(),
        )
        .file(
            "/p/a.ts",
            b"import './nested/b.js'; export const a = 1;".to_vec(),
        )
        .file("/p/nested/b.ts", b"export const b = 1;".to_vec())
        .file("/p/hard.mts", b"export const hard = 1;".to_vec())
        .build()
        .unwrap();
    let roots = ["/p/a.ts", "/p/hard.mts"].map(PathBuf::from);
    let no_ahead = NoAhead(host.clone());
    let baseline =
        load_no_lib_program(&no_ahead, &roots, options(), program_options(), limits(1)).unwrap();
    let inventory = |program: &tsc_program::PreparedProgram| {
        program
            .source_files()
            .iter()
            .map(|source| {
                (
                    source.path().clone(),
                    source.package_scope().cloned(),
                    source.implied_node_format(),
                    source.implied_node_format_for_emit(),
                )
            })
            .collect::<Vec<_>>()
    };
    let packages = baseline.packages().cloned().collect::<Vec<_>>();
    assert_eq!(packages.len(), 2);
    for workers in [1, 4, 16] {
        for direct in [false, true] {
            let mut program_options = program_options();
            if direct {
                program_options =
                    program_options.with_identity_domain(tsc_types::IdentityDomain::new());
            }
            let program =
                load_no_lib_program(&host, &roots, options(), program_options, limits(workers))
                    .unwrap();
            assert_eq!(inventory(&program), inventory(&baseline));
            assert_eq!(program.packages().cloned().collect::<Vec<_>>(), packages);
            assert_eq!(
                program.package_json_probes(),
                baseline.package_json_probes()
            );
        }
    }
}

#[test]
fn prefetched_scopes_preserve_the_ordered_manifest_spelling() {
    let host = MemoryCompilerHost::builder("/p")
        .case_sensitive(false)
        .file(
            "/p/pkg/package.json",
            br#"{"name":"root","version":"1.0.0","type":"module"}"#.to_vec(),
        )
        .file("/p/pkg/a.ts", b"export const a = 1;".to_vec())
        .file("/p/pkg/b.ts", b"export const b = 1;".to_vec())
        .build()
        .unwrap();
    let roots = ["/p/pkg/a.ts", "/p/PKG/b.ts"].map(PathBuf::from);
    let baseline = load_no_lib_program(
        &NoAhead(host.clone()),
        &roots,
        options(),
        program_options(),
        limits(1),
    )
    .unwrap();
    let packages = baseline.packages().cloned().collect::<Vec<_>>();
    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0].package_json().display(), "/p/PKG/package.json");
    for workers in [1, 4, 16] {
        let program = load_no_lib_program(
            &host,
            &roots,
            options(),
            program_options().with_identity_domain(tsc_types::IdentityDomain::new()),
            limits(workers),
        )
        .unwrap();
        assert_eq!(program.packages().cloned().collect::<Vec<_>>(), packages);
    }
}

#[test]
fn prefetched_scopes_keep_manifest_observations_from_automatic_types() {
    let host = MemoryCompilerHost::builder("/p")
        .case_sensitive(false)
        .file(
            "/p/node_modules/@types/shared/package.json",
            br#"{"name":"shared","version":"1.0.0","type":"module","types":"dist/index.d.ts","dependencies":{"runtime":"1.0.0"}}"#.to_vec(),
        )
        .file(
            "/p/node_modules/@types/shared/dist/index.d.ts",
            b"export declare const shared: number;".to_vec(),
        )
        .file(
            "/p/node_modules/@types/shared/a.ts",
            b"export const a = 1;".to_vec(),
        )
        .file(
            "/p/node_modules/@types/shared/node_modules/runtime/package.json",
            br#"{"name":"runtime","version":"1.0.0"}"#.to_vec(),
        )
        .file(
            "/real/runtime/package.json",
            br#"{"name":"runtime","version":"1.0.0"}"#.to_vec(),
        )
        .realpath(
            "/p/node_modules/@types/shared/node_modules/runtime",
            "/real/runtime",
        )
        .build()
        .unwrap();
    let roots = [PathBuf::from("/p/node_modules/@types/SHARED/a.ts")];
    let opts = program_options().with_types(vec!["shared".into()]);
    let baseline = load_no_lib_program(
        &NoAhead(host.clone()),
        &roots,
        options(),
        opts.clone(),
        limits(1),
    )
    .unwrap();
    assert_eq!(baseline.dependency_symlinks().len(), 1);
    assert_eq!(
        baseline.dependency_symlinks()[0].adjacent,
        "/p/node_modules/@types/shared/node_modules/runtime"
    );
    let packages = baseline.packages().cloned().collect::<Vec<_>>();
    for workers in [1, 4, 16] {
        let program = load_no_lib_program(
            &host,
            &roots,
            options(),
            opts.clone()
                .with_identity_domain(tsc_types::IdentityDomain::new()),
            limits(workers),
        )
        .unwrap();
        assert_eq!(program.packages().cloned().collect::<Vec<_>>(), packages);
        assert_eq!(
            program.dependency_symlinks(),
            baseline.dependency_symlinks()
        );
    }
}

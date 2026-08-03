use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(unix)]
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(unix)]
use std::{fs, io};

#[cfg(unix)]
use tsc_host::FsCompilerHost;
use tsc_host::{CompilerHost, HostError, HostErrorKind, HostOperation, MemoryCompilerHost};
use tsc_program::{
    load_no_lib_program, plan_source_requests, CompilerOptions, PathMapping, PreparedProgram,
    ProgramLoadError, ProgramLoadErrorKind, ProgramLoadLimit, ProgramLoadLimits,
    ProgramLoadOperation, ProgramOptions, ProgramPath, ResolutionError, ResolutionKey,
    ResolutionOutcome, ResolvedModuleTarget, TypeReferenceResolutionKey,
};

const GENEROUS_LIMIT: usize = 1_024;
#[cfg(unix)]
static NEXT_TEMP_TREE: AtomicU64 = AtomicU64::new(0);

#[cfg(unix)]
struct TempTree {
    root: PathBuf,
    cleanup_parent: PathBuf,
}

#[cfg(unix)]
impl TempTree {
    fn new() -> Self {
        loop {
            let sequence = NEXT_TEMP_TREE.fetch_add(1, Ordering::Relaxed);
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock is after the Unix epoch")
                .as_nanos();
            let candidate = std::env::temp_dir().join(format!(
                "tsc-rs-no-lib-loader-{}-{timestamp}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&candidate) {
                Ok(()) => {
                    let root = fs::canonicalize(&candidate).expect("canonicalize temp tree root");
                    let cleanup_parent =
                        root.parent().expect("temp tree has a parent").to_path_buf();
                    return Self {
                        root,
                        cleanup_parent,
                    };
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create temp tree: {error}"),
            }
        }
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }
}

#[cfg(unix)]
impl Drop for TempTree {
    fn drop(&mut self) {
        let file_name = self.root.file_name().and_then(|name| name.to_str());
        assert_eq!(self.root.parent(), Some(self.cleanup_parent.as_path()));
        assert!(file_name.is_some_and(|name| name.starts_with("tsc-rs-no-lib-loader-")));
        if let Err(error) = fs::remove_dir_all(&self.root) {
            if !std::thread::panicking() {
                panic!("remove temp tree {}: {error}", self.root.display());
            }
        }
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

fn limits(
    max_source_files: usize,
    max_request_edges: usize,
    max_source_depth: usize,
    max_source_file_bytes: usize,
    max_total_source_bytes: usize,
) -> ProgramLoadLimits {
    ProgramLoadLimits::new(
        max_source_files,
        max_request_edges,
        max_source_depth,
        max_source_file_bytes,
        max_total_source_bytes,
    )
}

fn generous_limits() -> ProgramLoadLimits {
    limits(
        GENEROUS_LIMIT,
        GENEROUS_LIMIT,
        GENEROUS_LIMIT,
        GENEROUS_LIMIT,
        GENEROUS_LIMIT,
    )
}

fn load(
    host: &dyn CompilerHost,
    roots: &[&str],
    limits: ProgramLoadLimits,
) -> Result<PreparedProgram, ProgramLoadError> {
    load_with_options(host, roots, compiler_options(), program_options(), limits)
}

fn load_with_options(
    host: &dyn CompilerHost,
    roots: &[&str],
    compiler_options: CompilerOptions,
    program_options: ProgramOptions,
    limits: ProgramLoadLimits,
) -> Result<PreparedProgram, ProgramLoadError> {
    let roots = roots.iter().map(PathBuf::from).collect::<Vec<_>>();
    load_no_lib_program(host, &roots, compiler_options, program_options, limits)
}

fn module_key(program: &PreparedProgram, source_path: &str, specifier: &str) -> ResolutionKey {
    let source = program
        .source_files()
        .iter()
        .find(|source| source.path().display() == Path::new(source_path))
        .expect("source is owned by program");
    plan_source_requests(source, program.compiler_options())
        .expect("re-plan source requests")
        .module_requests()
        .iter()
        .find(|key| key.specifier() == specifier)
        .expect("module request exists")
        .clone()
}

fn type_reference_key(
    program: &PreparedProgram,
    source_path: &str,
    specifier: &str,
) -> TypeReferenceResolutionKey {
    let source = program
        .source_files()
        .iter()
        .find(|source| source.path().display() == Path::new(source_path))
        .expect("source is owned by program");
    plan_source_requests(source, program.compiler_options())
        .expect("re-plan source requests")
        .type_reference_directives()
        .iter()
        .find(|directive| directive.key().specifier() == specifier)
        .expect("type-reference request exists")
        .key()
        .clone()
}

fn assert_limit_error(
    error: ProgramLoadError,
    expected_limit: ProgramLoadLimit,
    expected_path: &Path,
    expected_maximum: usize,
    expected_observed: usize,
) {
    assert_eq!(error.kind(), ProgramLoadErrorKind::ResourceLimit);
    let exceeded = error
        .limit_exceeded()
        .expect("resource-limit error carries structured evidence");
    assert_eq!(exceeded.limit(), expected_limit);
    assert_eq!(exceeded.path(), Some(expected_path));
    assert_eq!(exceeded.maximum(), expected_maximum);
    assert_eq!(exceeded.observed(), expected_observed);
}

fn assert_resolution_host_error(
    error: ProgramLoadError,
    expected_operation: ProgramLoadOperation,
    expected_host_error: &HostError,
) {
    assert_eq!(error.kind(), ProgramLoadErrorKind::Resolution);
    assert_eq!(error.operation(), expected_operation);
    let ProgramLoadError::Resolution {
        source: ResolutionError::Host(actual),
        ..
    } = error
    else {
        panic!("expected a nested resolver host failure");
    };
    assert_eq!(&actual, expected_host_error);
}

fn source_paths(program: &PreparedProgram) -> Vec<&Path> {
    program
        .source_files()
        .iter()
        .map(|source| source.path().display())
        .collect()
}

fn root_paths(program: &PreparedProgram) -> Vec<&Path> {
    program
        .roots()
        .iter()
        .map(|root| root.path().display())
        .collect()
}

#[test]
fn loads_dependencies_postorder_while_preserving_root_order_and_duplicates() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/a.ts", b"import './b';\nexport const a = 1;".to_vec())
        .file("/work/b.ts", b"export const b = 1;".to_vec())
        .build()
        .expect("build memory host");

    let program = load(
        &host,
        &["/work/a.ts", "/work/b.ts", "/work/a.ts"],
        generous_limits(),
    )
    .expect("load one noLib root closure");

    assert_eq!(
        source_paths(&program),
        [Path::new("/work/b.ts"), Path::new("/work/a.ts")]
    );
    assert_eq!(
        root_paths(&program),
        [
            Path::new("/work/a.ts"),
            Path::new("/work/b.ts"),
            Path::new("/work/a.ts"),
        ]
    );
    assert_eq!(program.roots()[0].source(), program.roots()[2].source());
    assert_ne!(program.roots()[0].source(), program.roots()[1].source());
    assert!(program.library_files().is_empty());
}

#[test]
fn paths_and_base_url_candidates_join_recursive_source_membership() {
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/root.ts",
            b"import { mapped } from '@app/mapped';\nimport { based } from 'based';\nexport { mapped, based };"
                .to_vec(),
        )
        .file(
            "/work/src/mapped.ts",
            b"export const mapped = 1;".to_vec(),
        )
        .file(
            "/work/base/based.ts",
            b"export const based = 1;".to_vec(),
        )
        .build()
        .expect("build paths host");
    let options = CompilerOptions {
        base_url: Some("/work/base".to_owned()),
        ..compiler_options()
    };
    let program_options = program_options().with_paths(vec![PathMapping::new(
        "@app/*",
        vec!["../src/*".to_owned()],
    )]);

    let program = load_with_options(
        &host,
        &["/work/root.ts"],
        options,
        program_options,
        generous_limits(),
    )
    .expect("load paths and baseUrl candidates");

    assert_eq!(
        source_paths(&program),
        [
            Path::new("/work/src/mapped.ts"),
            Path::new("/work/base/based.ts"),
            Path::new("/work/root.ts"),
        ]
    );
    for (specifier, expected) in [
        ("@app/mapped", "/work/src/mapped.ts"),
        ("based", "/work/base/based.ts"),
    ] {
        let key = module_key(&program, "/work/root.ts", specifier);
        let resolution = program
            .resolutions()
            .require_module(&key)
            .expect("mapped request has an authoritative row");
        let ResolutionOutcome::Resolved(resolved) = resolution.outcome() else {
            panic!("{specifier} must resolve");
        };
        let ResolvedModuleTarget::Source { resolved_file, .. } = resolved.target() else {
            panic!("mapped TypeScript target must join source membership");
        };
        assert_eq!(resolved_file.display(), Path::new(expected));
    }
}

#[test]
fn root_dirs_admit_alternate_sources_in_dependency_postorder() {
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/src/main.ts",
            b"import { shared } from './shared'; export { shared };".to_vec(),
        )
        .file(
            "/work/generated/shared.ts",
            b"import { leaf } from './leaf'; export const shared = leaf;".to_vec(),
        )
        .file(
            "/work/generated/leaf.ts",
            b"export const leaf = 1;".to_vec(),
        )
        .build()
        .expect("build rootDirs source graph");
    let root_dirs = ["/work/src", "/work/generated"]
        .into_iter()
        .map(|path| {
            ProgramPath::from_trusted_parts(path, path)
                .expect("construct case-sensitive rootDirs path")
        })
        .collect();
    let options = CompilerOptions {
        module_resolution: Some(100),
        ..compiler_options()
    };
    let program = load_with_options(
        &host,
        &["/work/src/main.ts"],
        options,
        program_options().with_root_dirs(root_dirs),
        generous_limits(),
    )
    .expect("load alternate rootDirs source graph");

    assert_eq!(
        source_paths(&program),
        [
            Path::new("/work/generated/leaf.ts"),
            Path::new("/work/generated/shared.ts"),
            Path::new("/work/src/main.ts"),
        ]
    );
    let key = module_key(&program, "/work/src/main.ts", "./shared");
    let resolution = program
        .resolutions()
        .require_module(&key)
        .expect("rootDirs request has an authoritative row");
    let ResolutionOutcome::Resolved(resolved) = resolution.outcome() else {
        panic!("rootDirs request must resolve");
    };
    let ResolvedModuleTarget::Source { resolved_file, .. } = resolved.target() else {
        panic!("rootDirs TypeScript target must join source membership");
    };
    assert_eq!(
        resolved_file.display(),
        Path::new("/work/generated/shared.ts")
    );
    assert_eq!(resolved.original_path(), None);
    assert!(!resolved.is_external_library_import());
}

#[test]
fn a_matched_paths_miss_suppresses_base_url_but_keeps_package_fallback() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", b"import { value } from 'pkg';".to_vec())
        .file(
            "/work/base/pkg.ts",
            b"export const value = 'wrong baseUrl candidate';".to_vec(),
        )
        .file(
            "/work/node_modules/pkg/package.json",
            br#"{"name":"pkg","version":"1.0.0","types":"index.d.ts"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/pkg/index.d.ts",
            b"export declare const value: 'package';".to_vec(),
        )
        .build()
        .expect("build fallback host");
    let options = CompilerOptions {
        base_url: Some("/work/base".to_owned()),
        ..compiler_options()
    };
    let program_options = program_options().with_paths(vec![PathMapping::new(
        "pkg",
        vec!["missing/pkg".to_owned()],
    )]);

    let program = load_with_options(
        &host,
        &["/work/root.ts"],
        options,
        program_options,
        generous_limits(),
    )
    .expect("fall through from a matched paths miss to node_modules");

    assert_eq!(
        source_paths(&program),
        [
            Path::new("/work/node_modules/pkg/index.d.ts"),
            Path::new("/work/root.ts"),
        ]
    );
}

#[test]
fn discovers_path_then_type_then_module_and_skips_lib_references_under_no_lib() {
    let root_text = concat!(
        "/// <reference path=\"./path.ts\" />\n",
        "/// <reference types=\"./types\" />\n",
        "/// <reference lib=\"es5\" />\n",
        "import './module';\n",
        "export {};\n",
    );
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", root_text.as_bytes().to_vec())
        .file("/work/path.ts", b"export const path = 1;".to_vec())
        .file("/work/types.d.ts", b"declare const types: 1;".to_vec())
        .file("/work/module.ts", b"export const module = 1;".to_vec())
        .file(
            "/work/lib.es5.d.ts",
            b"declare const skippedLib: 1;".to_vec(),
        )
        .build()
        .expect("build memory host");

    let program = load(&host, &["/work/root.ts"], generous_limits())
        .expect("load all non-library source phases");

    assert_eq!(
        source_paths(&program),
        [
            Path::new("/work/path.ts"),
            Path::new("/work/types.d.ts"),
            Path::new("/work/module.ts"),
            Path::new("/work/root.ts"),
        ]
    );
    assert!(program.library_files().is_empty());
}

#[test]
fn classic_and_node10_load_triple_slash_types_into_the_authoritative_table() {
    let root_text = "/// <reference types=\"legacy-types\" />\nexport {};\n";
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", root_text.as_bytes().to_vec())
        .file(
            "/work/node_modules/@types/legacy-types/package.json",
            br#"{"name":"@types/legacy-types","version":"1.0.0","types":"index.d.ts"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/@types/legacy-types/index.d.ts",
            b"declare const legacyTypes: true;".to_vec(),
        )
        .build()
        .expect("build legacy type-reference program");

    for module_resolution in [1, 2] {
        let options = CompilerOptions {
            no_emit: Some(true),
            module_resolution: Some(module_resolution),
            ..CompilerOptions::default()
        };
        let program = load_with_options(
            &host,
            &["/work/root.ts"],
            options,
            program_options(),
            generous_limits(),
        )
        .expect("load a legacy type-reference program");

        assert_eq!(
            source_paths(&program),
            [
                Path::new("/work/node_modules/@types/legacy-types/index.d.ts"),
                Path::new("/work/root.ts"),
            ]
        );
        let key = type_reference_key(&program, "/work/root.ts", "legacy-types");
        let resolution = program
            .resolutions()
            .require_type_reference(&key)
            .expect("legacy type-reference row is authoritative");
        let ResolutionOutcome::Resolved(reference) = resolution.outcome() else {
            panic!("expected a bound legacy type-reference target");
        };
        assert!(reference.primary());
        assert!(reference.is_external_library_import());
        assert_eq!(
            reference.target().display(),
            Path::new("/work/node_modules/@types/legacy-types/index.d.ts")
        );
        assert!(resolution.diagnostics().is_empty());
    }
}

#[test]
fn cycles_and_diamonds_produce_one_source_per_canonical_path() {
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/a.ts",
            b"import './b';\nimport './c';\nexport {};".to_vec(),
        )
        .file("/work/b.ts", b"import './d';\nexport {};".to_vec())
        .file(
            "/work/c.ts",
            b"import './d';\nimport './a';\nexport {};".to_vec(),
        )
        .file("/work/d.ts", b"import './a';\nexport {};".to_vec())
        .build()
        .expect("build memory host");

    let program =
        load(&host, &["/work/a.ts"], generous_limits()).expect("load cyclic diamond graph");

    assert_eq!(
        source_paths(&program),
        [
            Path::new("/work/d.ts"),
            Path::new("/work/b.ts"),
            Path::new("/work/c.ts"),
            Path::new("/work/a.ts"),
        ]
    );
}

#[test]
fn source_file_count_limit_accepts_the_boundary_and_rejects_one_more() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/a.ts", b"import './b';".to_vec())
        .file("/work/b.ts", b"export {};".to_vec())
        .build()
        .expect("build memory host");

    load(
        &host,
        &["/work/a.ts"],
        limits(
            2,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
        ),
    )
    .expect("two sources fit a two-source limit");
    let error = load(
        &host,
        &["/work/a.ts"],
        limits(
            1,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
        ),
    );
    assert_limit_error(
        error.expect_err("the second unique source exceeds a one-source limit"),
        ProgramLoadLimit::SourceFiles,
        Path::new("/work/b.ts"),
        1,
        2,
    );
}

#[test]
fn request_edge_limit_counts_duplicate_import_occurrences_before_deduplication() {
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/a.ts",
            b"import './b';\nimport './b';\nexport {};".to_vec(),
        )
        .file("/work/b.ts", b"export {};".to_vec())
        .build()
        .expect("build memory host");

    load(
        &host,
        &["/work/a.ts"],
        limits(
            GENEROUS_LIMIT,
            2,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
        ),
    )
    .expect("two import occurrences fit a two-edge limit");
    let error = load(
        &host,
        &["/work/a.ts"],
        limits(
            GENEROUS_LIMIT,
            1,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
        ),
    );
    assert_limit_error(
        error.expect_err("the duplicate import occurrence exceeds a one-edge limit"),
        ProgramLoadLimit::RequestEdges,
        Path::new("/work/a.ts"),
        1,
        2,
    );
}

#[test]
fn source_depth_limit_uses_zero_based_root_depth() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/a.ts", b"import './b';".to_vec())
        .file("/work/b.ts", b"import './c';".to_vec())
        .file("/work/c.ts", b"export {};".to_vec())
        .build()
        .expect("build memory host");

    load(
        &host,
        &["/work/a.ts"],
        limits(
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            2,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
        ),
    )
    .expect("root=0 makes the third source the depth-two boundary");
    let error = load(
        &host,
        &["/work/a.ts"],
        limits(
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            1,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
        ),
    );
    assert_limit_error(
        error.expect_err("the depth-two source exceeds a max depth of one"),
        ProgramLoadLimit::SourceDepth,
        Path::new("/work/c.ts"),
        1,
        2,
    );
}

#[test]
fn per_source_byte_limit_accepts_the_boundary_and_rejects_one_more_byte() {
    let source = b"export const value = 1;";
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/a.ts", source.to_vec())
        .build()
        .expect("build memory host");

    load(
        &host,
        &["/work/a.ts"],
        limits(
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            source.len(),
            GENEROUS_LIMIT,
        ),
    )
    .expect("the exact raw-byte length fits the per-source limit");
    let error = load(
        &host,
        &["/work/a.ts"],
        limits(
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            source.len() - 1,
            GENEROUS_LIMIT,
        ),
    );
    assert_limit_error(
        error.expect_err("one additional raw byte exceeds the per-source limit"),
        ProgramLoadLimit::SourceFileBytes,
        Path::new("/work/a.ts"),
        source.len() - 1,
        source.len(),
    );
}

#[test]
fn total_source_byte_limit_accepts_the_boundary_and_rejects_one_more_byte() {
    let a = b"import './b';";
    let b = b"export {};";
    let total = a.len() + b.len();
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/a.ts", a.to_vec())
        .file("/work/b.ts", b.to_vec())
        .build()
        .expect("build memory host");

    load(
        &host,
        &["/work/a.ts"],
        limits(
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            total,
        ),
    )
    .expect("the exact sum of retained source bytes fits the total limit");
    let error = load(
        &host,
        &["/work/a.ts"],
        limits(
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            total - 1,
        ),
    );
    assert_limit_error(
        error.expect_err("one additional retained byte exceeds the total limit"),
        ProgramLoadLimit::TotalSourceBytes,
        Path::new("/work/b.ts"),
        total - 1,
        total,
    );
}

#[test]
fn missing_root_entries_preserve_multiplicity_but_share_one_ts6053_diagnostic() {
    let host = MemoryCompilerHost::builder("/work")
        .build()
        .expect("build empty memory host");

    let program = load(
        &host,
        &["/work/missing.ts", "/work/missing.ts"],
        generous_limits(),
    )
    .expect("missing roots are retained as program facts");

    assert_eq!(program.roots().len(), 2);
    assert!(program.roots().iter().all(|root| root.source().is_none()));
    assert!(program.roots().iter().all(|root| root
        .missing_diagnostic()
        .is_some_and(|error| error.code() == 6053)));
    assert_eq!(
        program.roots()[0].missing_diagnostic(),
        program.roots()[1].missing_diagnostic()
    );
    assert_eq!(
        program
            .diagnostics()
            .program()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [6053]
    );
    let diagnostic = &program.diagnostics().program()[0];
    assert_eq!(program.roots()[0].missing_diagnostic(), Some(diagnostic));
    assert!(diagnostic.message.next_present);
    assert_eq!(diagnostic.message.next.len(), 1);
    assert_eq!(diagnostic.message.next[0].code, 1430);
    assert!(diagnostic.message.next[0].next_present);
    assert_eq!(diagnostic.message.next[0].next.len(), 1);
    assert_eq!(diagnostic.message.next[0].next[0].code, 1427);
}

#[test]
fn case_insensitive_missing_spellings_keep_distinct_ts6053_messages() {
    let host = MemoryCompilerHost::builder("/Work")
        .case_sensitive(false)
        .build()
        .expect("build case-insensitive empty host");

    let program = load(
        &host,
        &["/Work/Missing.ts", "/work/missing.ts"],
        generous_limits(),
    )
    .expect("missing display spellings remain independently diagnosable");
    assert_eq!(program.roots().len(), 2);
    let diagnostics = program.diagnostics().program();
    assert_eq!(diagnostics.len(), 2);
    assert!(diagnostics
        .iter()
        .all(|diagnostic| diagnostic.code() == 6053));
    assert_ne!(diagnostics[0].message_text(), diagnostics[1].message_text());
}

#[test]
fn loaded_case_alias_fails_typed_instead_of_silently_collapsing_display_spelling() {
    let host = MemoryCompilerHost::builder("/Work")
        .case_sensitive(false)
        .file("/Work/Root.ts", b"export {};".to_vec())
        .build()
        .expect("build case-insensitive source host");

    let error = load(
        &host,
        &["/Work/Root.ts", "/work/root.ts"],
        generous_limits(),
    )
    .expect_err("loaded aliases require an owned casing-diagnostic policy");
    assert_eq!(error.kind(), ProgramLoadErrorKind::Unsupported);
    assert_eq!(error.operation(), ProgramLoadOperation::ReadSource);
    let ProgramLoadError::Unsupported { feature, .. } = error else {
        unreachable!("kind identifies the unsupported variant");
    };
    assert_eq!(feature, "canonical-source-display-alias");
}

#[test]
fn path_reference_case_alias_fails_typed_outside_the_root_boundary() {
    let host = MemoryCompilerHost::builder("/Work")
        .case_sensitive(false)
        .file(
            "/Work/Root.ts",
            concat!(
                "/// <reference path=\"./Child.ts\" />\n",
                "/// <reference path=\"./child.ts\" />\n",
            )
            .as_bytes()
            .to_vec(),
        )
        .file("/Work/Child.ts", b"export {};".to_vec())
        .build()
        .expect("build case-insensitive path-reference host");

    let error = load(&host, &["/Work/Root.ts"], generous_limits())
        .expect_err("path-reference aliases require an owned casing-diagnostic policy");
    assert_eq!(error.kind(), ProgramLoadErrorKind::Unsupported);
    assert_eq!(error.operation(), ProgramLoadOperation::ReadSource);
    assert_eq!(error.path(), Some(Path::new("/Work/child.ts")));
    let ProgramLoadError::Unsupported { feature, .. } = error else {
        unreachable!("kind identifies the unsupported variant");
    };
    assert_eq!(feature, "canonical-source-display-alias");
}

#[test]
fn missing_explicit_path_reference_produces_located_ts6053() {
    let root_text = "/// <reference path=\"./missing.ts\" />\nexport {};\n";
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", root_text.as_bytes().to_vec())
        .build()
        .expect("build memory host");

    let program = load(&host, &["/work/root.ts"], generous_limits())
        .expect("a missing explicit path is a program diagnostic");
    let diagnostics = program.diagnostics().program();
    assert_eq!(diagnostics.len(), 1);
    let diagnostic = &diagnostics[0];
    let start = root_text.find("./missing.ts").expect("reference span") as u32;
    assert_eq!(diagnostic.code(), 6053);
    assert_eq!(diagnostic.file_name.as_deref(), Some("/work/root.ts"));
    assert_eq!(diagnostic.start, Some(start));
    assert_eq!(diagnostic.length, Some("./missing.ts".len() as u32));
}

#[test]
fn explicit_path_references_gate_json_and_report_javascript_or_unknown_extensions() {
    let json_root = "/// <reference path=\"./data.json\" />\nexport {};\n";
    let javascript_root = "/// <reference path=\"./dependency.js\" />\nexport {};\n";
    let text_root = "/// <reference path=\"./notes.txt\" />\nexport {};\n";
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/json-root.ts", json_root.as_bytes().to_vec())
        .file(
            "/work/javascript-root.ts",
            javascript_root.as_bytes().to_vec(),
        )
        .file("/work/text-root.ts", text_root.as_bytes().to_vec())
        .file("/work/data.json", br#"{"value":1}"#.to_vec())
        .file("/work/dependency.js", b"exports.value = 1;".to_vec())
        .file("/work/notes.txt", b"not a program source".to_vec())
        .build()
        .expect("build explicit path-extension host");
    let bundler_options = CompilerOptions {
        no_emit: Some(true),
        module: Some(99),
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };

    let enabled_json = load_no_lib_program(
        &host,
        &[PathBuf::from("/work/json-root.ts")],
        bundler_options.clone(),
        program_options(),
        generous_limits(),
    )
    .expect("Bundler's effective resolveJsonModule admits explicit JSON paths");
    assert_eq!(
        source_paths(&enabled_json),
        [
            Path::new("/work/data.json"),
            Path::new("/work/json-root.ts")
        ]
    );
    assert!(enabled_json.diagnostics().program().is_empty());
    assert!(!enabled_json.source_files()[0].may_be_emitted());

    let disabled_json = load_no_lib_program(
        &host,
        &[PathBuf::from("/work/json-root.ts")],
        CompilerOptions {
            resolve_json_module: Some(false),
            ..bundler_options
        },
        program_options(),
        generous_limits(),
    )
    .expect("disabled explicit JSON path is a located diagnostic");
    assert_eq!(
        source_paths(&disabled_json),
        [Path::new("/work/json-root.ts")]
    );
    let diagnostic = &disabled_json.diagnostics().program()[0];
    assert_eq!(diagnostic.code(), 6054);
    assert_eq!(diagnostic.file_name.as_deref(), Some("/work/json-root.ts"));
    assert_eq!(
        diagnostic.start,
        Some(json_root.find("./data.json").unwrap() as u32)
    );
    assert_eq!(diagnostic.length, Some("./data.json".len() as u32));

    for (root, root_text, specifier, expected_code) in [
        (
            "/work/javascript-root.ts",
            javascript_root,
            "./dependency.js",
            6504,
        ),
        ("/work/text-root.ts", text_root, "./notes.txt", 6054),
    ] {
        let program = load(&host, &[root], generous_limits())
            .expect("unsupported explicit path extension is a located diagnostic");
        assert_eq!(source_paths(&program), [Path::new(root)]);
        let diagnostics = program.diagnostics().program();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code(), expected_code);
        assert_eq!(diagnostics[0].file_name.as_deref(), Some(root));
        assert_eq!(
            diagnostics[0].start,
            Some(root_text.find(specifier).unwrap() as u32)
        );
        assert_eq!(diagnostics[0].length, Some(specifier.len() as u32));
    }
}

#[test]
fn extensionless_path_references_probe_ts_then_tsx_then_dts_and_report_ts6231() {
    let root_ts = "/// <reference path=\"./pick-ts\" />\n";
    let root_tsx = "/// <reference path=\"./pick-tsx\" />\n";
    let root_dts = "/// <reference path=\"./pick-dts\" />\n";
    let root_missing = "/// <reference path=\"./pick-missing\" />\n";
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root-ts.ts", root_ts.as_bytes().to_vec())
        .file("/work/root-tsx.ts", root_tsx.as_bytes().to_vec())
        .file("/work/root-dts.ts", root_dts.as_bytes().to_vec())
        .file("/work/root-missing.ts", root_missing.as_bytes().to_vec())
        .file("/work/pick-ts.ts", b"export {};".to_vec())
        .file("/work/pick-ts.tsx", b"export {};".to_vec())
        .file("/work/pick-ts.d.ts", b"export {};".to_vec())
        .file("/work/pick-tsx.tsx", b"export {};".to_vec())
        .file("/work/pick-tsx.d.ts", b"export {};".to_vec())
        .file("/work/pick-dts.d.ts", b"export {};".to_vec())
        .file(
            "/work/pick-missing.cts",
            b"export const mustNotBeProbed = 1;".to_vec(),
        )
        .build()
        .expect("build path-probe memory host");

    let program = load(
        &host,
        &[
            "/work/root-ts.ts",
            "/work/root-tsx.ts",
            "/work/root-dts.ts",
            "/work/root-missing.ts",
        ],
        generous_limits(),
    )
    .expect("probe all extensionless path-reference cases");

    assert_eq!(
        source_paths(&program),
        [
            Path::new("/work/pick-ts.ts"),
            Path::new("/work/root-ts.ts"),
            Path::new("/work/pick-tsx.tsx"),
            Path::new("/work/root-tsx.ts"),
            Path::new("/work/pick-dts.d.ts"),
            Path::new("/work/root-dts.ts"),
            Path::new("/work/root-missing.ts"),
        ]
    );
    let diagnostics = program.diagnostics().program();
    assert_eq!(diagnostics.len(), 1);
    let diagnostic = &diagnostics[0];
    assert_eq!(diagnostic.code(), 6231);
    assert_eq!(
        diagnostic.file_name.as_deref(),
        Some("/work/root-missing.ts")
    );
    assert_eq!(
        diagnostic.start,
        Some(root_missing.find("./pick-missing").unwrap() as u32)
    );
    assert_eq!(diagnostic.length, Some("./pick-missing".len() as u32));
}

#[test]
fn duplicate_missing_type_references_share_one_row_but_keep_each_ts2688_span() {
    let root_text = concat!(
        "/// <reference types=\"missing-types\" />\n",
        "/// <reference types=\"missing-types\" />\n",
        "export {};\n",
    );
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", root_text.as_bytes().to_vec())
        .build()
        .expect("build memory host");

    let program = load(&host, &["/work/root.ts"], generous_limits())
        .expect("missing type references remain authoritative NotFound rows");
    assert_eq!(program.resolutions().type_reference_len(), 1);
    let key = type_reference_key(&program, "/work/root.ts", "missing-types");
    let resolution = program
        .resolutions()
        .require_type_reference(&key)
        .expect("one deduplicated type-reference row");
    assert!(matches!(resolution.outcome(), ResolutionOutcome::NotFound));
    let diagnostics = resolution.diagnostics();
    assert_eq!(diagnostics.len(), 2);
    let expected_starts = root_text
        .match_indices("missing-types")
        .map(|(start, _)| start as u32)
        .collect::<Vec<_>>();
    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [2688, 2688]
    );
    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.start.unwrap())
            .collect::<Vec<_>>(),
        expected_starts
    );
    assert!(diagnostics.iter().all(|diagnostic| {
        diagnostic.file_name.as_deref() == Some("/work/root.ts")
            && diagnostic.length == Some("missing-types".len() as u32)
    }));
}

#[test]
fn module_not_found_and_unloaded_javascript_are_both_authoritative_rows() {
    let root_text = "import './missing';\nimport './dependency.js';\nexport {};\n";
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", root_text.as_bytes().to_vec())
        .file("/work/dependency.js", b"exports.value = 1;".to_vec())
        .build()
        .expect("build memory host");

    let program = load(&host, &["/work/root.ts"], generous_limits())
        .expect("retain module misses and skipped JavaScript targets");
    assert_eq!(source_paths(&program), [Path::new("/work/root.ts")]);
    assert_eq!(program.resolutions().module_len(), 2);

    let missing_key = module_key(&program, "/work/root.ts", "./missing");
    let missing = program
        .resolutions()
        .require_module(&missing_key)
        .expect("missing module has an authoritative row");
    assert!(matches!(missing.outcome(), ResolutionOutcome::NotFound));

    let javascript_key = module_key(&program, "/work/root.ts", "./dependency.js");
    let javascript = program
        .resolutions()
        .require_module(&javascript_key)
        .expect("JavaScript module has an authoritative row");
    let ResolutionOutcome::Resolved(javascript) = javascript.outcome() else {
        panic!("JavaScript request must resolve");
    };
    let ResolvedModuleTarget::Unloaded(path) = javascript.target() else {
        panic!("allowJs=false must keep the JavaScript target out of source membership");
    };
    assert_eq!(path.display(), Path::new("/work/dependency.js"));
}

#[test]
fn resolved_json_dependency_is_loaded_without_planning_requests_from_json_strings() {
    let json = r#"{"text":"import './phantom'","other":"require('./ghost')"}"#;
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/root.ts",
            b"import data from './data.json';\nexport { data };".to_vec(),
        )
        .file("/work/data.json", json.as_bytes().to_vec())
        .build()
        .expect("build JSON module host");
    let options = CompilerOptions {
        no_emit: Some(true),
        module: Some(99),
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };

    let program = load_no_lib_program(
        &host,
        &[PathBuf::from("/work/root.ts")],
        options,
        program_options(),
        generous_limits(),
    )
    .expect("load a JSON dependency under Bundler's resolveJsonModule default");
    assert_eq!(
        source_paths(&program),
        [Path::new("/work/data.json"), Path::new("/work/root.ts")]
    );
    assert_eq!(program.resolutions().module_len(), 1);
    assert_eq!(program.source_files()[0].text(), json);
    assert!(!program.source_files()[0].may_be_emitted());
    let key = module_key(&program, "/work/root.ts", "./data.json");
    let resolution = program
        .resolutions()
        .require_module(&key)
        .expect("JSON import has one authoritative row");
    let ResolutionOutcome::Resolved(resolved) = resolution.outcome() else {
        panic!("JSON import must resolve");
    };
    let ResolvedModuleTarget::Source {
        source,
        resolved_file,
    } = resolved.target()
    else {
        panic!("enabled resolveJsonModule loads JSON into source membership");
    };
    assert_eq!(resolved_file.display(), Path::new("/work/data.json"));
    assert_eq!(
        program.source_file(*source),
        Some(&program.source_files()[0])
    );
}

#[test]
fn json_probe_requires_both_an_explicit_json_suffix_and_effective_option() {
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/disabled.ts",
            b"import data from './data.json';\nexport { data };".to_vec(),
        )
        .file(
            "/work/extensionless.ts",
            b"import data from './data';\nexport { data };".to_vec(),
        )
        .file("/work/data.json", br#"{"value":1}"#.to_vec())
        .build()
        .expect("build JSON gating host");
    let bundler_options = CompilerOptions {
        no_emit: Some(true),
        module: Some(99),
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };

    let disabled = load_no_lib_program(
        &host,
        &[PathBuf::from("/work/disabled.ts")],
        CompilerOptions {
            resolve_json_module: Some(false),
            ..bundler_options.clone()
        },
        program_options(),
        generous_limits(),
    )
    .expect("disabled JSON resolution is an authoritative module miss");
    assert_eq!(source_paths(&disabled), [Path::new("/work/disabled.ts")]);
    let disabled_key = module_key(&disabled, "/work/disabled.ts", "./data.json");
    assert!(matches!(
        disabled
            .resolutions()
            .require_module(&disabled_key)
            .expect("disabled JSON request row")
            .outcome(),
        ResolutionOutcome::NotFound
    ));

    let extensionless = load_no_lib_program(
        &host,
        &[PathBuf::from("/work/extensionless.ts")],
        bundler_options,
        program_options(),
        generous_limits(),
    )
    .expect("extensionless requests do not acquire a JSON probe");
    assert_eq!(
        source_paths(&extensionless),
        [Path::new("/work/extensionless.ts")]
    );
    let extensionless_key = module_key(&extensionless, "/work/extensionless.ts", "./data");
    assert!(matches!(
        extensionless
            .resolutions()
            .require_module(&extensionless_key)
            .expect("extensionless request row")
            .outcome(),
        ResolutionOutcome::NotFound
    ));
}

#[test]
fn augmentation_only_typescript_target_is_rejected_without_program_membership() {
    let augment = "export {};\ndeclare module './target' { export const x: 1; }\n";
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/augment.ts", augment.as_bytes().to_vec())
        .file("/work/target.ts", b"export const value = 1;".to_vec())
        .build()
        .expect("build memory host");

    let error = load(&host, &["/work/augment.ts"], generous_limits())
        .expect_err("an augmentation-only TypeScript target has no source membership");
    assert_eq!(error.kind(), ProgramLoadErrorKind::Resolution);
    assert_eq!(error.operation(), ProgramLoadOperation::BindResolutions);
    let ProgramLoadError::Resolution { source, .. } = error else {
        unreachable!("kind identifies the resolution variant");
    };
    assert!(matches!(
        source,
        ResolutionError::Unsupported { ref feature, .. }
            if feature == "resolution-only-source-target"
    ));
}

#[test]
fn augmentation_target_binds_to_source_when_a_later_root_loads_the_same_file() {
    let augment = "export {};\ndeclare module './target' { export const x: 1; }\n";
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/augment.ts", augment.as_bytes().to_vec())
        .file("/work/target.ts", b"export const value = 1;".to_vec())
        .build()
        .expect("build memory host");

    let program = load(
        &host,
        &["/work/augment.ts", "/work/target.ts"],
        generous_limits(),
    )
    .expect("the later root supplies independent target membership");
    assert_eq!(
        source_paths(&program),
        [Path::new("/work/augment.ts"), Path::new("/work/target.ts")]
    );
    let key = module_key(&program, "/work/augment.ts", "./target");
    let resolution = program
        .resolutions()
        .require_module(&key)
        .expect("augmentation resolution row");
    let ResolutionOutcome::Resolved(resolved) = resolution.outcome() else {
        panic!("augmentation target must resolve");
    };
    let ResolvedModuleTarget::Source {
        source,
        resolved_file,
    } = resolved.target()
    else {
        panic!("the later root must bind the augmentation to an owned source");
    };
    assert_eq!(resolved_file.display(), Path::new("/work/target.ts"));
    assert_eq!(
        program.source_file(*source).unwrap().path().display(),
        Path::new("/work/target.ts")
    );
}

#[test]
fn loaded_package_target_with_original_path_fails_at_the_typed_consumer_boundary() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", b"import 'pkg';\nexport {};".to_vec())
        .file(
            "/work/node_modules/pkg/package.json",
            br#"{"name":"pkg","version":"1.0.0","exports":"./index.ts"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/pkg/index.ts",
            b"export const lexical = 1;".to_vec(),
        )
        .file("/store/pkg/index.ts", b"export const lexical = 1;".to_vec())
        .realpath("/work/node_modules/pkg/index.ts", "/store/pkg/index.ts")
        .build()
        .expect("build package realpath host");
    let options = CompilerOptions {
        no_emit: Some(true),
        module: Some(199),
        module_resolution: Some(99),
        ..CompilerOptions::default()
    };

    let error = load_no_lib_program(
        &host,
        &[PathBuf::from("/work/root.ts")],
        options,
        program_options(),
        generous_limits(),
    )
    .expect_err("loaded originalPath cannot yet enter the checker contract");
    assert_eq!(error.kind(), ProgramLoadErrorKind::Unsupported);
    assert_eq!(error.operation(), ProgramLoadOperation::ResolveModule);
    assert_eq!(error.path(), Some(Path::new("/store/pkg/index.ts")));
    let ProgramLoadError::Unsupported { feature, .. } = error else {
        unreachable!("kind identifies the unsupported variant");
    };
    assert_eq!(feature, "loaded-original-path");
}

#[test]
fn invalid_loader_options_fail_before_host_discovery_with_typed_context() {
    let host = MemoryCompilerHost::builder("/work")
        .build()
        .expect("build empty memory host");
    let roots = vec![PathBuf::from("/work/root.ts")];

    let missing_no_emit = load_no_lib_program(
        &host,
        &roots,
        CompilerOptions::default(),
        program_options(),
        generous_limits(),
    )
    .expect_err("noEmit must be explicit");
    assert_eq!(missing_no_emit.kind(), ProgramLoadErrorKind::InvalidInput);
    assert_eq!(
        missing_no_emit.operation(),
        ProgramLoadOperation::ValidateOptions
    );

    let missing_no_lib = load_no_lib_program(
        &host,
        &roots,
        compiler_options(),
        ProgramOptions::default(),
        generous_limits(),
    )
    .expect_err("noLib must be explicit");
    assert_eq!(missing_no_lib.kind(), ProgramLoadErrorKind::InvalidInput);
    assert_eq!(
        missing_no_lib.operation(),
        ProgramLoadOperation::ValidateOptions
    );

    let allow_js = load_no_lib_program(
        &host,
        &roots,
        CompilerOptions {
            allow_js: true,
            ..compiler_options()
        },
        program_options(),
        generous_limits(),
    )
    .expect_err("allowJs broadens the admitted source family");
    assert_eq!(allow_js.kind(), ProgramLoadErrorKind::Unsupported);
    assert_eq!(allow_js.operation(), ProgramLoadOperation::ValidateOptions);

    let no_lib_with_explicit_empty_lib = load_no_lib_program(
        &host,
        &roots,
        CompilerOptions {
            lib: Some(Vec::new()),
            ..compiler_options()
        },
        program_options(),
        generous_limits(),
    )
    .expect_err("the H0.5 noLib/lib option diagnostic is not yet owned");
    assert_eq!(
        no_lib_with_explicit_empty_lib.kind(),
        ProgramLoadErrorKind::Unsupported
    );
    assert_eq!(
        no_lib_with_explicit_empty_lib.operation(),
        ProgramLoadOperation::ValidateOptions
    );
    let ProgramLoadError::Unsupported { feature, .. } = no_lib_with_explicit_empty_lib else {
        unreachable!("kind identifies the unsupported variant");
    };
    assert_eq!(feature, "explicit-libraries");
}

#[test]
fn unpaired_utf16_surrogate_is_a_typed_decode_failure() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", vec![0xff, 0xfe, 0x00, 0xd8])
        .build()
        .expect("build memory host");

    let error = load(&host, &["/work/root.ts"], generous_limits())
        .expect_err("Rust String cannot retain an unpaired UTF-16 surrogate");
    assert_eq!(error.kind(), ProgramLoadErrorKind::Decode);
    assert_eq!(error.operation(), ProgramLoadOperation::DecodeSource);
    assert_eq!(error.path(), Some(Path::new("/work/root.ts")));
    let ProgramLoadError::Decode { source, .. } = error else {
        unreachable!("kind identifies the decode variant");
    };
    assert_eq!(source.code_unit_index(), 0);
    assert_eq!(source.unpaired_surrogate(), 0xd800);
}

#[test]
#[cfg(unix)]
fn memory_and_filesystem_hosts_build_identical_prepared_programs() {
    let tree = TempTree::new();
    fs::create_dir(tree.path("src")).expect("create source directory");
    let package_json = br#"{"name":"host-equivalence","private":true}"#;
    let root = concat!(
        "/// <reference path=\"./path.ts\" />\n",
        "/// <reference types=\"./types\" />\n",
        "import './dependency';\n",
        "import '@src/aliased';\n",
        "import 'src/based';\n",
        "import './missing';\n",
        "export {};\n",
    );
    let dependency = "import './leaf';\nexport {};\n";
    let files = [
        ("package.json", package_json.as_slice()),
        ("src/root.ts", root.as_bytes()),
        ("src/path.ts", b"export const path = 1;".as_slice()),
        ("src/types.d.ts", b"declare const types: 1;".as_slice()),
        ("src/dependency.ts", dependency.as_bytes()),
        ("src/leaf.ts", b"export const leaf = 1;".as_slice()),
        ("src/aliased.ts", b"export const aliased = 1;".as_slice()),
        ("src/based.ts", b"export const based = 1;".as_slice()),
    ];
    for (relative, bytes) in files {
        fs::write(tree.path(relative), bytes).expect("write temp source tree");
    }

    let filesystem = FsCompilerHost::new(tree.root(), true).expect("construct filesystem host");
    let mut memory = MemoryCompilerHost::builder(tree.root()).case_sensitive(true);
    for (relative, bytes) in files {
        memory = memory.file(tree.path(relative), bytes.to_vec());
    }
    let memory = memory.build().expect("construct memory host");
    let root_path = tree.path("src/root.ts");
    let root_text = root_path.to_str().expect("temp path is Unicode");
    let program_options =
        program_options().with_paths(vec![PathMapping::new("@src/*", vec!["src/*".to_owned()])]);

    for module_resolution in [1, 2, 100] {
        let options = CompilerOptions {
            base_url: Some(tree.root().to_string_lossy().into_owned()),
            module_resolution: Some(module_resolution),
            ..compiler_options()
        };
        let from_memory = load_with_options(
            &memory,
            &[root_text],
            options.clone(),
            program_options.clone(),
            generous_limits(),
        )
        .expect("load prepared program from memory host");
        let from_filesystem = load_with_options(
            &filesystem,
            &[root_text],
            options,
            program_options.clone(),
            generous_limits(),
        )
        .expect("load prepared program from filesystem host");

        assert_eq!(from_memory, from_filesystem);
    }
}

#[test]
fn module_phase_resolves_every_key_before_traversing_the_first_target() {
    let first_read = HostError::new(
        HostErrorKind::Other,
        HostOperation::ReadFile,
        Some(PathBuf::from("/work/first.ts")),
        "first target traversal must not start",
    );
    let second_resolution = HostError::new(
        HostErrorKind::Other,
        HostOperation::FileExists,
        Some(PathBuf::from("/work/second.ts")),
        "second module resolution wins",
    );
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/root.ts",
            b"import './first';\nimport './second';\nexport {};".to_vec(),
        )
        .file("/work/first.ts", b"export {};".to_vec())
        .file("/work/second.ts", b"export {};".to_vec())
        .failure(first_read)
        .failure(second_resolution.clone())
        .build()
        .expect("build precedence host");

    let error = load(&host, &["/work/root.ts"], generous_limits())
        .expect_err("second resolution fails before first-target traversal");
    assert_resolution_host_error(
        error,
        ProgramLoadOperation::ResolveModule,
        &second_resolution,
    );
}

#[test]
fn type_phase_resolves_every_key_before_traversing_the_first_target() {
    let first_read = HostError::new(
        HostErrorKind::Other,
        HostOperation::ReadFile,
        Some(PathBuf::from("/work/first.d.ts")),
        "first type target traversal must not start",
    );
    let second_resolution = HostError::new(
        HostErrorKind::Other,
        HostOperation::FileExists,
        Some(PathBuf::from("/work/second.d.ts")),
        "second type-reference resolution wins",
    );
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/root.ts",
            concat!(
                "/// <reference types=\"./first\" />\n",
                "/// <reference types=\"./second\" />\n",
            )
            .as_bytes()
            .to_vec(),
        )
        .file("/work/first.d.ts", b"export {};".to_vec())
        .file("/work/second.d.ts", b"export {};".to_vec())
        .failure(first_read)
        .failure(second_resolution.clone())
        .build()
        .expect("build precedence host");

    let error = load(&host, &["/work/root.ts"], generous_limits())
        .expect_err("second resolution fails before first-target traversal");
    assert_resolution_host_error(
        error,
        ProgramLoadOperation::ResolveTypeReference,
        &second_resolution,
    );
}

#[test]
fn path_phase_traversal_failure_precedes_module_resolution_failure() {
    let child_read = HostError::new(
        HostErrorKind::Other,
        HostOperation::ReadFile,
        Some(PathBuf::from("/work/child.ts")),
        "path child read wins",
    );
    let module_resolution = HostError::new(
        HostErrorKind::Other,
        HostOperation::FileExists,
        Some(PathBuf::from("/work/module.ts")),
        "module resolution must not start",
    );
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/root.ts",
            concat!(
                "/// <reference path=\"./child.ts\" />\n",
                "import './module';\n",
            )
            .as_bytes()
            .to_vec(),
        )
        .file("/work/child.ts", b"export {};".to_vec())
        .file("/work/module.ts", b"export {};".to_vec())
        .failure(child_read.clone())
        .failure(module_resolution)
        .build()
        .expect("build phase-precedence host");

    let error = load(&host, &["/work/root.ts"], generous_limits())
        .expect_err("path traversal fails before the module phase");
    assert_eq!(error.kind(), ProgramLoadErrorKind::Host);
    assert_eq!(error.operation(), ProgramLoadOperation::ReadSource);
    let ProgramLoadError::Host { source, .. } = error else {
        unreachable!("kind identifies the host variant");
    };
    assert_eq!(source, child_read);
}

#[test]
fn earlier_root_read_failure_precedes_later_root_normalization_failure() {
    let first_read = HostError::new(
        HostErrorKind::Other,
        HostOperation::ReadFile,
        Some(PathBuf::from("/work/first.ts")),
        "first root read wins",
    );
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/first.ts", b"export {};".to_vec())
        .failure(first_read.clone())
        .build()
        .expect("build root-precedence host");

    let error = load(
        &host,
        &["/work/first.ts", "/work/later.js"],
        generous_limits(),
    )
    .expect_err("the first root is loaded before the second root is normalized");
    assert_eq!(error.kind(), ProgramLoadErrorKind::Host);
    assert_eq!(error.operation(), ProgramLoadOperation::ReadSource);
    let ProgramLoadError::Host { source, .. } = error else {
        unreachable!("kind identifies the host variant");
    };
    assert_eq!(source, first_read);
}

#[test]
fn later_root_promotes_its_own_emit_eligibility_but_not_external_relative_children() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", b"import 'pkg';\nexport {};".to_vec())
        .file(
            "/work/node_modules/pkg/package.json",
            br#"{"name":"pkg","version":"1.0.0","type":"commonjs","exports":"./index.ts"}"#
                .to_vec(),
        )
        .file(
            "/work/node_modules/pkg/index.ts",
            b"import './child';\nexport {};".to_vec(),
        )
        .file(
            "/work/node_modules/pkg/child.ts",
            b"export const child = 1;".to_vec(),
        )
        .build()
        .expect("build external package host");
    let options = CompilerOptions {
        no_emit: Some(true),
        module: Some(199),
        module_resolution: Some(99),
        ..CompilerOptions::default()
    };

    let external_only = load_no_lib_program(
        &host,
        &[PathBuf::from("/work/root.ts")],
        options.clone(),
        program_options(),
        generous_limits(),
    )
    .expect("load package only through an external import");
    for path in [
        "/work/node_modules/pkg/index.ts",
        "/work/node_modules/pkg/child.ts",
    ] {
        let source = external_only
            .source_files()
            .iter()
            .find(|source| source.path().display() == Path::new(path))
            .expect("external package source is loaded");
        assert!(!source.may_be_emitted(), "{path}");
    }

    let promoted = load_no_lib_program(
        &host,
        &[
            PathBuf::from("/work/root.ts"),
            PathBuf::from("/work/node_modules/pkg/index.ts"),
        ],
        options,
        program_options(),
        generous_limits(),
    )
    .expect("load the package entry again as an explicit root");
    let index = promoted
        .source_files()
        .iter()
        .find(|source| source.path().display() == Path::new("/work/node_modules/pkg/index.ts"))
        .expect("promoted package entry is loaded");
    assert!(index.may_be_emitted());
    let child = promoted
        .source_files()
        .iter()
        .find(|source| source.path().display() == Path::new("/work/node_modules/pkg/child.ts"))
        .expect("relative package child is loaded");
    assert!(!child.may_be_emitted());
}

use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(unix)]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(unix)]
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(unix)]
use std::{fs, io};

use serde_json::{json, Value};
use tsc_diagnostics::MessageChain;
#[cfg(unix)]
use tsc_host::FsCompilerHost;
use tsc_host::{CompilerHost, HostError, HostErrorKind, HostOperation, MemoryCompilerHost};
use tsc_program::{
    load_no_lib_program, plan_source_requests, CompilerOptionNumber, CompilerOptions,
    ModuleExtension, PathMapping, PreparedProgram, ProgramLoadError, ProgramLoadErrorKind,
    ProgramLoadLimit, ProgramLoadLimits, ProgramLoadOperation, ProgramOptions, ProgramPath,
    ResolutionError, ResolutionKey, ResolutionMode, ResolutionOutcome, ResolvedModuleTarget,
    TypeReferenceResolutionKey, UnloadedModuleReason,
};

const GENEROUS_LIMIT: usize = 1_024;
const TYPESCRIPT_ROOT_EXTENSION_LIST: &str =
    "'.ts', '.tsx', '.d.ts', '.cts', '.d.cts', '.mts', '.d.mts'";
const ALL_ROOT_EXTENSION_LIST: &str =
    "'.ts', '.tsx', '.d.ts', '.js', '.jsx', '.cts', '.d.cts', '.cjs', '.mts', '.d.mts', '.mjs'";
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

fn assert_source_module_target(
    program: &PreparedProgram,
    source_path: &str,
    specifier: &str,
    expected_path: &str,
) {
    let resolution = program
        .resolutions()
        .require_module(&module_key(program, source_path, specifier))
        .expect("module request has an authoritative row");
    let ResolutionOutcome::Resolved(resolved) = resolution.outcome() else {
        panic!("module request must resolve: {source_path} -> {specifier}");
    };
    let ResolvedModuleTarget::Source {
        source,
        resolved_file,
    } = resolved.target()
    else {
        panic!("module target must join source membership: {source_path} -> {specifier}");
    };
    assert_eq!(resolved_file.display(), Path::new(expected_path));
    assert_eq!(
        program.source_file(*source).unwrap().path().display(),
        Path::new(expected_path)
    );
}

fn assert_unloaded_module_target(
    program: &PreparedProgram,
    source_path: &str,
    specifier: &str,
    expected_path: &str,
    expected_reason: UnloadedModuleReason,
) {
    let resolution = program
        .resolutions()
        .require_module(&module_key(program, source_path, specifier))
        .expect("module request has an authoritative row");
    let ResolutionOutcome::Resolved(resolved) = resolution.outcome() else {
        panic!("module request must resolve: {source_path} -> {specifier}");
    };
    let ResolvedModuleTarget::Unloaded {
        resolved_file,
        reason,
    } = resolved.target()
    else {
        panic!("module target must remain unloaded: {source_path} -> {specifier}");
    };
    assert_eq!(resolved_file.display(), Path::new(expected_path));
    assert_eq!(*reason, expected_reason);
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
fn type_reference_targets_admit_implementation_and_arbitrary_typescript_sources() {
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/root.ts",
            concat!(
                "/// <reference types=\"implementation\" />\n",
                "/// <reference types=\"styles\" />\n",
                "export {};\n",
            )
            .as_bytes()
            .to_vec(),
        )
        .file("/work/automatic.ts", b"export {};\n".to_vec())
        .file(
            "/work/node_modules/@types/implementation/package.json",
            br#"{"name":"@types/implementation","version":"1.0.0","types":"index.ts"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/@types/implementation/index.ts",
            b"declare const implementation: true;".to_vec(),
        )
        .file(
            "/work/node_modules/@types/styles/package.json",
            br#"{"name":"@types/styles","version":"1.0.0","types":"index.css"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/@types/styles/index.d.css.ts",
            b"declare const styles: true;".to_vec(),
        )
        .build()
        .expect("build TypeScript type-reference targets");
    let options = CompilerOptions {
        module: Some(199),
        module_resolution: Some(99),
        ..compiler_options()
    };

    let explicit = load_with_options(
        &host,
        &["/work/root.ts"],
        options.clone(),
        program_options(),
        generous_limits(),
    )
    .expect("load explicit TypeScript type-reference targets");
    assert_eq!(
        source_paths(&explicit),
        [
            Path::new("/work/node_modules/@types/implementation/index.ts"),
            Path::new("/work/node_modules/@types/styles/index.d.css.ts"),
            Path::new("/work/root.ts"),
        ]
    );

    let automatic = load_with_options(
        &host,
        &["/work/automatic.ts"],
        options,
        ProgramOptions::default()
            .with_no_lib(true)
            .with_types(vec!["implementation".to_owned(), "styles".to_owned()]),
        generous_limits(),
    )
    .expect("load automatic TypeScript type-reference targets");
    assert_eq!(
        source_paths(&automatic),
        [
            Path::new("/work/automatic.ts"),
            Path::new("/work/node_modules/@types/implementation/index.ts"),
            Path::new("/work/node_modules/@types/styles/index.d.css.ts"),
        ]
    );
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
fn javascript_family_roots_follow_allow_js_and_gate_host_reads() {
    let roots = [
        "/work/root.js",
        "/work/root.jsx",
        "/work/root.mjs",
        "/work/root.cjs",
    ];
    let mut enabled_host = MemoryCompilerHost::builder("/work");
    for root in roots {
        enabled_host = enabled_host.file(root, Vec::new());
    }
    let enabled_host = enabled_host
        .build()
        .expect("build admitted JavaScript-root host");
    let enabled = load_with_options(
        &enabled_host,
        &roots,
        CompilerOptions {
            allow_js: true,
            ..compiler_options()
        },
        program_options(),
        generous_limits(),
    )
    .expect("allowJs admits every JavaScript-family root");

    assert_eq!(source_paths(&enabled), roots.map(Path::new));
    assert_eq!(root_paths(&enabled), roots.map(Path::new));
    assert!(enabled.roots().iter().all(|root| root.source().is_some()));
    assert!(enabled.diagnostics().program().is_empty());

    let mut disabled_host = MemoryCompilerHost::builder("/work");
    for root in roots {
        disabled_host = disabled_host.file(root, Vec::new()).failure(HostError::new(
            HostErrorKind::Other,
            HostOperation::ReadFile,
            Some(PathBuf::from(root)),
            "unsupported JavaScript roots must be gated before readFile",
        ));
    }
    let disabled_host = disabled_host
        .build()
        .expect("build preflight-guarded JavaScript-root host");
    let disabled = load(&disabled_host, &roots, generous_limits())
        .expect("allowJs=false retains rejected JavaScript roots as program facts");

    assert!(disabled.source_files().is_empty());
    assert_eq!(root_paths(&disabled), roots.map(Path::new));
    assert!(disabled.roots().iter().all(|root| {
        root.source().is_none()
            && root
                .missing_diagnostic()
                .is_some_and(|diagnostic| diagnostic.code() == 6504)
    }));
    assert_eq!(
        disabled
            .diagnostics()
            .program()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [6504, 6504, 6504, 6504]
    );
    let diagnostic = &disabled.diagnostics().program()[0];
    assert!(diagnostic.message.next_present);
    assert_eq!(diagnostic.message.next.len(), 1);
    assert_eq!(diagnostic.message.next[0].code, 1430);
    assert!(diagnostic.message.next[0].next_present);
    assert_eq!(diagnostic.message.next[0].next.len(), 1);
    assert_eq!(diagnostic.message.next[0].next[0].code, 1427);
}

#[test]
fn root_extension_preflight_handles_json_unknown_and_extensionless_boundaries() {
    let disabled_host = MemoryCompilerHost::builder("/work")
        .file("/work/data.json", br#"{"value":1}"#.to_vec())
        .file("/work/notes.txt", b"text".to_vec())
        .failure(HostError::new(
            HostErrorKind::Other,
            HostOperation::ReadFile,
            Some(PathBuf::from("/work/data.json")),
            "disabled JSON roots must be gated before readFile",
        ))
        .failure(HostError::new(
            HostErrorKind::Other,
            HostOperation::ReadFile,
            Some(PathBuf::from("/work/notes.txt")),
            "unknown roots must be gated before readFile",
        ))
        .build()
        .expect("build unsupported-root host");
    let disabled = load_with_options(
        &disabled_host,
        &["/work/data.json", "/work/notes.txt"],
        CompilerOptions {
            module: Some(1),
            module_resolution: Some(2),
            ..compiler_options()
        },
        program_options(),
        generous_limits(),
    )
    .expect("unsupported extensions remain program diagnostics");
    assert!(disabled.source_files().is_empty());
    assert_eq!(
        disabled
            .diagnostics()
            .program()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [6054, 6054]
    );
    assert!(disabled.diagnostics().program().iter().all(|diagnostic| {
        let message = diagnostic.message_text();
        message.contains(TYPESCRIPT_ROOT_EXTENSION_LIST) && !message.contains("'.js'")
    }));

    let allow_js_unknown = load_with_options(
        &disabled_host,
        &["/work/notes.txt"],
        CompilerOptions {
            allow_js: true,
            module: Some(1),
            module_resolution: Some(2),
            ..compiler_options()
        },
        program_options(),
        generous_limits(),
    )
    .expect("allowJs still rejects unknown root extensions before readFile");
    assert_eq!(allow_js_unknown.diagnostics().program()[0].code(), 6054);
    assert!(allow_js_unknown.diagnostics().program()[0]
        .message_text()
        .contains(ALL_ROOT_EXTENSION_LIST));

    let enabled_host = MemoryCompilerHost::builder("/work")
        .file("/work/data.json", br#"{"value":1}"#.to_vec())
        .build()
        .expect("build admitted JSON-root host");
    let enabled = load_with_options(
        &enabled_host,
        &["/work/data.json"],
        CompilerOptions {
            module: Some(1),
            module_resolution: Some(2),
            resolve_json_module: Some(true),
            ..compiler_options()
        },
        program_options(),
        generous_limits(),
    )
    .expect("effective resolveJsonModule admits an explicit JSON root");
    assert_eq!(source_paths(&enabled), [Path::new("/work/data.json")]);
    assert!(!enabled.source_files()[0].may_be_emitted());
    assert_eq!(enabled.resolutions().module_len(), 0);

    let extensionless_host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", b"export {};".to_vec())
        .build()
        .expect("build extensionless-root host");
    let extensionless = load(&extensionless_host, &["/work/root"], generous_limits())
        .expect("extensionless roots probe the first TypeScript extension group");
    assert_eq!(source_paths(&extensionless), [Path::new("/work/root.ts")]);
    assert_eq!(root_paths(&extensionless), [Path::new("/work/root")]);
    let root_source = extensionless.roots()[0]
        .source()
        .expect("extensionless root retains its resolved source identity");
    assert_eq!(
        extensionless
            .source_file(root_source)
            .expect("root source belongs to the prepared program")
            .path()
            .display(),
        Path::new("/work/root.ts")
    );
    assert!(extensionless.diagnostics().program().is_empty());
}

#[test]
fn extensionless_roots_preserve_requests_probe_first_group_and_report_ts6231() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/ts.ts", b"export const winner = 'ts';".to_vec())
        .file("/work/ts.tsx", b"export const mustNotWin = 'tsx';".to_vec())
        .file(
            "/work/ts.d.ts",
            b"export declare const mustNotWin: 'dts';".to_vec(),
        )
        .file("/work/tsx.tsx", b"export const winner = 'tsx';".to_vec())
        .file(
            "/work/tsx.d.ts",
            b"export declare const mustNotWin: 'dts';".to_vec(),
        )
        .file(
            "/work/dts.d.ts",
            b"export declare const winner: 'dts';".to_vec(),
        )
        .file(
            "/work/missing.cts",
            b"export const mustNotBeProbed = 'cts';".to_vec(),
        )
        .file(
            "/work/missing.mts",
            b"export const mustNotBeProbed = 'mts';".to_vec(),
        )
        .build()
        .expect("build extensionless TypeScript-root host");

    let program = load(
        &host,
        &[
            "/work/ts",
            "/work/ts.ts",
            "/work/tsx",
            "/work/dts",
            "/work/missing",
        ],
        generous_limits(),
    )
    .expect("extensionless TypeScript roots follow getSourceFileFromReferenceWorker");

    assert_eq!(
        source_paths(&program),
        [
            Path::new("/work/ts.ts"),
            Path::new("/work/tsx.tsx"),
            Path::new("/work/dts.d.ts"),
        ]
    );
    assert_eq!(
        root_paths(&program),
        [
            Path::new("/work/ts"),
            Path::new("/work/ts.ts"),
            Path::new("/work/tsx"),
            Path::new("/work/dts"),
            Path::new("/work/missing"),
        ]
    );
    assert_eq!(program.roots()[0].source(), program.roots()[1].source());
    assert!(program.roots()[2].source().is_some());
    assert!(program.roots()[3].source().is_some());
    assert!(program.roots()[4].source().is_none());
    let diagnostics = program.diagnostics().program();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code(), 6231);
    assert!(diagnostics[0]
        .message_text()
        .contains("Could not resolve the path '/work/missing'"));
    assert!(diagnostics[0]
        .message_text()
        .contains(TYPESCRIPT_ROOT_EXTENSION_LIST));
    assert!(diagnostics[0].message.next_present);
    assert_eq!(diagnostics[0].message.next.len(), 1);
    assert_eq!(diagnostics[0].message.next[0].code, 1430);
    assert!(diagnostics[0].message.next[0].next_present);
    assert_eq!(diagnostics[0].message.next[0].next.len(), 1);
    assert_eq!(diagnostics[0].message.next[0].next[0].code, 1427);

    let allow_js_host = MemoryCompilerHost::builder("/work")
        .file("/work/js.js", b"exports.winner = 'js';".to_vec())
        .file("/work/js.jsx", b"exports.mustNotWin = 'jsx';".to_vec())
        .file("/work/jsx.jsx", b"exports.winner = 'jsx';".to_vec())
        .file(
            "/work/cjs.cjs",
            b"exports.mustNotBeProbed = 'cjs';".to_vec(),
        )
        .file("/work/json.json", br#"{"mustNotBeProbed":true}"#.to_vec())
        .build()
        .expect("build extensionless JavaScript-root host");
    let allow_js = load_with_options(
        &allow_js_host,
        &["/work/js", "/work/jsx", "/work/cjs", "/work/json"],
        CompilerOptions {
            allow_js: true,
            resolve_json_module: Some(true),
            ..compiler_options()
        },
        program_options(),
        generous_limits(),
    )
    .expect("allowJs extends only the first extensionless probe group");
    assert_eq!(
        source_paths(&allow_js),
        [Path::new("/work/js.js"), Path::new("/work/jsx.jsx")]
    );
    assert_eq!(
        allow_js
            .diagnostics()
            .program()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [6231, 6231]
    );
    assert!(allow_js
        .diagnostics()
        .program()
        .iter()
        .all(|diagnostic| { diagnostic.message_text().contains(ALL_ROOT_EXTENSION_LIST) }));

    let read_failure = HostError::new(
        HostErrorKind::Other,
        HostOperation::ReadFile,
        Some(PathBuf::from("/work/stop.ts")),
        "first extensionless candidate failed",
    );
    let failing_host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/stop.tsx",
            b"export const mustNotBeRead = true;".to_vec(),
        )
        .failure(read_failure.clone())
        .build()
        .expect("build extensionless failure-order host");
    let error = load(&failing_host, &["/work/stop"], generous_limits())
        .expect_err("the first host failure stops later extensionless probes");
    let ProgramLoadError::Host { source, .. } = error else {
        panic!("extensionless root reads retain typed host failures");
    };
    assert_eq!(source, read_failure);
}

#[test]
fn extensionless_root_trailing_separator_is_part_of_the_probe_spelling() {
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/directory/.ts",
            b"export const winner = 'directory';".to_vec(),
        )
        .file(
            "/work/directory.ts",
            b"export const mustNotWin = 'sibling';".to_vec(),
        )
        .build()
        .expect("build trailing-separator root host");

    let program = load(&host, &["/work/directory/"], generous_limits())
        .expect("a root trailing separator is retained before adding .ts");
    assert_eq!(source_paths(&program), [Path::new("/work/directory/.ts")]);
    assert_eq!(
        program.roots()[0].path().display().to_str(),
        Some("/work/directory/")
    );
}

#[test]
fn extensionless_missing_roots_deduplicate_by_display_text_not_path_components() {
    let host = MemoryCompilerHost::builder("/work")
        .build()
        .expect("build empty extensionless root host");
    let program = load(
        &host,
        &["/work/missing", "/work/missing/"],
        generous_limits(),
    )
    .expect("trailing separator variants retain distinct TS6231 diagnostics");

    assert_eq!(program.roots().len(), 2);
    assert_eq!(program.diagnostics().program().len(), 2);
    assert!(program.diagnostics().program().iter().any(|diagnostic| {
        diagnostic
            .message_text()
            .contains("Could not resolve the path '/work/missing' with")
    }));
    assert!(program.diagnostics().program().iter().any(|diagnostic| {
        diagnostic
            .message_text()
            .contains("Could not resolve the path '/work/missing/' with")
    }));
}

#[test]
fn case_insensitive_javascript_root_uses_canonical_extension_for_ts6504() {
    let host = MemoryCompilerHost::builder("/Work")
        .case_sensitive(false)
        .file("/Work/ROOT.JS", Vec::new())
        .failure(HostError::new(
            HostErrorKind::Other,
            HostOperation::ReadFile,
            Some(PathBuf::from("/Work/ROOT.JS")),
            "unsupported uppercase JavaScript roots must not be read",
        ))
        .build()
        .expect("build case-insensitive JavaScript-root host");

    let program = load(&host, &["/Work/ROOT.JS"], generous_limits())
        .expect("case-insensitive JavaScript roots retain a TS6504 fact");
    assert!(program.source_files().is_empty());
    assert_eq!(program.diagnostics().program()[0].code(), 6504);
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
fn loaded_case_alias_reports_ts1149_and_retains_the_alternate_spelling() {
    let host = MemoryCompilerHost::builder("/Work")
        .case_sensitive(false)
        .file("/Work/Root.ts", b"export {};".to_vec())
        .build()
        .expect("build case-insensitive source host");

    let program = load(
        &host,
        &["/Work/Root.ts", "/work/root.ts"],
        generous_limits(),
    )
    .expect("case aliases remain one source with a tsc diagnostic");
    assert_eq!(program.source_files().len(), 1);
    assert_eq!(
        program.source_files()[0].alternate_display_paths(),
        [Path::new("/work/root.ts")]
    );
    assert_eq!(program.diagnostics().program().len(), 1);
    let diagnostic = &program.diagnostics().program()[0];
    assert_eq!(diagnostic.code(), 1149);
    assert_eq!(diagnostic.file_name, None);
    assert_eq!(diagnostic.message.next.len(), 1);
    assert_eq!(diagnostic.message.next[0].code, 1430);
    assert_eq!(diagnostic.message.next[0].next.len(), 2);
    assert!(diagnostic.message.next[0]
        .next
        .iter()
        .all(|reason| reason.code == 1427));
    assert!(diagnostic.message_text().contains("/work/root.ts"));
}

#[test]
fn path_reference_case_alias_reports_ts1149_outside_the_root_boundary() {
    let root_text = concat!(
        "/// <reference path=\"./Child.ts\" />\n",
        "/// <reference path=\"./child.ts\" />\n",
    );
    let host = MemoryCompilerHost::builder("/Work")
        .case_sensitive(false)
        .file("/Work/Root.ts", root_text.as_bytes().to_vec())
        .file("/Work/Child.ts", b"export {};".to_vec())
        .build()
        .expect("build case-insensitive path-reference host");

    let program = load(&host, &["/Work/Root.ts"], generous_limits())
        .expect("path-reference aliases remain one source with a tsc diagnostic");
    assert_eq!(program.source_files().len(), 2);
    assert_eq!(
        program.source_files()[0].alternate_display_paths(),
        [Path::new("/Work/child.ts")]
    );
    assert_eq!(program.diagnostics().program().len(), 1);
    let diagnostic = &program.diagnostics().program()[0];
    assert_eq!(diagnostic.code(), 1149);
    let start = root_text.find("./child.ts").expect("reference span") as u32;
    assert_eq!(diagnostic.file_name.as_deref(), Some("/Work/Root.ts"));
    assert_eq!(diagnostic.start, Some(start));
    assert_eq!(diagnostic.length, Some("./child.ts".len() as u32));
}

#[test]
fn explicit_false_force_consistent_casing_suppresses_alias_diagnostic() {
    let host = MemoryCompilerHost::builder("/Work")
        .case_sensitive(false)
        .file("/Work/Root.ts", b"export {};".to_vec())
        .build()
        .expect("build case-insensitive source host");
    let mut options = compiler_options();
    options.force_consistent_casing_in_file_names = Some(false);
    let program = load_with_options(
        &host,
        &["/Work/Root.ts", "/work/root.ts"],
        options,
        program_options(),
        generous_limits(),
    )
    .expect("explicit false keeps the collapsed source valid");
    assert!(program.diagnostics().program().is_empty());
    assert_eq!(
        program.source_files()[0].alternate_display_paths(),
        [Path::new("/work/root.ts")]
    );
}

#[test]
fn case_sensitive_files_differing_only_in_case_remain_distinct_and_report_tsc_casing_errors() {
    const IMPORTER: &str = "import { value } from './Value';\nvoid value;\n";
    let cases = [
        (
            ["/project/Value.ts", "/project/value.ts"],
            ["/project/Value.ts", "/project/value.ts"].as_slice(),
            1149,
            None,
            "/project/value.ts",
            "/project/Value.ts",
            [1427, 1427].as_slice(),
        ),
        (
            ["/project/value.ts", "/project/Value.ts"],
            ["/project/value.ts", "/project/Value.ts"].as_slice(),
            1149,
            None,
            "/project/Value.ts",
            "/project/value.ts",
            [1427, 1427].as_slice(),
        ),
        (
            ["/project/main.ts", "/project/value.ts"],
            ["/project/Value.ts", "/project/main.ts", "/project/value.ts"].as_slice(),
            1261,
            Some(("/project/main.ts", 22, 9)),
            "/project/Value.ts",
            "/project/value.ts",
            [1393, 1427].as_slice(),
        ),
        (
            ["/project/value.ts", "/project/main.ts"],
            ["/project/value.ts", "/project/Value.ts", "/project/main.ts"].as_slice(),
            1149,
            Some(("/project/main.ts", 22, 9)),
            "/project/Value.ts",
            "/project/value.ts",
            [1427, 1393].as_slice(),
        ),
    ];

    for (roots, expected_sources, code, location, first_name, second_name, reason_codes) in cases {
        let host = MemoryCompilerHost::builder("/project")
            .case_sensitive(true)
            .file("/project/main.ts", IMPORTER.as_bytes().to_vec())
            .file("/project/Value.ts", b"export const value = 1;\n".to_vec())
            .file("/project/value.ts", b"export const value = 2;\n".to_vec())
            .build()
            .expect("build case-sensitive source host");
        let mut options = compiler_options();
        // tsc's case-sensitive `filesByNameIgnoreCase` check is independent
        // of this option; false only suppresses host-collapsed aliases.
        options.force_consistent_casing_in_file_names = Some(false);
        let program =
            load_with_options(&host, &roots, options, program_options(), generous_limits())
                .expect("distinct case-sensitive files remain a valid program");

        assert_eq!(
            source_paths(&program),
            expected_sources
                .iter()
                .map(|path| Path::new(*path))
                .collect::<Vec<_>>()
        );
        let [diagnostic] = program.diagnostics().program() else {
            panic!("case-only physical collision must publish one diagnostic");
        };
        assert_eq!(diagnostic.code(), code);
        assert!(diagnostic.message_text().contains(first_name));
        assert!(diagnostic.message_text().contains(second_name));
        match location {
            Some((file, start, length)) => {
                assert_eq!(diagnostic.file_name.as_deref(), Some(file));
                assert_eq!(diagnostic.start, Some(start));
                assert_eq!(diagnostic.length, Some(length));
            }
            None => {
                assert_eq!(diagnostic.file_name, None);
                assert_eq!(diagnostic.start, None);
                assert_eq!(diagnostic.length, None);
            }
        }
        assert_eq!(diagnostic.message.next.len(), 1);
        assert_eq!(diagnostic.message.next[0].code, 1430);
        assert_eq!(
            diagnostic.message.next[0]
                .next
                .iter()
                .map(|reason| reason.code)
                .collect::<Vec<_>>(),
            reason_codes
        );
        assert!(!diagnostic.related_information_present);
        assert!(diagnostic.related.is_empty());
    }
}

#[test]
#[ignore = "local H0 program oracle audit; requires the pinned Node runtime"]
fn case_sensitive_casing_collision_matrix_matches_vendored_typescript() {
    const PROBE: &str = r#"
const ts = require(process.argv[1]);
const files = new Map([
  ['/project/main.ts', "import { value } from './Value';\nvoid value;\n"],
  ['/project/Value.ts', 'export const value = 1;\n'],
  ['/project/value.ts', 'export const value = 2;\n'],
]);
const cases = [
  ['/project/Value.ts', '/project/value.ts'],
  ['/project/value.ts', '/project/Value.ts'],
  ['/project/main.ts', '/project/value.ts'],
  ['/project/value.ts', '/project/main.ts'],
];
function chain(message) {
  if (typeof message === 'string') return { code: null, text: message, next: null };
  return {
    code: message.code,
    text: message.messageText,
    next: message.next === undefined ? null : message.next.map(chain),
  };
}
function probe(rootNames) {
  const options = {
    noEmit: true,
    noLib: true,
    types: [],
    forceConsistentCasingInFileNames: false,
    module: ts.ModuleKind.CommonJS,
    moduleResolution: ts.ModuleResolutionKind.Node10,
  };
  const host = ts.createCompilerHost(options);
  host.useCaseSensitiveFileNames = () => true;
  host.getCanonicalFileName = path => path;
  host.realpath = path => path;
  host.getCurrentDirectory = () => '/project';
  host.directoryExists = path => path === '/project';
  host.getDirectories = () => [];
  host.fileExists = path => files.has(path);
  host.readFile = path => files.get(path);
  host.getSourceFile = (path, target) => files.has(path)
    ? ts.createSourceFile(path, files.get(path), target, true)
    : undefined;
  const program = ts.createProgram({ rootNames, options, host });
  return {
    sources: program.getSourceFiles().map(source => source.fileName),
    diagnostics: ts.getPreEmitDiagnostics(program)
      .filter(diagnostic => diagnostic.code === 1149 || diagnostic.code === 1261)
      .map(diagnostic => ({
        code: diagnostic.code,
        file: diagnostic.file ? diagnostic.file.fileName : null,
        start: diagnostic.start === undefined ? null : diagnostic.start,
        length: diagnostic.length === undefined ? null : diagnostic.length,
        message: chain(diagnostic.messageText),
        relatedPresent: diagnostic.relatedInformation !== undefined,
        related: (diagnostic.relatedInformation || []).map(related => ({
          code: related.code,
          file: related.file ? related.file.fileName : null,
          start: related.start === undefined ? null : related.start,
          length: related.length === undefined ? null : related.length,
          message: chain(related.messageText),
        })),
      })),
  };
}
process.stdout.write(JSON.stringify(cases.map(probe)));
"#;

    fn chain_json(message: &MessageChain) -> Value {
        json!({
            "code": message.code,
            "text": message.text,
            "next": message.next_present.then(|| {
                message.next.iter().map(chain_json).collect::<Vec<_>>()
            }),
        })
    }

    let bundle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("vendor/typescript-6.0.3/lib/typescript.js");
    let output = Command::new("node")
        .arg("-e")
        .arg(PROBE)
        .arg(bundle)
        .output()
        .expect("run vendored TypeScript case-sensitive casing probe");
    assert!(
        output.status.success(),
        "TypeScript probe failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let oracle: Value = serde_json::from_slice(&output.stdout).expect("probe output is JSON");

    let roots = [
        ["/project/Value.ts", "/project/value.ts"],
        ["/project/value.ts", "/project/Value.ts"],
        ["/project/main.ts", "/project/value.ts"],
        ["/project/value.ts", "/project/main.ts"],
    ];
    let rust = roots
        .iter()
        .map(|roots| {
            let host = MemoryCompilerHost::builder("/project")
                .case_sensitive(true)
                .file(
                    "/project/main.ts",
                    b"import { value } from './Value';\nvoid value;\n".to_vec(),
                )
                .file("/project/Value.ts", b"export const value = 1;\n".to_vec())
                .file("/project/value.ts", b"export const value = 2;\n".to_vec())
                .build()
                .expect("build Rust case-sensitive oracle host");
            let program = load_with_options(
                &host,
                roots,
                CompilerOptions {
                    force_consistent_casing_in_file_names: Some(false),
                    module: Some(1),
                    module_resolution: Some(2),
                    ..compiler_options()
                },
                program_options(),
                generous_limits(),
            )
            .expect("load Rust case-sensitive oracle program");
            json!({
                "sources": program.source_files().iter().map(|source| {
                    source.path().display().to_str().expect("source path is Unicode")
                }).collect::<Vec<_>>(),
                "diagnostics": program.diagnostics().program().iter()
                    .filter(|diagnostic| matches!(diagnostic.code(), 1149 | 1261))
                    .map(|diagnostic| json!({
                        "code": diagnostic.code(),
                        "file": diagnostic.file_name,
                        "start": diagnostic.start,
                        "length": diagnostic.length,
                        "message": chain_json(&diagnostic.message),
                        "relatedPresent": diagnostic.related_information_present,
                        "related": diagnostic.related.iter().map(|related| json!({
                            "code": related.message.code,
                            "file": related.file_name,
                            "start": related.start,
                            "length": related.length,
                            "message": chain_json(&related.message),
                        })).collect::<Vec<_>>(),
                    })).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(json!(rust), oracle);
}

#[test]
fn arbitrary_declaration_roots_and_explicit_paths_are_regular_typescript_sources() {
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/root.d.css.ts",
            b"/// <reference path=\"./dependency.d.json.ts\" />\nexport {};\n".to_vec(),
        )
        .file(
            "/work/dependency.d.json.ts",
            b"declare const dependency: true;\n".to_vec(),
        )
        .build()
        .expect("build arbitrary declaration root host");

    let program = load(&host, &["/work/root.d.css.ts"], generous_limits())
        .expect("arbitrary declaration roots and paths use their final .ts extension");
    assert_eq!(
        source_paths(&program),
        [
            Path::new("/work/dependency.d.json.ts"),
            Path::new("/work/root.d.css.ts"),
        ]
    );
    assert!(program.diagnostics().program().is_empty());
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

    let allow_js_unknown = load_with_options(
        &host,
        &["/work/text-root.ts"],
        CompilerOptions {
            allow_js: true,
            ..compiler_options()
        },
        program_options(),
        generous_limits(),
    )
    .expect("allowJs keeps unknown explicit path references diagnostic-only");
    let diagnostic = &allow_js_unknown.diagnostics().program()[0];
    assert_eq!(diagnostic.code(), 6054);
    assert!(diagnostic.message_text().contains(ALL_ROOT_EXTENSION_LIST));
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
fn empty_path_references_probe_the_containing_directory_and_report_located_ts6231() {
    let root_text = "/// <reference path=\"\" />\nexport {};\n";
    let missing_host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", root_text.as_bytes().to_vec())
        .build()
        .expect("build empty-path-reference host");
    let missing = load(&missing_host, &["/work/main.ts"], generous_limits())
        .expect("an empty path reference is a normal extensionless miss");

    assert_eq!(source_paths(&missing), [Path::new("/work/main.ts")]);
    let [diagnostic] = missing.diagnostics().program() else {
        panic!("empty path reference must publish one TS6231 diagnostic");
    };
    assert_eq!(diagnostic.code(), 6231);
    assert_eq!(diagnostic.file_name.as_deref(), Some("/work/main.ts"));
    let empty_value = root_text.find("\"\"").expect("empty reference literal") as u32 + 1;
    assert_eq!(diagnostic.start, Some(empty_value));
    assert_eq!(diagnostic.length, Some(0));
    assert_eq!(
        diagnostic.message_text(),
        "Could not resolve the path '/work' with the extensions: '.ts', '.tsx', '.d.ts', '.cts', '.d.cts', '.mts', '.d.mts'."
    );

    let resolved_host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", root_text.as_bytes().to_vec())
        .file("/work.ts", b"export {};".to_vec())
        .build()
        .expect("build resolved empty-path-reference host");
    let resolved = load(&resolved_host, &["/work/main.ts"], generous_limits())
        .expect("the containing directory participates in extensionless probing");
    assert_eq!(
        source_paths(&resolved),
        [Path::new("/work.ts"), Path::new("/work/main.ts")]
    );
    assert!(resolved.diagnostics().program().is_empty());
}

#[test]
#[ignore = "local H0 program oracle audit; requires the pinned Node runtime"]
fn empty_path_reference_diagnostic_matches_vendored_typescript() {
    const PROBE: &str = r#"
const ts = require(process.argv[1]);
const text = '/// <reference path="" />\nexport {};\n';
const options = { noEmit: true, noLib: true, types: [] };
const host = ts.createCompilerHost(options);
host.getCurrentDirectory = () => '/work';
host.fileExists = path => path === '/work/main.ts';
host.readFile = path => path === '/work/main.ts' ? text : undefined;
host.directoryExists = path => path === '/work';
host.getDirectories = () => [];
host.getSourceFile = (path, target) => path === '/work/main.ts'
  ? ts.createSourceFile(path, text, target, true)
  : undefined;
const program = ts.createProgram({ rootNames: ['/work/main.ts'], options, host });
const diagnostic = ts.getPreEmitDiagnostics(program).find(row => row.code === 6231);
if (!diagnostic) throw new Error('missing TS6231');
process.stdout.write(JSON.stringify({
  code: diagnostic.code,
  file: diagnostic.file && diagnostic.file.fileName,
  start: diagnostic.start,
  length: diagnostic.length,
  message: ts.flattenDiagnosticMessageText(diagnostic.messageText, '\n'),
}));
"#;
    let bundle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("vendor/typescript-6.0.3/lib/typescript.js");
    let output = Command::new("node")
        .arg("-e")
        .arg(PROBE)
        .arg(bundle)
        .output()
        .expect("run vendored TypeScript empty-path probe");
    assert!(
        output.status.success(),
        "TypeScript probe failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let oracle: Value = serde_json::from_slice(&output.stdout).expect("probe output is JSON");

    let text = "/// <reference path=\"\" />\nexport {};\n";
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", text.as_bytes().to_vec())
        .build()
        .expect("build Rust empty-path oracle host");
    let program = load(&host, &["/work/main.ts"], generous_limits())
        .expect("load Rust empty-path oracle program");
    let diagnostic = program
        .diagnostics()
        .program()
        .iter()
        .find(|row| row.code() == 6231)
        .expect("Rust publishes TS6231");
    assert_eq!(
        json!({
            "code": diagnostic.code(),
            "file": diagnostic.file_name,
            "start": diagnostic.start,
            "length": diagnostic.length,
            "message": diagnostic.message_text(),
        }),
        oracle
    );
}

#[test]
fn allow_js_path_references_admit_explicit_and_extensionless_js_after_ts() {
    let explicit_root = "/// <reference path=\"./explicit.cjs\" />\n";
    let javascript_root = "/// <reference path=\"./javascript\" />\n";
    let typescript_root = "/// <reference path=\"./priority\" />\n";
    let jsx_root = "/// <reference path=\"./jsx-only\" />\n";
    let missing_root = "/// <reference path=\"./missing\" />\n";
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/explicit-root.ts", explicit_root.as_bytes().to_vec())
        .file(
            "/work/javascript-root.ts",
            javascript_root.as_bytes().to_vec(),
        )
        .file(
            "/work/typescript-root.ts",
            typescript_root.as_bytes().to_vec(),
        )
        .file("/work/jsx-root.ts", jsx_root.as_bytes().to_vec())
        .file("/work/missing-root.ts", missing_root.as_bytes().to_vec())
        .file("/work/explicit.cjs", b"module.exports = 1;".to_vec())
        .file("/work/javascript.js", b"exports.value = 1;".to_vec())
        .file(
            "/work/javascript.jsx",
            b"export const mustNotWin = 1;".to_vec(),
        )
        .file("/work/priority.ts", b"export const winner = 1;".to_vec())
        .file("/work/priority.js", b"exports.mustNotWin = 1;".to_vec())
        .file("/work/jsx-only.jsx", b"export const jsx = 1;".to_vec())
        .file(
            "/work/missing.cjs",
            b"module.exports = 'must not be probed';".to_vec(),
        )
        .file(
            "/work/missing.mjs",
            b"export const mustNotBeProbed = true;".to_vec(),
        )
        .build()
        .expect("build allowJs path-reference host");

    let program = load_with_options(
        &host,
        &[
            "/work/explicit-root.ts",
            "/work/javascript-root.ts",
            "/work/typescript-root.ts",
            "/work/jsx-root.ts",
            "/work/missing-root.ts",
        ],
        CompilerOptions {
            allow_js: true,
            ..compiler_options()
        },
        program_options(),
        generous_limits(),
    )
    .expect("allowJs path references join source membership");

    assert_eq!(
        source_paths(&program),
        [
            Path::new("/work/explicit.cjs"),
            Path::new("/work/explicit-root.ts"),
            Path::new("/work/javascript.js"),
            Path::new("/work/javascript-root.ts"),
            Path::new("/work/priority.ts"),
            Path::new("/work/typescript-root.ts"),
            Path::new("/work/jsx-only.jsx"),
            Path::new("/work/jsx-root.ts"),
            Path::new("/work/missing-root.ts"),
        ]
    );
    let diagnostics = program.diagnostics().program();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code(), 6231);
    assert!(diagnostics[0]
        .message_text()
        .contains(ALL_ROOT_EXTENSION_LIST));
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
fn no_resolve_keeps_module_resolution_but_skips_reference_source_discovery() {
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/root.ts",
            concat!(
                "/// <reference path=\"./path.ts\" />\n",
                "/// <reference types=\"pkg\" />\n",
                "import { value } from './dependency';\n",
                "export { value };\n",
            )
            .as_bytes()
            .to_vec(),
        )
        .file("/work/path.ts", b"export const path = true;".to_vec())
        .file("/work/dependency.ts", b"export const value = 1;".to_vec())
        .file(
            "/work/node_modules/@types/pkg/index.d.ts",
            b"export {};".to_vec(),
        )
        .build()
        .expect("build noResolve host");

    let program = load_with_options(
        &host,
        &["/work/root.ts"],
        CompilerOptions {
            module: Some(1),
            module_resolution: Some(2),
            no_resolve: Some(true),
            ..compiler_options()
        },
        program_options(),
        generous_limits(),
    )
    .expect("noResolve still resolves module requests");

    assert_eq!(source_paths(&program), [Path::new("/work/root.ts")]);
    assert_eq!(program.resolutions().type_reference_len(), 0);
    let key = module_key(&program, "/work/root.ts", "./dependency");
    let resolution = program
        .resolutions()
        .require_module(&key)
        .expect("noResolve module request has an authoritative row");
    let ResolutionOutcome::Resolved(resolved) = resolution.outcome() else {
        panic!("noResolve module request must resolve");
    };
    let ResolvedModuleTarget::Unloaded {
        resolved_file,
        reason,
    } = resolved.target()
    else {
        panic!("noResolve must keep the target out of source membership");
    };
    assert_eq!(resolved_file.display(), Path::new("/work/dependency.ts"));
    assert_eq!(*reason, UnloadedModuleReason::NoResolve);
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
    let ResolvedModuleTarget::Unloaded {
        resolved_file: path,
        reason,
    } = javascript.target()
    else {
        panic!("allowJs=false must keep the JavaScript target out of source membership");
    };
    assert_eq!(*reason, UnloadedModuleReason::JavaScriptNotAdmitted);
    assert_eq!(path.display(), Path::new("/work/dependency.js"));
}

#[test]
fn allow_js_loads_local_javascript_dependencies_in_postorder_and_binds_source_rows() {
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/root.ts",
            b"import './dependency.js';\nexport {};\n".to_vec(),
        )
        .file(
            "/work/dependency.js",
            b"import './leaf.cjs';\nexport const dependency = 1;\n".to_vec(),
        )
        .file("/work/leaf.cjs", b"module.exports = 1;\n".to_vec())
        .build()
        .expect("build local JavaScript dependency host");
    let program = load_with_options(
        &host,
        &["/work/root.ts"],
        CompilerOptions {
            allow_js: true,
            module: Some(1),
            module_resolution: Some(2),
            ..compiler_options()
        },
        program_options(),
        generous_limits(),
    )
    .expect("allowJs loads the local JavaScript dependency closure");

    assert_eq!(
        source_paths(&program),
        [
            Path::new("/work/leaf.cjs"),
            Path::new("/work/dependency.js"),
            Path::new("/work/root.ts"),
        ]
    );
    assert_eq!(program.resolutions().module_len(), 2);
    for (source_path, specifier, expected_path) in [
        ("/work/root.ts", "./dependency.js", "/work/dependency.js"),
        ("/work/dependency.js", "./leaf.cjs", "/work/leaf.cjs"),
    ] {
        let key = module_key(&program, source_path, specifier);
        let resolution = program
            .resolutions()
            .require_module(&key)
            .expect("local JavaScript request has an authoritative row");
        let ResolutionOutcome::Resolved(resolved) = resolution.outcome() else {
            panic!("local JavaScript request must resolve: {specifier}");
        };
        let ResolvedModuleTarget::Source {
            source,
            resolved_file,
        } = resolved.target()
        else {
            panic!("allowJs local target must join source membership: {specifier}");
        };
        assert_eq!(resolved_file.display(), Path::new(expected_path));
        assert_eq!(
            program.source_file(*source).unwrap().path().display(),
            Path::new(expected_path)
        );
    }
}

#[test]
fn allow_js_loader_consumes_the_complete_written_jsx_replacement_group() {
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/root.ts",
            b"import './target.jsx';\nexport {};\n".to_vec(),
        )
        .file("/work/target.js", b"export const target = true;\n".to_vec())
        .build()
        .expect("build written-JSX loader host");
    let program = load_with_options(
        &host,
        &["/work/root.ts"],
        CompilerOptions {
            allow_js: true,
            module: Some(99),
            module_resolution: Some(100),
            ..compiler_options()
        },
        program_options(),
        generous_limits(),
    )
    .expect("the written JSX request loads its JavaScript replacement");

    assert_eq!(
        source_paths(&program),
        [Path::new("/work/target.js"), Path::new("/work/root.ts")]
    );
    let resolution = program
        .resolutions()
        .require_module(&module_key(&program, "/work/root.ts", "./target.jsx"))
        .expect("written JSX request has an authoritative row");
    let ResolutionOutcome::Resolved(resolved) = resolution.outcome() else {
        panic!("written JSX request must resolve");
    };
    let ResolvedModuleTarget::Source {
        source,
        resolved_file,
    } = resolved.target()
    else {
        panic!("allowJs admits the replacement JavaScript source");
    };
    assert_eq!(resolved_file.display(), Path::new("/work/target.js"));
    assert_eq!(
        program.source_file(*source).unwrap().path().display(),
        Path::new("/work/target.js")
    );
    assert!(!resolved.is_external_library_import());
    assert!(!resolved.resolved_using_ts_extension());
}

#[test]
fn arbitrary_declaration_twins_follow_the_resolution_diagnostic_admission_boundary() {
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/root.ts",
            b"import './data.json';\nexport {};\n".to_vec(),
        )
        .file(
            "/work/root.d.ts",
            b"import './data.json';\nexport {};\n".to_vec(),
        )
        .file(
            "/work/augmentation.ts",
            b"export {};\ndeclare module './data.json' { export const value: true; }\n".to_vec(),
        )
        .file(
            "/work/data.d.json.ts",
            b"import './leaf.json';\nexport {};\n".to_vec(),
        )
        .file("/work/leaf.d.json.ts", b"export {};\n".to_vec())
        .build()
        .expect("build arbitrary declaration-twin loader host");
    let base_options = CompilerOptions {
        module: Some(1),
        module_resolution: Some(2),
        resolve_json_module: Some(false),
        ..compiler_options()
    };

    let disabled = load_with_options(
        &host,
        &["/work/root.ts"],
        base_options.clone(),
        program_options(),
        generous_limits(),
    )
    .expect("retain a disabled arbitrary-extension resolution without loading its source");
    assert_eq!(source_paths(&disabled), [Path::new("/work/root.ts")]);
    let resolution = disabled
        .resolutions()
        .require_module(&module_key(&disabled, "/work/root.ts", "./data.json"))
        .expect("disabled arbitrary-extension request has an authoritative row");
    let ResolutionOutcome::Resolved(resolved) = resolution.outcome() else {
        panic!("the declaration twin must resolve");
    };
    assert_eq!(
        resolved.extension(),
        &ModuleExtension::Arbitrary(".d.json.ts".to_owned())
    );
    let ResolvedModuleTarget::Unloaded { reason, .. } = resolved.target() else {
        panic!("TS6263 prevents source membership when the option is disabled");
    };
    assert_eq!(
        *reason,
        UnloadedModuleReason::ArbitraryExtensionWithoutOption
    );

    let augmentation = load_with_options(
        &host,
        &["/work/augmentation.ts"],
        base_options.clone(),
        program_options(),
        generous_limits(),
    )
    .expect("an augmentation retains a resolution-only arbitrary row");
    assert_eq!(
        source_paths(&augmentation),
        [Path::new("/work/augmentation.ts")]
    );
    let resolution = augmentation
        .resolutions()
        .require_module(&module_key(
            &augmentation,
            "/work/augmentation.ts",
            "./data.json",
        ))
        .expect("augmentation-only arbitrary request has an authoritative row");
    let ResolutionOutcome::Resolved(resolved) = resolution.outcome() else {
        panic!("the augmentation declaration twin must resolve");
    };
    let ResolvedModuleTarget::Unloaded { reason, .. } = resolved.target() else {
        panic!("an augmentation-only target does not join source membership");
    };
    assert_eq!(*reason, UnloadedModuleReason::ResolutionOnly);

    let enabled = load_with_options(
        &host,
        &["/work/root.ts"],
        CompilerOptions {
            allow_arbitrary_extensions: Some(true),
            ..base_options.clone()
        },
        program_options(),
        generous_limits(),
    )
    .expect("allowArbitraryExtensions loads declaration twins recursively");
    assert_eq!(
        source_paths(&enabled),
        [
            Path::new("/work/leaf.d.json.ts"),
            Path::new("/work/data.d.json.ts"),
            Path::new("/work/root.ts"),
        ]
    );
    let resolution = enabled
        .resolutions()
        .require_module(&module_key(&enabled, "/work/root.ts", "./data.json"))
        .expect("enabled arbitrary-extension request has an authoritative row");
    assert!(matches!(
        resolution.outcome(),
        ResolutionOutcome::Resolved(resolved)
            if matches!(resolved.target(), ResolvedModuleTarget::Source { .. })
    ));

    let declaration = load_with_options(
        &host,
        &["/work/root.d.ts"],
        base_options,
        program_options(),
        generous_limits(),
    )
    .expect("a declaration source admits arbitrary declaration twins without the option");
    assert_eq!(
        source_paths(&declaration),
        [
            Path::new("/work/leaf.d.json.ts"),
            Path::new("/work/data.d.json.ts"),
            Path::new("/work/root.d.ts"),
        ]
    );
}

#[test]
fn jsx_without_mode_stays_unloaded_and_gates_local_and_package_reads() {
    let local_path = "/work/dependency.jsx";
    let package_path = "/work/node_modules/pkg/index.jsx";
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/root.ts",
            b"import './dependency.jsx';\nimport 'pkg';\nexport {};\n".to_vec(),
        )
        .file(local_path, b"export const local = 1;\n".to_vec())
        .file(
            "/work/node_modules/pkg/package.json",
            br#"{"name":"pkg","version":"1.0.0","main":"index.jsx"}"#.to_vec(),
        )
        .file(package_path, b"exports.package = 1;\n".to_vec())
        .failure(HostError::new(
            HostErrorKind::Other,
            HostOperation::ReadFile,
            Some(PathBuf::from(local_path)),
            "TS6142 must gate the local JSX read",
        ))
        .failure(HostError::new(
            HostErrorKind::Other,
            HostOperation::ReadFile,
            Some(PathBuf::from(package_path)),
            "TS6142 must gate the package JSX read",
        ))
        .build()
        .expect("build JSX resolution-diagnostic host");
    let program = load_with_options(
        &host,
        &["/work/root.ts"],
        CompilerOptions {
            allow_js: true,
            module: Some(1),
            module_resolution: Some(2),
            ..compiler_options()
        },
        program_options(),
        generous_limits(),
    )
    .expect("JSX resolution diagnostics retain rows without reading targets");

    assert_eq!(source_paths(&program), [Path::new("/work/root.ts")]);
    assert!(program.diagnostics().program().is_empty());
    for (specifier, expected_path, external) in [
        ("./dependency.jsx", local_path, false),
        ("pkg", package_path, true),
    ] {
        let resolution = program
            .resolutions()
            .require_module(&module_key(&program, "/work/root.ts", specifier))
            .expect("JSX request has an authoritative row");
        let ResolutionOutcome::Resolved(resolved) = resolution.outcome() else {
            panic!("JSX request must resolve: {specifier}");
        };
        let ResolvedModuleTarget::Unloaded {
            resolved_file,
            reason,
        } = resolved.target()
        else {
            panic!("JSX without a mode must remain unloaded: {specifier}");
        };
        assert_eq!(resolved_file.display(), Path::new(expected_path));
        assert_eq!(*reason, UnloadedModuleReason::JsxWithoutJsxOption);
        assert_eq!(resolved.is_external_library_import(), external);
    }
}

#[test]
fn allow_js_keeps_default_depth_external_package_javascript_unloaded() {
    let package_read = HostError::new(
        HostErrorKind::Other,
        HostOperation::ReadFile,
        Some(PathBuf::from("/work/node_modules/pkg/index.js")),
        "maxNodeModuleJsDepth=0 must gate package JavaScript before readFile",
    );
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", b"import 'pkg';\nexport {};\n".to_vec())
        .file(
            "/work/node_modules/pkg/package.json",
            br#"{"name":"pkg","version":"1.0.0","main":"index.js"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/pkg/index.js",
            b"require('./leaf.js');\nmodule.exports = 1;\n".to_vec(),
        )
        .file(
            "/work/node_modules/pkg/leaf.js",
            b"module.exports = 2;\n".to_vec(),
        )
        .failure(package_read)
        .build()
        .expect("build external JavaScript package host");
    let program = load_with_options(
        &host,
        &["/work/root.ts"],
        CompilerOptions {
            allow_js: true,
            module: Some(1),
            module_resolution: Some(2),
            ..compiler_options()
        },
        program_options(),
        generous_limits(),
    )
    .expect("default JavaScript package depth retains only its resolution fact");

    assert_eq!(source_paths(&program), [Path::new("/work/root.ts")]);
    assert_eq!(program.resolutions().module_len(), 1);
    let key = module_key(&program, "/work/root.ts", "pkg");
    let resolution = program
        .resolutions()
        .require_module(&key)
        .expect("external JavaScript import has an authoritative row");
    let ResolutionOutcome::Resolved(resolved) = resolution.outcome() else {
        panic!("external JavaScript package must resolve");
    };
    let ResolvedModuleTarget::Unloaded {
        resolved_file: path,
        reason,
    } = resolved.target()
    else {
        panic!("maxNodeModuleJsDepth=0 keeps external package JavaScript unloaded");
    };
    assert_eq!(*reason, UnloadedModuleReason::NodeModulesDepth);
    assert!(resolved.is_external_library_import());
    assert_eq!(path.display(), Path::new("/work/node_modules/pkg/index.js"));
}

#[test]
fn max_node_module_js_depth_admits_each_external_javascript_layer_in_tsc_postorder() {
    for max_depth in 0..=3 {
        let mut builder = MemoryCompilerHost::builder("/work")
            .file("/work/root.ts", b"import 'a';\nexport {};\n".to_vec())
            .file(
                "/work/node_modules/a/package.json",
                br#"{"name":"a","version":"1.0.0","main":"index.js"}"#.to_vec(),
            )
            .file(
                "/work/node_modules/a/index.js",
                b"import './leaf.js';\nimport 'b';\nexport {};\n".to_vec(),
            )
            .file(
                "/work/node_modules/a/leaf.js",
                b"export const aLeaf = true;\n".to_vec(),
            )
            .file(
                "/work/node_modules/b/package.json",
                br#"{"name":"b","version":"1.0.0","main":"index.js"}"#.to_vec(),
            )
            .file(
                "/work/node_modules/b/index.js",
                b"import './leaf.js';\nexport {};\n".to_vec(),
            )
            .file(
                "/work/node_modules/b/leaf.js",
                b"export const bLeaf = true;\n".to_vec(),
            );
        let unread_paths: &[&str] = match max_depth {
            0 => &["/work/node_modules/a/index.js"],
            1 => &[
                "/work/node_modules/a/leaf.js",
                "/work/node_modules/b/index.js",
            ],
            2 => &["/work/node_modules/b/leaf.js"],
            3 => &[],
            _ => unreachable!(),
        };
        for path in unread_paths {
            builder = builder.failure(HostError::new(
                HostErrorKind::Other,
                HostOperation::ReadFile,
                Some(PathBuf::from(path)),
                format!("maxNodeModuleJsDepth={max_depth} must gate {path}"),
            ));
        }
        let host = builder.build().expect("build nested package host");
        let program = load_with_options(
            &host,
            &["/work/root.ts"],
            CompilerOptions {
                allow_js: true,
                max_node_module_js_depth: Some(max_depth.into()),
                module: Some(1),
                module_resolution: Some(2),
                ..compiler_options()
            },
            program_options(),
            generous_limits(),
        )
        .unwrap_or_else(|error| panic!("load maxNodeModuleJsDepth={max_depth}: {error}"));

        let expected_paths: &[&str] = match max_depth {
            0 => &["/work/root.ts"],
            1 => &["/work/node_modules/a/index.js", "/work/root.ts"],
            2 => &[
                "/work/node_modules/a/leaf.js",
                "/work/node_modules/b/index.js",
                "/work/node_modules/a/index.js",
                "/work/root.ts",
            ],
            3 => &[
                "/work/node_modules/a/leaf.js",
                "/work/node_modules/b/leaf.js",
                "/work/node_modules/b/index.js",
                "/work/node_modules/a/index.js",
                "/work/root.ts",
            ],
            _ => unreachable!(),
        };
        assert_eq!(
            source_paths(&program),
            expected_paths
                .iter()
                .map(|path| Path::new(*path))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            program.resolutions().module_len(),
            match max_depth {
                0 => 1,
                1 => 3,
                2 | 3 => 4,
                _ => unreachable!(),
            }
        );

        if max_depth == 0 {
            assert_unloaded_module_target(
                &program,
                "/work/root.ts",
                "a",
                "/work/node_modules/a/index.js",
                UnloadedModuleReason::NodeModulesDepth,
            );
            continue;
        }
        assert_source_module_target(
            &program,
            "/work/root.ts",
            "a",
            "/work/node_modules/a/index.js",
        );
        if max_depth == 1 {
            for (specifier, expected_path) in [
                ("./leaf.js", "/work/node_modules/a/leaf.js"),
                ("b", "/work/node_modules/b/index.js"),
            ] {
                assert_unloaded_module_target(
                    &program,
                    "/work/node_modules/a/index.js",
                    specifier,
                    expected_path,
                    UnloadedModuleReason::NodeModulesDepth,
                );
            }
            continue;
        }
        assert_source_module_target(
            &program,
            "/work/node_modules/a/index.js",
            "./leaf.js",
            "/work/node_modules/a/leaf.js",
        );
        assert_source_module_target(
            &program,
            "/work/node_modules/a/index.js",
            "b",
            "/work/node_modules/b/index.js",
        );
        if max_depth == 2 {
            assert_unloaded_module_target(
                &program,
                "/work/node_modules/b/index.js",
                "./leaf.js",
                "/work/node_modules/b/leaf.js",
                UnloadedModuleReason::NodeModulesDepth,
            );
        } else {
            assert_source_module_target(
                &program,
                "/work/node_modules/b/index.js",
                "./leaf.js",
                "/work/node_modules/b/leaf.js",
            );
        }
    }
}

#[test]
fn external_type_reference_source_owns_the_next_node_module_js_depth_layer() {
    let type_source = "/work/node_modules/@types/a/index.d.ts";
    let javascript_target = "/work/node_modules/@types/a/b.js";
    for max_depth in [1, 2] {
        let mut builder = MemoryCompilerHost::builder("/work")
            .file(
                "/work/root.ts",
                b"/// <reference types=\"a\" />\nexport {};\n".to_vec(),
            )
            .file(
                "/work/node_modules/@types/a/package.json",
                br#"{"name":"@types/a","version":"1.0.0","types":"index.d.ts"}"#.to_vec(),
            )
            .file(type_source, b"import './b.js';\nexport {};\n".to_vec())
            .file(javascript_target, b"export const b = true;\n".to_vec());
        if max_depth == 1 {
            builder = builder.failure(HostError::new(
                HostErrorKind::Other,
                HostOperation::ReadFile,
                Some(PathBuf::from(javascript_target)),
                "the JavaScript child of an external type source is one depth layer deeper",
            ));
        }
        let host = builder
            .build()
            .expect("build external type-reference depth host");
        let program = load_with_options(
            &host,
            &["/work/root.ts"],
            CompilerOptions {
                allow_js: true,
                max_node_module_js_depth: Some(max_depth.into()),
                module: Some(1),
                module_resolution: Some(2),
                ..compiler_options()
            },
            program_options(),
            generous_limits(),
        )
        .unwrap_or_else(|error| {
            panic!("load external type-reference source at max depth {max_depth}: {error}")
        });

        let expected_paths: &[&str] = if max_depth == 1 {
            &[type_source, "/work/root.ts"]
        } else {
            &[javascript_target, type_source, "/work/root.ts"]
        };
        assert_eq!(
            source_paths(&program),
            expected_paths
                .iter()
                .map(|path| Path::new(*path))
                .collect::<Vec<_>>()
        );
        if max_depth == 1 {
            assert_unloaded_module_target(
                &program,
                type_source,
                "./b.js",
                javascript_target,
                UnloadedModuleReason::NodeModulesDepth,
            );
        } else {
            assert_source_module_target(&program, type_source, "./b.js", javascript_target);
        }
        let resolution = program
            .resolutions()
            .require_module(&module_key(&program, type_source, "./b.js"))
            .expect("relative JavaScript request has an authoritative row");
        let ResolutionOutcome::Resolved(resolved) = resolution.outcome() else {
            panic!("the relative JavaScript request must resolve");
        };
        assert!(resolved.is_external_library_import());
    }
}

#[test]
fn shallower_nonzero_revisit_reprocesses_only_imports() {
    let skipped_reference_leaf = "/work/node_modules/shared/reference-leaf.js";
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/root.ts",
            b"import 'a';\nimport 'shared';\nexport {};\n".to_vec(),
        )
        .file(
            "/work/node_modules/a/package.json",
            br#"{"name":"a","version":"1.0.0","main":"index.js"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/a/index.js",
            b"import 'shared';\nexport {};\n".to_vec(),
        )
        .file(
            "/work/node_modules/shared/package.json",
            br#"{"name":"shared","version":"1.0.0","main":"index.js"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/shared/index.js",
            concat!(
                "/// <reference path=\"./reference.js\" />\n",
                "import './leaf.js';\n",
                "export {};\n",
            )
            .as_bytes()
            .to_vec(),
        )
        .file(
            "/work/node_modules/shared/reference.js",
            b"import './reference-leaf.js';\nexport {};\n".to_vec(),
        )
        .file(
            skipped_reference_leaf,
            b"export const referenceLeaf = true;\n".to_vec(),
        )
        .file(
            "/work/node_modules/shared/leaf.js",
            b"export const leaf = true;\n".to_vec(),
        )
        .failure(HostError::new(
            HostErrorKind::Other,
            HostOperation::ReadFile,
            Some(PathBuf::from(skipped_reference_leaf)),
            "an imports-only revisit must not revisit path references",
        ))
        .build()
        .expect("build shallower-revisit host");
    let program = load_with_options(
        &host,
        &["/work/root.ts"],
        CompilerOptions {
            allow_js: true,
            max_node_module_js_depth: Some(2.into()),
            module: Some(1),
            module_resolution: Some(2),
            ..compiler_options()
        },
        program_options(),
        generous_limits(),
    )
    .expect("a shallower nonzero revisit processes only imported modules");

    assert_eq!(
        source_paths(&program),
        [
            Path::new("/work/node_modules/shared/reference.js"),
            Path::new("/work/node_modules/shared/index.js"),
            Path::new("/work/node_modules/a/index.js"),
            Path::new("/work/node_modules/shared/leaf.js"),
            Path::new("/work/root.ts"),
        ]
    );
    assert_source_module_target(
        &program,
        "/work/node_modules/shared/index.js",
        "./leaf.js",
        "/work/node_modules/shared/leaf.js",
    );
    assert_unloaded_module_target(
        &program,
        "/work/node_modules/shared/reference.js",
        "./reference-leaf.js",
        skipped_reference_leaf,
        UnloadedModuleReason::NodeModulesDepth,
    );
}

#[test]
fn depth_zero_root_promotion_reprocesses_path_reference_descendants() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", b"import 'a';\nexport {};\n".to_vec())
        .file(
            "/work/node_modules/a/package.json",
            br#"{"name":"a","version":"1.0.0","main":"index.js"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/a/index.js",
            b"/// <reference path=\"./reference.js\" />\nexport {};\n".to_vec(),
        )
        .file(
            "/work/node_modules/a/reference.js",
            b"import './leaf.js';\nexport {};\n".to_vec(),
        )
        .file(
            "/work/node_modules/a/leaf.js",
            b"export const leaf = true;\n".to_vec(),
        )
        .build()
        .expect("build root-promotion host");
    let program = load_with_options(
        &host,
        &["/work/root.ts", "/work/node_modules/a/index.js"],
        CompilerOptions {
            allow_js: true,
            max_node_module_js_depth: Some(1.into()),
            module: Some(1),
            module_resolution: Some(2),
            ..compiler_options()
        },
        program_options(),
        generous_limits(),
    )
    .expect("a depth-zero root promotion reprocesses every reference phase");

    assert_eq!(
        source_paths(&program),
        [
            Path::new("/work/node_modules/a/reference.js"),
            Path::new("/work/node_modules/a/index.js"),
            Path::new("/work/root.ts"),
            Path::new("/work/node_modules/a/leaf.js"),
        ]
    );
    assert_eq!(
        root_paths(&program),
        [
            Path::new("/work/root.ts"),
            Path::new("/work/node_modules/a/index.js"),
        ]
    );
    assert_source_module_target(
        &program,
        "/work/node_modules/a/reference.js",
        "./leaf.js",
        "/work/node_modules/a/leaf.js",
    );
    for path in [
        "/work/node_modules/a/reference.js",
        "/work/node_modules/a/index.js",
        "/work/root.ts",
    ] {
        let source = program
            .source_files()
            .iter()
            .find(|source| source.path().display() == Path::new(path))
            .expect("promoted source is owned");
        assert!(source.may_be_emitted(), "{path}");
    }
    let leaf = program
        .source_files()
        .iter()
        .find(|source| source.path().display() == Path::new("/work/node_modules/a/leaf.js"))
        .expect("external leaf is owned");
    assert!(!leaf.may_be_emitted());
}

#[test]
fn negative_max_node_module_js_depth_elides_every_external_javascript_target() {
    let target_path = "/work/node_modules/pkg/index.js";
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", b"import 'pkg';\nexport {};\n".to_vec())
        .file(
            "/work/node_modules/pkg/package.json",
            br#"{"name":"pkg","version":"1.0.0","main":"index.js"}"#.to_vec(),
        )
        .file(target_path, b"module.exports = {};\n".to_vec())
        .failure(HostError::new(
            HostErrorKind::Other,
            HostOperation::ReadFile,
            Some(PathBuf::from(target_path)),
            "a negative maxNodeModuleJsDepth must gate every external JavaScript read",
        ))
        .build()
        .expect("build negative-depth host");
    let program = load_with_options(
        &host,
        &["/work/root.ts"],
        CompilerOptions {
            allow_js: true,
            max_node_module_js_depth: Some((-1).into()),
            module: Some(1),
            module_resolution: Some(2),
            ..compiler_options()
        },
        program_options(),
        generous_limits(),
    )
    .expect("a negative maxNodeModuleJsDepth retains an unloaded row");

    assert_eq!(source_paths(&program), [Path::new("/work/root.ts")]);
    assert_unloaded_module_target(
        &program,
        "/work/root.ts",
        "pkg",
        target_path,
        UnloadedModuleReason::NodeModulesDepth,
    );
}

#[test]
fn javascript_number_depth_limits_preserve_fraction_infinity_and_nan_comparisons() {
    for (label, maximum, expected_paths) in [
        ("fraction below first layer", 0.5, &["/work/root.ts"][..]),
        (
            "fraction between layers",
            1.5,
            &["/work/node_modules/pkg/index.js", "/work/root.ts"][..],
        ),
        (
            "positive infinity",
            f64::INFINITY,
            &[
                "/work/node_modules/pkg/leaf.js",
                "/work/node_modules/pkg/index.js",
                "/work/root.ts",
            ][..],
        ),
        (
            "negative infinity",
            f64::NEG_INFINITY,
            &["/work/root.ts"][..],
        ),
        (
            "programmatic NaN",
            f64::NAN,
            &[
                "/work/node_modules/pkg/leaf.js",
                "/work/node_modules/pkg/index.js",
                "/work/root.ts",
            ][..],
        ),
    ] {
        let host = MemoryCompilerHost::builder("/work")
            .file("/work/root.ts", b"import 'pkg';\nexport {};\n".to_vec())
            .file(
                "/work/node_modules/pkg/package.json",
                br#"{"name":"pkg","version":"1.0.0","main":"index.js"}"#.to_vec(),
            )
            .file(
                "/work/node_modules/pkg/index.js",
                b"import './leaf.js';\nexport {};\n".to_vec(),
            )
            .file(
                "/work/node_modules/pkg/leaf.js",
                b"export const leaf = true;\n".to_vec(),
            )
            .build()
            .expect("build JavaScript-number depth host");
        let program = load_with_options(
            &host,
            &["/work/root.ts"],
            CompilerOptions {
                allow_js: true,
                max_node_module_js_depth: Some(CompilerOptionNumber::new(maximum)),
                module: Some(1),
                module_resolution: Some(2),
                ..compiler_options()
            },
            program_options(),
            generous_limits(),
        )
        .unwrap_or_else(|error| panic!("load {label} maxNodeModuleJsDepth: {error}"));

        assert_eq!(
            source_paths(&program),
            expected_paths
                .iter()
                .map(|path| Path::new(*path))
                .collect::<Vec<_>>(),
            "{label}"
        );
    }
}

#[test]
fn max_node_module_js_depth_does_not_override_allow_js_false() {
    let target_path = "/work/node_modules/pkg/index.js";
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", b"import 'pkg';\nexport {};\n".to_vec())
        .file(
            "/work/node_modules/pkg/package.json",
            br#"{"name":"pkg","version":"1.0.0","main":"index.js"}"#.to_vec(),
        )
        .file(target_path, b"module.exports = {};\n".to_vec())
        .failure(HostError::new(
            HostErrorKind::Other,
            HostOperation::ReadFile,
            Some(PathBuf::from(target_path)),
            "allowJs=false must gate an otherwise in-depth JavaScript read",
        ))
        .build()
        .expect("build allowJs=false depth host");
    let program = load_with_options(
        &host,
        &["/work/root.ts"],
        CompilerOptions {
            allow_js: false,
            max_node_module_js_depth: Some(1.into()),
            module: Some(1),
            module_resolution: Some(2),
            ..compiler_options()
        },
        program_options(),
        generous_limits(),
    )
    .expect("allowJs=false keeps an in-depth JavaScript resolution unloaded");

    assert_eq!(source_paths(&program), [Path::new("/work/root.ts")]);
    assert_unloaded_module_target(
        &program,
        "/work/root.ts",
        "pkg",
        target_path,
        UnloadedModuleReason::JavaScriptNotAdmitted,
    );
}

#[test]
fn allow_js_marks_augmentation_only_javascript_as_resolution_only() {
    let target_path = "/work/node_modules/pkg/index.js";
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/root.ts",
            b"export {};\ndeclare module 'pkg' { export const extra: number; }\n".to_vec(),
        )
        .file(
            "/work/node_modules/pkg/package.json",
            br#"{"name":"pkg","version":"1.0.0","main":"index.js"}"#.to_vec(),
        )
        .file(target_path, b"module.exports = {};\n".to_vec())
        .failure(HostError::new(
            HostErrorKind::Other,
            HostOperation::ReadFile,
            Some(PathBuf::from(target_path)),
            "an augmentation-only resolution must not load its target",
        ))
        .build()
        .expect("build augmentation-only JavaScript host");
    let program = load_with_options(
        &host,
        &["/work/root.ts"],
        CompilerOptions {
            allow_js: true,
            module: Some(1),
            module_resolution: Some(2),
            ..compiler_options()
        },
        program_options(),
        generous_limits(),
    )
    .expect("retain the augmentation-only resolution without source membership");

    assert_eq!(source_paths(&program), [Path::new("/work/root.ts")]);
    let resolution = program
        .resolutions()
        .require_module(&module_key(&program, "/work/root.ts", "pkg"))
        .expect("augmentation has an authoritative module row");
    let ResolutionOutcome::Resolved(resolved) = resolution.outcome() else {
        panic!("augmentation target must resolve");
    };
    let ResolvedModuleTarget::Unloaded {
        resolved_file,
        reason,
    } = resolved.target()
    else {
        panic!("augmentation-only JavaScript remains unloaded");
    };
    assert_eq!(resolved_file.display(), Path::new(target_path));
    assert_eq!(*reason, UnloadedModuleReason::ResolutionOnly);
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
fn preserve_symlinks_memory_canary_keeps_lexical_program_membership() {
    const APP: &str = "/app/app.ts";
    const PHYSICAL: &str = "/linked/index.d.ts";
    const LINKED: &str = "/app/node_modules/linked/index.d.ts";
    const LINKED2: &str = "/app/node_modules/linked2/index.d.ts";
    const REAL: &str = "/app/node_modules/real/index.d.ts";

    let linked_source = b"export { real } from \"real\";\nexport class C { private x; }\n".to_vec();
    let host = MemoryCompilerHost::builder("/app")
        .file(
            APP,
            concat!(
                "/// <reference types=\"linked\" />\n",
                "import { C as C1 } from \"linked\";\n",
                "import { C as C2 } from \"linked2\";\n",
            )
            .as_bytes()
            .to_vec(),
        )
        .file(PHYSICAL, linked_source.clone())
        .file(LINKED, linked_source.clone())
        .file(LINKED2, linked_source)
        .file(REAL, b"export const real: string;\n".to_vec())
        .realpath(LINKED, PHYSICAL)
        .realpath(LINKED2, PHYSICAL)
        .build()
        .expect("build the cross-platform preserveSymlinks topology");
    let options = CompilerOptions {
        target: Some(2),
        module: Some(5),
        module_resolution: Some(100),
        ..compiler_options()
    };

    let assert_module = |program: &PreparedProgram,
                         containing_file: &str,
                         specifier: &str,
                         expected_target: &str,
                         expected_original_path: Option<&str>| {
        let key = module_key(program, containing_file, specifier);
        assert_eq!(key.mode(), ResolutionMode::EsNext);
        let resolution = program
            .resolutions()
            .require_module(&key)
            .expect("module request has an authoritative row");
        let ResolutionOutcome::Resolved(module) = resolution.outcome() else {
            panic!("module request must resolve: {specifier}");
        };
        let ResolvedModuleTarget::Source {
            source,
            resolved_file,
        } = module.target()
        else {
            panic!("module target must join source membership: {specifier}");
        };
        assert_eq!(resolved_file.display(), Path::new(expected_target));
        assert_eq!(
            program.source_file(*source).unwrap().path().display(),
            Path::new(expected_target)
        );
        assert_eq!(
            module.original_path().map(ProgramPath::display),
            expected_original_path.map(Path::new)
        );
        *source
    };
    let assert_type_reference =
        |program: &PreparedProgram, expected_target: &str, expected_original_path: Option<&str>| {
            let key = type_reference_key(program, APP, "linked");
            assert_eq!(key.mode(), ResolutionMode::Unspecified);
            let resolution = program
                .resolutions()
                .require_type_reference(&key)
                .expect("type-reference request has an authoritative row");
            let ResolutionOutcome::Resolved(directive) = resolution.outcome() else {
                panic!("type-reference request must resolve");
            };
            assert_eq!(directive.target().display(), Path::new(expected_target));
            assert_eq!(
                program
                    .source_file(directive.source())
                    .unwrap()
                    .path()
                    .display(),
                Path::new(expected_target)
            );
            assert_eq!(
                directive.original_path().map(ProgramPath::display),
                expected_original_path.map(Path::new)
            );
            directive.source()
        };

    let preserved = load_with_options(
        &host,
        &[APP],
        options.clone(),
        program_options().with_preserve_symlinks(true),
        generous_limits(),
    )
    .expect("load lexical preserveSymlinks identities");
    assert_eq!(preserved.program_options().preserve_symlinks(), Some(true));
    assert_eq!(
        source_paths(&preserved),
        [
            Path::new(REAL),
            Path::new(LINKED),
            Path::new(LINKED2),
            Path::new(APP),
        ]
    );
    let linked = assert_module(&preserved, APP, "linked", LINKED, None);
    let linked2 = assert_module(&preserved, APP, "linked2", LINKED2, None);
    assert_ne!(linked, linked2);
    assert_eq!(assert_type_reference(&preserved, LINKED, None), linked);
    assert_eq!(
        assert_module(&preserved, LINKED, "real", REAL, None),
        assert_module(&preserved, LINKED2, "real", REAL, None)
    );

    let explicit_false = load_with_options(
        &host,
        &[APP],
        options.clone(),
        program_options().with_preserve_symlinks(false),
        generous_limits(),
    )
    .expect("load explicit-false physical identities");
    let omitted = load_with_options(&host, &[APP], options, program_options(), generous_limits())
        .expect("load omitted preserveSymlinks identities");
    assert_eq!(
        explicit_false.program_options().preserve_symlinks(),
        Some(false)
    );
    assert_eq!(omitted.program_options().preserve_symlinks(), None);
    assert_eq!(explicit_false.source_files(), omitted.source_files());
    assert_eq!(explicit_false.roots(), omitted.roots());
    assert_eq!(explicit_false.resolutions(), omitted.resolutions());
    assert_eq!(explicit_false.diagnostics(), omitted.diagnostics());

    for program in [&explicit_false, &omitted] {
        assert_eq!(source_paths(program), [Path::new(PHYSICAL), Path::new(APP)]);
        let linked = assert_module(program, APP, "linked", PHYSICAL, Some(LINKED));
        let linked2 = assert_module(program, APP, "linked2", PHYSICAL, Some(LINKED2));
        assert_eq!(linked, linked2);
        assert_eq!(
            assert_type_reference(program, PHYSICAL, Some(LINKED)),
            linked
        );
        let unresolved_real = module_key(program, PHYSICAL, "real");
        assert_eq!(unresolved_real.mode(), ResolutionMode::EsNext);
        assert!(matches!(
            program
                .resolutions()
                .require_module(&unresolved_real)
                .expect("physical declaration import has an authoritative row")
                .outcome(),
            ResolutionOutcome::NotFound
        ));
    }
}

#[test]
fn loaded_package_uses_lexical_extension_for_an_extensionless_physical_source() {
    let lexical = "/work/node_modules/pkg/index.ts";
    let physical = "/store/pkg/typed-blob";
    let package_source = b"import { leaf } from './leaf';\nexport const lexical = leaf;".to_vec();
    let forbidden_root_realpath = HostError::new(
        HostErrorKind::Other,
        HostOperation::Realpath,
        Some(PathBuf::from("/work/root.ts")),
        "root source discovery must preserve lexical identity",
    );
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", b"import 'pkg';\nexport {};".to_vec())
        .file(
            "/work/node_modules/pkg/package.json",
            br#"{"name":"pkg","version":"1.0.0","main":"index.ts"}"#.to_vec(),
        )
        .file(lexical, package_source.clone())
        .file(physical, package_source)
        .file("/store/pkg/leaf.ts", b"export const leaf = 1;".to_vec())
        .realpath(lexical, physical)
        .failure(forbidden_root_realpath)
        .failure(HostError::new(
            HostErrorKind::Other,
            HostOperation::Realpath,
            Some(PathBuf::from(physical)),
            "the physical source must not be realpathed a second time",
        ))
        .failure(HostError::new(
            HostErrorKind::Other,
            HostOperation::Realpath,
            Some(PathBuf::from("/store/pkg/leaf.ts")),
            "a local relative dependency must preserve lexical identity",
        ))
        .build()
        .expect("build package realpath host");
    let options = CompilerOptions {
        no_emit: Some(true),
        module: Some(1),
        module_resolution: Some(2),
        ..CompilerOptions::default()
    };

    let program = load_no_lib_program(
        &host,
        &[PathBuf::from("/work/root.ts")],
        options,
        program_options(),
        generous_limits(),
    )
    .expect("loaded originalPath enters the prepared-program contract");
    assert_eq!(
        source_paths(&program),
        [
            Path::new("/store/pkg/leaf.ts"),
            Path::new(physical),
            Path::new("/work/root.ts")
        ]
    );
    assert!(program
        .source_files()
        .iter()
        .all(|source| source.real_path().is_none()));

    let resolution = program
        .resolutions()
        .require_module(&module_key(&program, "/work/root.ts", "pkg"))
        .expect("package request has an authoritative row");
    let ResolutionOutcome::Resolved(module) = resolution.outcome() else {
        panic!("package request must resolve");
    };
    let ResolvedModuleTarget::Source {
        source,
        resolved_file,
    } = module.target()
    else {
        panic!("physical package target must be loaded");
    };
    assert_eq!(module.extension(), &ModuleExtension::Ts);
    assert_eq!(resolved_file.display(), Path::new(physical));
    assert_eq!(
        program.source_file(*source).unwrap().path().display(),
        Path::new(physical)
    );
    assert_eq!(
        module.original_path().map(ProgramPath::display),
        Some(Path::new(lexical))
    );

    let physical_source = program.source_file(*source).unwrap();
    let leaf_key = plan_source_requests(physical_source, program.compiler_options())
        .expect("plan requests from the extensionless physical source")
        .module_requests()[0]
        .clone();
    let leaf_resolution = program
        .resolutions()
        .require_module(&leaf_key)
        .expect("the physical source's relative request has an authoritative row");
    assert!(matches!(
        leaf_resolution.outcome(),
        ResolutionOutcome::Resolved(leaf)
            if matches!(
                leaf.target(),
                ResolvedModuleTarget::Source { resolved_file, .. }
                    if resolved_file.display() == Path::new("/store/pkg/leaf.ts")
            )
    ));
}

#[test]
fn lexical_symlink_root_and_physical_dependency_remain_distinct_sources() {
    let package_source = b"export const value = 1;\n".to_vec();
    let lexical_root = "/work/link.ts";
    let lexical_package = "/work/node_modules/pkg/index.ts";
    let physical = "/store/pkg/index.ts";
    let host = MemoryCompilerHost::builder("/work")
        .file(lexical_root, package_source.clone())
        .file(
            "/work/root.ts",
            b"import { value } from 'pkg';\nvalue;\n".to_vec(),
        )
        .file(
            "/work/node_modules/pkg/package.json",
            br#"{"name":"pkg","version":"1.0.0","exports":"./index.ts"}"#.to_vec(),
        )
        .file(lexical_package, package_source.clone())
        .file(physical, package_source)
        .realpath(lexical_root, physical)
        .realpath(lexical_package, physical)
        .failure(HostError::new(
            HostErrorKind::Other,
            HostOperation::Realpath,
            Some(PathBuf::from(lexical_root)),
            "root discovery must not query realpath",
        ))
        .build()
        .expect("build root and package symlink host");

    let program = load_with_options(
        &host,
        &[lexical_root, "/work/root.ts"],
        CompilerOptions {
            module: Some(199),
            module_resolution: Some(99),
            ..compiler_options()
        },
        program_options(),
        generous_limits(),
    )
    .expect("lexical root and physical dependency retain independent SourceFile identities");
    assert_eq!(
        source_paths(&program),
        [
            Path::new(lexical_root),
            Path::new(physical),
            Path::new("/work/root.ts")
        ]
    );
    assert!(program
        .source_files()
        .iter()
        .all(|source| source.real_path().is_none()));

    let resolution = program
        .resolutions()
        .require_module(&module_key(&program, "/work/root.ts", "pkg"))
        .expect("package request has an authoritative row");
    let ResolutionOutcome::Resolved(module) = resolution.outcome() else {
        panic!("package request must resolve");
    };
    let ResolvedModuleTarget::Source {
        source,
        resolved_file,
    } = module.target()
    else {
        panic!("physical package target must be loaded");
    };
    assert_eq!(resolved_file.display(), Path::new(physical));
    assert_eq!(
        program.source_file(*source).unwrap().path().display(),
        Path::new(physical)
    );
    assert_eq!(
        module.original_path().map(ProgramPath::display),
        Some(Path::new(lexical_package))
    );
}

#[test]
fn local_actual_then_bare_symlink_share_one_physical_source_id() {
    let actual = "/src/library-a/index.ts";
    let bare_lexical = "/node_modules/library-a/index.ts";
    let library = b"export const value = 1;\n".to_vec();
    let host = MemoryCompilerHost::builder("/")
        .file(
            "/src/local-consumer.ts",
            b"import { value } from './library-a';\nvalue;\n".to_vec(),
        )
        .file(
            "/src/bare-consumer.ts",
            b"import { value } from 'library-a';\nvalue;\n".to_vec(),
        )
        .file(actual, library.clone())
        .file(
            "/node_modules/library-a/package.json",
            br#"{"name":"library-a","version":"1.0.0","main":"index.ts"}"#.to_vec(),
        )
        .file(bare_lexical, library)
        .realpath(bare_lexical, actual)
        .failure(HostError::new(
            HostErrorKind::Other,
            HostOperation::Realpath,
            Some(PathBuf::from(actual)),
            "the local actual source must not be realpathed",
        ))
        .failure(HostError::new(
            HostErrorKind::Other,
            HostOperation::Realpath,
            Some(PathBuf::from("/src/local-consumer.ts")),
            "root source discovery must preserve lexical identity",
        ))
        .failure(HostError::new(
            HostErrorKind::Other,
            HostOperation::Realpath,
            Some(PathBuf::from("/src/bare-consumer.ts")),
            "root source discovery must preserve lexical identity",
        ))
        .build()
        .expect("build local and bare symlink host");
    let program = load_with_options(
        &host,
        &["/src/local-consumer.ts", "/src/bare-consumer.ts"],
        CompilerOptions {
            module: Some(1),
            module_resolution: Some(2),
            ..compiler_options()
        },
        program_options(),
        generous_limits(),
    )
    .expect("local and bare requests share one physical source");
    assert_eq!(
        source_paths(&program),
        [
            Path::new(actual),
            Path::new("/src/local-consumer.ts"),
            Path::new("/src/bare-consumer.ts")
        ]
    );

    let local_resolution = program
        .resolutions()
        .require_module(&module_key(
            &program,
            "/src/local-consumer.ts",
            "./library-a",
        ))
        .expect("local request has an authoritative row");
    let ResolutionOutcome::Resolved(local) = local_resolution.outcome() else {
        panic!("local request must resolve");
    };
    let ResolvedModuleTarget::Source {
        source: local_source,
        resolved_file: local_file,
    } = local.target()
    else {
        panic!("local actual must be loaded");
    };
    assert_eq!(local_file.display(), Path::new(actual));
    assert_eq!(local.original_path(), None);

    let bare_resolution = program
        .resolutions()
        .require_module(&module_key(&program, "/src/bare-consumer.ts", "library-a"))
        .expect("bare request has an authoritative row");
    let ResolutionOutcome::Resolved(bare) = bare_resolution.outcome() else {
        panic!("bare request must resolve");
    };
    let ResolvedModuleTarget::Source {
        source: bare_source,
        resolved_file: bare_file,
    } = bare.target()
    else {
        panic!("bare symlink target must join the owned actual source");
    };
    assert_eq!(local_source, bare_source);
    assert_eq!(bare_file.display(), Path::new(actual));
    assert_eq!(
        bare.original_path().map(ProgramPath::display),
        Some(Path::new(bare_lexical))
    );
    assert_eq!(
        program.source_file(*bare_source).unwrap().path().display(),
        Path::new(actual)
    );
}

#[test]
fn two_local_symlink_spellings_remain_two_lexical_sources() {
    let first = "/src/first.ts";
    let second = "/src/second.ts";
    let physical = "/store/shared.ts";
    let shared = b"export const value = 1;\n".to_vec();
    let host = MemoryCompilerHost::builder("/")
        .file(
            "/src/root.ts",
            b"import './first';\nimport './second';\nexport {};\n".to_vec(),
        )
        .file(first, shared.clone())
        .file(second, shared.clone())
        .file(physical, shared)
        .realpath(first, physical)
        .realpath(second, physical)
        .failure(HostError::new(
            HostErrorKind::Other,
            HostOperation::Realpath,
            Some(PathBuf::from(first)),
            "a local relative target must not be realpathed",
        ))
        .failure(HostError::new(
            HostErrorKind::Other,
            HostOperation::Realpath,
            Some(PathBuf::from(second)),
            "a local relative target must not be realpathed",
        ))
        .build()
        .expect("build two-local-symlink host");
    let program = load_with_options(
        &host,
        &["/src/root.ts"],
        CompilerOptions {
            module: Some(1),
            module_resolution: Some(2),
            ..compiler_options()
        },
        program_options(),
        generous_limits(),
    )
    .expect("local symlink spellings retain lexical source identities");
    assert_eq!(
        source_paths(&program),
        [
            Path::new(first),
            Path::new(second),
            Path::new("/src/root.ts")
        ]
    );

    let first_resolution = program
        .resolutions()
        .require_module(&module_key(&program, "/src/root.ts", "./first"))
        .expect("first local request has a row");
    let second_resolution = program
        .resolutions()
        .require_module(&module_key(&program, "/src/root.ts", "./second"))
        .expect("second local request has a row");
    let (ResolutionOutcome::Resolved(first_module), ResolutionOutcome::Resolved(second_module)) =
        (first_resolution.outcome(), second_resolution.outcome())
    else {
        panic!("both local symlink spellings must resolve");
    };
    let (Some(first_source), Some(second_source)) = (
        first_module.target().source(),
        second_module.target().source(),
    ) else {
        panic!("both local symlink spellings must be loaded");
    };
    assert_ne!(first_source, second_source);
    assert_eq!(first_module.original_path(), None);
    assert_eq!(second_module.original_path(), None);
    assert!(program
        .source_files()
        .iter()
        .all(|source| source.path().display() != Path::new(physical)));
}

#[test]
fn direct_and_nested_symlink_type_references_share_one_source_id() {
    let actual = "/node_modules/@types/shared/index.d.ts";
    let nested = "/node_modules/pkg/node_modules/@types/shared/index.d.ts";
    let declaration = b"declare const sharedValue: number;\n".to_vec();
    let host = MemoryCompilerHost::builder("/")
        .file(
            "/src/direct.ts",
            b"/// <reference types='shared' />\nexport {};\n".to_vec(),
        )
        .file(
            "/node_modules/pkg/index.d.ts",
            b"/// <reference types='shared' />\nexport {};\n".to_vec(),
        )
        .file(
            "/node_modules/@types/shared/package.json",
            br#"{"name":"@types/shared","version":"1.0.0","types":"index.d.ts"}"#.to_vec(),
        )
        .file(actual, declaration.clone())
        .file(
            "/node_modules/pkg/node_modules/@types/shared/package.json",
            br#"{"name":"@types/shared","version":"1.0.0","types":"index.d.ts"}"#.to_vec(),
        )
        .file(nested, declaration)
        .realpath(nested, actual)
        .failure(HostError::new(
            HostErrorKind::Other,
            HostOperation::Realpath,
            Some(PathBuf::from("/src/direct.ts")),
            "root source discovery must preserve lexical identity",
        ))
        .failure(HostError::new(
            HostErrorKind::Other,
            HostOperation::Realpath,
            Some(PathBuf::from("/node_modules/pkg/index.d.ts")),
            "root source discovery must preserve lexical identity",
        ))
        .build()
        .expect("build direct and nested type-reference symlink host");
    let program = load_with_options(
        &host,
        &["/src/direct.ts", "/node_modules/pkg/index.d.ts"],
        compiler_options(),
        program_options().with_type_roots(Vec::new()),
        generous_limits(),
    )
    .expect("direct and nested type references share one physical source");
    assert_eq!(
        source_paths(&program),
        [
            Path::new(actual),
            Path::new("/src/direct.ts"),
            Path::new("/node_modules/pkg/index.d.ts")
        ]
    );

    let direct_resolution = program
        .resolutions()
        .require_type_reference(&type_reference_key(&program, "/src/direct.ts", "shared"))
        .expect("direct type reference has an authoritative row");
    let nested_resolution = program
        .resolutions()
        .require_type_reference(&type_reference_key(
            &program,
            "/node_modules/pkg/index.d.ts",
            "shared",
        ))
        .expect("nested type reference has an authoritative row");
    let (ResolutionOutcome::Resolved(direct), ResolutionOutcome::Resolved(nested_directive)) =
        (direct_resolution.outcome(), nested_resolution.outcome())
    else {
        panic!("both type references must resolve");
    };
    assert_eq!(direct.source(), nested_directive.source());
    assert_eq!(direct.target().display(), Path::new(actual));
    assert_eq!(nested_directive.target().display(), Path::new(actual));
    assert!(!direct.primary());
    assert!(!nested_directive.primary());
    assert_eq!(direct.original_path(), None);
    assert_eq!(
        nested_directive.original_path().map(ProgramPath::display),
        Some(Path::new(nested))
    );
    assert_eq!(
        program
            .source_file(nested_directive.source())
            .unwrap()
            .path()
            .display(),
        Path::new(actual)
    );
}

#[test]
fn loaded_custom_type_root_reference_uses_physical_source_and_original_path() {
    let lexical = "/custom/types/pkg/index.d.ts";
    let physical = "/store/types/pkg/index.d.ts";
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/root.ts",
            b"/// <reference types='pkg' />\nexport {};\n".to_vec(),
        )
        .file(
            "/custom/types/pkg/package.json",
            br#"{"name":"pkg","version":"1.0.0","types":"index.d.ts"}"#.to_vec(),
        )
        .file(lexical, b"declare const packageValue: number;\n".to_vec())
        .file(physical, b"declare const packageValue: number;\n".to_vec())
        .realpath(lexical, physical)
        .failure(HostError::new(
            HostErrorKind::Other,
            HostOperation::Realpath,
            Some(PathBuf::from(physical)),
            "the physical custom type-root target must not be realpathed again",
        ))
        .build()
        .expect("build custom type-root realpath host");
    let type_root = ProgramPath::from_trusted_parts("/custom/types", "/custom/types")
        .expect("construct custom type root");
    let program = load_with_options(
        &host,
        &["/work/root.ts"],
        compiler_options(),
        program_options().with_type_roots(vec![type_root]),
        generous_limits(),
    )
    .expect("loaded custom type-root originalPath enters the prepared-program contract");

    assert_eq!(
        source_paths(&program),
        [Path::new(physical), Path::new("/work/root.ts")]
    );
    let resolution = program
        .resolutions()
        .require_type_reference(&type_reference_key(&program, "/work/root.ts", "pkg"))
        .expect("type-reference request has an authoritative row");
    let ResolutionOutcome::Resolved(directive) = resolution.outcome() else {
        panic!("type-reference request must resolve");
    };
    assert_eq!(directive.target().display(), Path::new(physical));
    assert_eq!(
        program
            .source_file(directive.source())
            .unwrap()
            .path()
            .display(),
        Path::new(physical)
    );
    assert_eq!(
        directive.original_path().map(ProgramPath::display),
        Some(Path::new(lexical))
    );
}

#[test]
fn unloaded_package_javascript_retains_each_physical_resolution_and_original_path() {
    for (physical, allow_js, expected_reason) in [
        (
            "/store/pkg/javascript-blob",
            false,
            UnloadedModuleReason::JavaScriptNotAdmitted,
        ),
        (
            "/store/node_modules/pkg/javascript-blob.data",
            true,
            UnloadedModuleReason::NodeModulesDepth,
        ),
    ] {
        let lexical = "/work/node_modules/pkg/index.js";
        let host = MemoryCompilerHost::builder("/work")
            .file("/work/root.ts", b"import 'pkg';\nexport {};".to_vec())
            .file(
                "/work/node_modules/pkg/package.json",
                br#"{"name":"pkg","version":"1.0.0","exports":"./index.js"}"#.to_vec(),
            )
            .file(lexical, b"module.exports = 1;".to_vec())
            .file(physical, b"module.exports = 1;".to_vec())
            .realpath(lexical, physical)
            .failure(HostError::new(
                HostErrorKind::Other,
                HostOperation::ReadFile,
                Some(PathBuf::from(physical)),
                "an unloaded JavaScript resolution must not read the physical target",
            ))
            .build()
            .expect("build unloaded package realpath host");
        let program = load_no_lib_program(
            &host,
            &[PathBuf::from("/work/root.ts")],
            CompilerOptions {
                no_emit: Some(true),
                allow_js,
                module: Some(199),
                module_resolution: Some(99),
                ..CompilerOptions::default()
            },
            program_options(),
            generous_limits(),
        )
        .expect("unloaded originalPath enters the prepared-program contract");

        let resolution = program
            .resolutions()
            .require_module(&module_key(&program, "/work/root.ts", "pkg"))
            .expect("package request has an authoritative row");
        let ResolutionOutcome::Resolved(module) = resolution.outcome() else {
            panic!("package request must resolve");
        };
        let ResolvedModuleTarget::Unloaded {
            resolved_file,
            reason,
        } = module.target()
        else {
            panic!("JavaScript package must remain unloaded");
        };
        assert_eq!(module.extension(), &ModuleExtension::Js);
        assert_eq!(resolved_file.display(), Path::new(physical));
        assert_eq!(*reason, expected_reason);
        assert_eq!(
            module.original_path().map(ProgramPath::display),
            Some(Path::new(lexical))
        );
    }
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
    .expect("allowJs is an admitted loader option");
    assert_eq!(
        allow_js
            .diagnostics()
            .program()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [6053]
    );

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
    .expect("noLib suppresses explicit library loading; TS5053 is owned by config diagnostics");
    assert!(no_lib_with_explicit_empty_lib.library_files().is_empty());
    assert_eq!(
        no_lib_with_explicit_empty_lib.compiler_options().lib,
        Some(Vec::new())
    );
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
        &["/work/first.ts", "//server/share/later.ts"],
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
fn windows_and_unc_root_spellings_use_typescript_lexical_normalization() {
    // tsc-port: getNormalizedAbsolutePath/simpleNormalizePath @6.0.3
    // tsc-hash: 538f15da938ce9f7bcd6aa26f945cffe1cadbc12095e8666dab9ca62320a13e2
    // tsc-span: _tsc.js:5349-5378,5653-5655
    let host = MemoryCompilerHost::builder("C:/work")
        .case_sensitive(true)
        .file("//server/share/unc.ts", b"export {};".to_vec())
        .file("//?/C:/sdk/extended.ts", b"export {};".to_vec())
        .file("/root/root-relative.ts", b"export {};".to_vec())
        .file("C:/work/drive.ts", b"export {};".to_vec())
        .build()
        .expect("build rooted Windows path host");
    let roots = [
        r"\\server\share\unc.ts",
        "//?/C:/sdk/extended.ts",
        r"\root\root-relative.ts",
        r"C:\work\drive.ts",
    ];
    let program = load(&host, &roots, generous_limits())
        .expect("rooted Windows spellings are valid TypeScript source roots");

    let expected = [
        Path::new("//server/share/unc.ts"),
        Path::new("//?/C:/sdk/extended.ts"),
        Path::new("/root/root-relative.ts"),
        Path::new("C:/work/drive.ts"),
    ];
    assert_eq!(source_paths(&program), expected);
    assert_eq!(
        program
            .roots()
            .iter()
            .map(|root| root.path().display())
            .collect::<Vec<_>>(),
        expected
    );
    assert!(program.diagnostics().program().is_empty());

    let error = load(&host, &["C:relative.ts"], generous_limits())
        .expect_err("raw drive-relative root display semantics remain fail-closed");
    assert_eq!(error.kind(), ProgramLoadErrorKind::Unsupported);
    assert_eq!(error.operation(), ProgramLoadOperation::NormalizeRoot);
    assert_eq!(error.path(), Some(Path::new("C:relative.ts")));
    let ProgramLoadError::Unsupported { feature, .. } = error else {
        unreachable!("kind identifies the unsupported variant");
    };
    assert_eq!(feature, "windows-path-form");
}

#[test]
#[ignore = "local H0 program oracle audit; requires the pinned Node runtime"]
fn windows_and_unc_root_normalization_matches_vendored_typescript() {
    const PROBE: &str = r#"
const ts = require(process.argv[1]);
const roots = [
  '\\\\server\\share\\unc.ts',
  '//?/C:/sdk/extended.ts',
  '\\root\\root-relative.ts',
  'C:\\work\\drive.ts',
];
const files = new Map([
  ['//server/share/unc.ts', 'export {};'],
  ['//?/C:/sdk/extended.ts', 'export {};'],
  ['/root/root-relative.ts', 'export {};'],
  ['C:/work/drive.ts', 'export {};'],
]);
const options = { noEmit: true, noLib: true, types: [] };
const host = ts.createCompilerHost(options);
host.useCaseSensitiveFileNames = () => true;
host.getCanonicalFileName = path => path;
host.getCurrentDirectory = () => 'C:/work';
host.directoryExists = () => true;
host.getDirectories = () => [];
host.fileExists = path => files.has(path);
host.readFile = path => files.get(path);
host.getSourceFile = (path, target) => files.has(path)
  ? ts.createSourceFile(path, files.get(path), target, true)
  : undefined;
const program = ts.createProgram({ rootNames: roots, options, host });
process.stdout.write(JSON.stringify(program.getSourceFiles().map(source => source.fileName)));
"#;

    let bundle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("vendor/typescript-6.0.3/lib/typescript.js");
    let output = Command::new("node")
        .arg("-e")
        .arg(PROBE)
        .arg(bundle)
        .output()
        .expect("run vendored TypeScript rooted Windows source probe");
    assert!(
        output.status.success(),
        "TypeScript probe failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let oracle: Value = serde_json::from_slice(&output.stdout).expect("probe output is JSON");

    let host = MemoryCompilerHost::builder("C:/work")
        .case_sensitive(true)
        .file("//server/share/unc.ts", b"export {};".to_vec())
        .file("//?/C:/sdk/extended.ts", b"export {};".to_vec())
        .file("/root/root-relative.ts", b"export {};".to_vec())
        .file("C:/work/drive.ts", b"export {};".to_vec())
        .build()
        .expect("build Rust rooted Windows oracle host");
    let program = load(
        &host,
        &[
            r"\\server\share\unc.ts",
            "//?/C:/sdk/extended.ts",
            r"\root\root-relative.ts",
            r"C:\work\drive.ts",
        ],
        generous_limits(),
    )
    .expect("load Rust rooted Windows oracle program");
    let rust = json!(program
        .source_files()
        .iter()
        .map(|source| source
            .path()
            .display()
            .to_str()
            .expect("source path is Unicode"))
        .collect::<Vec<_>>());
    assert_eq!(rust, oracle);
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

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

use tsc_host::{CompilerHost, HostError, HostErrorKind, HostOperation, MemoryCompilerHost};
use tsc_program::{
    CompilerOptions, HostResolvedModule, ModuleExtension, ModuleResolver, PackageId,
    PackageJsonType, PathMapping, ProgramOptions, ProgramPath, ResolutionError, ResolutionMode,
    ResolutionOutcome, ResolvedModuleTarget, SourceFileId,
};

const INNER_PACKAGE_JSON: &str = r#"{
    "name": "inner",
    "private": true,
    "exports": {
        "./cjs/*": "./*.cjs",
        "./cjs/exclude/*": null,
        "./mjs/*": "./*.mjs",
        "./mjs/exclude/*": null,
        "./js/*": "./*.js",
        "./js/exclude/*": null,
        "./conditional": { "types": "./index.js" },
        "./array": ["./index.js"]
    }
}"#;

struct MissingManifestContentsHost {
    inner: MemoryCompilerHost,
    manifest: PathBuf,
}

struct NthFileExistsFailureHost {
    inner: MemoryCompilerHost,
    watched_path: PathBuf,
    fail_on: usize,
    calls: RefCell<Vec<PathBuf>>,
    failure: HostError,
}

struct NthDirectoryExistsFailureHost {
    inner: MemoryCompilerHost,
    watched_path: PathBuf,
    fail_on: usize,
    calls: RefCell<Vec<PathBuf>>,
    failure: HostError,
}

struct RecordingFileExistsHost {
    inner: MemoryCompilerHost,
    calls: RefCell<Vec<PathBuf>>,
}

struct RecordingRealpathHost {
    inner: MemoryCompilerHost,
    calls: RefCell<Vec<PathBuf>>,
}

struct SequencedDirectoryExistsHost {
    inner: MemoryCompilerHost,
    watched_path: PathBuf,
    answers: Vec<bool>,
    calls: Cell<usize>,
}

struct RealpathAfterProbeHost {
    inner: MemoryCompilerHost,
    required_probe: PathBuf,
    primary_realpath: PathBuf,
    required_probe_seen: Cell<bool>,
}

struct PostManifestPackageRootMissHost {
    inner: MemoryCompilerHost,
    package_json: PathBuf,
    package_root: PathBuf,
    package_json_read: Cell<bool>,
    returned_root_miss: Cell<bool>,
    recover_after_miss: bool,
    file_calls: RefCell<Vec<PathBuf>>,
}

impl CompilerHost for PostManifestPackageRootMissHost {
    fn current_directory(&self) -> Result<PathBuf, HostError> {
        self.inner.current_directory()
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.inner.use_case_sensitive_file_names()
    }

    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError> {
        let result = self.inner.read_file(path)?;
        if path == self.package_json {
            self.package_json_read.set(true);
        }
        Ok(result)
    }

    fn file_exists(&self, path: &Path) -> Result<bool, HostError> {
        self.file_calls.borrow_mut().push(path.to_path_buf());
        self.inner.file_exists(path)
    }

    fn directory_exists(&self, path: &Path) -> Result<bool, HostError> {
        if path == self.package_root && self.package_json_read.get() {
            let already_missed = self.returned_root_miss.replace(true);
            if !self.recover_after_miss || !already_missed {
                return Ok(false);
            }
        }
        self.inner.directory_exists(path)
    }

    fn read_directory(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        self.inner.read_directory(path)
    }

    fn realpath(&self, path: &Path) -> Result<Option<PathBuf>, HostError> {
        self.inner.realpath(path)
    }
}

impl CompilerHost for SequencedDirectoryExistsHost {
    fn current_directory(&self) -> Result<PathBuf, HostError> {
        self.inner.current_directory()
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.inner.use_case_sensitive_file_names()
    }

    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError> {
        self.inner.read_file(path)
    }

    fn file_exists(&self, path: &Path) -> Result<bool, HostError> {
        self.inner.file_exists(path)
    }

    fn directory_exists(&self, path: &Path) -> Result<bool, HostError> {
        if path == self.watched_path {
            let call = self.calls.get();
            self.calls.set(call + 1);
            return Ok(self
                .answers
                .get(call)
                .copied()
                .or_else(|| self.answers.last().copied())
                .unwrap_or(false));
        }
        self.inner.directory_exists(path)
    }

    fn read_directory(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        self.inner.read_directory(path)
    }

    fn realpath(&self, path: &Path) -> Result<Option<PathBuf>, HostError> {
        self.inner.realpath(path)
    }
}

impl CompilerHost for RecordingFileExistsHost {
    fn current_directory(&self) -> Result<PathBuf, HostError> {
        self.inner.current_directory()
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.inner.use_case_sensitive_file_names()
    }

    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError> {
        self.inner.read_file(path)
    }

    fn file_exists(&self, path: &Path) -> Result<bool, HostError> {
        self.calls.borrow_mut().push(path.to_path_buf());
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
}

impl CompilerHost for RecordingRealpathHost {
    fn current_directory(&self) -> Result<PathBuf, HostError> {
        self.inner.current_directory()
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.inner.use_case_sensitive_file_names()
    }

    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError> {
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
        self.calls.borrow_mut().push(path.to_path_buf());
        self.inner.realpath(path)
    }
}

impl CompilerHost for RealpathAfterProbeHost {
    fn current_directory(&self) -> Result<PathBuf, HostError> {
        self.inner.current_directory()
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.inner.use_case_sensitive_file_names()
    }

    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError> {
        self.inner.read_file(path)
    }

    fn file_exists(&self, path: &Path) -> Result<bool, HostError> {
        if path == self.required_probe {
            self.required_probe_seen.set(true);
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
        if path == self.primary_realpath && !self.required_probe_seen.get() {
            return Err(HostError::new(
                HostErrorKind::Other,
                HostOperation::Realpath,
                Some(path.to_path_buf()),
                "primary realpath ran before the diagnostic alternate probe",
            ));
        }
        self.inner.realpath(path)
    }
}

impl CompilerHost for NthDirectoryExistsFailureHost {
    fn current_directory(&self) -> Result<PathBuf, HostError> {
        self.inner.current_directory()
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.inner.use_case_sensitive_file_names()
    }

    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError> {
        self.inner.read_file(path)
    }

    fn file_exists(&self, path: &Path) -> Result<bool, HostError> {
        self.inner.file_exists(path)
    }

    fn directory_exists(&self, path: &Path) -> Result<bool, HostError> {
        let mut calls = self.calls.borrow_mut();
        calls.push(path.to_path_buf());
        let watched_calls = calls
            .iter()
            .filter(|candidate| candidate.as_path() == self.watched_path.as_path())
            .count();
        if path == self.watched_path && watched_calls == self.fail_on {
            return Err(self.failure.clone());
        }
        drop(calls);
        self.inner.directory_exists(path)
    }

    fn read_directory(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        self.inner.read_directory(path)
    }

    fn realpath(&self, path: &Path) -> Result<Option<PathBuf>, HostError> {
        self.inner.realpath(path)
    }
}

impl CompilerHost for NthFileExistsFailureHost {
    fn current_directory(&self) -> Result<PathBuf, HostError> {
        self.inner.current_directory()
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.inner.use_case_sensitive_file_names()
    }

    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError> {
        self.inner.read_file(path)
    }

    fn file_exists(&self, path: &Path) -> Result<bool, HostError> {
        let mut calls = self.calls.borrow_mut();
        calls.push(path.to_path_buf());
        let watched_calls = calls
            .iter()
            .filter(|candidate| candidate.as_path() == self.watched_path.as_path())
            .count();
        if path == self.watched_path && watched_calls == self.fail_on {
            return Err(self.failure.clone());
        }
        drop(calls);
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
}

impl CompilerHost for MissingManifestContentsHost {
    fn current_directory(&self) -> Result<PathBuf, HostError> {
        self.inner.current_directory()
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.inner.use_case_sensitive_file_names()
    }

    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError> {
        if path == self.manifest {
            Ok(None)
        } else {
            self.inner.read_file(path)
        }
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
}

fn fixture_host() -> (MemoryCompilerHost, HostError) {
    let denied = HostError::new(
        HostErrorKind::PermissionDenied,
        HostOperation::ReadFile,
        Some(PathBuf::from("/node_modules/denied/package.json")),
        "denied by module-resolution contract",
    );
    let host = MemoryCompilerHost::builder("/")
        .file(
            "/package.json",
            br#"{"name":"root","private":true,"type":"module"}"#.to_vec(),
        )
        .file("/index.ts", b"export {};".to_vec())
        .file(
            "/node_modules/inner/package.json",
            INNER_PACKAGE_JSON.as_bytes().to_vec(),
        )
        .file("/node_modules/inner/test.d.ts", b"export {};".to_vec())
        .file(
            "/node_modules/inner/index.d.cts",
            b"export const cjs: true;".to_vec(),
        )
        .file(
            "/node_modules/inner/index.d.mts",
            b"export const mjs: true;".to_vec(),
        )
        .file(
            "/node_modules/inner/index.d.ts",
            b"export const js: true;".to_vec(),
        )
        // These files make an incorrect broad-pattern or legacy fallback
        // observable: the more-specific null entry must still terminate.
        .file(
            "/node_modules/inner/exclude/index.d.cts",
            b"export const excludedCjs: true;".to_vec(),
        )
        .file(
            "/node_modules/inner/exclude/index.d.mts",
            b"export const excludedMjs: true;".to_vec(),
        )
        .file(
            "/node_modules/inner/exclude/index.d.ts",
            b"export const excludedJs: true;".to_vec(),
        )
        .file(
            "/node_modules/denied/package.json",
            br#"{"name":"denied","exports":"./index.js"}"#.to_vec(),
        )
        .failure(denied.clone())
        .build()
        .expect("build one package-exports host tree");
    (host, denied)
}

fn options_for_module(module: i32) -> CompilerOptions {
    CompilerOptions {
        module: Some(module),
        ..CompilerOptions::default()
    }
}

fn program_path(value: &str) -> ProgramPath {
    ProgramPath::from_trusted_parts(value, value).expect("construct case-sensitive program path")
}

fn resolved(outcome: ResolutionOutcome<HostResolvedModule>) -> HostResolvedModule {
    let ResolutionOutcome::Resolved(resolved) = outcome else {
        panic!("expected a resolved package export");
    };
    resolved
}

fn assert_unsupported(error: ResolutionError, expected_feature: &str) {
    let ResolutionError::Unsupported { feature, detail } = error else {
        panic!("expected unsupported resolution, got {error:?}");
    };
    assert_eq!(feature, expected_feature);
    assert!(!detail.is_empty());
}

fn recorded_file_probes(host: &RecordingFileExistsHost, prefix: &str) -> Vec<String> {
    host.calls
        .borrow()
        .iter()
        .filter_map(|path| path.to_str())
        .filter(|path| path.starts_with(prefix))
        .map(str::to_owned)
        .collect()
}

#[test]
fn paths_exact_longest_prefix_and_substitution_order_are_stable() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file("/work/general/item.ts", b"export {};".to_vec())
        .file("/work/general/special/other.ts", b"export {};".to_vec())
        .file("/work/specific/item.ts", b"export {};".to_vec())
        .file("/work/specific/other.ts", b"export {};".to_vec())
        .file("/work/exact/item.ts", b"export {};".to_vec())
        .file("/work/tie-first/x.ts", b"export {};".to_vec())
        .file("/work/tie-second/x/tail.ts", b"export {};".to_vec())
        .file("/work/ordered/second.ts", b"export {};".to_vec())
        .build()
        .expect("build ordered paths host");
    let options = CompilerOptions {
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };
    let program_options = ProgramOptions::default().with_paths(vec![
        PathMapping::new("@pkg/*", vec!["general/*".to_owned()]),
        PathMapping::new("@pkg/special/*", vec!["specific/*".to_owned()]),
        PathMapping::new("@pkg/special/item", vec!["exact/item".to_owned()]),
        PathMapping::new("@tie/*/tail", vec!["tie-first/*".to_owned()]),
        PathMapping::new("@tie/*", vec!["tie-second/*".to_owned()]),
        PathMapping::new(
            "@ordered/*",
            vec!["ordered/missing/*".to_owned(), "ordered/*".to_owned()],
        ),
    ]);
    let mut resolver = ModuleResolver::new_with_program_options(&host, &options, &program_options)
        .expect("create paths resolver");

    for (specifier, expected) in [
        ("@pkg/item", "/work/general/item.ts"),
        ("@pkg/special/other", "/work/specific/other.ts"),
        ("@pkg/special/item", "/work/exact/item.ts"),
        ("@tie/x/tail", "/work/tie-first/x.ts"),
        ("@ordered/second", "/work/ordered/second.ts"),
    ] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.ts"),
                    specifier,
                    ResolutionMode::CommonJs,
                )
                .expect("resolve ordered paths candidate"),
        );
        assert_eq!(
            module.resolved_file().canonical().as_path(),
            Path::new(expected),
            "{specifier}"
        );
    }
}

#[test]
fn optional_settings_preserve_legacy_passes_and_modern_substitution_order() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file("/work/first/priority.js", b"module.exports = {};".to_vec())
        .file("/work/second/priority.ts", b"export {};".to_vec())
        .file("/work/first/explicit.js", b"module.exports = {};".to_vec())
        .file("/work/normalized.ts", b"export {};".to_vec())
        .build()
        .expect("build extension-pass paths host");
    let program_options = ProgramOptions::default().with_paths(vec![
        PathMapping::new(
            "priority",
            vec!["first/priority".to_owned(), "second/priority".to_owned()],
        ),
        PathMapping::new("explicit", vec!["first/explicit.js".to_owned()]),
        PathMapping::new("normalized", vec!["normalized.ts/.".to_owned()]),
    ]);

    for (resolution_kind, expected_priority) in [
        (1, "/work/second/priority.ts"),
        (2, "/work/second/priority.ts"),
        (3, "/work/first/priority.js"),
        (99, "/work/first/priority.js"),
        (100, "/work/first/priority.js"),
    ] {
        let options = CompilerOptions {
            module_resolution: Some(resolution_kind),
            ..CompilerOptions::default()
        };
        let mut resolver =
            ModuleResolver::new_with_program_options(&host, &options, &program_options)
                .expect("create resolver-kind paths resolver");
        let priority = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.ts"),
                    "priority",
                    ResolutionMode::CommonJs,
                )
                .expect("resolve extension-pass candidate"),
        );
        assert_eq!(
            priority.resolved_file().canonical().as_path(),
            Path::new(expected_priority),
            "moduleResolution={resolution_kind}"
        );

        let explicit = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.ts"),
                    "explicit",
                    ResolutionMode::CommonJs,
                )
                .expect("resolve raw explicit-extension substitution"),
        );
        assert_eq!(explicit.extension(), &ModuleExtension::Js);
        assert_eq!(
            explicit.resolved_file().canonical().as_path(),
            Path::new("/work/first/explicit.js")
        );

        let normalized = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.ts"),
                    "normalized",
                    ResolutionMode::CommonJs,
                )
                .expect("resolve a normalized written-extension substitution"),
        );
        assert_eq!(
            normalized.resolved_file().canonical().as_path(),
            Path::new("/work/normalized.ts")
        );
        assert!(
            normalized.resolved_using_ts_extension(),
            "moduleResolution={resolution_kind}"
        );
    }
}

#[test]
fn optional_node_candidates_preserve_trailing_directory_spelling() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file("/work/directory.ts", b"export const wrong: true;".to_vec())
        .directory("/work/directory/")
        .file(
            "/work/directory/index.ts",
            b"export const selected: true;".to_vec(),
        )
        .build()
        .expect("build trailing optional-candidate host");
    let options = CompilerOptions {
        module_resolution: Some(2),
        base_url: Some("/work".to_owned()),
        ..CompilerOptions::default()
    };

    let program_options = ProgramOptions::default().with_paths(vec![PathMapping::new(
        "mapped",
        vec!["directory/".to_owned()],
    )]);
    let mut resolver = ModuleResolver::new_with_program_options(&host, &options, &program_options)
        .expect("create trailing paths resolver");
    let mapped = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "mapped",
                ResolutionMode::CommonJs,
            )
            .expect("resolve a trailing paths substitution as a directory"),
    );
    assert_eq!(
        mapped.resolved_file().display(),
        Path::new("/work/directory/index.ts")
    );

    let mut resolver =
        ModuleResolver::new(&host, &options).expect("create trailing baseUrl resolver");
    let base_url = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "directory/",
                ResolutionMode::CommonJs,
            )
            .expect("resolve a trailing baseUrl candidate as a directory"),
    );
    assert_eq!(
        base_url.resolved_file().display(),
        Path::new("/work/directory/index.ts")
    );
}

#[test]
fn written_extension_replacement_groups_match_typescript_603_order() {
    for (specifier, existing, expected_extension, resolved_using_ts_extension, probes) in [
        (
            "./value.ts",
            "/work/value.jsx",
            ModuleExtension::Jsx,
            false,
            &[".ts", ".tsx", ".d.ts", ".js", ".jsx"][..],
        ),
        (
            "./value.d.ts",
            "/work/value.tsx",
            ModuleExtension::Tsx,
            true,
            &[".ts", ".tsx"][..],
        ),
        (
            "./value.tsx",
            "/work/value.js",
            ModuleExtension::Js,
            false,
            &[".tsx", ".ts", ".d.ts", ".jsx", ".js"][..],
        ),
        (
            "./value.jsx",
            "/work/value.ts",
            ModuleExtension::Ts,
            false,
            &[".tsx", ".ts"][..],
        ),
        (
            "./value.mts",
            "/work/value.d.mts",
            ModuleExtension::Dmts,
            true,
            &[".mts", ".d.mts"][..],
        ),
        (
            "./value.mjs",
            "/work/value.mjs",
            ModuleExtension::Mjs,
            false,
            &[".mts", ".d.mts", ".mjs"][..],
        ),
        (
            "./value.cts",
            "/work/value.d.cts",
            ModuleExtension::Dcts,
            true,
            &[".cts", ".d.cts"][..],
        ),
        (
            "./value.cjs",
            "/work/value.cjs",
            ModuleExtension::Cjs,
            false,
            &[".cts", ".d.cts", ".cjs"][..],
        ),
    ] {
        let inner = MemoryCompilerHost::builder("/work")
            .file("/work/main.ts", b"export {};".to_vec())
            .file(existing, b"export {};".to_vec())
            .build()
            .expect("build written-extension host");
        let host = RecordingFileExistsHost {
            inner,
            calls: RefCell::new(Vec::new()),
        };
        let options = CompilerOptions {
            module: Some(99),
            module_resolution: Some(100),
            ..CompilerOptions::default()
        };
        let mut resolver = ModuleResolver::new(&host, &options)
            .expect("create written-extension Bundler resolver");
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.ts"),
                    specifier,
                    ResolutionMode::EsNext,
                )
                .expect("resolve a written-extension replacement"),
        );
        assert_eq!(module.extension(), &expected_extension, "{specifier}");
        assert_eq!(
            module.resolved_file().display(),
            Path::new(existing),
            "{specifier}"
        );
        assert_eq!(
            module.resolved_using_ts_extension(),
            resolved_using_ts_extension,
            "{specifier}"
        );
        let expected = probes
            .iter()
            .map(|suffix| format!("/work/value{suffix}"))
            .collect::<Vec<_>>();
        assert_eq!(recorded_file_probes(&host, "/work/value"), expected);
    }
}

#[test]
fn commonjs_implicit_addition_follows_replacement_and_clears_ts_provenance() {
    for (module, module_resolution, mode, expected_path, expected_probes) in [
        (
            1,
            2,
            ResolutionMode::CommonJs,
            "/work/value.js.ts",
            &[".ts", ".tsx", ".d.ts", ".js.ts"][..],
        ),
        (
            99,
            100,
            ResolutionMode::EsNext,
            "/work/value.js",
            &[".ts", ".tsx", ".d.ts", ".js"][..],
        ),
    ] {
        let inner = MemoryCompilerHost::builder("/work")
            .file("/work/main.ts", b"export {};".to_vec())
            .file("/work/value.js", b"module.exports = {};".to_vec())
            .file("/work/value.js.ts", b"export {};".to_vec())
            .build()
            .expect("build implicit-addition precedence host");
        let host = RecordingFileExistsHost {
            inner,
            calls: RefCell::new(Vec::new()),
        };
        let options = CompilerOptions {
            module: Some(module),
            module_resolution: Some(module_resolution),
            ..CompilerOptions::default()
        };
        let mut resolver =
            ModuleResolver::new(&host, &options).expect("create implicit-addition resolver");
        let resolved = resolved(
            resolver
                .resolve(Path::new("/work/main.ts"), "./value.js", mode)
                .expect("resolve implicit-addition precedence"),
        );
        assert_eq!(resolved.resolved_file().display(), Path::new(expected_path));
        assert!(!resolved.resolved_using_ts_extension());
        let expected = expected_probes
            .iter()
            .map(|suffix| format!("/work/value{suffix}"))
            .collect::<Vec<_>>();
        assert_eq!(recorded_file_probes(&host, "/work/value"), expected);
    }
}

#[test]
fn implicit_addition_reobserves_its_parent_after_replacement_misses() {
    let failure = HostError::new(
        HostErrorKind::Other,
        HostOperation::DirectoryExists,
        Some(PathBuf::from("/work")),
        "the implicit-addition parent preflight remains observable",
    );
    let inner = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file("/work/value.js.ts", b"export {};".to_vec())
        .build()
        .expect("build implicit-addition parent host");
    let host = NthDirectoryExistsFailureHost {
        inner,
        watched_path: PathBuf::from("/work"),
        // resolve_relative preflights the candidate parent once; the
        // replacement and implicit tryAddingExtensions calls are two and
        // three respectively.
        fail_on: 3,
        calls: RefCell::new(Vec::new()),
        failure: failure.clone(),
    };
    let options = CompilerOptions {
        module: Some(1),
        module_resolution: Some(2),
        ..CompilerOptions::default()
    };
    let mut resolver =
        ModuleResolver::new(&host, &options).expect("create implicit-addition parent resolver");
    assert_eq!(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "./value.js",
                ResolutionMode::CommonJs,
            )
            .expect_err("the second tryAddingExtensions preflight must fail"),
        ResolutionError::from(failure)
    );
}

#[test]
fn node_esm_blocks_the_second_implicit_stage_but_node10_keeps_it() {
    let resolve = |module: i32,
                   module_resolution: i32,
                   mode: ResolutionMode|
     -> (ResolutionOutcome<HostResolvedModule>, Vec<String>) {
        let inner = MemoryCompilerHost::builder("/work")
            .file("/work/main.ts", b"export {};".to_vec())
            .file("/work/value.d.ts.ts", b"export {};".to_vec())
            .build()
            .expect("build implicit-stage ESM host");
        let host = RecordingFileExistsHost {
            inner,
            calls: RefCell::new(Vec::new()),
        };
        let options = CompilerOptions {
            module: Some(module),
            module_resolution: Some(module_resolution),
            ..CompilerOptions::default()
        };
        let mut resolver =
            ModuleResolver::new(&host, &options).expect("create implicit-stage ESM resolver");
        let outcome = resolver
            .resolve(Path::new("/work/main.ts"), "./value.d.ts", mode)
            .expect("resolve the implicit-stage ESM boundary");
        let probes = recorded_file_probes(&host, "/work/value");
        (outcome, probes)
    };

    let (node10, probes) = resolve(1, 2, ResolutionMode::CommonJs);
    let node10 = resolved(node10);
    assert_eq!(
        node10.resolved_file().display(),
        Path::new("/work/value.d.ts.ts")
    );
    assert_eq!(node10.extension(), &ModuleExtension::Ts);
    assert!(!node10.resolved_using_ts_extension());
    assert_eq!(
        probes,
        [
            "/work/value.ts",
            "/work/value.tsx",
            "/work/value.d.ts",
            "/work/value.d.ts.ts",
        ]
    );

    let (node_next_esm, probes) = resolve(199, 99, ResolutionMode::EsNext);
    assert_eq!(node_next_esm, ResolutionOutcome::NotFound);
    assert_eq!(
        probes,
        [
            "/work/value.ts",
            "/work/value.tsx",
            "/work/value.d.ts",
            "/work/value.js",
            "/work/value.jsx",
        ]
    );
}

#[test]
fn json_declaration_twins_keep_their_arbitrary_extension_identity() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file("/work/data.d.json.ts", b"declare const value: 1;".to_vec())
        .build()
        .expect("build JSON declaration-twin host");
    let options = CompilerOptions {
        module: Some(99),
        module_resolution: Some(100),
        resolve_json_module: Some(false),
        ..CompilerOptions::default()
    };
    let mut resolver =
        ModuleResolver::new(&host, &options).expect("create JSON declaration-twin resolver");
    let module = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "./data.json",
                ResolutionMode::EsNext,
            )
            .expect("resolve a JSON declaration twin"),
    );
    assert_eq!(
        module.extension(),
        &ModuleExtension::Arbitrary(".d.json.ts".to_owned())
    );
    assert_eq!(
        module.resolved_file().display(),
        Path::new("/work/data.d.json.ts")
    );
    assert!(!module.resolved_using_ts_extension());
}

#[test]
fn package_map_ts_targets_are_exact_but_js_targets_use_replacement_groups() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/pkg/package.json",
            br#"{
                "name":"pkg",
                "version":"1.0.0",
                "exports": {
                    "./exact-tsx":"./exact.tsx",
                    "./replace-jsx":"./replace.jsx"
                }
            }"#
            .to_vec(),
        )
        // A package-map TS target is exact-only in the preferred pass.
        .file("/work/node_modules/pkg/exact.ts", b"export {};".to_vec())
        // A written JavaScript target still enters tryAddingExtensions.
        .file(
            "/work/node_modules/pkg/replace.ts",
            b"export const replaced = true;".to_vec(),
        )
        .build()
        .expect("build package-map written-extension host");
    let options = CompilerOptions {
        module: Some(99),
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &options)
        .expect("create package-map written-extension resolver");

    assert_eq!(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "pkg/exact-tsx",
                ResolutionMode::EsNext,
            )
            .expect("an exact TS package target remains an authoritative miss"),
        ResolutionOutcome::NotFound
    );

    let module = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "pkg/replace-jsx",
                ResolutionMode::EsNext,
            )
            .expect("a JS package target uses its replacement family"),
    );
    assert_eq!(
        module.resolved_file().display(),
        Path::new("/work/node_modules/pkg/replace.ts")
    );
    assert_eq!(module.extension(), &ModuleExtension::Ts);
    assert!(!module.resolved_using_ts_extension());
    assert!(module.is_external_library_import());
    assert_eq!(module.package_id().map(PackageId::name), Some("pkg"));
}

#[test]
fn package_map_exact_targets_skip_parent_preflight_but_directory_fields_require_it() {
    let failure = HostError::new(
        HostErrorKind::Other,
        HostOperation::DirectoryExists,
        Some(PathBuf::from("/work/node_modules/pkg/nested")),
        "nested package-target parent preflight",
    );
    let package_map = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/pkg/package.json",
            br#"{"name":"pkg","version":"1.0.0","exports":"./nested/index.ts"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/pkg/nested/index.ts",
            b"export const value = true;".to_vec(),
        )
        .build()
        .expect("build exact package-map parent host");
    let host = NthDirectoryExistsFailureHost {
        inner: package_map,
        watched_path: PathBuf::from("/work/node_modules/pkg/nested"),
        fail_on: 1,
        calls: RefCell::new(Vec::new()),
        failure: failure.clone(),
    };
    let options = options_for_module(199);
    let mut resolver =
        ModuleResolver::new(&host, &options).expect("create exact package-map parent resolver");
    let module = resolved(
        resolver
            .resolve(Path::new("/work/main.ts"), "pkg", ResolutionMode::EsNext)
            .expect("an exact package-map target skips its parent observation"),
    );
    assert_eq!(
        module.resolved_file().display(),
        Path::new("/work/node_modules/pkg/nested/index.ts")
    );

    let package_field = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/pkg/package.json",
            br#"{"name":"pkg","version":"1.0.0","types":"nested/index.ts"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/pkg/nested/index.ts",
            b"export const value = true;".to_vec(),
        )
        .build()
        .expect("build exact package-field parent host");
    let host = NthDirectoryExistsFailureHost {
        inner: package_field,
        watched_path: PathBuf::from("/work/node_modules/pkg/nested"),
        fail_on: 1,
        calls: RefCell::new(Vec::new()),
        failure: failure.clone(),
    };
    let mut resolver =
        ModuleResolver::new(&host, &options).expect("create exact package-field parent resolver");
    assert_eq!(
        resolver
            .resolve(Path::new("/work/main.ts"), "pkg", ResolutionMode::EsNext)
            .expect_err("a package directory field observes its parent first"),
        ResolutionError::from(failure)
    );
}

#[test]
fn package_fields_retain_the_exact_phase_before_the_ordinary_loader_phase() {
    let build = || {
        MemoryCompilerHost::builder("/work")
            .file("/work/main.ts", b"export {};".to_vec())
            .file(
                "/work/node_modules/pkg/package.json",
                br#"{"name":"pkg","version":"1.0.0","types":"index.ts"}"#.to_vec(),
            )
            .file(
                "/work/node_modules/pkg/index.d.ts",
                b"export const value: true;".to_vec(),
            )
            .build()
            .expect("build package-field phase host")
    };
    let host = RecordingFileExistsHost {
        inner: build(),
        calls: RefCell::new(Vec::new()),
    };
    let options = options_for_module(199);
    let mut resolver =
        ModuleResolver::new(&host, &options).expect("create package-field phase resolver");
    let module = resolved(
        resolver
            .resolve(Path::new("/work/main.ts"), "pkg", ResolutionMode::EsNext)
            .expect("resolve through both package-field phases"),
    );
    assert_eq!(
        module.resolved_file().display(),
        Path::new("/work/node_modules/pkg/index.d.ts")
    );
    assert!(!module.resolved_using_ts_extension());
    assert_eq!(
        recorded_file_probes(&host, "/work/node_modules/pkg/index"),
        [
            "/work/node_modules/pkg/index.ts",
            "/work/node_modules/pkg/index.ts",
            "/work/node_modules/pkg/index.tsx",
            "/work/node_modules/pkg/index.d.ts",
        ]
    );

    let failure = HostError::new(
        HostErrorKind::Other,
        HostOperation::FileExists,
        Some(PathBuf::from("/work/node_modules/pkg/index.ts")),
        "the second package-field exact probe remains observable",
    );
    let host = NthFileExistsFailureHost {
        inner: build(),
        watched_path: PathBuf::from("/work/node_modules/pkg/index.ts"),
        fail_on: 2,
        calls: RefCell::new(Vec::new()),
        failure: failure.clone(),
    };
    let mut resolver =
        ModuleResolver::new(&host, &options).expect("create failing package-field phase resolver");
    assert_eq!(
        resolver
            .resolve(Path::new("/work/main.ts"), "pkg", ResolutionMode::EsNext)
            .expect_err("the second exact probe must fail before the declaration hit"),
        ResolutionError::from(failure)
    );
}

#[test]
fn package_directory_failure_latches_suppress_root_types_versions_and_commonjs_index() {
    let inner = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/pkg/package.json",
            br#"{
                "name":"pkg",
                "version":"1.0.0",
                "types":"missing/index.ts",
                "typesVersions": {
                    "*": {"missing/index.ts":["types/good.ts"]}
                }
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/pkg/types/good.ts",
            b"export const good = true;".to_vec(),
        )
        .build()
        .expect("build missing package-field parent host");
    let host = RecordingFileExistsHost {
        inner,
        calls: RefCell::new(Vec::new()),
    };
    let options = options_for_module(199);
    let mut resolver =
        ModuleResolver::new(&host, &options).expect("create missing package-field parent resolver");
    assert_eq!(
        resolver
            .resolve(Path::new("/work/main.ts"), "pkg", ResolutionMode::CommonJs,)
            .expect("a latched package-directory miss is an ordinary miss"),
        ResolutionOutcome::NotFound
    );
    assert!(
        recorded_file_probes(&host, "/work/node_modules/pkg/types/good").is_empty(),
        "typesVersions must only record failures after the package-field parent miss"
    );

    let inner = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/pkg/package.json",
            br#"{"name":"pkg","version":"1.0.0"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/pkg/index.d.ts",
            b"export const index: true;".to_vec(),
        )
        .build()
        .expect("build transient package-root host");
    let host = PostManifestPackageRootMissHost {
        inner,
        package_json: PathBuf::from("/work/node_modules/pkg/package.json"),
        package_root: PathBuf::from("/work/node_modules/pkg"),
        package_json_read: Cell::new(false),
        returned_root_miss: Cell::new(false),
        recover_after_miss: true,
        file_calls: RefCell::new(Vec::new()),
    };
    let mut resolver =
        ModuleResolver::new(&host, &options).expect("create transient package-root resolver");
    assert_eq!(
        resolver
            .resolve(Path::new("/work/main.ts"), "pkg", ResolutionMode::CommonJs,)
            .expect("a CommonJS index cannot revive after its directory latch"),
        ResolutionOutcome::NotFound
    );
    assert!(host.returned_root_miss.get());

    let inner = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/pkg/package.json",
            br#"{
                "name":"pkg",
                "version":"1.0.0",
                "typesVersions":{"*":{"sub":["types/value.ts"]}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/pkg/types/value.ts",
            b"export const value = true;".to_vec(),
        )
        .build()
        .expect("build missing subpath package-root host");
    let host = PostManifestPackageRootMissHost {
        inner,
        package_json: PathBuf::from("/work/node_modules/pkg/package.json"),
        package_root: PathBuf::from("/work/node_modules/pkg"),
        package_json_read: Cell::new(false),
        returned_root_miss: Cell::new(false),
        recover_after_miss: false,
        file_calls: RefCell::new(Vec::new()),
    };
    let mut resolver =
        ModuleResolver::new(&host, &options).expect("create missing subpath package-root resolver");
    assert_eq!(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "pkg/sub",
                ResolutionMode::CommonJs,
            )
            .expect("a subpath typesVersions lookup reuses its package-root latch"),
        ResolutionOutcome::NotFound
    );
    assert!(host.returned_root_miss.get());
    assert!(
        host.file_calls
            .borrow()
            .iter()
            .all(|path| path != Path::new("/work/node_modules/pkg/types/value.ts")),
        "a latched subpath mapping must not probe its exact target"
    );
}

#[test]
fn types_versions_distinguishes_raw_substitutions_and_root_package_field_provenance() {
    let inner = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/pkg/package.json",
            br#"{
                "name":"pkg",
                "version":"1.0.0",
                "types":"index.ts",
                "typesVersions": {
                    "*": {
                        "index.ts":["types/root.ts"],
                        "wild/*":["types/*"],
                        "normalized":["types/normalized.ts/."]
                    }
                }
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/pkg/types/root.tsx",
            b"export const root: true;".to_vec(),
        )
        .file(
            "/work/node_modules/pkg/types/value.tsx",
            b"export const value: true;".to_vec(),
        )
        .file(
            "/work/node_modules/pkg/types/normalized.ts",
            b"export const normalized: true;".to_vec(),
        )
        .build()
        .expect("build typesVersions substitution-phase host");
    let host = RecordingFileExistsHost {
        inner,
        calls: RefCell::new(Vec::new()),
    };
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options)
        .expect("create typesVersions substitution-phase resolver");

    let root = resolved(
        resolver
            .resolve(Path::new("/work/main.ts"), "pkg", ResolutionMode::EsNext)
            .expect("resolve a root typesVersions package-field substitution"),
    );
    assert_eq!(
        root.resolved_file().display(),
        Path::new("/work/node_modules/pkg/types/root.tsx")
    );
    assert!(!root.resolved_using_ts_extension());
    assert_eq!(
        recorded_file_probes(&host, "/work/node_modules/pkg/types/root"),
        [
            "/work/node_modules/pkg/types/root.ts",
            "/work/node_modules/pkg/types/root.ts",
            "/work/node_modules/pkg/types/root.ts",
            "/work/node_modules/pkg/types/root.tsx",
        ]
    );

    host.calls.borrow_mut().clear();
    let subpath = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "pkg/wild/value.ts",
                ResolutionMode::EsNext,
            )
            .expect("resolve a wildcard typesVersions ordinary substitution"),
    );
    assert_eq!(
        subpath.resolved_file().display(),
        Path::new("/work/node_modules/pkg/types/value.tsx")
    );
    assert!(subpath.resolved_using_ts_extension());
    assert_eq!(
        recorded_file_probes(&host, "/work/node_modules/pkg/types/value"),
        [
            "/work/node_modules/pkg/types/value.ts",
            "/work/node_modules/pkg/types/value.tsx",
        ]
    );

    host.calls.borrow_mut().clear();
    let normalized = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "pkg/normalized",
                ResolutionMode::EsNext,
            )
            .expect("resolve a normalized typesVersions substitution"),
    );
    assert_eq!(
        normalized.resolved_file().display(),
        Path::new("/work/node_modules/pkg/types/normalized.ts")
    );
    assert!(normalized.resolved_using_ts_extension());
}

#[test]
fn node_esm_index_exception_runs_after_an_owned_types_versions_miss() {
    let build = || {
        MemoryCompilerHost::builder("/work")
            .file("/work/main.mts", b"export {};".to_vec())
            .file(
                "/work/node_modules/pkg/package.json",
                br#"{
                    "name":"pkg",
                    "version":"1.0.0",
                    "types":"types.d.ts",
                    "typesVersions":{"*":{"types.d.ts":["missing"]}}
                }"#
                .to_vec(),
            )
            .file(
                "/work/node_modules/pkg/index.d.ts",
                b"export const index: true;".to_vec(),
            )
            .build()
            .expect("build Node ESM typesVersions miss host")
    };
    let options = options_for_module(199);
    let host = build();
    let mut resolver =
        ModuleResolver::new(&host, &options).expect("create Node ESM index resolver");
    let module = resolved(
        resolver
            .resolve(Path::new("/work/main.mts"), "pkg", ResolutionMode::EsNext)
            .expect("the outer Node ESM loader retries its exceptional index.js"),
    );
    assert_eq!(
        module.resolved_file().display(),
        Path::new("/work/node_modules/pkg/index.d.ts")
    );

    let host = build();
    let mut resolver =
        ModuleResolver::new(&host, &options).expect("create CommonJS terminal-miss resolver");
    assert_eq!(
        resolver
            .resolve(Path::new("/work/main.mts"), "pkg", ResolutionMode::CommonJs,)
            .expect("the matched mapping owns a CommonJS miss"),
        ResolutionOutcome::NotFound
    );
}

#[test]
fn types_versions_use_normalized_root_names_and_replace_only_the_first_star() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/root-name/package.json",
            br#"{
                "name":"root-name",
                "version":"1.0.0",
                "types":"./src/../index.ts",
                "typesVersions":{"*":{
                    "index.ts":["types/good.d.ts"],
                    "src/../index.ts":["types/bad.d.ts"]
                }}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/root-name/types/good.d.ts",
            b"export const selected: 'good';".to_vec(),
        )
        .file(
            "/work/node_modules/root-name/types/bad.d.ts",
            b"export const selected: 'bad';".to_vec(),
        )
        .file(
            "/work/node_modules/stars/package.json",
            br#"{
                "name":"stars",
                "version":"1.0.0",
                "typesVersions":{"*":{"foo/*":["types/*/copy-*"]}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/stars/types/x/copy-*.d.ts",
            b"export const selected: 'literal';".to_vec(),
        )
        .file(
            "/work/node_modules/stars/types/x/copy-x.d.ts",
            b"export const selected: 'replaced';".to_vec(),
        )
        .build()
        .expect("build normalized typesVersions host");
    let options = CompilerOptions {
        module: Some(1),
        module_resolution: Some(2),
        ..CompilerOptions::default()
    };
    let mut resolver =
        ModuleResolver::new(&host, &options).expect("create normalized typesVersions resolver");

    let root = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "root-name",
                ResolutionMode::CommonJs,
            )
            .expect("match the normalized package-field-relative name"),
    );
    assert_eq!(
        root.resolved_file().display(),
        Path::new("/work/node_modules/root-name/types/good.d.ts")
    );

    let star = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "stars/foo/x",
                ResolutionMode::CommonJs,
            )
            .expect("replace only the first substitution star"),
    );
    assert_eq!(
        star.resolved_file().display(),
        Path::new("/work/node_modules/stars/types/x/copy-*.d.ts")
    );
}

#[test]
fn types_versions_keep_empty_stars_and_first_equal_prefix_patterns() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/empty/package.json",
            br#"{
                "name":"empty",
                "version":"1.0.0",
                "typesVersions":{"*":{"foo*":["types/*.d.ts"]}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/empty/types/*.d.ts",
            b"export const selected: 'literal';".to_vec(),
        )
        .file(
            "/work/node_modules/empty/types/.d.ts",
            b"export const selected: 'removed';".to_vec(),
        )
        .file(
            "/work/node_modules/tie/package.json",
            br#"{
                "name":"tie",
                "version":"1.0.0",
                "typesVersions":{"*":{
                    "foo/*":["general.d.ts"],
                    "foo/*.ts":["specific.d.ts"]
                }}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/tie/general.d.ts",
            b"export const selected: 'first';".to_vec(),
        )
        .file(
            "/work/node_modules/tie/specific.d.ts",
            b"export const selected: 'second';".to_vec(),
        )
        .build()
        .expect("build typesVersions pattern-boundary host");
    let options = options_for_module(1);
    let mut resolver =
        ModuleResolver::new(&host, &options).expect("create typesVersions pattern resolver");

    let empty = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "empty/foo",
                ResolutionMode::CommonJs,
            )
            .expect("retain a literal substitution star for an empty capture"),
    );
    assert_eq!(
        empty.resolved_file().display(),
        Path::new("/work/node_modules/empty/types/*.d.ts")
    );

    let tie = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "tie/foo/x.ts",
                ResolutionMode::CommonJs,
            )
            .expect("retain insertion order for equal-prefix patterns"),
    );
    assert_eq!(
        tie.resolved_file().display(),
        Path::new("/work/node_modules/tie/general.d.ts")
    );
}

#[test]
fn package_root_logical_names_keep_root_component_boundaries() {
    let host = MemoryCompilerHost::builder("/")
        .file("/src/main.ts", b"export {};".to_vec())
        .file(
            "/package.json",
            br#"{
                "name":"root",
                "version":"1.0.0",
                "types":"index.ts",
                "typesVersions":{"*":{"index.ts":["versioned.d.ts"]}}
            }"#
            .to_vec(),
        )
        .file("/versioned.d.ts", b"export const root: true;".to_vec())
        .build()
        .expect("build filesystem-root package host");
    let options = options_for_module(1);
    let mut resolver =
        ModuleResolver::new(&host, &options).expect("create filesystem-root package resolver");
    let module = resolved(
        resolver
            .resolve(Path::new("/src/main.ts"), "../..", ResolutionMode::CommonJs)
            .expect("resolve a package rooted at the filesystem root"),
    );
    assert_eq!(
        module.resolved_file().display(),
        Path::new("/versioned.d.ts")
    );
}

#[test]
fn drive_root_package_logical_names_ignore_drive_letter_case() {
    let host = MemoryCompilerHost::builder("C:/")
        .case_sensitive(false)
        .file("C:/src/main.ts", b"export {};".to_vec())
        .file(
            "C:/package.json",
            br#"{
                "name":"root",
                "version":"1.0.0",
                "types":"c:/index.ts",
                "typesVersions":{"*":{"index.ts":["versioned.d.ts"]}}
            }"#
            .to_vec(),
        )
        .file("C:/versioned.d.ts", b"export const root: true;".to_vec())
        .build()
        .expect("build drive-root package host");
    let options = options_for_module(1);
    let mut resolver =
        ModuleResolver::new(&host, &options).expect("create drive-root package resolver");
    let module = resolved(
        resolver
            .resolve(
                Path::new("C:/src/main.ts"),
                "../..",
                ResolutionMode::CommonJs,
            )
            .expect("resolve a package rooted at a case-varied drive root"),
    );
    assert_eq!(
        module.resolved_file().display(),
        Path::new("C:/versioned.d.ts")
    );
}

#[test]
fn trailing_package_fields_skip_files_and_trim_types_versions_logical_names() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/direct/package.json",
            br#"{"name":"direct","version":"1.0.0","types":"dir/"}"#.to_vec(),
        )
        .directory("/work/node_modules/direct/dir/")
        .file(
            "/work/node_modules/direct/dir/.ts",
            b"export const wrong: true;".to_vec(),
        )
        .file(
            "/work/node_modules/direct/dir/index.d.ts",
            b"export const direct: true;".to_vec(),
        )
        .file(
            "/work/node_modules/versioned/package.json",
            br#"{
                "name":"versioned",
                "version":"1.0.0",
                "types":"dir/",
                "typesVersions":{"*":{"dir":["versioned.d.ts"]}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/versioned/versioned.d.ts",
            b"export const versioned: true;".to_vec(),
        )
        .file(
            "/work/node_modules/dotted/package.json",
            br#"{"name":"dotted","version":"1.0.0","types":"dir.ext/"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/dotted/dir.d.ext/.ts",
            b"export const dotted: true;".to_vec(),
        )
        .build()
        .expect("build trailing package-field host");
    let options = options_for_module(1);
    let mut resolver =
        ModuleResolver::new(&host, &options).expect("create trailing package-field resolver");

    let direct = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "direct",
                ResolutionMode::CommonJs,
            )
            .expect("resolve a trailing package field as a directory"),
    );
    assert_eq!(
        direct.resolved_file().display(),
        Path::new("/work/node_modules/direct/dir/index.d.ts")
    );

    let versioned = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "versioned",
                ResolutionMode::CommonJs,
            )
            .expect("match a trailing package field without a logical slash"),
    );
    assert_eq!(
        versioned.resolved_file().display(),
        Path::new("/work/node_modules/versioned/versioned.d.ts")
    );

    let dotted = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "dotted",
                ResolutionMode::CommonJs,
            )
            .expect("resolve the path-bearing arbitrary twin of a dotted directory"),
    );
    assert_eq!(
        dotted.resolved_file().display(),
        Path::new("/work/node_modules/dotted/dir.d.ext/.ts")
    );
    assert_eq!(
        dotted.extension(),
        &ModuleExtension::Arbitrary(".d.ext/.ts".to_owned())
    );
}

#[test]
fn legacy_package_fields_and_types_versions_may_escape_the_package_root() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/pkg/package.json",
            br#"{
                "name":"pkg",
                "version":"1.0.0",
                "types":"../shared.d.ts",
                "typesVersions":{"*":{"../shared.d.ts":["types/bad.d.ts"]}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/pkg/types/bad.d.ts",
            b"export const selected: 'bad';".to_vec(),
        )
        .file(
            "/work/node_modules/shared.d.ts",
            b"export const selected: 'shared';".to_vec(),
        )
        .file(
            "/work/node_modules/mapped/package.json",
            br#"{
                "name":"mapped",
                "version":"1.0.0",
                "typesVersions":{"*":{
                    "exact":["../shared.d.ts"],
                    "generic":["../shared"],
                    "trailing":["./"]
                }}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/mapped.ts",
            b"export const selected: 'sibling';".to_vec(),
        )
        // TypeScript preserves this trailing spelling when it observes the
        // mapped directory. The in-memory host intentionally does not fold
        // lexical spellings, so make that exact host identity available.
        .directory("/work/node_modules/mapped/")
        .file(
            "/work/node_modules/mapped/index.d.ts",
            b"export const selected: 'index';".to_vec(),
        )
        .build()
        .expect("build escaping legacy package target host");
    let options = options_for_module(1);
    let mut resolver =
        ModuleResolver::new(&host, &options).expect("create escaping package target resolver");

    let field = resolved(
        resolver
            .resolve(Path::new("/work/main.ts"), "pkg", ResolutionMode::CommonJs)
            .expect("resolve a package field outside its package root"),
    );
    assert_eq!(
        field.resolved_file().display(),
        Path::new("/work/node_modules/shared.d.ts")
    );
    assert_eq!(field.package_id().map(PackageId::name), Some("pkg"));
    assert_eq!(
        field.package_id().map(PackageId::submodule_name),
        Some("ed.d.ts")
    );

    let exact = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "mapped/exact",
                ResolutionMode::CommonJs,
            )
            .expect("resolve an exact escaping typesVersions substitution"),
    );
    assert_eq!(
        exact.resolved_file().display(),
        Path::new("/work/node_modules/shared.d.ts")
    );
    assert_eq!(exact.package_id(), None);

    let generic = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "mapped/generic",
                ResolutionMode::CommonJs,
            )
            .expect("resolve a generic escaping typesVersions substitution"),
    );
    assert_eq!(
        generic.resolved_file().display(),
        Path::new("/work/node_modules/shared.d.ts")
    );
    assert_eq!(generic.package_id().map(PackageId::name), Some("mapped"));

    let trailing = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "mapped/trailing",
                ResolutionMode::CommonJs,
            )
            .expect("a trailing substitution skips the package-root file phase"),
    );
    assert_eq!(
        trailing.resolved_file().display(),
        Path::new("/work/node_modules/mapped/index.d.ts")
    );
}

#[test]
fn root_dirs_preserve_legacy_passes_modern_candidate_order_and_relative_gate() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/src/app/main.ts", b"export {};".to_vec())
        .file("/work/src/app/value.js", b"module.exports = {};".to_vec())
        .file("/work/gen/app/value.ts", b"export {};".to_vec())
        .file("/work/gen/app/root-only.ts", b"export {};".to_vec())
        .file("/work/mapped/priority.ts", b"export {};".to_vec())
        .file("/work/gen/app/path-priority.ts", b"export {};".to_vec())
        .file("/work/gen/app/path-miss.ts", b"export {};".to_vec())
        .build()
        .expect("build rootDirs extension-order host");
    let program_options = ProgramOptions::default()
        .with_root_dirs(vec![program_path("/work/src"), program_path("/work/gen")])
        .with_paths(vec![
            PathMapping::new(
                "/work/src/app/path-priority",
                vec!["mapped/priority".to_owned()],
            ),
            PathMapping::new("/work/src/app/path-miss", vec!["mapped/missing".to_owned()]),
        ]);

    for (resolution_kind, expected_value) in [
        (1, "/work/gen/app/value.ts"),
        (2, "/work/gen/app/value.ts"),
        (3, "/work/src/app/value.js"),
        (99, "/work/src/app/value.js"),
        (100, "/work/src/app/value.js"),
    ] {
        let options = CompilerOptions {
            module_resolution: Some(resolution_kind),
            ..CompilerOptions::default()
        };
        let mut resolver =
            ModuleResolver::new_with_program_options(&host, &options, &program_options)
                .expect("create rootDirs resolver");

        let value = resolved(
            resolver
                .resolve(
                    Path::new("/work/src/app/main.ts"),
                    "./value",
                    ResolutionMode::CommonJs,
                )
                .expect("resolve extension-order rootDirs candidate"),
        );
        assert_eq!(
            value.resolved_file().display(),
            Path::new(expected_value),
            "moduleResolution={resolution_kind}"
        );
        assert_eq!(value.original_path(), None);
        assert!(!value.is_external_library_import());

        let root_only = resolved(
            resolver
                .resolve(
                    Path::new("/work/src/app/main.ts"),
                    "./root-only",
                    ResolutionMode::CommonJs,
                )
                .expect("resolve alternate rootDirs candidate"),
        );
        assert_eq!(
            root_only.resolved_file().display(),
            Path::new("/work/gen/app/root-only.ts")
        );

        let rooted = resolved(
            resolver
                .resolve(
                    Path::new("/work/src/app/main.ts"),
                    "/work/src/app/root-only",
                    ResolutionMode::CommonJs,
                )
                .expect("rooted disk requests participate in rootDirs"),
        );
        assert_eq!(
            rooted.resolved_file().display(),
            Path::new("/work/gen/app/root-only.ts")
        );

        let paths_first = resolved(
            resolver
                .resolve(
                    Path::new("/work/src/app/main.ts"),
                    "/work/src/app/path-priority",
                    ResolutionMode::CommonJs,
                )
                .expect("rooted disk requests remain eligible for paths"),
        );
        assert_eq!(
            paths_first.resolved_file().display(),
            Path::new("/work/mapped/priority.ts")
        );
        assert_eq!(
            resolver
                .resolve(
                    Path::new("/work/src/app/main.ts"),
                    "/work/src/app/path-miss",
                    ResolutionMode::CommonJs,
                )
                .expect("a matching paths miss owns rooted optional settings"),
            ResolutionOutcome::NotFound
        );

        let bare = resolver
            .resolve(
                Path::new("/work/src/app/main.ts"),
                "root-only",
                ResolutionMode::CommonJs,
            )
            .expect("bare requests bypass rootDirs");
        assert!(matches!(bare, ResolutionOutcome::NotFound));

        assert_unsupported(
            resolver
                .resolve(
                    Path::new("/work/src/app/main.ts"),
                    "//server/share/value",
                    ResolutionMode::CommonJs,
                )
                .expect_err("unowned UNC forms must not be retargeted as POSIX paths"),
            "windows-path-form",
        );
    }

    let root_dirs_only = ProgramOptions::default()
        .with_root_dirs(vec![program_path("/work/src"), program_path("/work/gen")]);
    let options = CompilerOptions {
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new_with_program_options(&host, &options, &root_dirs_only)
        .expect("create rootDirs-only UNC guard resolver");
    assert_unsupported(
        resolver
            .resolve(
                Path::new("/work/src/app/main.ts"),
                "//server/share/value",
                ResolutionMode::CommonJs,
            )
            .expect_err("rootDirs alone must not retarget an unowned UNC form"),
        "non-bare-module-specifier",
    );
}

#[test]
fn root_dirs_use_the_longest_prefix_then_declared_alternate_order() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/src/app/main.ts", b"export {};".to_vec())
        .file("/mirror/first/app/item.ts", b"export {};".to_vec())
        .file("/work/app/item.ts", b"export {};".to_vec())
        .file("/mirror/second/app/item.ts", b"export {};".to_vec())
        // This would win if the shorter `/work` prefix were selected.
        .file("/mirror/first/src/app/item.ts", b"export {};".to_vec())
        .build()
        .expect("build longest-prefix rootDirs host");
    let program_options = ProgramOptions::default().with_root_dirs(vec![
        program_path("/mirror/first"),
        program_path("/work"),
        program_path("/work/src"),
        program_path("/mirror/second"),
    ]);

    for resolution_kind in [1, 2, 3, 99, 100] {
        let options = CompilerOptions {
            module_resolution: Some(resolution_kind),
            ..CompilerOptions::default()
        };
        let mut resolver =
            ModuleResolver::new_with_program_options(&host, &options, &program_options)
                .expect("create longest-prefix rootDirs resolver");
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/src/app/main.ts"),
                    "./item",
                    ResolutionMode::CommonJs,
                )
                .expect("resolve longest-prefix rootDirs candidate"),
        );
        assert_eq!(
            module.resolved_file().display(),
            Path::new("/mirror/first/app/item.ts"),
            "moduleResolution={resolution_kind}"
        );
    }
}

#[test]
fn rooted_final_dot_components_keep_node_directory_spelling() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/src/app/dir.ts", b"export {};".to_vec())
        .build()
        .expect("build rooted dot-component host");
    let program_options = ProgramOptions::default()
        .with_root_dirs(vec![program_path("/work/src"), program_path("/work/gen")]);

    for resolution_kind in [1, 2, 3, 99, 100] {
        let options = CompilerOptions {
            module_resolution: Some(resolution_kind),
            ..CompilerOptions::default()
        };
        let mut resolver =
            ModuleResolver::new_with_program_options(&host, &options, &program_options)
                .expect("create rooted dot-component resolver");
        let outcome = resolver
            .resolve(
                Path::new("/else/main.ts"),
                "/work/src/app/dir/.",
                ResolutionMode::CommonJs,
            )
            .expect("resolve rooted final dot component");
        if resolution_kind == 1 {
            assert_eq!(
                resolved(outcome).resolved_file().display(),
                Path::new("/work/src/app/dir.ts")
            );
        } else {
            assert_eq!(
                outcome,
                ResolutionOutcome::NotFound,
                "moduleResolution={resolution_kind}"
            );
        }
    }
}

#[test]
fn root_dirs_reuse_each_resolvers_directory_rules() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/src/app/main.ts", b"export {};".to_vec())
        .file("/work/src/app/folder.ts", b"export {};".to_vec())
        // The host boundary preserves the trailing spelling TypeScript sends
        // to directoryExists for an explicitly slash-terminated candidate.
        .directory("/work/gen/app/folder/")
        .file("/work/gen/app/folder/index.ts", b"export {};".to_vec())
        .build()
        .expect("build rootDirs directory host");
    let program_options = ProgramOptions::default()
        .with_root_dirs(vec![program_path("/work/src"), program_path("/work/gen")]);

    for (resolution_kind, mode, should_resolve) in [
        (1, ResolutionMode::CommonJs, false),
        (2, ResolutionMode::CommonJs, true),
        (3, ResolutionMode::CommonJs, true),
        (3, ResolutionMode::EsNext, false),
        (99, ResolutionMode::CommonJs, true),
        (99, ResolutionMode::EsNext, false),
        (100, ResolutionMode::EsNext, true),
    ] {
        let options = CompilerOptions {
            module_resolution: Some(resolution_kind),
            ..CompilerOptions::default()
        };
        let mut resolver =
            ModuleResolver::new_with_program_options(&host, &options, &program_options)
                .expect("create rootDirs directory resolver");
        let outcome = resolver
            .resolve(Path::new("/work/src/app/main.ts"), "./folder/", mode)
            .expect("probe rootDirs directory candidate");
        match outcome {
            ResolutionOutcome::Resolved(module) if should_resolve => assert_eq!(
                module.resolved_file().display(),
                Path::new("/work/gen/app/folder/index.ts")
            ),
            ResolutionOutcome::NotFound if !should_resolve => {}
            outcome => panic!(
                "unexpected rootDirs directory outcome for moduleResolution={resolution_kind}, mode={mode:?}: {outcome:?}"
            ),
        }
    }
}

#[test]
fn matched_paths_miss_suppresses_base_url_but_keeps_ordinary_fallbacks() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file("/work/base/pkg.ts", b"export const wrong = true;".to_vec())
        .file("/work/base/unmapped.ts", b"export {};".to_vec())
        .file("/work/node_modules/pkg/index.d.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/@types/pkg/index.d.ts",
            b"export {};".to_vec(),
        )
        .build()
        .expect("build paths fallback host");
    let program_options = ProgramOptions::default().with_paths(vec![PathMapping::new(
        "pkg",
        vec!["missing/pkg".to_owned()],
    )]);

    for resolution_kind in [1, 2, 3, 99, 100] {
        let options = CompilerOptions {
            module_resolution: Some(resolution_kind),
            base_url: Some("./base".to_owned()),
            ..CompilerOptions::default()
        };
        let mut resolver =
            ModuleResolver::new_with_program_options(&host, &options, &program_options)
                .expect("create fallback resolver");
        let module = resolved(
            resolver
                .resolve(Path::new("/work/main.ts"), "pkg", ResolutionMode::CommonJs)
                .expect("fall through from matched paths miss"),
        );
        let expected = if resolution_kind == 1 {
            "/work/node_modules/@types/pkg/index.d.ts"
        } else {
            "/work/node_modules/pkg/index.d.ts"
        };
        assert_eq!(
            module.resolved_file().canonical().as_path(),
            Path::new(expected),
            "moduleResolution={resolution_kind}"
        );

        let base_url = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.ts"),
                    "unmapped",
                    ResolutionMode::CommonJs,
                )
                .expect("a paths non-match continues to baseUrl"),
        );
        assert_eq!(
            base_url.resolved_file().canonical().as_path(),
            Path::new("/work/base/unmapped.ts")
        );
    }
}

#[test]
fn optional_settings_preserve_each_observable_parent_directory_latch() {
    for use_paths in [false, true] {
        let inner = MemoryCompilerHost::builder("/")
            .file("/main.ts", b"export {};".to_vec())
            .file("/base/pkg.ts", b"export const found = true;".to_vec())
            .build()
            .expect("build sequenced optional-settings host");
        let host = SequencedDirectoryExistsHost {
            inner,
            watched_path: PathBuf::from("/base"),
            answers: vec![true, true, false],
            calls: Cell::new(0),
        };
        let options = CompilerOptions {
            module_resolution: Some(100),
            base_url: (!use_paths).then(|| "/base".to_owned()),
            ..CompilerOptions::default()
        };
        let program_options = if use_paths {
            ProgramOptions::default()
                .with_paths(vec![PathMapping::new("pkg", vec!["base/pkg".to_owned()])])
        } else {
            ProgramOptions::default()
        };
        let mut resolver =
            ModuleResolver::new_with_program_options(&host, &options, &program_options)
                .expect("create sequenced Node optional-settings resolver");
        assert_eq!(
            resolver
                .resolve(Path::new("/main.ts"), "pkg", ResolutionMode::EsNext)
                .expect("observe every Node parent-directory latch"),
            ResolutionOutcome::NotFound,
            "use_paths={use_paths}"
        );
        assert_eq!(host.calls.get(), 3, "use_paths={use_paths}");
    }

    for use_paths in [false, true] {
        let inner = MemoryCompilerHost::builder("/")
            .file("/main.ts", b"export {};".to_vec())
            .file("/base/pkg.ts", b"export const found = true;".to_vec())
            .build()
            .expect("build sequenced Classic optional-settings host");
        let host = SequencedDirectoryExistsHost {
            inner,
            watched_path: PathBuf::from("/base"),
            answers: vec![true, false],
            calls: Cell::new(0),
        };
        let options = CompilerOptions {
            module_resolution: Some(1),
            base_url: (!use_paths).then(|| "/base".to_owned()),
            ..CompilerOptions::default()
        };
        let program_options = if use_paths {
            ProgramOptions::default()
                .with_paths(vec![PathMapping::new("pkg", vec!["base/pkg".to_owned()])])
        } else {
            ProgramOptions::default()
        };
        let mut resolver =
            ModuleResolver::new_with_program_options(&host, &options, &program_options)
                .expect("create sequenced Classic optional-settings resolver");
        assert_eq!(
            resolver
                .resolve(Path::new("/main.ts"), "pkg", ResolutionMode::CommonJs)
                .expect("observe both Classic parent-directory latches"),
            ResolutionOutcome::NotFound,
            "use_paths={use_paths}"
        );
        assert_eq!(host.calls.get(), 3, "use_paths={use_paths}");
    }

    let inner = MemoryCompilerHost::builder("/")
        .file("/main.ts", b"export {};".to_vec())
        .file("/base/pkg.ts", b"export const exact = true;".to_vec())
        .build()
        .expect("build exact paths shortcut host");
    let host = SequencedDirectoryExistsHost {
        inner,
        watched_path: PathBuf::from("/base"),
        answers: vec![false],
        calls: Cell::new(0),
    };
    let options = CompilerOptions {
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };
    let program_options = ProgramOptions::default().with_paths(vec![PathMapping::new(
        "pkg",
        vec!["base/pkg.ts".to_owned()],
    )]);
    let mut resolver = ModuleResolver::new_with_program_options(&host, &options, &program_options)
        .expect("create exact paths shortcut resolver");
    assert_eq!(
        resolved(
            resolver
                .resolve(Path::new("/main.ts"), "pkg", ResolutionMode::EsNext)
                .expect("resolve the raw recognized-extension substitution")
        )
        .resolved_file()
        .display(),
        Path::new("/base/pkg.ts")
    );
    assert_eq!(
        host.calls.get(),
        0,
        "the raw recognized-extension shortcut bypasses the caller latch"
    );
}

#[test]
fn paths_without_base_url_use_cwd_and_path_matching_remains_case_sensitive() {
    let host = MemoryCompilerHost::builder("/Work/Project")
        .case_sensitive(false)
        .file("/work/project/main.ts", b"export {};".to_vec())
        .file("/work/project/src/value.ts", b"export {};".to_vec())
        .file("/work/shared/parent.ts", b"export {};".to_vec())
        .file("/shared/absolute.ts", b"export {};".to_vec())
        .file(
            "/work/project/package.json",
            br#"{"name":"cwd","version":"1.0.0"}"#.to_vec(),
        )
        .file("/work/project/index.ts", b"export {};".to_vec())
        .build()
        .expect("build case-insensitive cwd host");
    let options = CompilerOptions {
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };
    let program_options = ProgramOptions::default().with_paths(vec![
        PathMapping::new("@Alias/*", vec!["./src/*".to_owned()]),
        PathMapping::new("@parent", vec!["../shared/parent".to_owned()]),
        PathMapping::new("@absolute", vec!["/shared/absolute".to_owned()]),
        PathMapping::new("@cwd", vec![String::new()]),
    ]);
    let mut resolver = ModuleResolver::new_with_program_options(&host, &options, &program_options)
        .expect("create cwd paths resolver");

    for (specifier, expected) in [
        ("@Alias/Value", "/work/project/src/value.ts"),
        ("@parent", "/work/shared/parent.ts"),
        ("@absolute", "/shared/absolute.ts"),
        ("@cwd", "/work/project/index.ts"),
    ] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/Work/Project/main.ts"),
                    specifier,
                    ResolutionMode::CommonJs,
                )
                .expect("resolve cwd-relative paths mapping"),
        );
        assert_eq!(
            module.resolved_file().canonical().as_path(),
            Path::new(expected),
            "{specifier}"
        );
    }
    assert_eq!(
        resolver
            .resolve(
                Path::new("/Work/Project/main.ts"),
                "@alias/Value",
                ResolutionMode::CommonJs,
            )
            .expect("case-distinct pattern is a supported miss"),
        ResolutionOutcome::NotFound
    );

    let base_options = CompilerOptions {
        module_resolution: Some(100),
        base_url: Some("./base/../src".to_owned()),
        ..CompilerOptions::default()
    };
    let mut base_resolver =
        ModuleResolver::new(&host, &base_options).expect("normalize relative baseUrl from cwd");
    let module = resolved(
        base_resolver
            .resolve(
                Path::new("/Work/Project/main.ts"),
                "Value",
                ResolutionMode::CommonJs,
            )
            .expect("resolve normalized baseUrl candidate"),
    );
    assert_eq!(
        module.resolved_file().canonical().as_path(),
        Path::new("/work/project/src/value.ts")
    );
}

#[test]
fn paths_host_failures_stop_before_later_substitutions() {
    let denied = HostError::new(
        HostErrorKind::PermissionDenied,
        HostOperation::FileExists,
        Some(PathBuf::from("/work/first/value.ts")),
        "first paths substitution denied",
    );
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file("/work/first/placeholder.txt", b"present".to_vec())
        .file("/work/second/value.ts", b"export {};".to_vec())
        .failure(denied.clone())
        .build()
        .expect("build paths failure host");
    let options = CompilerOptions {
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };
    let program_options = ProgramOptions::default().with_paths(vec![PathMapping::new(
        "@value",
        vec!["first/value".to_owned(), "second/value".to_owned()],
    )]);
    let mut resolver = ModuleResolver::new_with_program_options(&host, &options, &program_options)
        .expect("create paths failure resolver");

    let error = resolver
        .resolve(
            Path::new("/work/main.ts"),
            "@value",
            ResolutionMode::CommonJs,
        )
        .expect_err("first substitution host failure must not become a miss");
    assert_eq!(error, ResolutionError::Host(denied));
}

#[test]
fn root_dirs_reprobe_the_original_candidate_and_propagate_host_failures() {
    let watched_path = PathBuf::from("/work/src/app/value.ts");
    let program_options = ProgramOptions::default().with_root_dirs(vec![
        program_path("/work/src"),
        program_path("/work/src"),
        program_path("/work/gen"),
    ]);

    for resolution_kind in [1, 2, 3, 99, 100] {
        let failure = HostError::new(
            HostErrorKind::PermissionDenied,
            HostOperation::FileExists,
            Some(watched_path.clone()),
            format!("second original rootDirs probe denied for {resolution_kind}"),
        );
        let inner = MemoryCompilerHost::builder("/work")
            .file("/work/src/app/main.ts", b"export {};".to_vec())
            .file("/work/gen/app/.keep", Vec::new())
            .build()
            .expect("build rootDirs reprobe host");
        let host = NthFileExistsFailureHost {
            inner,
            watched_path: watched_path.clone(),
            fail_on: 2,
            calls: RefCell::new(Vec::new()),
            failure: failure.clone(),
        };
        let options = CompilerOptions {
            module_resolution: Some(resolution_kind),
            ..CompilerOptions::default()
        };
        let mut resolver =
            ModuleResolver::new_with_program_options(&host, &options, &program_options)
                .expect("create rootDirs reprobe resolver");
        let error = resolver
            .resolve(
                Path::new("/work/src/app/main.ts"),
                "./value",
                ResolutionMode::CommonJs,
            )
            .expect_err("the repeated original probe must expose its host failure");
        assert_eq!(error, ResolutionError::Host(failure));

        let ts_calls = host
            .calls
            .borrow()
            .iter()
            .filter(|path| path.file_name().is_some_and(|name| name == "value.ts"))
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            ts_calls,
            [
                PathBuf::from("/work/src/app/value.ts"),
                PathBuf::from("/work/gen/app/value.ts"),
                PathBuf::from("/work/src/app/value.ts"),
            ],
            "moduleResolution={resolution_kind}"
        );
    }

    let duplicate_alternate = PathBuf::from("/work/gen/app/value.ts");
    let failure = HostError::new(
        HostErrorKind::PermissionDenied,
        HostOperation::FileExists,
        Some(duplicate_alternate.clone()),
        "second equal alternate rootDirs probe denied",
    );
    let inner = MemoryCompilerHost::builder("/work")
        .file("/work/src/app/main.ts", b"export {};".to_vec())
        .file("/work/gen/app/.keep", Vec::new())
        .build()
        .expect("build duplicate alternate rootDirs host");
    let host = NthFileExistsFailureHost {
        inner,
        watched_path: duplicate_alternate,
        fail_on: 2,
        calls: RefCell::new(Vec::new()),
        failure: failure.clone(),
    };
    let options = CompilerOptions {
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };
    let duplicate_alternate_options = ProgramOptions::default().with_root_dirs(vec![
        program_path("/work/src"),
        program_path("/work/gen"),
        program_path("/work/gen"),
    ]);
    let mut resolver =
        ModuleResolver::new_with_program_options(&host, &options, &duplicate_alternate_options)
            .expect("create duplicate alternate rootDirs resolver");
    assert_eq!(
        resolver
            .resolve(
                Path::new("/work/src/app/main.ts"),
                "./value",
                ResolutionMode::CommonJs,
            )
            .expect_err("equal non-matched roots retain duplicate observable probes"),
        ResolutionError::Host(failure)
    );
}

#[test]
fn root_dirs_preflight_containing_directories_before_candidate_probes() {
    let program_options = ProgramOptions::default()
        .with_root_dirs(vec![program_path("/work/src"), program_path("/work/gen")]);

    for resolution_kind in [1, 2, 3, 99, 100] {
        let denied = HostError::new(
            HostErrorKind::PermissionDenied,
            HostOperation::DirectoryExists,
            Some(PathBuf::from("/work/src/app")),
            format!("rootDirs containing directory denied for {resolution_kind}"),
        );
        let host = MemoryCompilerHost::builder("/work")
            .file("/work/src/app/main.ts", b"export {};".to_vec())
            .file("/work/gen/app/sub/value.ts", b"export {};".to_vec())
            .failure(denied.clone())
            .build()
            .expect("build rootDirs containing-directory failure host");
        let options = CompilerOptions {
            module_resolution: Some(resolution_kind),
            ..CompilerOptions::default()
        };
        let mut resolver =
            ModuleResolver::new_with_program_options(&host, &options, &program_options)
                .expect("create rootDirs preflight resolver");
        assert_eq!(
            resolver
                .resolve(
                    Path::new("/work/src/app/main.ts"),
                    "./sub/value",
                    ResolutionMode::CommonJs,
                )
                .expect_err("containing-directory preflight failure must propagate"),
            ResolutionError::Host(denied)
        );

        let host = MemoryCompilerHost::builder("/work")
            .file("/work/src/app/value.ts", b"export {};".to_vec())
            .file("/work/gen/app/value.ts", b"export {};".to_vec())
            .build()
            .expect("build rootDirs absent-containing-directory host");
        let mut resolver =
            ModuleResolver::new_with_program_options(&host, &options, &program_options)
                .expect("create rooted preflight resolver");
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/else/main.ts"),
                    "/work/src/app/value",
                    ResolutionMode::CommonJs,
                )
                .expect("skip an original candidate whose containing directory is absent"),
        );
        assert_eq!(
            module.resolved_file().display(),
            Path::new("/work/gen/app/value.ts"),
            "moduleResolution={resolution_kind}"
        );

        let hidden_failure = HostError::new(
            HostErrorKind::PermissionDenied,
            HostOperation::DirectoryExists,
            Some(PathBuf::from("/work/src/app/missing/value")),
            format!("onlyRecordFailures must hide candidate directory for {resolution_kind}"),
        );
        let host = MemoryCompilerHost::builder("/work")
            .file("/work/src/app/main.ts", b"export {};".to_vec())
            .failure(hidden_failure)
            .build()
            .expect("build missing candidate-parent host");
        let mut resolver =
            ModuleResolver::new_with_program_options(&host, &options, &program_options)
                .expect("create onlyRecordFailures rootDirs resolver");
        assert_eq!(
            resolver
                .resolve(
                    Path::new("/work/src/app/main.ts"),
                    "./missing/value",
                    ResolutionMode::CommonJs,
                )
                .expect("a missing parent suppresses later candidate-directory host work"),
            ResolutionOutcome::NotFound,
            "moduleResolution={resolution_kind}"
        );
    }
}

#[test]
fn malformed_paths_configuration_fails_before_resolution() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .build()
        .expect("build paths validation host");
    let options = CompilerOptions::default();
    let cases = [
        ProgramOptions::default().with_paths(vec![PathMapping::new("", vec!["value".to_owned()])]),
        ProgramOptions::default().with_paths(vec![
            PathMapping::new("dup", vec!["first".to_owned()]),
            PathMapping::new("dup", vec!["second".to_owned()]),
        ]),
        ProgramOptions::default()
            .with_paths(vec![PathMapping::new("two**", vec!["value".to_owned()])]),
        ProgramOptions::default().with_paths(vec![PathMapping::new("empty", Vec::new())]),
        ProgramOptions::default()
            .with_paths(vec![PathMapping::new("two", vec!["value**".to_owned()])]),
        ProgramOptions::default().with_paths(vec![PathMapping::new(
            "nul",
            vec!["value\0path".to_owned()],
        )]),
        ProgramOptions::default().with_paths(vec![PathMapping::new(
            "unc",
            vec![r"\\server\share\*".to_owned()],
        )]),
        ProgramOptions::default().with_paths(vec![PathMapping::new(
            "drive",
            vec!["C:relative/*".to_owned()],
        )]),
    ];

    for program_options in cases {
        let error =
            match ModuleResolver::new_with_program_options(&host, &options, &program_options) {
                Ok(_) => panic!("malformed paths configuration must fail closed"),
                Err(error) => error,
            };
        assert!(matches!(
            error,
            ResolutionError::InvalidData(_) | ResolutionError::Unsupported { .. }
        ));
    }

    for base_url in ["\0", r"\\server\share", "C:relative"] {
        let options = CompilerOptions {
            base_url: Some(base_url.to_owned()),
            ..CompilerOptions::default()
        };
        let error = match ModuleResolver::new(&host, &options) {
            Ok(_) => panic!("malformed baseUrl must fail closed"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            ResolutionError::InvalidData(_) | ResolutionError::Unsupported { .. }
        ));
    }
}

#[test]
fn root_dirs_require_normalized_paths_and_match_display_case() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .build()
        .expect("build rootDirs validation host");
    let options = CompilerOptions {
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };
    let non_normalized =
        ProgramPath::from_trusted_parts("/work/generated/../generated", "/work/generated")
            .expect("construct representable but non-normalized rootDirs path");
    let program_options = ProgramOptions::default().with_root_dirs(vec![non_normalized]);
    let error = match ModuleResolver::new_with_program_options(&host, &options, &program_options) {
        Ok(_) => panic!("non-normalized rootDirs path must fail closed"),
        Err(error) => error,
    };
    let ResolutionError::Canonicalization { path, detail } = error else {
        panic!("expected rootDirs canonicalization failure, got {error:?}");
    };
    assert_eq!(
        path.as_deref(),
        Some(Path::new("/work/generated/../generated"))
    );
    assert!(detail.contains("rootDirs"));

    let insensitive = MemoryCompilerHost::builder("/work")
        .case_sensitive(false)
        .file("/work/src/app/main.ts", b"export {};".to_vec())
        .file("/work/gen/app/value.ts", b"export {};".to_vec())
        .build()
        .expect("build case-insensitive rootDirs host");
    let program_options = ProgramOptions::default().with_root_dirs(vec![
        ProgramPath::from_trusted_parts("/Work/Src", "/work/src")
            .expect("construct normalized case-insensitive rootDir"),
        ProgramPath::from_trusted_parts("/work/gen", "/work/gen")
            .expect("construct alternate case-insensitive rootDir"),
    ]);
    let mut resolver =
        ModuleResolver::new_with_program_options(&insensitive, &options, &program_options)
            .expect("accept normalized case-insensitive rootDirs identities");
    assert_eq!(
        resolver
            .resolve(
                Path::new("/work/src/app/main.ts"),
                "./value",
                ResolutionMode::CommonJs,
            )
            .expect("case-distinct display prefix is a supported miss"),
        ResolutionOutcome::NotFound
    );
}

#[test]
fn optional_local_file_probe_does_not_read_an_ancestor_package() {
    let denied = HostError::new(
        HostErrorKind::PermissionDenied,
        HostOperation::ReadFile,
        Some(PathBuf::from("/work/package.json")),
        "local optional resolution must not inspect an ancestor package",
    );
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file("/work/src/value.ts", b"export const value = 1;".to_vec())
        .file(
            "/work/package.json",
            br#"{"name":"unrelated","version":"1.0.0"}"#.to_vec(),
        )
        .failure(denied)
        .build()
        .expect("build local optional package-failure host");
    let options = CompilerOptions {
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };
    let program_options = ProgramOptions::default().with_paths(vec![PathMapping::new(
        "value",
        vec!["src/value".to_owned()],
    )]);
    let mut resolver = ModuleResolver::new_with_program_options(&host, &options, &program_options)
        .expect("create local optional resolver");

    let module = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "value",
                ResolutionMode::CommonJs,
            )
            .expect("resolve before unrelated ancestor package metadata"),
    );
    assert_eq!(
        module.resolved_file().canonical().as_path(),
        Path::new("/work/src/value.ts")
    );
    assert_eq!(module.package_id(), None);
    assert_eq!(module.package_metadata(), None);
}

#[test]
fn optional_external_files_use_the_package_root_and_follow_realpath() {
    let source = b"export const value = 1;".to_vec();
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/pkg/package.json",
            br#"{"name":"pkg","version":"1.0.0"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/pkg/sub/package.json",
            br#"{"name":"wrong-nested-package","version":"9.0.0"}"#.to_vec(),
        )
        .file("/work/node_modules/pkg/sub/value.ts", source.clone())
        .file("/store/pkg/sub/value.ts", source)
        .realpath(
            "/work/node_modules/pkg/sub/value.ts",
            "/store/pkg/sub/value.ts",
        )
        .build()
        .expect("build external optional realpath host");
    let options = CompilerOptions {
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };
    let program_options = ProgramOptions::default().with_paths(vec![
        PathMapping::new("value", vec!["node_modules/pkg/sub/value".to_owned()]),
        PathMapping::new("exact", vec!["node_modules/pkg/sub/value.ts".to_owned()]),
    ]);
    let mut resolver = ModuleResolver::new_with_program_options(&host, &options, &program_options)
        .expect("create external optional resolver");

    let module = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "value",
                ResolutionMode::CommonJs,
            )
            .expect("resolve external optional file"),
    );
    assert_eq!(
        module.resolved_file().canonical().as_path(),
        Path::new("/store/pkg/sub/value.ts")
    );
    assert_eq!(
        module
            .original_path()
            .expect("external lexical path is retained")
            .canonical()
            .as_path(),
        Path::new("/work/node_modules/pkg/sub/value.ts")
    );
    let package_id = module.package_id().expect("node package id is attached");
    assert_eq!(package_id.name(), "pkg");
    assert_eq!(package_id.submodule_name(), "sub/value.ts");
    assert_eq!(
        module
            .package_metadata()
            .and_then(|metadata| metadata.name()),
        Some("pkg")
    );

    let exact = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "exact",
                ResolutionMode::CommonJs,
            )
            .expect("resolve exact-extension external optional file"),
    );
    assert_eq!(
        exact.resolved_file().canonical().as_path(),
        Path::new("/store/pkg/sub/value.ts")
    );
    assert!(exact.original_path().is_some());
    assert_eq!(exact.package_id(), None);
}

#[test]
fn preserve_symlinks_keeps_upstream_module_and_type_reference_results_lexical() {
    // tests/cases/compiler/moduleResolutionWithSymlinks_preserveSymlinks.ts
    // gives two node_modules links the same physical declaration target and
    // requires both imports plus the triple-slash type reference to retain
    // their caller-visible spellings when preserveSymlinks is true.
    let linked_source = b"export { real } from \"real\";\nexport class C { private x; }\n".to_vec();
    let host = RecordingRealpathHost {
        inner: MemoryCompilerHost::builder("/app")
            .file(
                "/app/app.ts",
                concat!(
                    "/// <reference types=\"linked\" />\n",
                    "import { C as C1 } from \"linked\";\n",
                    "import { C as C2 } from \"linked2\";\n",
                )
                .as_bytes()
                .to_vec(),
            )
            .file("/linked/index.d.ts", linked_source.clone())
            .file("/app/node_modules/linked/index.d.ts", linked_source.clone())
            .file("/app/node_modules/linked2/index.d.ts", linked_source)
            .file(
                "/app/node_modules/real/index.d.ts",
                b"export const real: string;\n".to_vec(),
            )
            .realpath("/app/node_modules/linked/index.d.ts", "/linked/index.d.ts")
            .realpath("/app/node_modules/linked2/index.d.ts", "/linked/index.d.ts")
            .build()
            .expect("build the upstream preserveSymlinks topology"),
        calls: RefCell::new(Vec::new()),
    };
    let options = CompilerOptions {
        target: Some(2),
        module: Some(5),
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };
    let omitted = ProgramOptions::default();
    let explicit_false = ProgramOptions::default().with_preserve_symlinks(false);
    let preserve = ProgramOptions::default().with_preserve_symlinks(true);
    assert_eq!(omitted.preserve_symlinks(), None);
    assert!(!omitted.preserve_symlinks_effective());
    assert_eq!(explicit_false.preserve_symlinks(), Some(false));
    assert!(!explicit_false.preserve_symlinks_effective());
    assert_eq!(preserve.preserve_symlinks(), Some(true));
    assert!(preserve.preserve_symlinks_effective());

    let resolve_fixture = |program_options: Option<&ProgramOptions>| {
        let mut resolver = match program_options {
            Some(program_options) => {
                ModuleResolver::new_with_program_options(&host, &options, program_options)
                    .expect("create an option-aware resolver")
            }
            None => ModuleResolver::new(&host, &options).expect("create the default resolver"),
        };
        let ResolutionOutcome::Resolved(type_reference) = resolver
            .resolve_type_reference(
                Path::new("/app/app.ts"),
                "linked",
                ResolutionMode::Unspecified,
                None,
            )
            .expect("resolve the upstream type reference")
        else {
            panic!("expected the upstream type reference to resolve");
        };
        let linked = resolved(
            resolver
                .resolve(Path::new("/app/app.ts"), "linked", ResolutionMode::EsNext)
                .expect("resolve the first upstream import"),
        );
        let linked2 = resolved(
            resolver
                .resolve(Path::new("/app/app.ts"), "linked2", ResolutionMode::EsNext)
                .expect("resolve the second upstream import"),
        );
        (type_reference, linked, linked2)
    };

    host.calls.borrow_mut().clear();
    let preserved = resolve_fixture(Some(&preserve));
    // This recording host observes only the direct resolver's final
    // result-conversion seam; it is not a promise that no compiler subsystem
    // may ever ask the host for a real path under preserveSymlinks.
    assert!(host.calls.borrow().is_empty());
    let (preserved_reference, preserved_linked, preserved_linked2) = &preserved;
    assert_eq!(
        preserved_reference.resolved_file().canonical().as_path(),
        Path::new("/app/node_modules/linked/index.d.ts")
    );
    assert_eq!(preserved_reference.original_path(), None);
    assert!(!preserved_reference.primary());
    assert!(preserved_reference.is_external_library_import());
    assert_eq!(preserved_reference.extension(), &ModuleExtension::Dts);
    for (module, expected) in [
        (
            preserved_linked,
            Path::new("/app/node_modules/linked/index.d.ts"),
        ),
        (
            preserved_linked2,
            Path::new("/app/node_modules/linked2/index.d.ts"),
        ),
    ] {
        assert_eq!(module.resolved_file().canonical().as_path(), expected);
        assert_eq!(module.original_path(), None);
        assert!(module.is_external_library_import());
        assert_eq!(module.extension(), &ModuleExtension::Dts);
    }

    let expected_realpath_calls = vec![
        PathBuf::from("/app/node_modules/linked/index.d.ts"),
        PathBuf::from("/app/node_modules/linked/index.d.ts"),
        PathBuf::from("/app/node_modules/linked2/index.d.ts"),
    ];
    host.calls.borrow_mut().clear();
    let omitted_results = resolve_fixture(None);
    assert_eq!(*host.calls.borrow(), expected_realpath_calls);

    host.calls.borrow_mut().clear();
    let explicit_false_results = resolve_fixture(Some(&explicit_false));
    assert_eq!(*host.calls.borrow(), expected_realpath_calls);
    assert_eq!(explicit_false_results, omitted_results);

    let (followed_reference, followed_linked, followed_linked2) = &omitted_results;
    assert_eq!(
        followed_reference.resolved_file().canonical().as_path(),
        Path::new("/linked/index.d.ts")
    );
    assert_eq!(
        followed_reference
            .original_path()
            .expect("retain the type reference's lexical link")
            .canonical()
            .as_path(),
        Path::new("/app/node_modules/linked/index.d.ts")
    );
    for (module, expected_original) in [
        (
            followed_linked,
            Path::new("/app/node_modules/linked/index.d.ts"),
        ),
        (
            followed_linked2,
            Path::new("/app/node_modules/linked2/index.d.ts"),
        ),
    ] {
        assert_eq!(
            module.resolved_file().canonical().as_path(),
            Path::new("/linked/index.d.ts")
        );
        assert_eq!(
            module
                .original_path()
                .expect("retain the module's lexical link")
                .canonical()
                .as_path(),
            expected_original
        );
    }
}

#[test]
fn optional_directory_targets_classify_the_final_path_before_realpath() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/local/package.json",
            br#"{"name":"local","version":"1.0.0","main":"../node_modules/pkg/value.js"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/pkg/value.js",
            b"exports.value = true;".to_vec(),
        )
        .file("/store/pkg/value.js", b"exports.value = true;".to_vec())
        .realpath("/work/node_modules/pkg/value.js", "/store/pkg/value.js")
        .file(
            "/work/node_modules/escape/package.json",
            br#"{"name":"escape","version":"1.0.0","main":"../../outside/value.js"}"#.to_vec(),
        )
        .file(
            "/work/outside/value.js",
            b"exports.outside = true;".to_vec(),
        )
        .file(
            "/store/outside/value.js",
            b"exports.outside = true;".to_vec(),
        )
        .realpath("/work/outside/value.js", "/store/outside/value.js")
        .build()
        .expect("build optional final-path classification host");
    let options = CompilerOptions {
        module_resolution: Some(2),
        ..CompilerOptions::default()
    };
    let program_options = ProgramOptions::default().with_paths(vec![
        PathMapping::new("into-node-modules", vec!["local".to_owned()]),
        PathMapping::new(
            "out-of-node-modules",
            vec!["node_modules/escape".to_owned()],
        ),
    ]);
    let mut resolver = ModuleResolver::new_with_program_options(&host, &options, &program_options)
        .expect("create optional final-path resolver");

    let external = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "into-node-modules",
                ResolutionMode::CommonJs,
            )
            .expect("a local optional package target may become external"),
    );
    assert_eq!(
        external.resolved_file().display(),
        Path::new("/store/pkg/value.js")
    );
    assert_eq!(
        external.original_path().map(ProgramPath::display),
        Some(Path::new("/work/node_modules/pkg/value.js"))
    );
    assert!(external.is_external_library_import());

    let local = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "out-of-node-modules",
                ResolutionMode::CommonJs,
            )
            .expect("an external optional package target may become local"),
    );
    assert_eq!(
        local.resolved_file().display(),
        Path::new("/work/outside/value.js")
    );
    assert_eq!(local.original_path(), None);
    assert!(!local.is_external_library_import());
}

#[test]
fn arbitrary_extension_twins_resolve_in_legacy_and_node_esm_modes() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/src/theme.d.css.ts",
            b"declare const theme: string; export default theme;".to_vec(),
        )
        .build()
        .expect("build arbitrary declaration-twin host");
    let program_options = ProgramOptions::default().with_paths(vec![PathMapping::new(
        "theme",
        vec!["src/theme.css".to_owned()],
    )]);

    for resolution_kind in [1, 2, 3, 99, 100] {
        let options = CompilerOptions {
            module_resolution: Some(resolution_kind),
            ..CompilerOptions::default()
        };
        let mut resolver =
            ModuleResolver::new_with_program_options(&host, &options, &program_options)
                .expect("create arbitrary-extension resolver");
        let mode = if matches!(resolution_kind, 3 | 99) {
            ResolutionMode::EsNext
        } else {
            ResolutionMode::CommonJs
        };
        let module = resolved(
            resolver
                .resolve(Path::new("/work/main.ts"), "theme", mode)
                .expect("resolve arbitrary declaration twin"),
        );
        assert_eq!(
            module.resolved_file().canonical().as_path(),
            Path::new("/work/src/theme.d.css.ts"),
            "moduleResolution={resolution_kind}"
        );
        assert_eq!(
            module.extension(),
            &ModuleExtension::Arbitrary(".d.css.ts".to_owned())
        );
    }
}

#[test]
fn empty_captures_keep_the_literal_star_and_paths_precede_modern_package_maps() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/package.json",
            br##"{
                "name":"app",
                "imports":{"#mapped":"./imports.ts","#fallback":"./fallback.ts"},
                "exports":{"./self":"./self.ts"}
            }"##
            .to_vec(),
        )
        .file("/work/literal/*.ts", b"export {};".to_vec())
        .file("/work/literal/bar.ts", b"export {};".to_vec())
        .file("/work/paths/imports.ts", b"export {};".to_vec())
        .file("/work/paths/self.ts", b"export {};".to_vec())
        .file("/work/imports.ts", b"export {};".to_vec())
        .file("/work/fallback.ts", b"export {};".to_vec())
        .file("/work/self.ts", b"export {};".to_vec())
        .build()
        .expect("build paths/package-map precedence host");
    let options = CompilerOptions {
        module: Some(199),
        ..CompilerOptions::default()
    };
    let program_options = ProgramOptions::default().with_paths(vec![
        PathMapping::new("foo*", vec!["literal/*.ts".to_owned()]),
        PathMapping::new("#mapped", vec!["paths/imports.ts".to_owned()]),
        PathMapping::new("#fallback", vec!["missing/imports".to_owned()]),
        PathMapping::new("app/self", vec!["paths/self.ts".to_owned()]),
    ]);
    let mut resolver = ModuleResolver::new_with_program_options(&host, &options, &program_options)
        .expect("create paths/package-map resolver");

    for (specifier, expected) in [
        ("foo", "/work/literal/*.ts"),
        ("foobar", "/work/literal/bar.ts"),
        ("#mapped", "/work/paths/imports.ts"),
        ("#fallback", "/work/fallback.ts"),
        ("app/self", "/work/paths/self.ts"),
    ] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.ts"),
                    specifier,
                    ResolutionMode::EsNext,
                )
                .expect("resolve paths/package-map precedence candidate"),
        );
        assert_eq!(
            module.resolved_file().canonical().as_path(),
            Path::new(expected),
            "{specifier}"
        );
    }
}

#[test]
fn classic_resolution_is_bounded_to_legacy_files_and_at_types() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/src/app.ts", b"export {};".to_vec())
        .file("/work/src/other.ts", b"export const x = 1;".to_vec())
        .file("/work/src/legacy.ts", b"export const x = 1;".to_vec())
        .file(
            "/work/node_modules/direct/index.d.ts",
            b"export const x: 1;".to_vec(),
        )
        .file(
            "/work/node_modules/@types/traditional/package.json",
            br#"{"name":"@types/traditional","version":"1.0.0","types":"index.d.ts"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/@types/traditional/index.d.ts",
            b"export const x: 1;".to_vec(),
        )
        .file(
            "/work/node_modules/@types/foo/package.json",
            br#"{
                "name":"@types/foo",
                "version":"1.0.0",
                "exports":{
                    ".":{
                        "import":"./index.d.mts",
                        "require":"./index.d.cts"
                    }
                }
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/@types/foo/index.d.mts",
            b"export const x: \"module\";".to_vec(),
        )
        .file(
            "/work/node_modules/@types/foo/index.d.cts",
            b"export const x: \"script\";".to_vec(),
        )
        .build()
        .expect("build Classic resolver host");
    let options = CompilerOptions {
        module: Some(99),
        module_resolution: Some(1),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &options).expect("create Classic resolver");

    for (specifier, expected) in [
        ("./other", "/work/src/other.ts"),
        ("legacy", "/work/src/legacy.ts"),
        (
            "traditional",
            "/work/node_modules/@types/traditional/index.d.ts",
        ),
    ] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/src/app.ts"),
                    specifier,
                    ResolutionMode::EsNext,
                )
                .expect("resolve a Classic legacy target"),
        );
        assert_eq!(
            module.resolved_file().canonical().as_path(),
            Path::new(expected)
        );
    }

    assert_eq!(
        resolver
            .resolve(
                Path::new("/work/src/app.ts"),
                "direct",
                ResolutionMode::EsNext,
            )
            .expect("ordinary node_modules packages are outside Classic"),
        ResolutionOutcome::NotFound
    );
    for mode in [
        ResolutionMode::Unspecified,
        ResolutionMode::EsNext,
        ResolutionMode::CommonJs,
    ] {
        let facts = resolver
            .resolve_with_facts(Path::new("/work/src/app.ts"), "foo", mode)
            .expect("Classic exports-only @types package is an authoritative miss");
        assert_eq!(facts.outcome(), &ResolutionOutcome::NotFound);
        assert_eq!(facts.alternate_result(), None);
    }
}

#[test]
fn node10_primary_miss_retains_the_bundler_declaration_alternate() {
    let host = MemoryCompilerHost::builder("/")
        .file("/index.ts", b"import { pkg } from 'pkg';".to_vec())
        .file(
            "/node_modules/pkg/package.json",
            br#"{
                "name":"pkg",
                "version":"1.0.0",
                "exports":{".":"./definitely-not-index.js"}
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/pkg/definitely-not-index.d.ts",
            b"export {};".to_vec(),
        )
        .build()
        .expect("build Node10 alternate host");
    let options = CompilerOptions {
        module_resolution: Some(2),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &options).expect("create Node10 resolver");

    let facts = resolver
        .resolve_with_facts(Path::new("/index.ts"), "pkg", ResolutionMode::Unspecified)
        .expect("resolve Node10 primary and diagnostic alternate");
    assert_eq!(facts.outcome(), &ResolutionOutcome::NotFound);
    assert_eq!(
        facts
            .alternate_result()
            .expect("Bundler preferred retry finds the declaration twin")
            .canonical()
            .as_path(),
        Path::new("/node_modules/pkg/definitely-not-index.d.ts")
    );

    assert_eq!(
        resolver
            .resolve(Path::new("/index.ts"), "pkg", ResolutionMode::Unspecified)
            .expect("legacy wrapper keeps the primary outcome"),
        ResolutionOutcome::NotFound
    );
}

#[test]
fn node10_legacy_primary_and_bundler_retry_keep_their_exact_boundaries() {
    let host = MemoryCompilerHost::builder("/")
        .file("/index.ts", b"export {};".to_vec())
        .file(
            "/node_modules/typed/package.json",
            br#"{
                "name":"typed",
                "version":"1.0.0",
                "types":"./legacy.d.ts",
                "exports":{".":"./modern.js"}
            }"#
            .to_vec(),
        )
        .file("/node_modules/typed/legacy.d.ts", b"export {};".to_vec())
        .file("/node_modules/typed/modern.d.ts", b"export {};".to_vec())
        .file(
            "/node_modules/untyped/package.json",
            br#"{
                "name":"untyped",
                "version":"1.0.0",
                "main":"./legacy.js",
                "exports":{".":"./modern.js"}
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/untyped/legacy.js",
            b"module.exports = {};".to_vec(),
        )
        .file("/node_modules/untyped/modern.d.ts", b"export {};".to_vec())
        .file(
            "/node_modules/js-only/package.json",
            br#"{
                "name":"js-only",
                "version":"1.0.0",
                "exports":{".":"./modern.js"}
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/js-only/modern.js",
            b"module.exports = {};".to_vec(),
        )
        .file(
            "/node_modules/conditions/package.json",
            br#"{
                "name":"conditions",
                "version":"1.0.0",
                "exports":{
                    ".":{
                        "node":"./node.js",
                        "import":"./import.js",
                        "require":"./require.js"
                    }
                }
            }"#
            .to_vec(),
        )
        .file("/node_modules/conditions/node.d.ts", b"export {};".to_vec())
        .file(
            "/node_modules/conditions/import.d.ts",
            b"export {};".to_vec(),
        )
        .file(
            "/node_modules/conditions/require.d.ts",
            b"export {};".to_vec(),
        )
        .file(
            "/node_modules/manifestless/placeholder.txt",
            b"package directory without a manifest".to_vec(),
        )
        .file(
            "/node_modules/@types/manifestless/package.json",
            br#"{
                "name":"@types/manifestless",
                "version":"1.0.0",
                "exports":{".":"./modern.js"}
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/@types/manifestless/modern.d.ts",
            b"export {};".to_vec(),
        )
        .file(
            "/node_modules/no-manifests/placeholder.txt",
            b"package directory without a manifest".to_vec(),
        )
        .file(
            "/node_modules/@types/no-manifests/placeholder.txt",
            b"types package directory without a manifest".to_vec(),
        )
        .build()
        .expect("build bounded Node10 host");
    let options = CompilerOptions {
        module_resolution: Some(2),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &options).expect("create Node10 resolver");

    let typed = resolver
        .resolve_with_facts(Path::new("/index.ts"), "typed", ResolutionMode::Unspecified)
        .expect("Node10 legacy types field wins");
    let ResolutionOutcome::Resolved(typed_primary) = typed.outcome() else {
        panic!("expected typed legacy primary: {typed:#?}");
    };
    assert_eq!(
        typed_primary.resolved_file().canonical().as_path(),
        Path::new("/node_modules/typed/legacy.d.ts")
    );
    assert_eq!(typed.alternate_result(), None);

    let untyped = resolver
        .resolve_with_facts(
            Path::new("/index.ts"),
            "untyped",
            ResolutionMode::Unspecified,
        )
        .expect("Node10 JavaScript primary retains a declaration alternate");
    let ResolutionOutcome::Resolved(untyped_primary) = untyped.outcome() else {
        panic!("expected untyped legacy primary: {untyped:#?}");
    };
    assert_eq!(untyped_primary.extension(), &ModuleExtension::Js);
    assert_eq!(
        untyped
            .alternate_result()
            .expect("Bundler retry finds modern types")
            .canonical()
            .as_path(),
        Path::new("/node_modules/untyped/modern.d.ts")
    );

    let js_only = resolver
        .resolve_with_facts(
            Path::new("/index.ts"),
            "js-only",
            ResolutionMode::Unspecified,
        )
        .expect("preferred-only retry does not accept JavaScript");
    assert_eq!(js_only.outcome(), &ResolutionOutcome::NotFound);
    assert_eq!(js_only.alternate_result(), None);

    let conditions = resolver
        .resolve_with_facts(
            Path::new("/index.ts"),
            "conditions",
            ResolutionMode::Unspecified,
        )
        .expect("Bundler retry uses Bundler default conditions");
    assert_eq!(conditions.outcome(), &ResolutionOutcome::NotFound);
    assert_eq!(
        conditions
            .alternate_result()
            .expect("Bundler defaults select import and exclude node")
            .canonical()
            .as_path(),
        Path::new("/node_modules/conditions/import.d.ts")
    );

    let manifestless = resolver
        .resolve_with_facts(
            Path::new("/index.ts"),
            "manifestless",
            ResolutionMode::Unspecified,
        )
        .expect("an observed @types package manifest enables Bundler retry");
    assert_eq!(manifestless.outcome(), &ResolutionOutcome::NotFound);
    assert_eq!(
        manifestless
            .alternate_result()
            .expect("Bundler retry honors the observed @types exports")
            .canonical()
            .as_path(),
        Path::new("/node_modules/@types/manifestless/modern.d.ts")
    );

    let no_manifests = resolver
        .resolve_with_facts(
            Path::new("/index.ts"),
            "no-manifests",
            ResolutionMode::Unspecified,
        )
        .expect("manifestless package directories do not enable Bundler retry");
    assert_eq!(no_manifests.outcome(), &ResolutionOutcome::NotFound);
    assert_eq!(no_manifests.alternate_result(), None);
}

#[test]
fn node10_explicit_modes_enable_all_features_and_select_conditions() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/pkg/package.json",
            br#"{
                "name":"pkg","version":"1.0.0","types":"./legacy.d.ts",
                "exports":{".":{
                    "import":"./import.js","require":"./require.js"
                }}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/pkg/legacy.d.ts",
            b"export const selected: 'legacy';".to_vec(),
        )
        .file(
            "/work/node_modules/pkg/import.d.ts",
            b"export const selected: 'import';".to_vec(),
        )
        .file(
            "/work/node_modules/pkg/require.d.ts",
            b"export const selected: 'require';".to_vec(),
        )
        .file(
            "/work/node_modules/diagnostic/package.json",
            br#"{
                "name":"diagnostic","version":"1.0.0",
                "exports":{".":{
                    "import":"./import.js","require":"./require.js"
                }}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/diagnostic/require.js",
            b"exports.selected = 'require';".to_vec(),
        )
        .file(
            "/work/node_modules/diagnostic/import.d.ts",
            b"export const selected: 'import';".to_vec(),
        )
        .build()
        .expect("build Node10 explicit-mode host");
    let options = CompilerOptions {
        module_resolution: Some(2),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &options).expect("create Node10 resolver");

    for (mode, expected) in [
        (
            ResolutionMode::Unspecified,
            "/work/node_modules/pkg/legacy.d.ts",
        ),
        (
            ResolutionMode::CommonJs,
            "/work/node_modules/pkg/require.d.ts",
        ),
        (ResolutionMode::EsNext, "/work/node_modules/pkg/import.d.ts"),
    ] {
        let module = resolved(
            resolver
                .resolve(Path::new("/work/main.ts"), "pkg", mode)
                .expect("resolve a Node10 mode-aware package"),
        );
        assert_eq!(
            module.resolved_file().display(),
            Path::new(expected),
            "{mode:?}"
        );
    }

    let exports_disabled = CompilerOptions {
        module_resolution: Some(2),
        resolve_package_json_exports: Some(false),
        ..CompilerOptions::default()
    };
    let mut resolver =
        ModuleResolver::new(&host, &exports_disabled).expect("create explicit Node10 resolver");
    let facts = resolver
        .resolve_with_facts(
            Path::new("/work/main.ts"),
            "diagnostic",
            ResolutionMode::CommonJs,
        )
        .expect("explicit Node10 and its Bundler retry force package exports");
    let ResolutionOutcome::Resolved(primary) = facts.outcome() else {
        panic!("expected the CommonJS exports implementation: {facts:#?}");
    };
    assert_eq!(
        primary.resolved_file().display(),
        Path::new("/work/node_modules/diagnostic/require.js")
    );
    assert_eq!(
        facts.alternate_result().map(ProgramPath::display),
        Some(Path::new("/work/node_modules/diagnostic/import.d.ts"))
    );
}

#[test]
fn node16_and_nodenext_keep_fixed_package_map_features_but_bundler_applies_overrides() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.mts", b"export {};".to_vec())
        .file(
            "/work/node_modules/profile/package.json",
            br#"{
                "name":"profile","version":"1.0.0",
                "types":"./legacy.d.ts","exports":"./modern.js"
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/profile/legacy.d.ts",
            b"export const selected: 'legacy';".to_vec(),
        )
        .file(
            "/work/node_modules/profile/modern.d.ts",
            b"export const selected: 'modern';".to_vec(),
        )
        .build()
        .expect("build package-map feature profile host");

    for (module_resolution, expected) in [
        (3, "/work/node_modules/profile/modern.d.ts"),
        (99, "/work/node_modules/profile/modern.d.ts"),
        (100, "/work/node_modules/profile/legacy.d.ts"),
    ] {
        let options = CompilerOptions {
            module_resolution: Some(module_resolution),
            resolve_package_json_exports: Some(false),
            ..CompilerOptions::default()
        };
        let mut resolver = ModuleResolver::new(&host, &options).expect("create profile resolver");
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.mts"),
                    "profile",
                    ResolutionMode::EsNext,
                )
                .expect("resolve with the profile's package-map features"),
        );
        assert_eq!(
            module.resolved_file().display(),
            Path::new(expected),
            "moduleResolution={module_resolution}"
        );
    }
}

#[test]
fn classic_and_node10_type_references_share_the_node_style_primary_secondary_spine() {
    let linked = b"declare const linked: true;".to_vec();
    let host = MemoryCompilerHost::builder("/work/project")
        .file("/work/project/src/main.ts", b"export {};".to_vec())
        .file(
            "/work/custom/direct.d.ts",
            b"declare const direct: true;".to_vec(),
        )
        .file(
            "/work/custom/pkg/package.json",
            br#"{"name":"custom-pkg","version":"1.2.3","types":"legacy.d.ts"}"#.to_vec(),
        )
        .file(
            "/work/custom/pkg/legacy.d.ts",
            b"declare const customPackage: true;".to_vec(),
        )
        .file("/work/custom/linked.d.ts", linked.clone())
        .file("/physical/linked.d.ts", linked)
        .realpath("/work/custom/linked.d.ts", "/physical/linked.d.ts")
        .file(
            "/work/project/node_modules/@types/defaulted/package.json",
            br#"{"name":"@types/defaulted","version":"2.0.0","types":"index.d.ts"}"#.to_vec(),
        )
        .file(
            "/work/project/node_modules/@types/defaulted/index.d.ts",
            b"declare const defaulted: true;".to_vec(),
        )
        .file(
            "/work/project/src/node_modules/secondary/package.json",
            br#"{"name":"secondary","version":"3.0.0","types":"index.d.ts"}"#.to_vec(),
        )
        .file(
            "/work/project/src/node_modules/secondary/index.d.ts",
            b"declare const secondary: true;".to_vec(),
        )
        .file(
            "/work/project/src/relative.d.ts",
            b"declare const relative: true;".to_vec(),
        )
        .build()
        .expect("build legacy type-reference host");
    let custom_root = ProgramPath::from_trusted_parts("/work/custom", "/work/custom")
        .expect("create custom type root");
    let no_primary_roots: Vec<ProgramPath> = Vec::new();

    for module_resolution in [1, 2] {
        let options = CompilerOptions {
            module_resolution: Some(module_resolution),
            ..CompilerOptions::default()
        };
        let mut resolver =
            ModuleResolver::new(&host, &options).expect("create legacy type resolver");

        let ResolutionOutcome::Resolved(direct) = resolver
            .resolve_type_reference(
                Path::new("/work/project/src/main.ts"),
                "direct",
                ResolutionMode::Unspecified,
                Some(std::slice::from_ref(&custom_root)),
            )
            .expect("resolve a direct custom-root declaration")
        else {
            panic!("expected direct custom-root type reference");
        };
        assert_eq!(
            direct.resolved_file().canonical().as_path(),
            Path::new("/work/custom/direct.d.ts")
        );
        assert!(direct.primary());
        assert!(!direct.is_external_library_import());

        let ResolutionOutcome::Resolved(package) = resolver
            .resolve_type_reference(
                Path::new("/work/project/src/main.ts"),
                "pkg",
                ResolutionMode::Unspecified,
                Some(std::slice::from_ref(&custom_root)),
            )
            .expect("resolve a custom-root package")
        else {
            panic!("expected custom-root package type reference");
        };
        assert_eq!(
            package.resolved_file().canonical().as_path(),
            Path::new("/work/custom/pkg/legacy.d.ts")
        );
        assert_eq!(
            package.package_id().map(PackageId::name),
            Some("custom-pkg")
        );
        assert!(package.primary());

        let ResolutionOutcome::Resolved(linked) = resolver
            .resolve_type_reference(
                Path::new("/work/project/src/main.ts"),
                "linked",
                ResolutionMode::Unspecified,
                Some(std::slice::from_ref(&custom_root)),
            )
            .expect("resolve a custom-root realpath transition")
        else {
            panic!("expected linked custom-root type reference");
        };
        assert_eq!(
            linked.resolved_file().canonical().as_path(),
            Path::new("/physical/linked.d.ts")
        );
        assert_eq!(
            linked
                .original_path()
                .expect("retain the lexical custom-root path")
                .canonical()
                .as_path(),
            Path::new("/work/custom/linked.d.ts")
        );
        assert!(linked.primary());

        let ResolutionOutcome::Resolved(defaulted) = resolver
            .resolve_type_reference(
                Path::new("/work/project/src/main.ts"),
                "defaulted",
                ResolutionMode::Unspecified,
                None,
            )
            .expect("resolve a default @types primary")
        else {
            panic!("expected default-root type reference");
        };
        assert_eq!(
            defaulted.resolved_file().canonical().as_path(),
            Path::new("/work/project/node_modules/@types/defaulted/index.d.ts")
        );
        assert!(defaulted.primary());
        assert!(defaulted.is_external_library_import());
        assert_eq!(
            defaulted.package_id().map(PackageId::name),
            Some("@types/defaulted")
        );

        let ResolutionOutcome::Resolved(secondary) = resolver
            .resolve_type_reference(
                Path::new("/work/project/src/main.ts"),
                "secondary",
                ResolutionMode::Unspecified,
                Some(&no_primary_roots),
            )
            .expect("resolve a nearest node_modules secondary")
        else {
            panic!("expected secondary type reference");
        };
        assert_eq!(
            secondary.resolved_file().canonical().as_path(),
            Path::new("/work/project/src/node_modules/secondary/index.d.ts")
        );
        assert!(!secondary.primary());
        assert!(secondary.is_external_library_import());

        let ResolutionOutcome::Resolved(relative) = resolver
            .resolve_type_reference(
                Path::new("/work/project/src/main.ts"),
                "./relative",
                ResolutionMode::Unspecified,
                Some(&no_primary_roots),
            )
            .expect("resolve a relative secondary declaration")
        else {
            panic!("expected relative type reference");
        };
        assert_eq!(
            relative.resolved_file().canonical().as_path(),
            Path::new("/work/project/src/relative.d.ts")
        );
        assert!(!relative.primary());
        assert!(!relative.is_external_library_import());
    }
}

#[test]
fn legacy_type_reference_modes_enable_exports_only_for_secondary_lookup() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/types/conditional/package.json",
            br#"{
                "name":"conditional-types",
                "version":"1.0.0",
                "types":"legacy.d.ts",
                "exports":{
                    ".":{
                        "import":"./import.d.mts",
                        "require":"./require.d.cts"
                    }
                }
            }"#
            .to_vec(),
        )
        .file(
            "/work/types/conditional/legacy.d.ts",
            b"declare const selected: 'legacy';".to_vec(),
        )
        .file(
            "/work/types/conditional/import.d.mts",
            b"export declare const selected: 'import';".to_vec(),
        )
        .file(
            "/work/types/conditional/require.d.cts",
            b"export declare const selected: 'require';".to_vec(),
        )
        .file(
            "/work/node_modules/secondary-conditional/package.json",
            br#"{
                "name":"secondary-conditional-types",
                "version":"1.0.0",
                "types":"legacy.d.ts",
                "exports":{
                    ".":{
                        "import":"./import.d.mts",
                        "require":"./require.d.cts"
                    }
                }
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/secondary-conditional/legacy.d.ts",
            b"declare const selected: 'secondary-legacy';".to_vec(),
        )
        .file(
            "/work/node_modules/secondary-conditional/import.d.mts",
            b"export declare const selected: 'secondary-import';".to_vec(),
        )
        .file(
            "/work/node_modules/secondary-conditional/require.d.cts",
            b"export declare const selected: 'secondary-require';".to_vec(),
        )
        .build()
        .expect("build legacy conditional type-reference host");
    let type_root = ProgramPath::from_trusted_parts("/work/types", "/work/types")
        .expect("create custom type root");

    for module_resolution in [1, 2] {
        let options = CompilerOptions {
            module_resolution: Some(module_resolution),
            ..CompilerOptions::default()
        };
        let mut resolver =
            ModuleResolver::new(&host, &options).expect("create legacy type resolver");
        for mode in [
            ResolutionMode::Unspecified,
            ResolutionMode::CommonJs,
            ResolutionMode::EsNext,
        ] {
            let ResolutionOutcome::Resolved(reference) = resolver
                .resolve_type_reference(
                    Path::new("/work/main.ts"),
                    "conditional",
                    mode,
                    Some(std::slice::from_ref(&type_root)),
                )
                .expect("resolve a legacy conditional type reference")
            else {
                panic!("expected legacy conditional type reference");
            };
            assert_eq!(
                reference.resolved_file().canonical().as_path(),
                Path::new("/work/types/conditional/legacy.d.ts")
            );
            assert_eq!(reference.extension(), &ModuleExtension::Dts);
            assert_eq!(
                reference.package_id().map(PackageId::name),
                Some("conditional-types")
            );
            assert!(reference.primary());
        }

        for (mode, expected, extension) in [
            (
                ResolutionMode::Unspecified,
                "/work/node_modules/secondary-conditional/legacy.d.ts",
                ModuleExtension::Dts,
            ),
            (
                ResolutionMode::CommonJs,
                "/work/node_modules/secondary-conditional/require.d.cts",
                ModuleExtension::Dcts,
            ),
            (
                ResolutionMode::EsNext,
                "/work/node_modules/secondary-conditional/import.d.mts",
                ModuleExtension::Dmts,
            ),
        ] {
            let ResolutionOutcome::Resolved(reference) = resolver
                .resolve_type_reference(
                    Path::new("/work/main.ts"),
                    "secondary-conditional",
                    mode,
                    Some(&[]),
                )
                .expect("resolve a secondary legacy conditional type reference")
            else {
                panic!("expected secondary legacy conditional type reference");
            };
            assert_eq!(
                reference.resolved_file().canonical().as_path(),
                Path::new(expected)
            );
            assert_eq!(reference.extension(), &extension);
            assert_eq!(
                reference.package_id().map(PackageId::name),
                Some("secondary-conditional-types")
            );
            assert!(!reference.primary());
        }
    }
}

#[test]
fn node10_unspecified_type_reference_exports_use_an_empty_condition_set() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/conditions/package.json",
            br#"{
                "name":"conditions",
                "version":"1.0.0",
                "types":"legacy.d.ts",
                "exports":{
                    ".":{
                        "types":"./types.d.ts",
                        "require":"./require.d.cts",
                        "custom":"./custom.d.ts",
                        "default":"./default.d.ts"
                    }
                }
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/conditions/legacy.d.ts",
            b"declare const selected: 'legacy';".to_vec(),
        )
        .file(
            "/work/node_modules/conditions/types.d.ts",
            b"declare const selected: 'types';".to_vec(),
        )
        .file(
            "/work/node_modules/conditions/require.d.cts",
            b"export declare const selected: 'require';".to_vec(),
        )
        .file(
            "/work/node_modules/conditions/custom.d.ts",
            b"declare const selected: 'custom';".to_vec(),
        )
        .file(
            "/work/node_modules/conditions/default.d.ts",
            b"declare const selected: 'default';".to_vec(),
        )
        .build()
        .expect("build legacy condition-set host");
    let no_primary_roots: Vec<ProgramPath> = Vec::new();

    for (module_resolution, expected) in [
        (1, "/work/node_modules/conditions/types.d.ts"),
        (2, "/work/node_modules/conditions/default.d.ts"),
    ] {
        let options = CompilerOptions {
            module_resolution: Some(module_resolution),
            resolve_package_json_exports: Some(true),
            custom_conditions: Some(vec!["custom".to_owned()]),
            ..CompilerOptions::default()
        };
        let mut resolver =
            ModuleResolver::new(&host, &options).expect("create legacy type resolver");
        let ResolutionOutcome::Resolved(reference) = resolver
            .resolve_type_reference(
                Path::new("/work/main.ts"),
                "conditions",
                ResolutionMode::Unspecified,
                Some(&no_primary_roots),
            )
            .expect("resolve an unspecified legacy conditional type reference")
        else {
            panic!("expected legacy conditional type reference");
        };
        assert_eq!(
            reference.resolved_file().canonical().as_path(),
            Path::new(expected)
        );
        assert!(!reference.primary());
    }
}

#[test]
fn type_reference_exports_pattern_trailers_follow_node_feature_profiles() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/trailer/package.json",
            br#"{
                "name":"trailer",
                "version":"1.0.0",
                "exports":{"./*.js":"./types/*.d.ts"}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/trailer/types/entry.d.ts",
            b"export declare const trailer: true;".to_vec(),
        )
        .file(
            "/work/node_modules/terminal/package.json",
            br#"{
                "name":"terminal",
                "version":"1.0.0",
                "exports":{"./*":"./types/*.d.ts"}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/terminal/types/entry.d.ts",
            b"export declare const terminal: true;".to_vec(),
        )
        .file(
            "/work/node_modules/literal/package.json",
            br#"{
                "name":"literal",
                "version":"1.0.0",
                "exports":{"./x*y":"./literal/","./x*":"./broad/*"}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/literal/literal/file.d.ts",
            b"export declare const literal: true;".to_vec(),
        )
        .file(
            "/work/node_modules/literal/broad/y/file.d.ts",
            b"export declare const broad: true;".to_vec(),
        )
        .build()
        .expect("build exports pattern feature host");
    let no_primary_roots: Vec<ProgramPath> = Vec::new();

    for resolution_kind in [1, 2, 3, 99, 100] {
        let options = CompilerOptions {
            module_resolution: Some(resolution_kind),
            resolve_package_json_exports: Some(true),
            ..CompilerOptions::default()
        };
        let mut resolver =
            ModuleResolver::new(&host, &options).expect("create pattern feature resolver");

        for mode in [
            ResolutionMode::Unspecified,
            ResolutionMode::CommonJs,
            ResolutionMode::EsNext,
        ] {
            let trailer = resolver
                .resolve_type_reference(
                    Path::new("/work/main.ts"),
                    "trailer/entry.js",
                    mode,
                    Some(&no_primary_roots),
                )
                .expect("resolve a trailer-pattern type reference");
            let trailers_enabled =
                mode != ResolutionMode::Unspecified || matches!(resolution_kind, 3 | 99 | 100);
            if trailers_enabled {
                let ResolutionOutcome::Resolved(reference) = trailer else {
                    panic!(
                        "expected trailer pattern for moduleResolution={resolution_kind}, mode={mode:?}"
                    );
                };
                assert_eq!(
                    reference.resolved_file().canonical().as_path(),
                    Path::new("/work/node_modules/trailer/types/entry.d.ts"),
                    "moduleResolution={resolution_kind}, mode={mode:?}"
                );
            } else {
                assert_eq!(
                    trailer,
                    ResolutionOutcome::NotFound,
                    "moduleResolution={resolution_kind}, mode={mode:?}"
                );
            }

            let ResolutionOutcome::Resolved(terminal) = resolver
                .resolve_type_reference(
                    Path::new("/work/main.ts"),
                    "terminal/entry",
                    mode,
                    Some(&no_primary_roots),
                )
                .expect("resolve a terminal-star type reference")
            else {
                panic!(
                    "terminal-star pattern should not depend on trailer features: moduleResolution={resolution_kind}, mode={mode:?}"
                );
            };
            assert_eq!(
                terminal.resolved_file().canonical().as_path(),
                Path::new("/work/node_modules/terminal/types/entry.d.ts"),
                "moduleResolution={resolution_kind}, mode={mode:?}"
            );

            let ResolutionOutcome::Resolved(literal) = resolver
                .resolve_type_reference(
                    Path::new("/work/main.ts"),
                    "literal/x*y/file.d.ts",
                    mode,
                    Some(&no_primary_roots),
                )
                .expect("resolve a literal star-key prefix")
            else {
                panic!(
                    "literal star-key prefix should not depend on trailer features: moduleResolution={resolution_kind}, mode={mode:?}"
                );
            };
            assert_eq!(
                literal.resolved_file().canonical().as_path(),
                Path::new("/work/node_modules/literal/literal/file.d.ts"),
                "moduleResolution={resolution_kind}, mode={mode:?}"
            );
        }
    }
}

#[test]
fn legacy_secondary_subpaths_honor_nested_packages_for_ordinary_and_at_types_lookups() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/pkg/package.json",
            br#"{"name":"root-pkg","version":"1.0.0","types":"root.d.ts"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/pkg/sub/package.json",
            br#"{
                "name":"nested-pkg",
                "version":"2.0.0",
                "types":"index.d.ts",
                "typesVersions":{"*":{"index.d.ts":["v6/index"]}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/pkg/sub/index.d.ts",
            b"declare const wrongVersion: true;".to_vec(),
        )
        .file(
            "/work/node_modules/pkg/sub/v6/index.d.ts",
            b"declare const nestedPackage: true;".to_vec(),
        )
        .file(
            "/work/node_modules/@types/only/sub/package.json",
            br#"{"name":"nested-at-types","version":"3.0.0","types":"entry.d.ts"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/@types/only/sub/entry.d.ts",
            b"declare const nestedAtTypes: true;".to_vec(),
        )
        .file(
            "/work/node_modules/direct/sub.d.ts",
            b"declare const directFile: true;".to_vec(),
        )
        .file(
            "/work/node_modules/direct/sub/package.json",
            br#"{"name":"nested-direct","version":"4.0.0","types":"entry.d.ts"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/direct/sub/entry.d.ts",
            b"declare const wrongNestedDirectory: true;".to_vec(),
        )
        .file(
            "/work/node_modules/governed/package.json",
            br#"{
                "name":"governed-root",
                "version":"5.0.0",
                "exports":{"./sub":{"require":"./root-export.d.cts"}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/governed/root-export.d.cts",
            b"export declare const rootExport: true;".to_vec(),
        )
        .file(
            "/work/node_modules/governed/sub/package.json",
            br#"{"name":"wrong-nested-governed","version":"9.0.0","types":"entry.d.ts"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/governed/sub/entry.d.ts",
            b"declare const wrongNestedGoverned: true;".to_vec(),
        )
        .build()
        .expect("build nested secondary type-reference host");
    let no_primary_roots: Vec<ProgramPath> = Vec::new();

    for module_resolution in [1, 2] {
        let options = CompilerOptions {
            module_resolution: Some(module_resolution),
            ..CompilerOptions::default()
        };
        let mut resolver =
            ModuleResolver::new(&host, &options).expect("create legacy type resolver");

        for (specifier, expected, package_name) in [
            (
                "pkg/sub",
                "/work/node_modules/pkg/sub/v6/index.d.ts",
                Some("nested-pkg"),
            ),
            (
                "only/sub",
                "/work/node_modules/@types/only/sub/entry.d.ts",
                Some("nested-at-types"),
            ),
            ("direct/sub", "/work/node_modules/direct/sub.d.ts", None),
        ] {
            let ResolutionOutcome::Resolved(reference) = resolver
                .resolve_type_reference(
                    Path::new("/work/main.ts"),
                    specifier,
                    ResolutionMode::Unspecified,
                    Some(&no_primary_roots),
                )
                .expect("resolve a legacy secondary subpath")
            else {
                panic!("expected secondary subpath type reference for {specifier}");
            };
            assert_eq!(
                reference.resolved_file().canonical().as_path(),
                Path::new(expected)
            );
            assert_eq!(reference.package_id().map(PackageId::name), package_name);
            assert!(!reference.primary());
            assert!(reference.is_external_library_import());
        }

        let ResolutionOutcome::Resolved(governed) = resolver
            .resolve_type_reference(
                Path::new("/work/main.ts"),
                "governed/sub",
                ResolutionMode::CommonJs,
                Some(&no_primary_roots),
            )
            .expect("root exports govern after the nested manifest observation")
        else {
            panic!("expected root-governed secondary subpath");
        };
        assert_eq!(
            governed.resolved_file().canonical().as_path(),
            Path::new("/work/node_modules/governed/root-export.d.cts")
        );
        assert_eq!(
            governed.package_id().map(PackageId::name),
            Some("governed-root")
        );
        assert!(!governed.primary());
    }
}

#[test]
fn nested_type_module_extensionless_entries_are_blocked_only_in_node_esm_modes() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/extensionless/sub/package.json",
            br#"{
                "name":"extensionless-nested",
                "version":"1.0.0",
                "type":"module",
                "types":"entry"
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/extensionless/sub/entry.d.ts",
            b"declare const extensionless: true;".to_vec(),
        )
        .file(
            "/work/node_modules/versioned/sub/package.json",
            br#"{
                "name":"extensionless-versioned-nested",
                "version":"2.0.0",
                "type":"module",
                "types":"index.d.ts",
                "typesVersions":{"*":{"index.d.ts":["entry"]}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/versioned/sub/entry.d.ts",
            b"declare const extensionlessVersioned: true;".to_vec(),
        )
        .build()
        .expect("build extensionless nested-package host");
    let no_primary_roots: Vec<ProgramPath> = Vec::new();

    for (module_resolution, mode, should_resolve) in [
        (1, ResolutionMode::EsNext, true),
        (2, ResolutionMode::EsNext, true),
        (3, ResolutionMode::CommonJs, true),
        (99, ResolutionMode::CommonJs, true),
        (3, ResolutionMode::EsNext, false),
        (99, ResolutionMode::EsNext, false),
    ] {
        let options = CompilerOptions {
            module_resolution: Some(module_resolution),
            ..CompilerOptions::default()
        };
        let mut resolver =
            ModuleResolver::new(&host, &options).expect("create type-reference resolver");
        for (specifier, expected, package_name) in [
            (
                "extensionless/sub",
                "/work/node_modules/extensionless/sub/entry.d.ts",
                "extensionless-nested",
            ),
            (
                "versioned/sub",
                "/work/node_modules/versioned/sub/entry.d.ts",
                "extensionless-versioned-nested",
            ),
        ] {
            let outcome = resolver
                .resolve_type_reference(
                    Path::new("/work/main.ts"),
                    specifier,
                    mode,
                    Some(&no_primary_roots),
                )
                .expect("resolve an extensionless nested package entry");

            if should_resolve {
                let ResolutionOutcome::Resolved(reference) = outcome else {
                    panic!(
                        "expected {specifier} with moduleResolution {module_resolution} in {mode:?} mode to resolve"
                    );
                };
                assert_eq!(
                    reference.resolved_file().canonical().as_path(),
                    Path::new(expected)
                );
                assert_eq!(
                    reference.package_id().map(PackageId::name),
                    Some(package_name)
                );
                assert!(!reference.primary());
            } else {
                assert_eq!(outcome, ResolutionOutcome::NotFound);
            }
        }
    }
}

#[test]
fn legacy_secondary_subpath_manifest_failures_precede_root_exports() {
    let denied = HostError::new(
        HostErrorKind::PermissionDenied,
        HostOperation::FileExists,
        Some(PathBuf::from(
            "/work/node_modules/governed/sub/package.json",
        )),
        "nested manifest denied",
    );
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/governed/package.json",
            br#"{
                "name":"governed-root",
                "version":"1.0.0",
                "exports":{"./sub":{"require":"./root-export.d.cts"}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/governed/root-export.d.cts",
            b"export declare const rootExport: true;".to_vec(),
        )
        .file(
            "/work/node_modules/governed/sub/index.d.ts",
            b"declare const nestedDirectoryExists: true;".to_vec(),
        )
        .failure(denied.clone())
        .build()
        .expect("build nested-manifest failure host");
    let options = CompilerOptions {
        module_resolution: Some(2),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &options).expect("create Node10 type resolver");
    let no_primary_roots: Vec<ProgramPath> = Vec::new();
    let error = resolver
        .resolve_type_reference(
            Path::new("/work/main.ts"),
            "governed/sub",
            ResolutionMode::CommonJs,
            Some(&no_primary_roots),
        )
        .expect_err("nested manifest failure must precede root exports");
    assert_eq!(error, ResolutionError::Host(denied));
}

#[test]
fn legacy_direct_type_reference_hits_do_not_read_unrelated_ancestor_manifests() {
    let unrelated_manifest = HostError::new(
        HostErrorKind::PermissionDenied,
        HostOperation::FileExists,
        Some(PathBuf::from("/work/package.json")),
        "an unrelated manifest must not be observed",
    );
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/types/direct.d.ts",
            b"declare const direct: true;".to_vec(),
        )
        .file(
            "/work/relative.d.ts",
            b"declare const relative: true;".to_vec(),
        )
        .failure(unrelated_manifest)
        .build()
        .expect("build direct custom-root precedence host");
    let type_root = ProgramPath::from_trusted_parts("/work/types", "/work/types")
        .expect("create custom type root");

    for module_resolution in [1, 2] {
        let options = CompilerOptions {
            module_resolution: Some(module_resolution),
            ..CompilerOptions::default()
        };
        let mut resolver =
            ModuleResolver::new(&host, &options).expect("create legacy type resolver");
        let ResolutionOutcome::Resolved(reference) = resolver
            .resolve_type_reference(
                Path::new("/work/main.ts"),
                "direct",
                ResolutionMode::Unspecified,
                Some(std::slice::from_ref(&type_root)),
            )
            .expect("resolve without observing an unrelated package manifest")
        else {
            panic!("expected direct custom-root type reference");
        };
        assert_eq!(
            reference.resolved_file().canonical().as_path(),
            Path::new("/work/types/direct.d.ts")
        );
        assert_eq!(reference.package_id(), None);
        assert!(reference.primary());

        let ResolutionOutcome::Resolved(relative) = resolver
            .resolve_type_reference(
                Path::new("/work/main.ts"),
                "./relative",
                ResolutionMode::Unspecified,
                Some(&[]),
            )
            .expect("resolve a relative file without observing an unrelated manifest")
        else {
            panic!("expected relative type reference");
        };
        assert_eq!(
            relative.resolved_file().canonical().as_path(),
            Path::new("/work/relative.d.ts")
        );
        assert_eq!(relative.package_id(), None);
        assert!(!relative.primary());
    }
}

#[test]
fn legacy_external_custom_root_direct_hits_use_the_actual_package_root_before_realpath() {
    let declaration = b"declare const direct: true;".to_vec();
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/pkg/package.json",
            br#"{"name":"actual-package","version":"1.2.3"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/pkg/types/package.json",
            br#"{"name":"wrong-nested-package","version":"9.9.9"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/pkg/types/direct.d.ts",
            declaration.clone(),
        )
        .file("/store/pkg/direct.d.ts", declaration)
        .realpath(
            "/work/node_modules/pkg/types/direct.d.ts",
            "/store/pkg/direct.d.ts",
        )
        .build()
        .expect("build external custom-root direct-hit host");
    let type_root = ProgramPath::from_trusted_parts(
        "/work/node_modules/pkg/types",
        "/work/node_modules/pkg/types",
    )
    .expect("create external custom type root");

    for module_resolution in [1, 2] {
        let options = CompilerOptions {
            module_resolution: Some(module_resolution),
            ..CompilerOptions::default()
        };
        let mut resolver =
            ModuleResolver::new(&host, &options).expect("create legacy type resolver");
        let ResolutionOutcome::Resolved(reference) = resolver
            .resolve_type_reference(
                Path::new("/work/main.ts"),
                "direct",
                ResolutionMode::Unspecified,
                Some(std::slice::from_ref(&type_root)),
            )
            .expect("resolve through the actual node_modules package root")
        else {
            panic!("expected external direct type reference");
        };
        assert_eq!(
            reference.resolved_file().canonical().as_path(),
            Path::new("/store/pkg/direct.d.ts")
        );
        assert_eq!(
            reference
                .original_path()
                .expect("retain the lexical node_modules path")
                .canonical()
                .as_path(),
            Path::new("/work/node_modules/pkg/types/direct.d.ts")
        );
        assert_eq!(
            reference.package_id().map(PackageId::name),
            Some("actual-package")
        );
        assert!(reference.primary());
        assert!(reference.is_external_library_import());
    }
}

#[test]
fn a_more_specific_null_pattern_is_a_terminal_not_found() {
    let (host, _) = fixture_host();
    let options = options_for_module(100);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    for (specifier, mode) in [
        ("inner/cjs/exclude/index", ResolutionMode::CommonJs),
        ("inner/mjs/exclude/index", ResolutionMode::EsNext),
        ("inner/js/exclude/index", ResolutionMode::EsNext),
    ] {
        assert_eq!(
            resolver
                .resolve(Path::new("/index.ts"), specifier, mode)
                .expect("resolve selected null export"),
            ResolutionOutcome::NotFound,
            "specific null export must beat the earlier broad pattern for {specifier}",
        );
    }
}

#[test]
fn declaration_twins_and_external_provenance_hold_for_all_node_module_kinds() {
    let (host, _) = fixture_host();
    let targets = [
        (
            "inner/cjs/index",
            ResolutionMode::CommonJs,
            "/node_modules/inner/index.d.cts",
            ModuleExtension::Dcts,
        ),
        (
            "inner/mjs/index",
            ResolutionMode::EsNext,
            "/node_modules/inner/index.d.mts",
            ModuleExtension::Dmts,
        ),
        (
            "inner/js/index",
            ResolutionMode::EsNext,
            "/node_modules/inner/index.d.ts",
            ModuleExtension::Dts,
        ),
    ];

    for module in [100, 101, 102, 199] {
        let options = options_for_module(module);
        let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

        for (specifier, mode, expected_path, expected_extension) in &targets {
            let external = resolved(
                resolver
                    .resolve(Path::new("/index.ts"), specifier, *mode)
                    .expect("resolve through node_modules"),
            );
            assert_eq!(external.resolved_file().display(), Path::new(expected_path));
            assert_eq!(
                external.resolved_file().canonical().as_path(),
                Path::new(expected_path)
            );
            assert_eq!(external.extension(), expected_extension);
            assert!(external.is_external_library_import());
            assert!(!external.resolved_using_ts_extension());
            assert_eq!(external.original_path(), None);
            assert_eq!(external.package_id(), None);
            let package_metadata = external
                .package_metadata()
                .expect("manifest-backed export retains package metadata");
            assert_eq!(package_metadata.name(), Some("inner"));
            assert_eq!(package_metadata.module_type(), PackageJsonType::Unspecified);
            let caller_spelling = ProgramPath::from_trusted_parts(
                expected_path.trim_start_matches('/'),
                *expected_path,
            )
            .expect("create caller-owned target spelling");
            let rebound = external
                .clone()
                .into_resolved_module(ResolvedModuleTarget::Source {
                    source: SourceFileId::from_raw(0),
                    resolved_file: caller_spelling,
                })
                .expect("canonical target identity binds across display spellings");
            assert_eq!(
                rebound.target().resolved_file().display(),
                Path::new(expected_path.trim_start_matches('/'))
            );

            let self_reference = resolved(
                resolver
                    .resolve(Path::new("/node_modules/inner/test.d.ts"), specifier, *mode)
                    .expect("resolve package self-reference"),
            );
            assert_eq!(
                self_reference.resolved_file().canonical().as_path(),
                Path::new(expected_path)
            );
            assert_eq!(self_reference.extension(), expected_extension);
            assert!(
                !self_reference.is_external_library_import(),
                "a package self-reference is not an external-library traversal"
            );
        }

        let inner_scope = resolver
            .package_scope_for_file(Path::new("/node_modules/inner/test.d.ts"))
            .expect("observe inner package scope")
            .expect("inner package scope exists");
        assert_eq!(inner_scope.module_type(), PackageJsonType::Unspecified);
        assert_eq!(
            inner_scope.package_json().canonical().as_path(),
            Path::new("/node_modules/inner/package.json")
        );
        let root_scope = resolver
            .package_scope_for_file(Path::new("/index.ts"))
            .expect("observe root package scope")
            .expect("root package scope exists");
        assert_eq!(root_scope.module_type(), PackageJsonType::Module);
        assert_eq!(resolver.observed_package_metadata().count(), 2);
    }
}

#[test]
fn conditional_and_array_targets_resolve_while_host_failures_propagate() {
    let (host, denied) = fixture_host();
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    for specifier in ["inner/conditional", "inner/array"] {
        let resolution = resolved(
            resolver
                .resolve(Path::new("/index.ts"), specifier, ResolutionMode::EsNext)
                .expect("resolve selected package-map target"),
        );
        assert_eq!(
            resolution.resolved_file().canonical().as_path(),
            Path::new("/node_modules/inner/index.d.ts")
        );
    }

    let error = resolver
        .resolve(Path::new("/index.ts"), "denied", ResolutionMode::EsNext)
        .expect_err("host read failure must propagate");
    let ResolutionError::Host(actual) = error else {
        panic!("expected host resolution error, got {error:?}");
    };
    assert_eq!(actual, denied);
}

#[test]
fn h02c_exports_targets_and_relative_requests_follow_the_authoritative_map() {
    let host = MemoryCompilerHost::builder("/")
        .file(
            "/package.json",
            br#"{"name":"root","type":"module"}"#.to_vec(),
        )
        .file("/src/index.ts", b"export {};".to_vec())
        .file("/src/other.ts", b"export const other = true;".to_vec())
        .file(
            "/node_modules/source/package.json",
            br#"{"name":"source","version":"1.0.0","exports":"./index.ts"}"#.to_vec(),
        )
        .file(
            "/node_modules/source/index.ts",
            b"export const source = true;".to_vec(),
        )
        .file(
            "/node_modules/conditions/package.json",
            br#"{
                "name":"conditions",
                "version":"1.0.0",
                "exports": {
                    "./yes": {
                        "types@<4":"./wrong.d.ts",
                        "types@>=4":"./right.d.ts"
                    },
                    "./fallback": {
                        "types":"./missing.d.ts",
                        "default":"./right.d.ts"
                    },
                    "./null": {
                        "types":null,
                        "default":"./right.d.ts"
                    },
                    "./no": { "types@<4":"./wrong.d.ts" }
                }
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/conditions/right.d.ts",
            b"export const right: true;".to_vec(),
        )
        .file(
            "/node_modules/directory/package.json",
            br#"{"name":"directory","exports":{"./":"./"}}"#.to_vec(),
        )
        .file(
            "/node_modules/directory/index.d.ts",
            b"export const directory: true;".to_vec(),
        )
        .file(
            "/node_modules/directory/other.d.ts",
            b"export const mustNotResolveImplicitly: true;".to_vec(),
        )
        .file(
            "/node_modules/double/package.json",
            br#"{"name":"double","exports":{"./a/*/b/*":"./index.js"}}"#.to_vec(),
        )
        .file(
            "/node_modules/double/index.d.ts",
            b"export const wrong: true;".to_vec(),
        )
        .file(
            "/node_modules/versioned/package.json",
            br#"{
                "name":"versioned",
                "version":"1.0.0",
                "typesVersions":{"*":{"foo":["./types/foo.d.ts"]}}
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/versioned/types/foo.d.ts",
            b"export const versioned: true;".to_vec(),
        )
        .build()
        .expect("build H0.2c package-map host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    let relative = resolved(
        resolver
            .resolve(
                Path::new("/src/index.ts"),
                "./other.js",
                ResolutionMode::EsNext,
            )
            .expect("resolve relative written-JS request"),
    );
    assert_eq!(
        relative.resolved_file().canonical().as_path(),
        Path::new("/src/other.ts")
    );
    assert!(!relative.is_external_library_import());

    for (specifier, expected) in [
        ("source", "/node_modules/source/index.ts"),
        ("conditions/yes", "/node_modules/conditions/right.d.ts"),
        ("conditions/fallback", "/node_modules/conditions/right.d.ts"),
        ("directory/index.js", "/node_modules/directory/index.d.ts"),
        ("versioned/foo", "/node_modules/versioned/types/foo.d.ts"),
    ] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/src/index.ts"),
                    specifier,
                    ResolutionMode::EsNext,
                )
                .expect("resolve H0.2c package request"),
        );
        assert_eq!(
            module.resolved_file().canonical().as_path(),
            Path::new(expected)
        );
        assert!(module.is_external_library_import());
        if specifier == "source" {
            assert_eq!(module.extension(), &ModuleExtension::Ts);
            assert!(!module.resolved_using_ts_extension());
        }
        if specifier == "versioned/foo" {
            assert_eq!(module.package_id(), None);
        }
    }

    for specifier in [
        "conditions/no",
        "conditions/null",
        "directory/other",
        "double/a/*/b/*",
    ] {
        assert_eq!(
            resolver
                .resolve(
                    Path::new("/src/index.ts"),
                    specifier,
                    ResolutionMode::EsNext
                )
                .expect("unsupported package key is an authoritative miss"),
            ResolutionOutcome::NotFound
        );
    }
}

#[test]
fn package_imports_cover_relative_bare_conditional_array_null_and_cycles() {
    let host = MemoryCompilerHost::builder("/")
        .file(
            "/package.json",
            br##"{
                "name":"root",
                "version":"1.0.0",
                "type":"module",
                "exports":"./index.cjs",
                "imports": {
                    "#exact":"./src/exact.js",
                    "#pattern/*":"./src/*.js",
                    "#condition": {
                        "import":"./src/import.js",
                        "require":"./src/require.cjs"
                    },
                    "#array":["./src/missing.js", "./src/fallback.js"],
                    "#blocked":null,
                    "#external":"dep/subpath",
                    "#empty":"",
                    "#cycle-a":"#cycle-b",
                    "#cycle-b":"#cycle-a",
                    "#self":"root",
                    "#direct.ts":"./src/direct.ts",
                    "#mapped/*":"./src/*"
                }
            }"##
            .to_vec(),
        )
        .file("/index.ts", b"export {};".to_vec())
        .file("/base.ts", b"export const emptyTarget: true;".to_vec())
        // If an imports-to-self rewrite incorrectly re-enters this package's
        // exports map, the written .cjs target substitutes this source.
        .file("/index.cts", b"export const wrongSelf = true;".to_vec())
        .file("/src/exact.ts", b"export const exact = true;".to_vec())
        .file("/src/pattern.ts", b"export const pattern = true;".to_vec())
        .file("/src/import.ts", b"export const esm = true;".to_vec())
        .file("/src/require.cts", b"export const cjs = true;".to_vec())
        .file(
            "/src/fallback.ts",
            b"export const fallback = true;".to_vec(),
        )
        .file("/src/direct.ts", b"export const direct = true;".to_vec())
        .file(
            "/src/from-pattern.ts",
            b"export const mapped = true;".to_vec(),
        )
        .file(
            "/node_modules/dep/package.json",
            br#"{
                "name":"dep",
                "version":"2.0.0",
                "exports":{"./subpath":"./types.d.ts"}
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/dep/types.d.ts",
            b"export const dep: true;".to_vec(),
        )
        .build()
        .expect("build package-imports host");
    let options = CompilerOptions {
        module: Some(199),
        base_url: Some("/base.ts".to_owned()),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    for (specifier, mode, expected) in [
        ("#exact", ResolutionMode::EsNext, "/src/exact.ts"),
        (
            "#pattern/pattern",
            ResolutionMode::EsNext,
            "/src/pattern.ts",
        ),
        ("#condition", ResolutionMode::EsNext, "/src/import.ts"),
        ("#condition", ResolutionMode::CommonJs, "/src/require.cts"),
        ("#array", ResolutionMode::EsNext, "/src/fallback.ts"),
    ] {
        let module = resolved(
            resolver
                .resolve(Path::new("/index.ts"), specifier, mode)
                .expect("resolve package-imports target"),
        );
        assert_eq!(
            module.resolved_file().canonical().as_path(),
            Path::new(expected)
        );
        assert!(!module.is_external_library_import());
        assert_eq!(module.package_id().map(PackageId::name), Some("root"));
    }

    let external = resolved(
        resolver
            .resolve(Path::new("/index.ts"), "#external", ResolutionMode::EsNext)
            .expect("reinsert bare imports target into node_modules lookup"),
    );
    assert_eq!(
        external.resolved_file().canonical().as_path(),
        Path::new("/node_modules/dep/types.d.ts")
    );
    assert_eq!(external.package_id().map(PackageId::name), Some("dep"));
    assert!(
        !external.is_external_library_import(),
        "the outer imports boundary owns an external bare target"
    );

    let empty = resolved(
        resolver
            .resolve(Path::new("/index.ts"), "#empty", ResolutionMode::EsNext)
            .expect("an empty imports target re-enters baseUrl resolution"),
    );
    assert_eq!(empty.resolved_file().display(), Path::new("/base.ts"));

    for specifier in ["#blocked", "#cycle-a", "#cycle-b", "#self"] {
        assert_eq!(
            resolver
                .resolve(Path::new("/index.ts"), specifier, ResolutionMode::EsNext)
                .expect("terminal or bounded imports miss"),
            ResolutionOutcome::NotFound,
            "{specifier} must not escape the package-map boundary"
        );
    }

    let direct = resolved(
        resolver
            .resolve(Path::new("/index.ts"), "#direct.ts", ResolutionMode::EsNext)
            .expect("resolve explicit TypeScript imports target"),
    );
    assert!(!direct.resolved_using_ts_extension());
    let substituted = resolved(
        resolver
            .resolve(
                Path::new("/index.ts"),
                "#mapped/from-pattern.ts",
                ResolutionMode::EsNext,
            )
            .expect("resolve TypeScript extension introduced by pattern capture"),
    );
    assert!(substituted.resolved_using_ts_extension());
}

#[test]
fn empty_package_import_target_reaches_node_modules_after_base_url_misses() {
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/package.json",
            br##"{"name":"owner","imports":{"#empty":""}}"##.to_vec(),
        )
        .file("/work/src/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/package.json",
            br#"{"name":"node-modules-root","types":"./entry.d.ts"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/entry.d.ts",
            b"export const emptyNodeTarget: true;".to_vec(),
        )
        .build()
        .expect("build empty imports node_modules target host");
    let options = CompilerOptions {
        module: Some(199),
        base_url: Some("/base".to_owned()),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &options).expect("create NodeNext resolver");

    let module = resolved(
        resolver
            .resolve(
                Path::new("/work/src/main.ts"),
                "#empty",
                ResolutionMode::EsNext,
            )
            .expect("continue an empty imports target through node_modules"),
    );
    assert_eq!(
        module.resolved_file().display(),
        Path::new("/work/node_modules/entry.d.ts")
    );
}

#[test]
fn package_imports_deep_acyclic_bare_chain_has_no_artificial_depth_limit() {
    let mut imports = serde_json::Map::new();
    for index in 0..300 {
        imports.insert(
            format!("#{index}"),
            serde_json::Value::String(format!("#{}", index + 1)),
        );
    }
    imports.insert(
        "#300".to_owned(),
        serde_json::Value::String("./target.d.ts".to_owned()),
    );
    let package_json = serde_json::to_vec(&serde_json::json!({
        "name": "root",
        "version": "1.0.0",
        "type": "module",
        "imports": imports,
    }))
    .expect("serialize a deep imports map");
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/package.json", package_json)
        .file("/work/index.ts", b"export {};".to_vec())
        .file("/work/target.d.ts", b"export const target: true;".to_vec())
        .build()
        .expect("build a deep acyclic imports host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    let module = resolved(
        resolver
            .resolve(Path::new("/work/index.ts"), "#0", ResolutionMode::EsNext)
            .expect("resolve every distinct imports rewrite"),
    );
    assert_eq!(
        module.resolved_file().canonical().as_path(),
        Path::new("/work/target.d.ts")
    );
    assert_eq!(module.package_id().map(PackageId::name), Some("root"));
    assert!(!module.is_external_library_import());
}

#[test]
fn package_imports_growing_wildcard_chain_fails_with_a_typed_resource_limit() {
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/package.json",
            br##"{
                "name":"root",
                "type":"module",
                "imports":{"#*":"#x*"}
            }"##
            .to_vec(),
        )
        .file("/work/index.ts", b"export {};".to_vec())
        .build()
        .expect("build a growing imports host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    let error = resolver
        .resolve(Path::new("/work/index.ts"), "#a", ResolutionMode::EsNext)
        .expect_err("an unbounded imports rewrite must fail before exhausting the Rust stack");
    assert!(
        matches!(error, ResolutionError::ResourceLimit(_)),
        "unexpected growing-chain failure: {error}"
    );
}

#[test]
fn deep_conditional_import_rewrites_probe_base_url_before_every_map_step() {
    let mut imports = serde_json::Map::new();
    for index in 0..300 {
        let next = if index == 299 {
            "#300.js".to_owned()
        } else {
            format!("#{}", index + 1)
        };
        imports.insert(
            format!("#{index}"),
            if index % 2 == 0 {
                serde_json::json!({"default":[next]})
            } else {
                serde_json::json!([{"default":next}])
            },
        );
    }
    imports.insert(
        "#300.js".to_owned(),
        serde_json::Value::String("./wrong.d.ts".to_owned()),
    );
    let package_json = serde_json::to_vec(&serde_json::json!({
        "name":"root",
        "version":"1.0.0",
        "imports":imports,
    }))
    .expect("serialize conditional imports chain");
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/package.json", package_json)
        .file("/work/index.ts", b"export {};".to_vec())
        .file("/work/wrong.d.ts", b"export const wrong: true;".to_vec())
        .file(
            "/base/#300.d.ts",
            b"export const fromBaseUrl: true;".to_vec(),
        )
        .build()
        .expect("build conditional imports and baseUrl host");
    let mut options = options_for_module(199);
    options.base_url = Some("/base".to_owned());
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    let module = resolved(
        resolver
            .resolve(Path::new("/work/index.ts"), "#0", ResolutionMode::CommonJs)
            .expect("resolve a deep conditional chain through baseUrl"),
    );
    assert_eq!(
        module.resolved_file().canonical().as_path(),
        Path::new("/base/#300.d.ts")
    );
    assert_eq!(module.package_id(), None);
}

#[test]
fn invalid_import_roots_continue_to_the_ordinary_node_modules_lookup() {
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/package.json",
            br##"{
                "name":"root",
                "imports":{
                    "#to-hash":"#",
                    "#":"./false-hash.d.ts",
                    "#to-slash":"#/x",
                    "#/x":"./false-slash.d.ts"
                }
            }"##
            .to_vec(),
        )
        .file("/work/index.ts", b"export {};".to_vec())
        .file("/work/false-hash.d.ts", b"export {};".to_vec())
        .file("/work/false-slash.d.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/#/package.json",
            br##"{"name":"#","version":"1.0.0","types":"index.d.ts"}"##.to_vec(),
        )
        .file(
            "/work/node_modules/#/index.d.ts",
            b"export const rootHash: true;".to_vec(),
        )
        .file(
            "/work/node_modules/#/x.d.ts",
            b"export const hashSubpath: true;".to_vec(),
        )
        .build()
        .expect("build invalid imports-root fallback host");
    let options = options_for_module(100);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    for (specifier, expected, rewritten) in [
        ("#", "/work/node_modules/#/index.d.ts", false),
        ("#/x", "/work/node_modules/#/x.d.ts", false),
        ("#to-hash", "/work/node_modules/#/index.d.ts", true),
        ("#to-slash", "/work/node_modules/#/x.d.ts", true),
    ] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/index.ts"),
                    specifier,
                    ResolutionMode::CommonJs,
                )
                .expect("invalid imports roots are ordinary lookup misses"),
        );
        assert_eq!(
            module.resolved_file().canonical().as_path(),
            Path::new(expected)
        );
        assert_eq!(module.is_external_library_import(), !rewritten);
    }
}

#[test]
fn relative_bare_import_targets_use_the_nested_node_relative_loader() {
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/package.json",
            br##"{"name":"root","imports":{"#dot":"."}}"##.to_vec(),
        )
        .file("/work/source.ts", b"export {};".to_vec())
        .directory("/work/")
        .file("/work/index.d.ts", b"export const index: true;".to_vec())
        .build()
        .expect("build relative imports-target host");

    for (options, mode, expected) in [
        (options_for_module(199), ResolutionMode::CommonJs, true),
        (options_for_module(199), ResolutionMode::EsNext, false),
        (
            CompilerOptions {
                module: Some(99),
                module_resolution: Some(100),
                ..CompilerOptions::default()
            },
            ResolutionMode::EsNext,
            true,
        ),
    ] {
        let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
        let outcome = resolver
            .resolve(Path::new("/work/source.ts"), "#dot", mode)
            .expect("a relative imports target is a typed lookup outcome");
        if expected {
            let ResolutionOutcome::Resolved(module) = outcome else {
                panic!("expected a relative imports hit for mode {mode:?}");
            };
            assert_eq!(
                module.resolved_file().canonical().as_path(),
                Path::new("/work/index.d.ts")
            );
        } else {
            assert_eq!(outcome, ResolutionOutcome::NotFound);
        }
    }
}

#[test]
fn backslash_relative_and_drive_rooted_requests_use_node_path_normalization() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/src/main.ts", b"export {};".to_vec())
        .file("/work/src.ts", b"export const wrong: true;".to_vec())
        .directory("/work/src/")
        .file(
            "/work/src/index.ts",
            b"export const directory: true;".to_vec(),
        )
        .file("C:/pkg/index.ts", b"export const rooted: true;".to_vec())
        .build()
        .expect("build backslash relative host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    let directory = resolved(
        resolver
            .resolve(
                Path::new("/work/src/main.ts"),
                ".\\",
                ResolutionMode::CommonJs,
            )
            .expect("a trailing backslash denotes a directory"),
    );
    assert_eq!(
        directory.resolved_file().canonical().as_path(),
        Path::new("/work/src/index.ts")
    );

    let rooted = resolved(
        resolver
            .resolve(
                Path::new("/work/src/main.ts"),
                "C:\\pkg\\index.js",
                ResolutionMode::EsNext,
            )
            .expect("a drive-backslash request is a rooted disk path"),
    );
    assert_eq!(
        rooted.resolved_file().canonical().as_path(),
        Path::new("C:/pkg/index.ts")
    );
    assert!(!rooted.is_external_library_import());
}

#[test]
fn imports_pattern_selection_preserves_literal_prefix_overlap_and_directory_gates() {
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/package.json",
            br##"{
                "name":"root",
                "imports":{
                    "#foo/":"#bar",
                    "#barx":"./false-directory.d.ts",
                    "#ab*bc":"./overlap/*.d.ts",
                    "#x*y":"./literal/",
                    "#x*":"./broad/*",
                    "#*":"./false-overlap.d.ts"
                }
            }"##
            .to_vec(),
        )
        .file("/work/index.ts", b"export {};".to_vec())
        .file("/work/false-directory.d.ts", b"export {};".to_vec())
        .file("/work/false-overlap.d.ts", b"export {};".to_vec())
        .file(
            "/work/overlap/b.d.ts",
            b"export const overlap: true;".to_vec(),
        )
        .file(
            "/work/literal/file.d.ts",
            b"export const literal: true;".to_vec(),
        )
        .file(
            "/work/broad/y/file.d.ts",
            b"export const broad: true;".to_vec(),
        )
        .build()
        .expect("build imports pattern boundary host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    assert_eq!(
        resolver
            .resolve(
                Path::new("/work/index.ts"),
                "#foo/x",
                ResolutionMode::EsNext,
            )
            .expect("an invalid directory target is an ordinary miss"),
        ResolutionOutcome::NotFound
    );
    let overlap = resolved(
        resolver
            .resolve(Path::new("/work/index.ts"), "#abc", ResolutionMode::EsNext)
            .expect("overlapping pattern bounds follow JavaScript substring"),
    );
    assert_eq!(
        overlap.resolved_file().canonical().as_path(),
        Path::new("/work/overlap/b.d.ts")
    );

    let literal = resolved(
        resolver
            .resolve(
                Path::new("/work/index.ts"),
                "#x*y/file.d.ts",
                ResolutionMode::EsNext,
            )
            .expect("a nonmatching star key remains a literal directory prefix"),
    );
    assert_eq!(
        literal.resolved_file().canonical().as_path(),
        Path::new("/work/literal/file.d.ts")
    );
}

#[test]
fn exports_preserve_literal_stars_prefix_fallbacks_and_backslash_separators() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/index.mts", b"export {};".to_vec())
        .file(
            "/work/node_modules/star-target/package.json",
            br#"{"name":"star-target","exports":"./*.d.ts"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/star-target/*.d.ts",
            b"export const literalTarget: true;".to_vec(),
        )
        .file(
            "/work/node_modules/backslash/package.json",
            br#"{"name":"backslash","exports":"./types\\index.js"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/backslash/types/index.ts",
            b"export const backslash: true;".to_vec(),
        )
        .file(
            "/work/node_modules/literal-key/package.json",
            br#"{
                "name":"literal-key",
                "exports":{
                    "./x*y":"./literal/",
                    "./x*":"./broad/*"
                }
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/literal-key/literal/file.d.ts",
            b"export const literal: true;".to_vec(),
        )
        .file(
            "/work/node_modules/literal-key/broad/y/file.d.ts",
            b"export const broad: true;".to_vec(),
        )
        .build()
        .expect("build literal package-map target host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    for (specifier, expected) in [
        ("star-target", "/work/node_modules/star-target/*.d.ts"),
        ("backslash", "/work/node_modules/backslash/types/index.ts"),
        (
            "literal-key/x*y/file.d.ts",
            "/work/node_modules/literal-key/literal/file.d.ts",
        ),
    ] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/index.mts"),
                    specifier,
                    ResolutionMode::EsNext,
                )
                .expect("resolve a literal package-map target"),
        );
        assert_eq!(
            module.resolved_file().canonical().as_path(),
            Path::new(expected)
        );
    }
}

#[test]
fn imports_backslash_targets_keep_raw_paths_matching_before_normalized_fallbacks() {
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/package.json",
            br##"{"name":"root","imports":{"#back":"dep\\sub"}}"##.to_vec(),
        )
        .file("/work/index.ts", b"export {};".to_vec())
        .file("/mapped/raw.ts", b"export const raw: true;".to_vec())
        .file("/mapped/slash.ts", b"export const slash: true;".to_vec())
        .file("/base/dep/sub.ts", b"export const base: true;".to_vec())
        .file(
            "/work/node_modules/dep/sub/package.json",
            br#"{"name":"dep\\sub","version":"4.5.6","types":"index.d.ts"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/dep/sub/index.d.ts",
            b"export const package: true;".to_vec(),
        )
        .build()
        .expect("build backslash imports-target host");

    let mut optional = options_for_module(199);
    optional.base_url = Some("/base".to_owned());
    let paths = ProgramOptions::default().with_paths(vec![
        PathMapping::new("dep\\sub", vec!["/mapped/raw.ts".to_owned()]),
        PathMapping::new("dep/sub", vec!["/mapped/slash.ts".to_owned()]),
    ]);
    let mut resolver = ModuleResolver::new_with_program_options(&host, &optional, &paths)
        .expect("create paths resolver");
    let raw_outcome = resolver
        .resolve(
            Path::new("/work/index.ts"),
            "#back",
            ResolutionMode::CommonJs,
        )
        .expect("raw backslash paths key has a typed outcome");
    let ResolutionOutcome::Resolved(raw) = raw_outcome else {
        panic!("raw backslash paths key must win");
    };
    assert_eq!(
        raw.resolved_file().canonical().as_path(),
        Path::new("/mapped/raw.ts")
    );

    let mut resolver = ModuleResolver::new(&host, &optional).expect("create baseUrl resolver");
    let base_outcome = resolver
        .resolve(
            Path::new("/work/index.ts"),
            "#back",
            ResolutionMode::CommonJs,
        )
        .expect("baseUrl backslash target has a typed outcome");
    let ResolutionOutcome::Resolved(base) = base_outcome else {
        panic!("baseUrl must normalize a backslash target after paths");
    };
    assert_eq!(
        base.resolved_file().canonical().as_path(),
        Path::new("/base/dep/sub.ts")
    );

    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create package resolver");
    let package_outcome = resolver
        .resolve(
            Path::new("/work/index.ts"),
            "#back",
            ResolutionMode::CommonJs,
        )
        .expect("node_modules backslash target has a typed outcome");
    let ResolutionOutcome::Resolved(package) = package_outcome else {
        panic!("node_modules must preserve the raw package name");
    };
    assert_eq!(
        package.resolved_file().canonical().as_path(),
        Path::new("/work/node_modules/dep/sub/index.d.ts")
    );
    let package_id = package.package_id().expect("backslash package id");
    assert_eq!(package_id.name(), "dep\\sub");
    assert_eq!(package_id.submodule_name(), "index.d.ts");
    assert!(!package.is_external_library_import());
}

#[test]
fn long_import_pattern_captures_are_input_not_rewrite_work() {
    let capture = "a".repeat(5_000);
    let target = format!("/work/node_modules/dep/{capture}.d.ts");
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/package.json",
            br##"{"name":"root","imports":{"#*":"dep/*.js"}}"##.to_vec(),
        )
        .file("/work/index.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/dep/package.json",
            br#"{"name":"dep","version":"1.0.0"}"#.to_vec(),
        )
        .file(target.clone(), b"export const long: true;".to_vec())
        .build()
        .expect("build a long caller-owned imports capture");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
    let specifier = format!("#{capture}");

    let module = resolved(
        resolver
            .resolve(
                Path::new("/work/index.ts"),
                &specifier,
                ResolutionMode::CommonJs,
            )
            .expect("a long finite capture is not a rewrite resource failure"),
    );
    assert_eq!(
        module.resolved_file().canonical().as_path(),
        Path::new(&target)
    );
}

#[test]
fn package_imports_preserve_non_root_self_and_option_boundaries() {
    let host = MemoryCompilerHost::builder("/")
        .file(
            "/workspace/package.json",
            br##"{
                "name":"workspace",
                "exports":"./index.js",
                "imports": {
                    "#self":"workspace",
                    "#exact":"./src/exact.js",
                    "#x:y":"./src/exact.js",
                    "#x\\y":"./src/exact.js",
                    "#x\u0000y":"./src/exact.js",
                    "#x/../y":"./src/exact.js",
                    "#dot":".dependency",
                    "#blocked":null
                }
            }"##
            .to_vec(),
        )
        .file("/workspace/main.ts", b"export {};".to_vec())
        .file("/workspace/index.ts", b"export const self = true;".to_vec())
        .file(
            "/workspace/src/exact.ts",
            b"export const exact = true;".to_vec(),
        )
        .file(
            "/workspace/node_modules/.dependency/package.json",
            br#"{"name":".dependency","exports":"./index.js"}"#.to_vec(),
        )
        .file(
            "/workspace/node_modules/.dependency/index.ts",
            b"export const dot = true;".to_vec(),
        )
        .file(
            "/workspace/node_modules/#missing/package.json",
            br##"{"name":"#missing","exports":"./index.js"}"##.to_vec(),
        )
        .file(
            "/workspace/node_modules/#missing/index.ts",
            b"export const fallback = true;".to_vec(),
        )
        .file(
            "/workspace/node_modules/#blocked/package.json",
            br##"{"name":"#blocked","exports":"./index.js"}"##.to_vec(),
        )
        .file(
            "/workspace/node_modules/#blocked/index.ts",
            b"export const blocked = true;".to_vec(),
        )
        .file(
            "/workspace/node_modules/#exact/package.json",
            br##"{"name":"#exact","exports":"./index.js"}"##.to_vec(),
        )
        .file(
            "/workspace/node_modules/#exact/index.ts",
            b"export const fallback = true;".to_vec(),
        )
        .build()
        .expect("build non-root package-imports host");

    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
    let self_target = resolved(
        resolver
            .resolve(
                Path::new("/workspace/main.ts"),
                "#self",
                ResolutionMode::EsNext,
            )
            .expect("resolve non-root imports-to-self target"),
    );
    assert_eq!(
        self_target.resolved_file().canonical().as_path(),
        Path::new("/workspace/index.ts")
    );

    for specifier in ["#x:y", "#x\\y", "#x\0y", "#x/../y"] {
        let exact = resolved(
            resolver
                .resolve(
                    Path::new("/workspace/main.ts"),
                    specifier,
                    ResolutionMode::EsNext,
                )
                .expect("exact imports keys are looked up before target validation"),
        );
        assert_eq!(
            exact.resolved_file().canonical().as_path(),
            Path::new("/workspace/src/exact.ts"),
            "{specifier:?}"
        );
    }

    let dot_package = resolved(
        resolver
            .resolve(
                Path::new("/workspace/main.ts"),
                "#dot",
                ResolutionMode::EsNext,
            )
            .expect("bare imports target beginning with a dot is not a relative target"),
    );
    assert_eq!(
        dot_package.resolved_file().canonical().as_path(),
        Path::new("/workspace/node_modules/.dependency/index.ts")
    );

    let missing_import = resolved(
        resolver
            .resolve(
                Path::new("/workspace/main.ts"),
                "#missing",
                ResolutionMode::EsNext,
            )
            .expect("a missing imports entry continues through node_modules"),
    );
    assert_eq!(
        missing_import.resolved_file().canonical().as_path(),
        Path::new("/workspace/node_modules/#missing/index.ts")
    );
    assert_eq!(
        resolver
            .resolve(
                Path::new("/workspace/main.ts"),
                "#blocked",
                ResolutionMode::EsNext,
            )
            .expect("an explicit null imports target remains terminal"),
        ResolutionOutcome::NotFound
    );

    let exports_disabled = CompilerOptions {
        module: Some(199),
        resolve_package_json_exports: Some(false),
        ..CompilerOptions::default()
    };
    let mut resolver =
        ModuleResolver::new(&host, &exports_disabled).expect("create imports-only resolver");
    let exact = resolved(
        resolver
            .resolve(
                Path::new("/workspace/main.ts"),
                "#exact",
                ResolutionMode::EsNext,
            )
            .expect("imports remain enabled independently from exports"),
    );
    assert_eq!(
        exact.resolved_file().canonical().as_path(),
        Path::new("/workspace/src/exact.ts")
    );
    assert_eq!(
        resolved(
            resolver
                .resolve(
                    Path::new("/workspace/main.ts"),
                    "workspace",
                    ResolutionMode::EsNext,
                )
                .expect("SelfName remains enabled independently from Exports"),
        )
        .resolved_file()
        .display(),
        Path::new("/workspace/index.ts"),
    );

    let imports_disabled = CompilerOptions {
        module: Some(199),
        resolve_package_json_imports: Some(false),
        ..CompilerOptions::default()
    };
    let mut resolver =
        ModuleResolver::new(&host, &imports_disabled).expect("create NodeNext resolver");
    let hardcoded = resolved(
        resolver
            .resolve(
                Path::new("/workspace/main.ts"),
                "#exact",
                ResolutionMode::EsNext,
            )
            .expect("NodeNext's fixed feature mask keeps imports enabled"),
    );
    assert_eq!(
        hardcoded.resolved_file().canonical().as_path(),
        Path::new("/workspace/src/exact.ts")
    );

    let bundler_imports_disabled = CompilerOptions {
        module_resolution: Some(100),
        resolve_package_json_imports: Some(false),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &bundler_imports_disabled)
        .expect("create Bundler resolver with imports disabled");
    let fallback = resolved(
        resolver
            .resolve(
                Path::new("/workspace/main.ts"),
                "#exact",
                ResolutionMode::EsNext,
            )
            .expect("Bundler applies the imports feature override"),
    );
    assert_eq!(
        fallback.resolved_file().canonical().as_path(),
        Path::new("/workspace/node_modules/#exact/index.ts")
    );
}

#[test]
fn package_imports_reject_rooted_disk_targets_before_paths_lookup() {
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/package.json",
            br##"{
                "name":"workspace",
                "imports": {
                    "#drive":"C:/alias",
                    "#volume":"D:",
                    "#posix":"/alias",
                    "#unc":"//server/share/alias",
                    "#backslash":"E:\\alias",
                    "#colon":"node:alias"
                }
            }"##
            .to_vec(),
        )
        .file("/work/main.ts", b"export {};".to_vec())
        .file("/work/wrong.ts", b"export const wrong = true;".to_vec())
        .file("/work/colon.ts", b"export const boundary = true;".to_vec())
        .build()
        .expect("build rooted imports-target host");
    let options = CompilerOptions {
        module: Some(199),
        base_url: Some("/work".to_owned()),
        ..CompilerOptions::default()
    };
    let paths = ProgramOptions::default().with_paths(vec![
        // C:/alias would resolve here if the imports-target gate ran after
        // optional paths lookup.
        PathMapping::new("C*", vec!["wrong.ts".to_owned()]),
        PathMapping::new("node:alias", vec!["colon.ts".to_owned()]),
    ]);
    let mut resolver = ModuleResolver::new_with_program_options(&host, &options, &paths)
        .expect("create rooted imports-target resolver");

    let direct_paths = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "C:/alias",
                ResolutionMode::EsNext,
            )
            .expect("prove the rooted spelling is otherwise eligible for paths"),
    );
    assert_eq!(
        direct_paths.resolved_file().display(),
        Path::new("/work/wrong.ts")
    );

    for specifier in ["#drive", "#volume", "#posix", "#unc", "#backslash"] {
        assert_eq!(
            resolver
                .resolve(
                    Path::new("/work/main.ts"),
                    specifier,
                    ResolutionMode::EsNext,
                )
                .expect("an invalid rooted target is a supported miss"),
            ResolutionOutcome::NotFound,
            "{specifier} must be rejected as a rooted disk target"
        );
    }

    let colon_target = resolved(
        resolver
            .resolve(Path::new("/work/main.ts"), "#colon", ResolutionMode::EsNext)
            .expect("a colon-containing bare target remains non-rooted"),
    );
    assert_eq!(
        colon_target.resolved_file().display(),
        Path::new("/work/colon.ts")
    );
    assert!(!colon_target.is_external_library_import());
}

#[test]
fn uri_like_names_run_optional_imports_and_self_name_before_the_node_uri_gate() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.mts", b"export {};".to_vec())
        .file(
            "/work/package.json",
            br##"{"name":"node:fs","version":"1.0.0","exports":"./self.d.ts","imports":{"#x":"node:fs"}}"##.to_vec(),
        )
        .file("/work/self.d.ts", b"export const fromSelf: true;".to_vec())
        .file("/work/shim.ts", b"export const fromPaths = true;".to_vec())
        .build()
        .expect("build URI ordering host");
    let options = CompilerOptions {
        module: Some(199),
        base_url: Some("/work".to_owned()),
        ..CompilerOptions::default()
    };
    let paths = ProgramOptions::default().with_paths(vec![PathMapping::new(
        "node:fs",
        vec!["shim.ts".to_owned()],
    )]);
    let mut resolver = ModuleResolver::new_with_program_options(&host, &options, &paths)
        .expect("create URI paths resolver");
    for specifier in ["node:fs", "#x"] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.mts"),
                    specifier,
                    ResolutionMode::EsNext,
                )
                .expect("resolve a URI-looking name through paths before the URI gate"),
        );
        assert_eq!(module.resolved_file().display(), Path::new("/work/shim.ts"));
        assert!(!module.is_external_library_import(), "{specifier}");
    }

    let self_options = options_for_module(199);
    let mut resolver =
        ModuleResolver::new(&host, &self_options).expect("create URI SelfName resolver");
    for specifier in ["node:fs", "#x"] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.mts"),
                    specifier,
                    ResolutionMode::EsNext,
                )
                .expect("resolve a URI-looking SelfName before the URI gate"),
        );
        assert_eq!(
            module.resolved_file().display(),
            Path::new("/work/self.d.ts")
        );
        assert!(!module.is_external_library_import(), "{specifier}");
        assert_eq!(module.package_id().map(PackageId::name), Some("node:fs"));
    }
    assert_eq!(
        resolver
            .resolve(
                Path::new("/work/main.mts"),
                "node:missing",
                ResolutionMode::EsNext,
            )
            .expect("a URI-looking optional/imports/SelfName miss is supported"),
        ResolutionOutcome::NotFound
    );

    let classic_host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file("/work/node:fs.ts", b"export const classic = true;".to_vec())
        .build()
        .expect("build Classic URI spelling host");
    let classic_options = CompilerOptions {
        module_resolution: Some(1),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&classic_host, &classic_options)
        .expect("create Classic URI spelling resolver");
    assert_eq!(
        resolved(
            resolver
                .resolve(
                    Path::new("/work/main.ts"),
                    "node:fs",
                    ResolutionMode::CommonJs,
                )
                .expect("Classic keeps URI-looking names in ancestor file search"),
        )
        .resolved_file()
        .display(),
        Path::new("/work/node:fs.ts")
    );
}

#[test]
fn package_name_slicing_keeps_a_scope_without_a_package_component() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.mts", b"export {};".to_vec())
        .file(
            "/work/package.json",
            br##"{"name":"workspace","imports":{"#x":"@scope"}}"##.to_vec(),
        )
        .file(
            "/work/node_modules/@scope/package.json",
            br#"{"name":"@scope","version":"1.0.0","types":"index.d.ts"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/@scope/index.d.ts",
            b"export const scoped: true;".to_vec(),
        )
        .build()
        .expect("build package-name slicing host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create NodeNext resolver");

    for specifier in ["@scope", "#x"] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.mts"),
                    specifier,
                    ResolutionMode::EsNext,
                )
                .expect("resolve an upstream-compatible unsplit scope name"),
        );
        assert_eq!(
            module.resolved_file().display(),
            Path::new("/work/node_modules/@scope/index.d.ts")
        );
        assert_eq!(module.package_id().map(PackageId::name), Some("@scope"));
        assert_eq!(module.is_external_library_import(), specifier == "@scope");
    }

    let at_types_host = MemoryCompilerHost::builder("/work")
        .file("/work/main.mts", b"export {};".to_vec())
        .file(
            "/work/package.json",
            br##"{"name":"workspace","imports":{"#x":"@scope"}}"##.to_vec(),
        )
        .file(
            "/work/node_modules/@types/@scope/package.json",
            br#"{"name":"@types/@scope","version":"1.0.0","types":"index.d.ts"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/@types/@scope/index.d.ts",
            b"export const scopedTypes: true;".to_vec(),
        )
        .build()
        .expect("build unsplit-scope @types host");
    let mut resolver =
        ModuleResolver::new(&at_types_host, &options).expect("create @types resolver");
    for specifier in ["@scope", "#x"] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.mts"),
                    specifier,
                    ResolutionMode::EsNext,
                )
                .expect("keep @scope unmangled when it has no package separator"),
        );
        assert_eq!(
            module.resolved_file().display(),
            Path::new("/work/node_modules/@types/@scope/index.d.ts")
        );
        assert_eq!(module.is_external_library_import(), specifier == "@scope");
    }
}

#[test]
fn package_name_slicing_normalizes_unvalidated_rest_segments_like_typescript() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.mts", b"export {};".to_vec())
        .file(
            "/work/package.json",
            br##"{"name":"workspace","imports":{"#x":"pkg//sub"}}"##.to_vec(),
        )
        .file(
            "/work/node_modules/pkg/sub/package.json",
            br#"{"name":"nested","version":"1.0.0","types":"index.d.ts"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/pkg/sub/index.d.ts",
            b"export const nested: true;".to_vec(),
        )
        .build()
        .expect("build unvalidated package-rest host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create NodeNext resolver");

    for specifier in ["pkg//sub", "pkg/./sub", "#x"] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.mts"),
                    specifier,
                    ResolutionMode::EsNext,
                )
                .expect("normalize raw rest segments after upstream package-name slicing"),
        );
        assert_eq!(
            module.resolved_file().display(),
            Path::new("/work/node_modules/pkg/sub/index.d.ts")
        );
        assert_eq!(module.package_id().map(PackageId::name), Some("nested"));
        assert_eq!(module.is_external_library_import(), specifier != "#x");
    }
}

#[test]
fn bare_import_targets_preserve_the_current_extension_mask_and_features() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/package.json",
            br##"{"name":"workspace","imports":{"#x":"dep"}}"##.to_vec(),
        )
        .file(
            "/work/node_modules/dep/package.json",
            br#"{"name":"dep","version":"1.0.0","exports":"./index.js"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/dep/index.js",
            b"exports.dep = true;".to_vec(),
        )
        .file(
            "/work/node_modules/#x/package.json",
            br##"{"name":"#x","version":"1.0.0","types":"index.d.ts"}"##.to_vec(),
        )
        .file(
            "/work/node_modules/#x/index.d.ts",
            b"export const fallback: true;".to_vec(),
        )
        .build()
        .expect("build bare imports extension-mask host");
    let options = CompilerOptions {
        module_resolution: Some(2),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &options).expect("create Node10 resolver");

    let module = resolved(
        resolver
            .resolve(Path::new("/work/main.ts"), "#x", ResolutionMode::EsNext)
            .expect("a preferred imports rewrite must not consume JavaScript"),
    );
    assert_eq!(
        module.resolved_file().display(),
        Path::new("/work/node_modules/#x/index.d.ts")
    );
    assert!(module.is_external_library_import());
}

#[test]
fn bare_import_targets_search_preferred_extensions_across_all_ancestors_first() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.mts", b"export {};".to_vec())
        .file(
            "/work/package.json",
            br##"{"name":"workspace","imports":{"#x":"dep"}}"##.to_vec(),
        )
        .file(
            "/work/node_modules/dep/package.json",
            br#"{"name":"dep","version":"1.0.0","exports":"./runtime.js"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/dep/runtime.js",
            b"exports.near = true;".to_vec(),
        )
        .file(
            "/node_modules/dep/package.json",
            br#"{"name":"dep","version":"2.0.0","exports":"./types.js"}"#.to_vec(),
        )
        .file(
            "/node_modules/dep/types.d.ts",
            b"export const far: true;".to_vec(),
        )
        .build()
        .expect("build bare-target ancestor ordering host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create NodeNext resolver");

    let module = resolved(
        resolver
            .resolve(Path::new("/work/main.mts"), "#x", ResolutionMode::EsNext)
            .expect("search preferred extensions through every ancestor before fallback"),
    );
    assert_eq!(
        module.resolved_file().display(),
        Path::new("/node_modules/dep/types.d.ts")
    );
    assert_eq!(module.package_id().map(PackageId::version), Some("2.0.0"));
    assert!(!module.is_external_library_import());
}

#[test]
fn nested_node10_fallback_runs_the_zero_extension_bundler_retry() {
    let failure = HostError::new(
        HostErrorKind::Other,
        HostOperation::DirectoryExists,
        Some(PathBuf::from("/work/mapped")),
        "fourth mapped-directory observation from the zero-extension retry",
    );
    let inner = MemoryCompilerHost::builder("/work")
        .file("/work/main.cts", b"export {};".to_vec())
        .file(
            "/work/package.json",
            br##"{"name":"workspace","imports":{"#x":"dep"}}"##.to_vec(),
        )
        .file(
            "/work/node_modules/dep/package.json",
            br#"{"name":"dep","version":"1.0.0","types":"missing.d.ts","main":"missing.js"}"#
                .to_vec(),
        )
        .build()
        .expect("build nested zero-extension retry host");
    let host = NthDirectoryExistsFailureHost {
        inner,
        watched_path: PathBuf::from("/work/mapped"),
        fail_on: 4,
        calls: RefCell::new(Vec::new()),
        failure: failure.clone(),
    };
    let options = CompilerOptions {
        module: Some(1),
        module_resolution: Some(2),
        base_url: Some("/work".to_owned()),
        ..CompilerOptions::default()
    };
    let program_options = ProgramOptions::default().with_paths(vec![PathMapping::new(
        "dep",
        vec!["mapped/missing".to_owned()],
    )]);
    let mut resolver = ModuleResolver::new_with_program_options(&host, &options, &program_options)
        .expect("create nested Node10 resolver");

    let error = resolver
        .resolve(Path::new("/work/main.cts"), "#x", ResolutionMode::CommonJs)
        .expect_err("the zero-extension Bundler retry must observe the fourth host failure");
    assert_eq!(error, ResolutionError::Host(failure));
    assert_eq!(
        host.calls
            .borrow()
            .iter()
            .filter(|path| path.as_path() == Path::new("/work/mapped"))
            .count(),
        4
    );
}

#[test]
fn nested_exports_disabled_retry_retains_the_bundler_condition_profile() {
    let danger = HostError::new(
        HostErrorKind::Other,
        HostOperation::FileExists,
        Some(PathBuf::from("/work/danger.d.ts")),
        "the Node condition must stay disabled inside the Bundler diagnostic",
    );
    let inner = MemoryCompilerHost::builder("/work")
        .file("/work/main.cts", b"export {};".to_vec())
        .file(
            "/work/package.json",
            br##"{
                "name":"workspace",
                "imports":{
                    "#outer":{"import":"#inner"},
                    "#inner":{"node":"./danger.d.ts"}
                }
            }"##
            .to_vec(),
        )
        .file("/work/danger.d.ts", b"export const danger: true;".to_vec())
        .file(
            "/work/node_modules/#outer/package.json",
            br##"{"name":"#outer","version":"1.0.0","exports":{"require":"./runtime.js"}}"##
                .to_vec(),
        )
        .file(
            "/work/node_modules/#outer/runtime.js",
            b"exports.outer = true;".to_vec(),
        )
        .file(
            "/work/node_modules/#inner/package.json",
            br##"{
                "name":"#inner",
                "version":"1.0.0",
                "types":"index.d.ts",
                "typesVersions":{"*":{"index.d.ts":["runtime.js"]}}
            }"##
            .to_vec(),
        )
        .file(
            "/work/node_modules/#inner/runtime.js",
            b"exports.inner = true;".to_vec(),
        )
        .build()
        .expect("build nested Bundler-condition host");
    let host = NthFileExistsFailureHost {
        inner,
        watched_path: PathBuf::from("/work/danger.d.ts"),
        fail_on: 1,
        calls: RefCell::new(Vec::new()),
        failure: danger,
    };
    let options = CompilerOptions {
        module: Some(1),
        module_resolution: Some(2),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &options).expect("create Node10 resolver");

    let module = resolved(
        resolver
            .resolve(
                Path::new("/work/main.cts"),
                "#outer",
                ResolutionMode::CommonJs,
            )
            .expect("keep Bundler conditions through the nested diagnostic retry"),
    );
    assert_eq!(
        module.resolved_file().display(),
        Path::new("/work/node_modules/#outer/runtime.js")
    );
    assert!(module.is_external_library_import());
    assert_eq!(module.alternate_result(), None);
    assert!(
        !host
            .calls
            .borrow()
            .iter()
            .any(|path| path == Path::new("/work/danger.d.ts")),
        "the Bundler diagnostic must not re-enable Node10's node condition"
    );
}

#[test]
fn modern_diagnostic_imports_retain_exports_disabled_in_bare_targets() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.mts", b"export {};".to_vec())
        .file(
            "/work/package.json",
            br##"{"name":"workspace","imports":{"#x":"dep"}}"##.to_vec(),
        )
        .file(
            "/work/node_modules/dep/package.json",
            br#"{
                "name":"dep","version":"1.0.0",
                "exports":"./missing.js","types":"./legacy.d.ts"
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/dep/legacy.d.ts",
            b"export const dep: true;".to_vec(),
        )
        .file(
            "/work/node_modules/#x/package.json",
            br##"{
                "name":"#x","version":"1.0.0",
                "exports":"./runtime.js","types":"./legacy.d.ts"
            }"##
            .to_vec(),
        )
        .file(
            "/work/node_modules/#x/runtime.js",
            b"exports.runtime = true;".to_vec(),
        )
        .file(
            "/work/node_modules/#x/legacy.d.ts",
            b"export const fallback: true;".to_vec(),
        )
        .build()
        .expect("build diagnostic imports feature host");
    let options = CompilerOptions {
        module_resolution: Some(99),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &options).expect("create NodeNext resolver");

    let module = resolved(
        resolver
            .resolve(Path::new("/work/main.mts"), "#x", ResolutionMode::EsNext)
            .expect("diagnostic imports rewrite owns its local alternate"),
    );
    assert_eq!(
        module.resolved_file().display(),
        Path::new("/work/node_modules/#x/runtime.js")
    );
    assert_eq!(module.alternate_result(), None);
}

#[test]
fn nested_bare_import_diagnostics_run_before_primary_realpath() {
    let inner = MemoryCompilerHost::builder("/work")
        .file("/work/main.mts", b"export {};".to_vec())
        .file(
            "/work/package.json",
            br##"{"name":"workspace","imports":{"#x":"dep"}}"##.to_vec(),
        )
        .file(
            "/work/node_modules/dep/package.json",
            br#"{
                "name":"dep","version":"1.0.0",
                "exports":"./index.js","types":"./legacy.d.ts"
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/dep/index.js",
            b"exports.dep = true;".to_vec(),
        )
        .file("/physical/dep/index.js", b"exports.dep = true;".to_vec())
        .realpath("/work/node_modules/dep/index.js", "/physical/dep/index.js")
        .file(
            "/work/node_modules/dep/legacy.d.ts",
            b"export const dep: true;".to_vec(),
        )
        .build()
        .expect("build nested imports diagnostic-order host");
    let host = RealpathAfterProbeHost {
        inner,
        required_probe: PathBuf::from("/work/node_modules/dep/legacy.d.ts"),
        primary_realpath: PathBuf::from("/work/node_modules/dep/index.js"),
        required_probe_seen: Cell::new(false),
    };
    let options = CompilerOptions {
        module_resolution: Some(99),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &options).expect("create NodeNext resolver");

    let module = resolved(
        resolver
            .resolve(Path::new("/work/main.mts"), "#x", ResolutionMode::EsNext)
            .expect("run the nested diagnostic retry before realpath"),
    );
    assert_eq!(
        module.resolved_file().display(),
        Path::new("/physical/dep/index.js")
    );
    assert_eq!(
        module.original_path().map(ProgramPath::display),
        Some(Path::new("/work/node_modules/dep/index.js"))
    );
    assert!(!module.is_external_library_import());
    assert_eq!(module.alternate_result(), None);
    assert!(host.required_probe_seen.get());
}

#[test]
fn root_imports_patterns_are_gated_for_node16_but_enabled_elsewhere() {
    let host = MemoryCompilerHost::builder("/")
        .file(
            "/package.json",
            br##"{"name":"root","imports":{"#/*":"./src/*"}}"##.to_vec(),
        )
        .file("/index.ts", b"export {};".to_vec())
        .file("/src/foo.ts", b"export const foo = true;".to_vec())
        .build()
        .expect("build root-wildcard imports host");

    let node16_options = options_for_module(100);
    let mut node16 = ModuleResolver::new(&host, &node16_options).expect("create Node16 resolver");
    assert_eq!(
        node16
            .resolve(Path::new("/index.ts"), "#/foo.ts", ResolutionMode::EsNext,)
            .expect("Node16 rejects root imports patterns as an authoritative miss"),
        ResolutionOutcome::NotFound
    );

    let node_next_options = options_for_module(199);
    let mut node_next =
        ModuleResolver::new(&host, &node_next_options).expect("create NodeNext resolver");
    assert_eq!(
        resolved(
            node_next
                .resolve(Path::new("/index.ts"), "#/foo.ts", ResolutionMode::EsNext,)
                .expect("NodeNext enables root imports patterns"),
        )
        .resolved_file()
        .canonical()
        .as_path(),
        Path::new("/src/foo.ts")
    );

    let bundler_options = CompilerOptions {
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };
    let mut bundler =
        ModuleResolver::new(&host, &bundler_options).expect("create Bundler resolver");
    assert_eq!(
        resolved(
            bundler
                .resolve(Path::new("/index.ts"), "#/foo.ts", ResolutionMode::EsNext,)
                .expect("Bundler enables root imports patterns"),
        )
        .resolved_file()
        .canonical()
        .as_path(),
        Path::new("/src/foo.ts")
    );
}

#[test]
fn relative_node_modules_targets_are_external_without_realpath_rewriting() {
    let forbidden_realpath = HostError::new(
        HostErrorKind::Other,
        HostOperation::Realpath,
        Some(PathBuf::from("/node_modules/pkg/other.ts")),
        "relative resolution must not rewrite through realpath",
    );
    let host = MemoryCompilerHost::builder("/")
        .file(
            "/node_modules/pkg/package.json",
            br#"{"name":"pkg","type":"module"}"#.to_vec(),
        )
        .file("/node_modules/pkg/index.ts", b"export {};".to_vec())
        .file(
            "/node_modules/pkg/other.ts",
            b"export const other = true;".to_vec(),
        )
        .file("/work/main.ts", b"export {};".to_vec())
        .file("/work/foo.js", b"exports.foo = true;".to_vec())
        .failure(forbidden_realpath)
        .build()
        .expect("build relative node_modules host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
    let module = resolved(
        resolver
            .resolve(
                Path::new("/node_modules/pkg/index.ts"),
                "./other.js",
                ResolutionMode::EsNext,
            )
            .expect("relative package source resolves without realpath"),
    );
    assert_eq!(
        module.resolved_file().canonical().as_path(),
        Path::new("/node_modules/pkg/other.ts")
    );
    assert!(module.is_external_library_import());
    assert_eq!(module.original_path(), None);

    let raw_external = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "./node_modules/../foo.js",
                ResolutionMode::CommonJs,
            )
            .expect("classify the unnormalized relative path components"),
    );
    assert_eq!(
        raw_external.resolved_file().display(),
        Path::new("/work/foo.js")
    );
    assert!(raw_external.is_external_library_import());
    assert_eq!(raw_external.original_path(), None);
}

#[test]
fn untyped_exports_retain_the_esm_legacy_alternate_and_package_facts() {
    let host = MemoryCompilerHost::builder("/")
        .file("/main.mts", b"export {};".to_vec())
        .file("/main.cts", b"export {};".to_vec())
        .file(
            "/node_modules/pkg/package.json",
            br#"{
                "name":"pkg",
                "version":"1.0.0",
                "exports":{"./foo":"./dist/foo.js"},
                "typesVersions":{"*":{"foo":["./types/foo.d.ts"]}}
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/pkg/dist/foo.js",
            b"module.exports = {};".to_vec(),
        )
        .file("/node_modules/pkg/types/foo.d.ts", b"export {};".to_vec())
        .file(
            "/node_modules/no-alternate/package.json",
            br#"{
                "name":"no-alternate",
                "exports":"./dist/index.js"
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/no-alternate/dist/index.js",
            b"module.exports = {};".to_vec(),
        )
        .file(
            "/node_modules/no-alternate/index.d.ts",
            b"export {};".to_vec(),
        )
        .build()
        .expect("build untyped package host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    let esm = resolved(
        resolver
            .resolve(Path::new("/main.mts"), "pkg/foo", ResolutionMode::EsNext)
            .expect("resolve ESM implementation"),
    );
    assert_eq!(esm.extension(), &ModuleExtension::Js);
    assert_eq!(
        esm.alternate_result()
            .expect("ESM implementation has a legacy alternate")
            .canonical()
            .as_path(),
        Path::new("/node_modules/pkg/types/foo.d.ts")
    );
    assert_eq!(esm.package_id().map(PackageId::name), Some("pkg"));

    let commonjs = resolved(
        resolver
            .resolve(Path::new("/main.cts"), "pkg/foo", ResolutionMode::CommonJs)
            .expect("resolve CommonJS implementation"),
    );
    assert_eq!(commonjs.extension(), &ModuleExtension::Js);
    assert_eq!(commonjs.alternate_result(), None);

    let no_alternate = resolved(
        resolver
            .resolve(
                Path::new("/main.mts"),
                "no-alternate",
                ResolutionMode::EsNext,
            )
            .expect("resolve an exports implementation without a legacy type target"),
    );
    assert_eq!(no_alternate.extension(), &ModuleExtension::Js);
    assert_eq!(no_alternate.alternate_result(), None);
}

#[test]
fn modern_alternate_restarts_all_ancestors_before_primary_realpath() {
    let inner = MemoryCompilerHost::builder("/work")
        .file("/work/src/main.mts", b"export {};".to_vec())
        .file(
            "/work/src/node_modules/pkg/package.json",
            br#"{"name":"near","version":"1.0.0","exports":"./index.js"}"#.to_vec(),
        )
        .file(
            "/work/src/node_modules/pkg/index.js",
            b"exports.near = true;".to_vec(),
        )
        .file("/physical/near/index.js", b"exports.near = true;".to_vec())
        .realpath(
            "/work/src/node_modules/pkg/index.js",
            "/physical/near/index.js",
        )
        .file(
            "/work/node_modules/pkg/package.json",
            br#"{
                "name":"outer","version":"2.0.0",
                "exports":"./outer.js","types":"./legacy.d.ts"
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/pkg/outer.js",
            b"exports.outer = true;".to_vec(),
        )
        .file(
            "/work/node_modules/pkg/legacy.d.ts",
            b"export const legacy: true;".to_vec(),
        )
        .build()
        .expect("build full modern alternate host");
    let host = RealpathAfterProbeHost {
        inner,
        required_probe: PathBuf::from("/work/node_modules/pkg/legacy.d.ts"),
        primary_realpath: PathBuf::from("/work/src/node_modules/pkg/index.js"),
        required_probe_seen: Cell::new(false),
    };
    let options = CompilerOptions {
        module_resolution: Some(99),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &options).expect("create NodeNext resolver");

    let module = resolved(
        resolver
            .resolve(
                Path::new("/work/src/main.mts"),
                "pkg",
                ResolutionMode::EsNext,
            )
            .expect("resolve primary and full-scope legacy alternate"),
    );
    assert_eq!(
        module.resolved_file().display(),
        Path::new("/physical/near/index.js")
    );
    assert_eq!(
        module.original_path().map(ProgramPath::display),
        Some(Path::new("/work/src/node_modules/pkg/index.js"))
    );
    assert_eq!(
        module.alternate_result().map(ProgramPath::display),
        Some(Path::new("/work/node_modules/pkg/legacy.d.ts"))
    );
    assert!(host.required_probe_seen.get());
}

#[test]
fn falsy_exports_use_legacy_fields_and_overlapping_patterns_follow_js_substring() {
    for (package, exports) in [("empty", "\"\""), ("falsey", "false"), ("zero", "0")] {
        let package_json =
            format!(r#"{{"name":"{package}","types":"./index.d.ts","exports":{exports}}}"#);
        let package_path = format!("/work/node_modules/{package}/package.json");
        let host = MemoryCompilerHost::builder("/work")
            .file("/work/index.mts", b"export {};".to_vec())
            .file(package_path, package_json.into_bytes())
            .file(
                format!("/work/node_modules/{package}/index.d.ts"),
                b"export {};".to_vec(),
            )
            .build()
            .expect("build falsy-exports host");
        let options = options_for_module(199);
        let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/index.mts"),
                    package,
                    ResolutionMode::EsNext,
                )
                .expect("falsy exports uses the legacy package fields"),
        );
        assert_eq!(
            module.resolved_file().display(),
            PathBuf::from(format!("/work/node_modules/{package}/index.d.ts"))
        );
    }

    for (package, exports) in [("truthy", "true"), ("one", "1"), ("infinite", "1e309")] {
        let package_json = format!(r#"{{"name":"{package}","exports":{exports}}}"#);
        let host = MemoryCompilerHost::builder("/work")
            .file("/work/index.mts", b"export {};".to_vec())
            .file(
                format!("/work/node_modules/{package}/package.json"),
                package_json.into_bytes(),
            )
            .build()
            .expect("build truthy primitive exports host");
        let options = options_for_module(199);
        let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
        assert_eq!(
            resolver
                .resolve(
                    Path::new("/work/index.mts"),
                    package,
                    ResolutionMode::EsNext,
                )
                .expect("truthy primitive exports blocks legacy resolution without throwing"),
            ResolutionOutcome::NotFound
        );
    }

    let host = MemoryCompilerHost::builder("/work")
        .file("/work/index.mts", b"export {};".to_vec())
        .file(
            "/work/node_modules/nullish/package.json",
            br#"{"name":"nullish","exports":null}"#.to_vec(),
        )
        .file(
            "/work/node_modules/nullish/index.d.ts",
            b"export {};".to_vec(),
        )
        .build()
        .expect("build null-exports legacy-index host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
    let nullish = resolved(
        resolver
            .resolve(
                Path::new("/work/index.mts"),
                "nullish",
                ResolutionMode::EsNext,
            )
            .expect("null exports permits the Node ESM package-root index exception"),
    );
    assert_eq!(
        nullish.resolved_file().canonical().as_path(),
        Path::new("/work/node_modules/nullish/index.d.ts")
    );

    let host = MemoryCompilerHost::builder("/work")
        .file("/work/index.mts", b"export {};".to_vec())
        .file(
            "/work/node_modules/overlap/package.json",
            br#"{"name":"overlap","exports":{"./aba*aba":"./types/*.d.ts"}}"#.to_vec(),
        )
        .file(
            "/work/node_modules/overlap/types/aba.d.ts",
            b"export const overlap: true;".to_vec(),
        )
        .build()
        .expect("build overlapping-pattern host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
    let overlap = resolved(
        resolver
            .resolve(
                Path::new("/work/index.mts"),
                "overlap/aba",
                ResolutionMode::EsNext,
            )
            .expect("overlapping bounds use JavaScript substring swapping"),
    );
    assert_eq!(
        overlap.resolved_file().canonical().as_path(),
        Path::new("/work/node_modules/overlap/types/aba.d.ts")
    );
}

#[test]
fn external_walk_prefers_types_across_ancestors_and_continues_after_null() {
    let host = MemoryCompilerHost::builder("/work/project")
        .file("/work/project/src/index.mts", b"export {};".to_vec())
        .file(
            "/work/project/src/node_modules/inner/package.json",
            br#"{
                "name":"inner",
                "exports": {
                    "./typed":"./near.js",
                    "./blocked":null
                }
            }"#
            .to_vec(),
        )
        .file(
            "/work/project/src/node_modules/inner/near.js",
            b"module.exports = {};".to_vec(),
        )
        .file(
            "/work/project/node_modules/inner/package.json",
            br#"{
                "name":"inner",
                "exports": {
                    "./typed":"./outer.js",
                    "./blocked":"./outer.js"
                }
            }"#
            .to_vec(),
        )
        .file(
            "/work/project/node_modules/inner/outer.d.ts",
            b"export const typed: true;".to_vec(),
        )
        .build()
        .expect("build nested node_modules tree");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    for specifier in ["inner/typed", "inner/blocked"] {
        let resolution = resolved(
            resolver
                .resolve(
                    Path::new("/work/project/src/index.mts"),
                    specifier,
                    ResolutionMode::EsNext,
                )
                .expect("resolve across node_modules ancestors"),
        );
        assert_eq!(resolution.extension(), &ModuleExtension::Dts);
        assert_eq!(
            resolution.resolved_file().canonical().as_path(),
            Path::new("/work/project/node_modules/inner/outer.d.ts")
        );
    }
}

#[test]
fn a_manifestless_near_package_miss_continues_to_an_outer_node_modules() {
    let host = MemoryCompilerHost::builder("/work/project")
        .file("/work/project/src/index.mts", b"export {};".to_vec())
        .file(
            "/work/project/src/node_modules/inner/placeholder.txt",
            b"legacy package".to_vec(),
        )
        .file(
            "/work/project/node_modules/inner/package.json",
            br#"{"name":"inner","exports":{"./x":"./x.js"}}"#.to_vec(),
        )
        .file(
            "/work/project/node_modules/inner/x.d.ts",
            b"export const x: true;".to_vec(),
        )
        .build()
        .expect("build legacy-shadowing package tree");
    let options = options_for_module(100);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    let module = resolved(
        resolver
            .resolve(
                Path::new("/work/project/src/index.mts"),
                "inner/x",
                ResolutionMode::EsNext,
            )
            .expect("a manifestless miss continues the ancestor walk"),
    );
    assert_eq!(
        module.resolved_file().canonical().as_path(),
        Path::new("/work/project/node_modules/inner/x.d.ts")
    );
}

#[test]
fn at_types_fallback_preserves_declaration_only_conditions_and_scoped_names() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/index.mts", b"export {};".to_vec())
        .file(
            "/work/node_modules/@types/inner/package.json",
            br#"{
                "name":"@types/inner",
                "version":"1.0.0",
                "exports":{
                    ".":{
                        "import":"./index.d.mts",
                        "require":"./index.d.cts"
                    }
                }
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/@types/inner/index.d.mts",
            b"export const mode: 'import';".to_vec(),
        )
        .file(
            "/work/node_modules/@types/inner/index.d.cts",
            b"export const mode: 'require';".to_vec(),
        )
        .file(
            "/work/node_modules/@types/scope__pkg/package.json",
            br#"{"name":"@types/scope__pkg","version":"2.0.0","types":"index.d.ts"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/@types/scope__pkg/index.d.ts",
            b"export const scoped: true;".to_vec(),
        )
        .build()
        .expect("build @types fallback tree");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    for (mode, expected, extension) in [
        (
            ResolutionMode::EsNext,
            "/work/node_modules/@types/inner/index.d.mts",
            ModuleExtension::Dmts,
        ),
        (
            ResolutionMode::CommonJs,
            "/work/node_modules/@types/inner/index.d.cts",
            ModuleExtension::Dcts,
        ),
    ] {
        let module = resolved(
            resolver
                .resolve(Path::new("/work/index.mts"), "inner", mode)
                .expect("resolve conditional @types fallback"),
        );
        assert_eq!(
            module.resolved_file().canonical().as_path(),
            Path::new(expected)
        );
        assert_eq!(module.extension(), &extension);
        assert_eq!(
            module.package_id().map(PackageId::name),
            Some("@types/inner")
        );
    }

    let scoped = resolved(
        resolver
            .resolve(
                Path::new("/work/index.mts"),
                "@scope/pkg",
                ResolutionMode::EsNext,
            )
            .expect("resolve a mangled scoped @types fallback"),
    );
    assert_eq!(
        scoped.resolved_file().canonical().as_path(),
        Path::new("/work/node_modules/@types/scope__pkg/index.d.ts")
    );
    assert_eq!(
        scoped.package_id().map(PackageId::name),
        Some("@types/scope__pkg")
    );
    assert!(scoped.is_external_library_import());
}

#[test]
fn at_types_fallback_is_declaration_only_for_manifestless_packages() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/@types/pkg/index.ts",
            b"export const wrong: true;".to_vec(),
        )
        .file(
            "/work/node_modules/@types/pkg/index.d.ts",
            b"export const right: true;".to_vec(),
        )
        .build()
        .expect("build manifestless @types package");
    let options = options_for_module(1);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
    let module = resolved(
        resolver
            .resolve(Path::new("/work/main.ts"), "pkg", ResolutionMode::CommonJs)
            .expect("resolve declaration-only @types fallback"),
    );
    assert_eq!(
        module.resolved_file().canonical().as_path(),
        Path::new("/work/node_modules/@types/pkg/index.d.ts")
    );
    assert_eq!(module.extension(), &ModuleExtension::Dts);
    assert_eq!(module.package_id(), None);
}

#[test]
fn type_reference_primary_custom_roots_use_direct_then_directory_precedence() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/types/direct.d.ts",
            b"declare const direct: true;".to_vec(),
        )
        .file(
            "/work/types/direct/index.d.ts",
            b"declare const wrongDirectory: true;".to_vec(),
        )
        .file(
            "/work/types/versioned/package.json",
            br#"{
                "name":"versioned-types",
                "version":"1.0.0",
                "types":"index",
                "typesVersions":{"*":{"index":["v6/index"]}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/types/versioned/index.d.ts",
            b"declare const wrongVersion: true;".to_vec(),
        )
        .file(
            "/work/types/versioned/v6/index.d.ts",
            b"declare const versioned: true;".to_vec(),
        )
        .file(
            "/work/types/twinned/package.json",
            br#"{"name":"twinned-types","version":"1.0.0","types":"index.ts"}"#.to_vec(),
        )
        .file(
            "/work/types/twinned/index.ts",
            b"declare const wrongImplementation: true;".to_vec(),
        )
        .file(
            "/work/types/twinned/index.d.ts",
            b"declare const declarationTwin: true;".to_vec(),
        )
        .file(
            "/work/types/esm-index/package.json",
            br#"{"name":"esm-index-types","version":"1.0.0"}"#.to_vec(),
        )
        .file(
            "/work/types/esm-index/index.d.ts",
            b"declare const cjsOnlyIndex: true;".to_vec(),
        )
        .file(
            "/work/types/esm-manifestless/index.d.ts",
            b"declare const cjsOnlyManifestlessIndex: true;".to_vec(),
        )
        .build()
        .expect("build custom typeRoots tree");
    let options = options_for_module(1);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
    let type_root = ProgramPath::from_trusted_parts("/work/types", "/work/types")
        .expect("create custom type root");

    let ResolutionOutcome::Resolved(direct) = resolver
        .resolve_type_reference(
            Path::new("/work/main.ts"),
            "direct",
            ResolutionMode::CommonJs,
            Some(std::slice::from_ref(&type_root)),
        )
        .expect("resolve direct custom-root declaration")
    else {
        panic!("expected direct custom-root type reference");
    };
    assert_eq!(
        direct.resolved_file().canonical().as_path(),
        Path::new("/work/types/direct.d.ts")
    );
    assert!(direct.primary());
    assert!(!direct.is_external_library_import());

    let ResolutionOutcome::Resolved(versioned) = resolver
        .resolve_type_reference(
            Path::new("/work/main.ts"),
            "versioned",
            ResolutionMode::CommonJs,
            Some(std::slice::from_ref(&type_root)),
        )
        .expect("resolve directory package through custom type root")
    else {
        panic!("expected versioned custom-root type reference");
    };
    assert_eq!(
        versioned.resolved_file().canonical().as_path(),
        Path::new("/work/types/versioned/v6/index.d.ts")
    );
    assert!(versioned.primary());
    assert_eq!(
        versioned.package_id().map(PackageId::name),
        Some("versioned-types")
    );
    let bound_versioned = versioned
        .clone()
        .into_resolved_type_reference_directive(
            versioned.resolved_file().clone(),
            SourceFileId::from_raw(7),
        )
        .expect("bind the primary package-backed type reference");
    assert!(bound_versioned.primary());
    assert_eq!(bound_versioned.source(), SourceFileId::from_raw(7));
    assert_eq!(
        bound_versioned.package_id().map(PackageId::name),
        Some("versioned-types")
    );

    let ResolutionOutcome::Resolved(twinned) = resolver
        .resolve_type_reference(
            Path::new("/work/main.ts"),
            "twinned",
            ResolutionMode::CommonJs,
            Some(std::slice::from_ref(&type_root)),
        )
        .expect("resolve a package-field declaration twin before its implementation")
    else {
        panic!("expected twinned custom-root type reference");
    };
    assert_eq!(
        twinned.resolved_file().canonical().as_path(),
        Path::new("/work/types/twinned/index.d.ts")
    );

    let esm_options = options_for_module(199);
    let mut esm_resolver =
        ModuleResolver::new(&host, &esm_options).expect("create Node ESM resolver");
    for specifier in ["esm-index", "esm-manifestless"] {
        assert_eq!(
            esm_resolver
                .resolve_type_reference(
                    Path::new("/work/main.mts"),
                    specifier,
                    ResolutionMode::EsNext,
                    Some(std::slice::from_ref(&type_root)),
                )
                .expect("Node ESM primary directory lookup is an ordinary miss"),
            ResolutionOutcome::NotFound
        );
    }
}

#[test]
fn non_external_custom_type_roots_retain_realpath_transitions() {
    let declaration = b"declare const linked: true;".to_vec();
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file("/work/types/linked.d.ts", declaration.clone())
        .file("/actual/linked.d.ts", declaration)
        .realpath("/work/types/linked.d.ts", "/actual/linked.d.ts")
        .build()
        .expect("build custom typeRoot symlink");
    let options = options_for_module(1);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
    let type_root = ProgramPath::from_trusted_parts("/work/types", "/work/types")
        .expect("create custom type root");

    let ResolutionOutcome::Resolved(reference) = resolver
        .resolve_type_reference(
            Path::new("/work/main.ts"),
            "linked",
            ResolutionMode::CommonJs,
            Some(std::slice::from_ref(&type_root)),
        )
        .expect("resolve custom-root symlink")
    else {
        panic!("expected custom-root symlink type reference");
    };
    assert_eq!(
        reference.resolved_file().canonical().as_path(),
        Path::new("/actual/linked.d.ts")
    );
    assert_eq!(
        reference
            .original_path()
            .expect("lexical path")
            .canonical()
            .as_path(),
        Path::new("/work/types/linked.d.ts")
    );
    assert!(reference.primary());
    assert!(!reference.is_external_library_import());
    let caller_spelling = ProgramPath::from_trusted_parts(
        "actual/linked.d.ts",
        reference.resolved_file().canonical().as_path(),
    )
    .expect("create caller-owned target spelling");
    let bound = reference
        .clone()
        .into_resolved_type_reference_directive(caller_spelling, SourceFileId::from_raw(11))
        .expect("bind canonical target identity across display spellings");
    assert_eq!(bound.target().display(), Path::new("actual/linked.d.ts"));
    assert_eq!(bound.source(), SourceFileId::from_raw(11));
    assert_eq!(
        bound
            .original_path()
            .expect("bound directive retains lexical path")
            .canonical()
            .as_path(),
        Path::new("/work/types/linked.d.ts")
    );
    assert!(bound.primary());
    assert!(!bound.is_external_library_import());

    let mismatched = ProgramPath::from_trusted_parts("/other.d.ts", "/other.d.ts")
        .expect("create mismatched target");
    assert!(matches!(
        reference.into_resolved_type_reference_directive(mismatched, SourceFileId::from_raw(12)),
        Err(ResolutionError::InvalidData(_))
    ));
}

#[test]
fn type_reference_default_roots_and_secondary_lookup_preserve_spelling_and_origin() {
    let host = MemoryCompilerHost::builder("/work/project")
        .case_sensitive(true)
        .file("/work/project/src/main.ts", b"export {};".to_vec())
        .file(
            "/work/project/node_modules/@types/defaulted/package.json",
            br#"{"name":"@types/defaulted","version":"1.0.0","types":"index.d.ts"}"#.to_vec(),
        )
        .file(
            "/work/project/node_modules/@types/defaulted/index.d.ts",
            b"declare const defaulted: true;".to_vec(),
        )
        .file(
            "/work/project/src/node_modules/secondary/package.json",
            br#"{"name":"secondary","version":"1.0.0","types":"index.d.ts"}"#.to_vec(),
        )
        .file(
            "/work/project/src/node_modules/secondary/index.d.ts",
            b"declare const secondary: true;".to_vec(),
        )
        .build()
        .expect("build default and secondary type-reference tree");
    let options = options_for_module(1);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    let ResolutionOutcome::Resolved(defaulted) = resolver
        .resolve_type_reference(
            Path::new("/work/project/src/main.ts"),
            "defaulted",
            ResolutionMode::CommonJs,
            None,
        )
        .expect("resolve from current-directory default type root")
    else {
        panic!("expected default-root type reference");
    };
    assert!(defaulted.primary());
    assert!(defaulted.is_external_library_import());
    assert_eq!(
        defaulted.resolved_file().canonical().as_path(),
        Path::new("/work/project/node_modules/@types/defaulted/index.d.ts")
    );
    let bound_defaulted = defaulted
        .clone()
        .into_resolved_type_reference_directive(
            defaulted.resolved_file().clone(),
            SourceFileId::from_raw(13),
        )
        .expect("bind an external default-root type reference");
    assert!(bound_defaulted.primary());
    assert!(bound_defaulted.is_external_library_import());

    let no_primary_roots: Vec<ProgramPath> = Vec::new();
    let ResolutionOutcome::Resolved(secondary) = resolver
        .resolve_type_reference(
            Path::new("/work/project/src/main.ts"),
            "secondary",
            ResolutionMode::CommonJs,
            Some(&no_primary_roots),
        )
        .expect("resolve from nearest secondary node_modules")
    else {
        panic!("expected secondary type reference");
    };
    assert!(!secondary.primary());
    assert_eq!(
        secondary.resolved_file().canonical().as_path(),
        Path::new("/work/project/src/node_modules/secondary/index.d.ts")
    );

    assert_eq!(
        resolver
            .resolve_type_reference(
                Path::new("/work/project/src/main.ts"),
                "DEFAULTED",
                ResolutionMode::CommonJs,
                None,
            )
            .expect("case-sensitive miss remains an ordinary miss"),
        ResolutionOutcome::NotFound
    );

    assert_eq!(
        resolver
            .resolve_type_reference(
                Path::new("/work/project/__inferred type names__.ts"),
                "defaulted",
                ResolutionMode::Unspecified,
                Some(&no_primary_roots),
            )
            .expect("custom automatic roots suppress secondary lookup"),
        ResolutionOutcome::NotFound
    );
}

#[test]
fn type_reference_secondary_at_types_exports_preserve_import_and_require_modes() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/@types/mode/package.json",
            br#"{
                "name":"@types/mode",
                "version":"1.0.0",
                "exports":{
                    ".":{
                        "import":"./index.d.mts",
                        "require":"./index.d.cts"
                    }
                }
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/@types/mode/index.d.mts",
            b"export const mode: 'import';".to_vec(),
        )
        .file(
            "/work/node_modules/@types/mode/index.d.cts",
            b"export const mode: 'require';".to_vec(),
        )
        .build()
        .expect("build conditional type-reference package");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
    let no_primary_roots: Vec<ProgramPath> = Vec::new();

    for (mode, expected, extension) in [
        (
            ResolutionMode::EsNext,
            "/work/node_modules/@types/mode/index.d.mts",
            ModuleExtension::Dmts,
        ),
        (
            ResolutionMode::CommonJs,
            "/work/node_modules/@types/mode/index.d.cts",
            ModuleExtension::Dcts,
        ),
    ] {
        let ResolutionOutcome::Resolved(reference) = resolver
            .resolve_type_reference(
                Path::new("/work/main.ts"),
                "mode",
                mode,
                Some(&no_primary_roots),
            )
            .expect("resolve conditional secondary type reference")
        else {
            panic!("expected conditional secondary type reference");
        };
        assert_eq!(
            reference.resolved_file().canonical().as_path(),
            Path::new(expected)
        );
        assert_eq!(reference.extension(), &extension);
        assert!(!reference.primary());
        assert_eq!(
            reference.package_id().map(PackageId::name),
            Some("@types/mode")
        );
    }
}

#[test]
fn manifest_and_target_directory_host_failures_propagate() {
    let manifest_failure = HostError::new(
        HostErrorKind::PermissionDenied,
        HostOperation::FileExists,
        Some(PathBuf::from("/work/node_modules/manifest/package.json")),
        "manifest existence denied",
    );
    let manifest_host = MemoryCompilerHost::builder("/work")
        .file("/work/index.mts", b"export {};".to_vec())
        .file(
            "/work/node_modules/manifest/package.json",
            br#"{"name":"manifest","exports":"./index.js"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/manifest/index.d.ts",
            b"export const manifest: true;".to_vec(),
        )
        .failure(manifest_failure.clone())
        .build()
        .expect("build manifest-failure host");
    let options = options_for_module(199);
    let mut resolver =
        ModuleResolver::new(&manifest_host, &options).expect("create manifest resolver");
    let error = resolver
        .resolve(
            Path::new("/work/index.mts"),
            "manifest",
            ResolutionMode::EsNext,
        )
        .expect_err("manifest fileExists failure must propagate");
    let ResolutionError::Host(actual) = error else {
        panic!("expected manifest host error, got {error:?}");
    };
    assert_eq!(actual, manifest_failure);

    let directory_failure = HostError::new(
        HostErrorKind::PermissionDenied,
        HostOperation::DirectoryExists,
        Some(PathBuf::from("/work/node_modules/target/nested")),
        "target directory denied",
    );
    let target_host = MemoryCompilerHost::builder("/work")
        .file("/work/index.mts", b"export {};".to_vec())
        .file(
            "/work/node_modules/target/package.json",
            br#"{"name":"target","exports":"./nested/index.js"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/target/nested/index.d.ts",
            b"export const target: true;".to_vec(),
        )
        .failure(directory_failure.clone())
        .build()
        .expect("build target-directory-failure host");
    let mut resolver = ModuleResolver::new(&target_host, &options).expect("create target resolver");
    let error = resolver
        .resolve(
            Path::new("/work/index.mts"),
            "target",
            ResolutionMode::EsNext,
        )
        .expect_err("target directory failure must propagate");
    let ResolutionError::Host(actual) = error else {
        panic!("expected target directory host error, got {error:?}");
    };
    assert_eq!(actual, directory_failure);
}

#[test]
fn self_name_all_extensions_preserve_priority_before_fallback_and_allow_js_exception() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.mts", b"export {};".to_vec())
        .file("/work/node_modules/main.mts", b"export {};".to_vec())
        .file(
            "/work/package.json",
            br##"{
                "name":"workspace",
                "version":"1.0.0",
                "imports":{"#x":"workspace"},
                "exports":["./runtime.js","./typed.js"]
            }"##
            .to_vec(),
        )
        .file("/work/runtime.js", b"exports.runtime = true;".to_vec())
        .file("/work/typed.d.ts", b"export const typed: true;".to_vec())
        .build()
        .expect("build SelfName extension-priority host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create NodeNext resolver");
    for specifier in ["workspace", "#x"] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.mts"),
                    specifier,
                    ResolutionMode::EsNext,
                )
                .expect("finish every preferred SelfName target before JavaScript fallback"),
        );
        assert_eq!(
            module.resolved_file().display(),
            Path::new("/work/typed.d.ts")
        );
        assert!(!module.is_external_library_import(), "{specifier}");
    }

    let allow_js_options = CompilerOptions {
        module: Some(199),
        allow_js: true,
        ..CompilerOptions::default()
    };
    let mut resolver =
        ModuleResolver::new(&host, &allow_js_options).expect("create allowJs NodeNext resolver");
    for specifier in ["workspace", "#x"] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.mts"),
                    specifier,
                    ResolutionMode::EsNext,
                )
                .expect("allowJs keeps a local SelfName search in one combined pass"),
        );
        assert_eq!(
            module.resolved_file().display(),
            Path::new("/work/runtime.js")
        );
        assert!(!module.is_external_library_import(), "{specifier}");
    }
    let terminal_node_modules_directory = resolved(
        resolver
            .resolve(
                Path::new("/work/node_modules/main.mts"),
                "workspace",
                ResolutionMode::EsNext,
            )
            .expect("a terminal node_modules directory keeps the literal allowJs fast path"),
    );
    assert_eq!(
        terminal_node_modules_directory.resolved_file().display(),
        Path::new("/work/runtime.js")
    );
}

#[test]
fn self_name_observes_the_empty_secondary_extension_mask() {
    let failure = HostError::new(
        HostErrorKind::Other,
        HostOperation::DirectoryExists,
        Some(PathBuf::from("/work/missing")),
        "SelfName's empty secondary mask re-observed the target parent",
    );
    let inner = MemoryCompilerHost::builder("/work")
        .file("/work/main.cts", b"export {};".to_vec())
        .file(
            "/work/package.json",
            br#"{"name":"workspace","exports":"./missing/value.js"}"#.to_vec(),
        )
        .build()
        .expect("build empty SelfName secondary-mask host");
    let host = NthDirectoryExistsFailureHost {
        inner,
        watched_path: PathBuf::from("/work/missing"),
        fail_on: 2,
        calls: RefCell::new(Vec::new()),
        failure: failure.clone(),
    };
    let options = CompilerOptions {
        module_resolution: Some(2),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &options).expect("create Node10 resolver");
    let error = resolver
        .resolve(
            Path::new("/work/main.cts"),
            "workspace",
            ResolutionMode::CommonJs,
        )
        .expect_err("the empty SelfName secondary mask must propagate its host failure");
    assert_eq!(error, ResolutionError::Host(failure));
    assert_eq!(
        host.calls
            .borrow()
            .iter()
            .filter(|path| path.as_path() == Path::new("/work/missing"))
            .count(),
        2
    );
}

#[test]
fn self_references_skip_external_realpath_and_case_only_realpaths_stay_lexical() {
    let realpath_failure = HostError::new(
        HostErrorKind::Other,
        HostOperation::Realpath,
        Some(PathBuf::from("/node_modules/inner/index.d.ts")),
        "realpath must not run for a self-reference",
    );
    let host = MemoryCompilerHost::builder("/")
        .file(
            "/node_modules/inner/package.json",
            br#"{"name":"inner","exports":{"./x":"./index.js"}}"#.to_vec(),
        )
        .file("/node_modules/inner/test.d.ts", b"export {};".to_vec())
        .file(
            "/node_modules/inner/index.d.ts",
            b"export const x: true;".to_vec(),
        )
        .failure(realpath_failure)
        .build()
        .expect("build self-reference host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
    let self_reference = resolved(
        resolver
            .resolve(
                Path::new("/node_modules/inner/test.d.ts"),
                "inner/x",
                ResolutionMode::CommonJs,
            )
            .expect("self-reference does not query realpath"),
    );
    assert_eq!(self_reference.original_path(), None);

    let insensitive = MemoryCompilerHost::builder("/")
        .case_sensitive(false)
        .file(
            "/Node_Modules/Inner/package.json",
            br#"{"name":"inner","exports":{"./x":"./index.js"}}"#.to_vec(),
        )
        .file(
            "/Node_Modules/Inner/index.d.ts",
            b"export const x: true;".to_vec(),
        )
        .build()
        .expect("build case-insensitive host");
    let mut resolver = ModuleResolver::new(&insensitive, &options).expect("create resolver");
    let external = resolved(
        resolver
            .resolve(Path::new("/index.mts"), "inner/x", ResolutionMode::EsNext)
            .expect("resolve case-insensitive external package"),
    );
    assert_eq!(external.original_path(), None);
    assert_eq!(
        external.resolved_file().display(),
        Path::new("/node_modules/inner/index.d.ts")
    );
}

#[test]
fn self_reference_misses_continue_to_node_modules_but_null_stays_terminal() {
    let host = MemoryCompilerHost::builder("/work/package")
        .file(
            "/work/package/package.json",
            br#"{
                "name":"same-name",
                "exports": {
                    "./missing-file":"./missing.js",
                    "./blocked":null
                }
            }"#
            .to_vec(),
        )
        .file("/work/package/src/index.mts", b"export {};".to_vec())
        .file(
            "/work/package/node_modules/same-name/package.json",
            br#"{
                "name":"same-name",
                "exports": {
                    "./unmapped":"./index.js",
                    "./missing-file":"./index.js",
                    "./blocked":"./index.js"
                }
            }"#
            .to_vec(),
        )
        .file(
            "/work/package/node_modules/same-name/index.d.ts",
            b"export const external: true;".to_vec(),
        )
        .build()
        .expect("build self-reference fallback host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    for specifier in ["same-name/unmapped", "same-name/missing-file"] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/package/src/index.mts"),
                    specifier,
                    ResolutionMode::EsNext,
                )
                .expect("ordinary self-reference miss falls through"),
        );
        assert!(module.is_external_library_import());
        assert_eq!(
            module.resolved_file().canonical().as_path(),
            Path::new("/work/package/node_modules/same-name/index.d.ts")
        );
    }

    assert_eq!(
        resolver
            .resolve(
                Path::new("/work/package/src/index.mts"),
                "same-name/blocked",
                ResolutionMode::EsNext,
            )
            .expect("explicit null self-reference is an authoritative miss"),
        ResolutionOutcome::NotFound
    );
}

#[test]
fn directory_export_targets_require_a_trailing_slash_before_appending_subpaths() {
    let host = MemoryCompilerHost::builder("/")
        .file("/index.mts", b"export {};".to_vec())
        .file(
            "/node_modules/pkg/package.json",
            br#"{"name":"pkg","exports":{"./foo/":"./bar.js"}}"#.to_vec(),
        )
        // Concatenating the invalid target and subpath would produce this
        // false hit (`./bar.js` + `x` => `./bar.jsx`).
        .file("/node_modules/pkg/bar.jsx", b"export {};".to_vec())
        .build()
        .expect("build invalid directory-target host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    assert_eq!(
        resolver
            .resolve(Path::new("/index.mts"), "pkg/foo/x", ResolutionMode::EsNext,)
            .expect("invalid directory target is an ordinary miss"),
        ResolutionOutcome::NotFound
    );
}

#[test]
fn empty_export_array_continues_to_later_matching_conditions() {
    let host = MemoryCompilerHost::builder("/")
        .file("/index.mts", b"export {};".to_vec())
        .file(
            "/node_modules/pkg/package.json",
            br#"{
                "name":"pkg",
                "exports": {
                    ".": {
                        "types":[],
                        "default":"./index.js"
                    }
                }
            }"#
            .to_vec(),
        )
        .file("/node_modules/pkg/index.d.ts", b"export {};".to_vec())
        .build()
        .expect("build empty-array condition host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    let module = resolved(
        resolver
            .resolve(Path::new("/index.mts"), "pkg", ResolutionMode::EsNext)
            .expect("empty active-condition array continues"),
    );
    assert_eq!(
        module.resolved_file().canonical().as_path(),
        Path::new("/node_modules/pkg/index.d.ts")
    );
}

#[test]
fn types_versions_explicit_extensions_probe_exactly_before_loader_substitution() {
    let host = MemoryCompilerHost::builder("/")
        .file("/index.mts", b"export {};".to_vec())
        .file(
            "/node_modules/pkg/package.json",
            br#"{
                "name":"pkg",
                "version":"1.0.0",
                "typesVersions": {
                    "*": {
                        "prefer-exact":["./types/prefer.js"],
                        "exact-declaration":["./types/exact.d.ts"],
                        "fallback-after-miss":["./types/fallback.js"],
                        "missing":["./types/missing.js"]
                    }
                }
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/pkg/types/prefer.js",
            b"module.exports = {};".to_vec(),
        )
        // This declaration used to win the preferred pass before the exact
        // JavaScript substitution was checked.
        .file(
            "/node_modules/pkg/types/prefer.d.ts",
            b"export {};".to_vec(),
        )
        .file("/node_modules/pkg/types/exact.d.ts", b"export {};".to_vec())
        // An exact miss still enters the ordinary package loader, where the
        // written .js extension may substitute its declaration twin.
        .file(
            "/node_modules/pkg/types/fallback.d.ts",
            b"export {};".to_vec(),
        )
        .build()
        .expect("build explicit typesVersions target host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    let exact_js = resolved(
        resolver
            .resolve(
                Path::new("/index.mts"),
                "pkg/prefer-exact",
                ResolutionMode::EsNext,
            )
            .expect("resolve exact JavaScript substitution first"),
    );
    assert_eq!(exact_js.extension(), &ModuleExtension::Js);
    assert_eq!(
        exact_js.resolved_file().canonical().as_path(),
        Path::new("/node_modules/pkg/types/prefer.js")
    );
    assert_eq!(exact_js.package_id(), None);

    let exact_declaration = resolved(
        resolver
            .resolve(
                Path::new("/index.mts"),
                "pkg/exact-declaration",
                ResolutionMode::EsNext,
            )
            .expect("resolve exact declaration substitution"),
    );
    assert_eq!(exact_declaration.extension(), &ModuleExtension::Dts);
    assert_eq!(exact_declaration.package_id(), None);

    let fallback = resolved(
        resolver
            .resolve(
                Path::new("/index.mts"),
                "pkg/fallback-after-miss",
                ResolutionMode::EsNext,
            )
            .expect("fall through after an exact substitution miss"),
    );
    assert_eq!(fallback.extension(), &ModuleExtension::Dts);
    assert_eq!(fallback.package_id().map(PackageId::name), Some("pkg"));

    assert_eq!(
        resolver
            .resolve(
                Path::new("/index.mts"),
                "pkg/missing",
                ResolutionMode::EsNext,
            )
            .expect("missing exact and loader candidates are authoritative miss"),
        ResolutionOutcome::NotFound
    );
}

#[test]
fn manifestless_node_modules_probe_direct_files_and_commonjs_indexes_without_fake_facts() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/index.cts", b"export {};".to_vec())
        .file(
            "/work/node_modules/plain/direct.ts",
            b"export const direct = true;".to_vec(),
        )
        .file(
            "/work/node_modules/plain/folder/index.d.ts",
            b"export const folder: true;".to_vec(),
        )
        .file(
            "/work/node_modules/root-index/index.d.ts",
            b"export const root: true;".to_vec(),
        )
        .build()
        .expect("build manifestless node_modules host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    let direct = resolved(
        resolver
            .resolve(
                Path::new("/work/index.cts"),
                "plain/direct.ts",
                ResolutionMode::CommonJs,
            )
            .expect("resolve an explicit manifestless TypeScript subpath"),
    );
    assert_eq!(
        direct.resolved_file().canonical().as_path(),
        Path::new("/work/node_modules/plain/direct.ts")
    );
    assert!(direct.resolved_using_ts_extension());
    assert!(direct.is_external_library_import());
    assert_eq!(direct.package_id(), None);
    assert_eq!(direct.package_metadata(), None);

    for (specifier, expected) in [
        ("plain/folder", "/work/node_modules/plain/folder/index.d.ts"),
        ("root-index", "/work/node_modules/root-index/index.d.ts"),
    ] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/index.cts"),
                    specifier,
                    ResolutionMode::CommonJs,
                )
                .expect("resolve a CommonJS manifestless index"),
        );
        assert_eq!(
            module.resolved_file().canonical().as_path(),
            Path::new(expected)
        );
        assert_eq!(module.package_id(), None);
        assert_eq!(module.package_metadata(), None);
    }

    assert_eq!(
        resolver
            .resolve(
                Path::new("/work/index.cts"),
                "plain/folder",
                ResolutionMode::EsNext,
            )
            .expect("Node ESM does not perform a manifestless directory lookup"),
        ResolutionOutcome::NotFound
    );
}

#[test]
fn legacy_package_fields_preserve_priority_nonrecursive_main_and_node_esm_directory_rules() {
    let host = MemoryCompilerHost::builder("/")
        .file("/index.mts", b"export {};".to_vec())
        .file(
            "/node_modules/priority/package.json",
            br#"{
                "name":"priority",
                "version":"1.0.0",
                "typings":"typings.d.ts",
                "types":"types.d.ts",
                "main":"main.js"
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/priority/typings.d.ts",
            b"export {};".to_vec(),
        )
        .file("/node_modules/priority/types.d.ts", b"export {};".to_vec())
        .file(
            "/node_modules/priority/main.js",
            b"module.exports = {};".to_vec(),
        )
        .file(
            "/node_modules/first-field-miss/package.json",
            br#"{
                "name":"first-field-miss",
                "typings":"missing.d.ts",
                "types":"must-not-win.d.ts",
                "main":"must-not-win.js"
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/first-field-miss/must-not-win.d.ts",
            b"export {};".to_vec(),
        )
        .file(
            "/node_modules/first-field-miss/must-not-win.js",
            b"module.exports = {};".to_vec(),
        )
        .file(
            "/node_modules/first-field-miss/index.d.ts",
            b"export {};".to_vec(),
        )
        .file(
            "/node_modules/direct-root/package.json",
            br#"{
                "name":"direct-root",
                "version":"1.0.0",
                "type":"module",
                "types":"index.d.ts"
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/direct-root/index.d.ts",
            b"export {};".to_vec(),
        )
        .file("/node_modules/direct-root.ts", b"export {};".to_vec())
        .file(
            "/node_modules/nonrecursive/package.json",
            br#"{"name":"nonrecursive","main":"nested"}"#.to_vec(),
        )
        .file(
            "/node_modules/nonrecursive/nested/package.json",
            br#"{"main":"actual"}"#.to_vec(),
        )
        .file(
            "/node_modules/nonrecursive/nested/actual.js",
            b"module.exports = {};".to_vec(),
        )
        .file(
            "/node_modules/mode/package.json",
            br#"{
                "name":"mode",
                "version":"1.0.0",
                "type":"module",
                "main":"dist/index.js"
            }"#
            .to_vec(),
        )
        .file("/node_modules/mode/dist/index.d.ts", b"export {};".to_vec())
        .file(
            "/node_modules/mode/dist/dir/index.d.ts",
            b"export {};".to_vec(),
        )
        .build()
        .expect("build legacy package-field host");

    let bundler_options = CompilerOptions {
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };
    let mut bundler =
        ModuleResolver::new(&host, &bundler_options).expect("create Bundler resolver");
    let priority = resolved(
        bundler
            .resolve(Path::new("/index.mts"), "priority", ResolutionMode::EsNext)
            .expect("typings wins over types and main"),
    );
    assert_eq!(
        priority.resolved_file().canonical().as_path(),
        Path::new("/node_modules/priority/typings.d.ts")
    );
    assert_eq!(priority.package_id().map(PackageId::name), Some("priority"));
    let first_field_miss = resolved(
        bundler
            .resolve(
                Path::new("/index.mts"),
                "first-field-miss",
                ResolutionMode::EsNext,
            )
            .expect("a selected typings miss falls through to index, not types or main"),
    );
    assert_eq!(
        first_field_miss.resolved_file().canonical().as_path(),
        Path::new("/node_modules/first-field-miss/index.d.ts")
    );
    let direct_root = resolved(
        bundler
            .resolve(
                Path::new("/index.mts"),
                "direct-root",
                ResolutionMode::CommonJs,
            )
            .expect("CommonJS probes the direct package-root file before package fields"),
    );
    assert_eq!(
        direct_root.resolved_file().canonical().as_path(),
        Path::new("/node_modules/direct-root.ts")
    );
    let direct_root_id = direct_root
        .package_id()
        .expect("the package-root loader attaches the manifest package id");
    assert_eq!(direct_root_id.name(), "direct-root");
    assert_eq!(direct_root_id.submodule_name(), "ts");
    assert_eq!(
        bundler
            .resolve(
                Path::new("/index.mts"),
                "nonrecursive",
                ResolutionMode::CommonJs,
            )
            .expect("a main target does not recursively consume nested package.json"),
        ResolutionOutcome::NotFound
    );

    let node_options = options_for_module(100);
    let mut node = ModuleResolver::new(&host, &node_options).expect("create Node16 resolver");
    let root = resolved(
        node.resolve(Path::new("/index.mts"), "mode", ResolutionMode::EsNext)
            .expect("an explicit main target resolves in Node ESM mode"),
    );
    assert_eq!(
        root.resolved_file().canonical().as_path(),
        Path::new("/node_modules/mode/dist/index.d.ts")
    );
    assert_eq!(
        node.resolve(
            Path::new("/index.mts"),
            "mode/dist/dir",
            ResolutionMode::EsNext,
        )
        .expect("Node ESM forbids package-subpath directory lookup"),
        ResolutionOutcome::NotFound
    );
    let commonjs_directory = resolved(
        node.resolve(
            Path::new("/index.mts"),
            "mode/dist/dir",
            ResolutionMode::CommonJs,
        )
        .expect("Node CommonJS permits package-subpath directory lookup"),
    );
    assert_eq!(
        commonjs_directory.resolved_file().canonical().as_path(),
        Path::new("/node_modules/mode/dist/dir/index.d.ts")
    );
}

#[test]
fn types_versions_root_back_references_unmapped_fallback_and_mapped_misses_are_distinct() {
    let host = MemoryCompilerHost::builder("/")
        .file("/main.ts", b"export {};".to_vec())
        .file(
            "/node_modules/ext/package.json",
            br#"{
                "name":"ext",
                "version":"1.0.0",
                "types":"index",
                "typesVersions":{">=3.1.0-0":{"*":["ts3.1/*"]}}
            }"#
            .to_vec(),
        )
        .file("/node_modules/ext/index.d.ts", b"export {};".to_vec())
        .file("/node_modules/ext/other.d.ts", b"export {};".to_vec())
        .file("/node_modules/ext/ts3.1/index.d.ts", b"export {};".to_vec())
        .file("/node_modules/ext/ts3.1/other.d.ts", b"export {};".to_vec())
        .directory("/node_modules/ext/")
        .file(
            "/node_modules/unmapped/package.json",
            br#"{
                "name":"unmapped",
                "version":"1.0.0",
                "types":"index",
                "typesVersions":{">=3.1.0-0":{"index":["ts3.1/index"]}}
            }"#
            .to_vec(),
        )
        .file("/node_modules/unmapped/index.d.ts", b"export {};".to_vec())
        .file("/node_modules/unmapped/other.d.ts", b"export {};".to_vec())
        .file(
            "/node_modules/unmapped/ts3.1/index.d.ts",
            b"export {};".to_vec(),
        )
        .file(
            "/node_modules/mapped-miss/package.json",
            br#"{
                "name":"mapped-miss",
                "types":"index",
                "typesVersions":{"*":{"*":["missing/*","also-missing/*"]}}
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/mapped-miss/index.d.ts",
            b"export {};".to_vec(),
        )
        .file("/node_modules/mapped-miss/foo.d.ts", b"export {};".to_vec())
        .file(
            "/node_modules/first-range/package.json",
            br#"{
                "name":"first-range",
                "version":"1.0.0",
                "types":"index",
                "typesVersions":{
                    "*":{"index":["first/index"]},
                    ">=3.1":{"index":["second/index"]}
                }
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/first-range/first/index.d.ts",
            b"export {};".to_vec(),
        )
        .file(
            "/node_modules/first-range/second/index.d.ts",
            b"export {};".to_vec(),
        )
        .file(
            "/node_modules/root-exact/package.json",
            br#"{
                "name":"root-exact",
                "version":"1.0.0",
                "types":"index.d.ts",
                "typesVersions":{"*":{"index.d.ts":["types/root.d.ts"]}}
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/root-exact/types/root.d.ts",
            b"export {};".to_vec(),
        )
        .build()
        .expect("build typesVersions host");
    let options = options_for_module(1);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    for (specifier, expected) in [
        ("ext", "/node_modules/ext/ts3.1/index.d.ts"),
        ("ext/other", "/node_modules/ext/ts3.1/other.d.ts"),
        ("unmapped", "/node_modules/unmapped/ts3.1/index.d.ts"),
        ("unmapped/other", "/node_modules/unmapped/other.d.ts"),
        ("first-range", "/node_modules/first-range/first/index.d.ts"),
        ("root-exact", "/node_modules/root-exact/types/root.d.ts"),
    ] {
        let module = resolved(
            resolver
                .resolve(Path::new("/main.ts"), specifier, ResolutionMode::CommonJs)
                .expect("resolve versioned or legacy package target"),
        );
        assert_eq!(
            module.resolved_file().canonical().as_path(),
            Path::new(expected)
        );
        let expected_package = specifier.split('/').next().expect("package name");
        assert_eq!(
            module.package_id().map(PackageId::name),
            Some(expected_package)
        );
    }

    let self_back_reference = resolved(
        resolver
            .resolve(
                Path::new("/node_modules/ext/ts3.1/index.d.ts"),
                "../",
                ResolutionMode::CommonJs,
            )
            .expect("a package-root back-reference re-enters typesVersions"),
    );
    assert_eq!(
        self_back_reference.resolved_file().canonical().as_path(),
        Path::new("/node_modules/ext/ts3.1/index.d.ts")
    );
    assert_eq!(
        self_back_reference.package_id().map(PackageId::name),
        Some("ext")
    );
    let root_other = resolved(
        resolver
            .resolve(
                Path::new("/node_modules/ext/ts3.1/other.d.ts"),
                "../other",
                ResolutionMode::CommonJs,
            )
            .expect("an extensionless relative file resolves before directory metadata"),
    );
    assert_eq!(
        root_other.resolved_file().canonical().as_path(),
        Path::new("/node_modules/ext/other.d.ts")
    );
    assert_eq!(root_other.package_id().map(PackageId::name), Some("ext"));

    for specifier in ["mapped-miss", "mapped-miss/foo"] {
        assert_eq!(
            resolver
                .resolve(Path::new("/main.ts"), specifier, ResolutionMode::CommonJs)
                .expect("a selected mapping owns an all-target miss"),
            ResolutionOutcome::NotFound,
            "{specifier} must not fall through to its legacy file"
        );
    }
}

#[test]
fn relative_directory_spellings_reenter_the_root_package_entry_field() {
    let host = MemoryCompilerHost::builder("/")
        .file(
            "/pkg/package.json",
            br#"{
                "name":"pkg",
                "version":"1.0.0",
                "types":"entry.d.ts",
                "typesVersions":{"*":{
                    "entry.d.ts":["good/index.d.ts"],
                    "index":["bad/index.d.ts"]
                }}
            }"#
            .to_vec(),
        )
        .file("/pkg/src/main.ts", b"export {};".to_vec())
        .file("/pkg/good/index.d.ts", b"export const good: true;".to_vec())
        .file("/pkg/bad/index.d.ts", b"export const bad: true;".to_vec())
        .directory("/pkg/")
        .build()
        .expect("build a package-root directory-spelling host");
    let options = CompilerOptions {
        module: Some(1),
        module_resolution: Some(2),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &options).expect("create Node10 resolver");

    for specifier in ["..", "../"] {
        let outcome = resolver
            .resolve(
                Path::new("/pkg/src/main.ts"),
                specifier,
                ResolutionMode::CommonJs,
            )
            .expect("resolve a directory spelling of the package root");
        let ResolutionOutcome::Resolved(module) = outcome else {
            panic!("{specifier} returned {outcome:?}");
        };
        assert_eq!(
            module.resolved_file().display(),
            Path::new("/pkg/good/index.d.ts"),
            "{specifier} must use the package entry as its typesVersions logical name"
        );
        let package_id = module
            .package_id()
            .expect("the root package entry retains its package id");
        assert_eq!(package_id.name(), "pkg");
        assert_eq!(package_id.version(), "1.0.0");
        assert_eq!(package_id.submodule_name(), "ood/index.d.ts");
    }
}

#[test]
fn relative_package_ids_follow_file_and_directory_manifest_boundaries() {
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/package.json",
            br#"{"name":"workspace","version":"1.0.0"}"#.to_vec(),
        )
        .file("/work/index.ts", b"export {};".to_vec())
        .file("/work/other.ts", b"export {};".to_vec())
        .file(
            "/work/directory/package.json",
            br#"{"name":"directory","version":"1.0.0"}"#.to_vec(),
        )
        .file("/work/directory/index.d.ts", b"export {};".to_vec())
        .directory("/work/directory/")
        .file(
            "/work/node_modules/outer/package.json",
            br#"{"name":"outer","version":"1.0.0"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/outer/index.d.ts",
            b"export {};".to_vec(),
        )
        .file(
            "/work/node_modules/outer/nested/index.d.ts",
            b"export {};".to_vec(),
        )
        .directory("/work/node_modules/outer/nested/")
        .build()
        .expect("build relative workspace host");
    let options = options_for_module(1);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
    let module = resolved(
        resolver
            .resolve(
                Path::new("/work/index.ts"),
                "./other",
                ResolutionMode::CommonJs,
            )
            .expect("resolve an ordinary relative source"),
    );
    assert_eq!(module.package_id(), None);
    assert!(!module.is_external_library_import());

    let directory_package = resolved(
        resolver
            .resolve(
                Path::new("/work/index.ts"),
                "./directory/",
                ResolutionMode::CommonJs,
            )
            .expect("resolve a relative directory with its own package manifest"),
    );
    assert_eq!(
        directory_package.package_id().map(PackageId::name),
        Some("directory")
    );
    assert!(!directory_package.is_external_library_import());

    let manifestless_directory = resolved(
        resolver
            .resolve(
                Path::new("/work/node_modules/outer/index.d.ts"),
                "./nested/",
                ResolutionMode::CommonJs,
            )
            .expect("resolve a manifestless directory index inside a package"),
    );
    assert_eq!(manifestless_directory.package_id(), None);
    assert!(manifestless_directory.is_external_library_import());
}

#[test]
fn relative_directory_spellings_skip_direct_files_for_modules_and_type_references() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file("/work/dir.ts", b"export const wrong: true;".to_vec())
        .file("/work/dir.d.ts", b"declare const wrongType: true;".to_vec())
        .directory("/work/dir/")
        .file(
            "/work/dir/index.ts",
            b"export const selected: true;".to_vec(),
        )
        .file(
            "/work/dir/index.d.ts",
            b"declare const selectedType: true;".to_vec(),
        )
        .build()
        .expect("build relative trailing-directory host");
    let options = CompilerOptions {
        module: Some(1),
        module_resolution: Some(2),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    let module = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "./dir/",
                ResolutionMode::CommonJs,
            )
            .expect("resolve an explicitly trailing module directory"),
    );
    assert_eq!(
        module.resolved_file().display(),
        Path::new("/work/dir/index.ts")
    );

    let empty_type_roots = Vec::<ProgramPath>::new();
    let ResolutionOutcome::Resolved(reference) = resolver
        .resolve_type_reference(
            Path::new("/work/main.ts"),
            "./dir/",
            ResolutionMode::CommonJs,
            Some(&empty_type_roots),
        )
        .expect("resolve an explicitly trailing type-reference directory")
    else {
        panic!("expected a relative directory type reference");
    };
    assert_eq!(
        reference.resolved_file().display(),
        Path::new("/work/dir/index.d.ts")
    );
    assert!(!reference.primary());
}

#[test]
fn custom_type_roots_preserve_combine_paths_spelling_and_rooted_children() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file("/types.d.ts", b"declare const wrong: true;".to_vec())
        .file("/types/.d..ts", b"declare const dot: true;".to_vec())
        .file("/absolute.d.ts", b"declare const absolute: true;".to_vec())
        .build()
        .expect("build raw custom-type-root host");
    let options = options_for_module(1);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
    let type_root = program_path("/types");

    let ResolutionOutcome::Resolved(dot) = resolver
        .resolve_type_reference(
            Path::new("/work/main.ts"),
            ".",
            ResolutionMode::CommonJs,
            Some(std::slice::from_ref(&type_root)),
        )
        .expect("preserve a final-dot primary candidate")
    else {
        panic!("expected the raw final-dot declaration twin");
    };
    assert_eq!(dot.resolved_file().display(), Path::new("/types/.d..ts"));
    assert!(dot.primary());

    let ResolutionOutcome::Resolved(absolute) = resolver
        .resolve_type_reference(
            Path::new("/work/main.ts"),
            "/absolute",
            ResolutionMode::CommonJs,
            Some(std::slice::from_ref(&type_root)),
        )
        .expect("a rooted type name replaces its configured type root")
    else {
        panic!("expected the rooted custom-type-root candidate");
    };
    assert_eq!(
        absolute.resolved_file().display(),
        Path::new("/absolute.d.ts")
    );
    assert!(absolute.primary());
}

#[test]
fn non_relative_modules_use_explicit_type_roots_for_primary_and_alternate_results() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.mts", b"export {};".to_vec())
        .file(
            "/custom/primary/package.json",
            br#"{"name":"primary","version":"1.0.0","types":"entry.d.ts"}"#.to_vec(),
        )
        .file(
            "/custom/primary/entry.d.ts",
            b"export const primary: true;".to_vec(),
        )
        .file(
            "/physical/primary/entry.d.ts",
            b"export const primary: true;".to_vec(),
        )
        .realpath("/custom/primary/entry.d.ts", "/physical/primary/entry.d.ts")
        .file(
            "/work/node_modules/alternate/package.json",
            br#"{"name":"alternate","version":"1.0.0","exports":"./index.js"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/alternate/index.js",
            b"exports.alternate = true;".to_vec(),
        )
        .file(
            "/physical/alternate/index.js",
            b"exports.alternate = true;".to_vec(),
        )
        .realpath(
            "/work/node_modules/alternate/index.js",
            "/physical/alternate/index.js",
        )
        .file(
            "/custom/alternate/package.json",
            br#"{"name":"alternate-types","version":"2.0.0","types":"legacy.d.ts"}"#.to_vec(),
        )
        .file(
            "/custom/alternate/legacy.d.ts",
            b"export const alternate: true;".to_vec(),
        )
        .build()
        .expect("build module typeRoots fallback host");
    let options = CompilerOptions {
        module_resolution: Some(99),
        ..CompilerOptions::default()
    };
    let program_options = ProgramOptions::default().with_type_roots(vec![program_path("/custom")]);
    let mut resolver = ModuleResolver::new_with_program_options(&host, &options, &program_options)
        .expect("create resolver with explicit typeRoots");

    let primary = resolved(
        resolver
            .resolve(
                Path::new("/work/main.mts"),
                "primary",
                ResolutionMode::EsNext,
            )
            .expect("resolve a module from explicit typeRoots"),
    );
    assert_eq!(
        primary.resolved_file().display(),
        Path::new("/physical/primary/entry.d.ts")
    );
    assert_eq!(
        primary.original_path().map(ProgramPath::display),
        Some(Path::new("/custom/primary/entry.d.ts"))
    );
    assert!(primary.is_external_library_import());

    let alternate = resolved(
        resolver
            .resolve(
                Path::new("/work/main.mts"),
                "alternate",
                ResolutionMode::EsNext,
            )
            .expect("resolve an exports implementation with a typeRoots alternate"),
    );
    assert_eq!(
        alternate.resolved_file().display(),
        Path::new("/physical/alternate/index.js")
    );
    assert_eq!(
        alternate.alternate_result().map(ProgramPath::display),
        Some(Path::new("/custom/alternate/legacy.d.ts"))
    );
}

#[test]
fn types_versions_ranges_follow_javascript_own_property_order() {
    let host = MemoryCompilerHost::builder("/")
        .file("/main.ts", b"export {};".to_vec())
        .file(
            "/node_modules/range-order/package.json",
            br#"{
                "name":"range-order",
                "version":"1.0.0",
                "typesVersions":{
                    "*":{"x":["generic.d.ts"]},
                    "6":{"x":["six.d.ts"]}
                }
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/range-order/generic.d.ts",
            b"export const generic: true;".to_vec(),
        )
        .file(
            "/node_modules/range-order/six.d.ts",
            b"export const six: true;".to_vec(),
        )
        .build()
        .expect("build JavaScript-property-order package");
    let options = options_for_module(1);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
    let module = resolved(
        resolver
            .resolve(
                Path::new("/main.ts"),
                "range-order/x",
                ResolutionMode::CommonJs,
            )
            .expect("select the numeric range before later string keys"),
    );
    assert_eq!(
        module.resolved_file().display(),
        Path::new("/node_modules/range-order/six.d.ts")
    );
}

#[test]
fn types_versions_targets_follow_javascript_array_like_iteration() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/string-target/package.json",
            br#"{
                "name":"string-target","version":"1.0.0",
                "typesVersions":{"*":{"x":"m"}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/string-target/m.ts",
            b"export const stringTarget: true;".to_vec(),
        )
        .file(
            "/work/node_modules/object-target/package.json",
            br#"{
                "name":"object-target","version":"1.0.0",
                "typesVersions":{"*":{"x":{"0":"m","length":"1"}}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/object-target/m.ts",
            b"export const objectTarget: true;".to_vec(),
        )
        .file(
            "/work/node_modules/boolean-length/package.json",
            br#"{
                "name":"boolean-length","version":"1.0.0",
                "typesVersions":{"*":{"x":{"0":"m","length":true}}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/boolean-length/m.ts",
            b"export const booleanLength: true;".to_vec(),
        )
        .file(
            "/work/node_modules/fractional-length/package.json",
            br#"{
                "name":"fractional-length","version":"1.0.0",
                "typesVersions":{"*":{"x":{
                    "0":"missing","1":"m","length":1.5
                }}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/fractional-length/m.ts",
            b"export const fractionalLength: true;".to_vec(),
        )
        .file(
            "/work/node_modules/early-array/package.json",
            br#"{
                "name":"early-array","version":"1.0.0",
                "typesVersions":{"*":{"x":["m",null]}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/early-array/m.ts",
            b"export const earlyArray: true;".to_vec(),
        )
        .file(
            "/work/node_modules/early-object/package.json",
            br#"{
                "name":"early-object","version":"1.0.0",
                "typesVersions":{"*":{"x":{"0":"m","length":"Infinity"}}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/early-object/m.ts",
            b"export const earlyObject: true;".to_vec(),
        )
        .file(
            "/work/node_modules/jsonc-object-prototype/package.json",
            br#"{/* force convertToJson */
                "name":"jsonc-object-prototype","version":"1.0.0",
                "typesVersions":{"*":{"x":{
                    "__proto__":{"0":"m","length":1}
                }}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/jsonc-object-prototype/m.ts",
            b"export const inheritedObjectTarget: true;".to_vec(),
        )
        .file(
            "/work/node_modules/jsonc-array-prototype/package.json",
            br#"{/* force convertToJson */
                "name":"jsonc-array-prototype","version":"1.0.0",
                "typesVersions":{"*":{"x":{"__proto__":["m"]}}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/jsonc-array-prototype/m.ts",
            b"export const inheritedArrayTarget: true;".to_vec(),
        )
        .file(
            "/work/node_modules/jsonc-array-string/package.json",
            br#"{/* force convertToJson */
                "name":"jsonc-array-string","version":"1.0.0",
                "typesVersions":{"*":{
                    "x*":[{"__proto__":{"__proto__":["m"]}}],
                    "join*":[{"__proto__":["m"],"join":null}],
                    "own*":[{"__proto__":["p"],"0":"o"}],
                    "gap*":[{"__proto__":["a"],"length":3}]
                }}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/jsonc-array-string/m.ts",
            b"export const inheritedArrayToString: true;".to_vec(),
        )
        .file(
            "/work/node_modules/jsonc-array-string/[object Object].ts",
            b"export const shadowedJoinFallback: true;".to_vec(),
        )
        .file(
            "/work/node_modules/jsonc-array-string/o.ts",
            b"export const ownIndexWins: true;".to_vec(),
        )
        .file(
            "/work/node_modules/jsonc-array-string/a,,.ts",
            b"export const sparseArrayJoin: true;".to_vec(),
        )
        .file(
            "/work/node_modules/jsonc-raw-array-methods/package.json",
            br#"{/* force convertToJson */
                "name":"jsonc-raw-array-methods","version":"1.0.0",
                "typesVersions":{"*":{
                    "match*":[{"__proto__":["a",".ts","b","c"]}],
                    "miss*":[{"__proto__":["a","b","c","d"]}],
                    "shadow*":[{
                        "__proto__":["a",".ts","b","c"],"indexOf":null
                    }]
                }}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/jsonc-raw-array-methods/a,.ts,b,c",
            b"export const rawInheritedIndexOf: true;".to_vec(),
        )
        .file(
            "/work/node_modules/jsonc-raw-array-methods/a,b,c,d.ts",
            b"export const rawInheritedIndexOfMiss: true;".to_vec(),
        )
        .file(
            "/work/node_modules/strict-proto-own/package.json",
            br#"{
                "name":"strict-proto-own","version":"1.0.0",
                "typesVersions":{"*":{"x":{
                    "__proto__":{"0":"m","length":1}
                }}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/strict-proto-own/m.ts",
            b"export const hiddenStrictProtoTarget: true;".to_vec(),
        )
        .file(
            "/work/node_modules/strict-proto-own/x.ts",
            b"export const hiddenStrictProtoFallback: true;".to_vec(),
        )
        .file(
            "/work/node_modules/exact-false/package.json",
            br#"{
                "name":"exact-false","version":"1.0.0","types":"root.d.ts",
                "typesVersions":{"*":{"x":[false]}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/exact-false/root.d.ts",
            b"export const exactFalse: true;".to_vec(),
        )
        .file(
            "/work/node_modules/exact-zero/package.json",
            br#"{
                "name":"exact-zero","version":"1.0.0","types":"root.d.ts",
                "typesVersions":{"*":{"x":[0]}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/exact-zero/root.d.ts",
            b"export const exactZero: true;".to_vec(),
        )
        .file(
            "/work/node_modules/wildcard-coercion/package.json",
            br#"{
                "name":"wildcard-coercion","version":"1.0.0",
                "typesVersions":{"*":{
                    "false*":[false],"true*":[true],"zero*":[0],"one*":[1],
                    "object*":[{}],"array*":[["m"]],
                    "badObject*":[{"length":4}]
                }}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/wildcard-coercion/false.ts",
            b"export const coercedFalse: true;".to_vec(),
        )
        .file(
            "/work/node_modules/wildcard-coercion/true.ts",
            b"export const coercedTrue: true;".to_vec(),
        )
        .file(
            "/work/node_modules/wildcard-coercion/0.ts",
            b"export const coercedZero: true;".to_vec(),
        )
        .file(
            "/work/node_modules/wildcard-coercion/1.ts",
            b"export const coercedOne: true;".to_vec(),
        )
        .file(
            "/work/node_modules/wildcard-coercion/[object Object].ts",
            b"export const coercedObject: true;".to_vec(),
        )
        .file(
            "/work/node_modules/wildcard-coercion/m.ts",
            b"export const coercedArray: true;".to_vec(),
        )
        .file(
            "/work/node_modules/replacement-tokens/package.json",
            br#"{
                "name":"replacement-tokens","version":"1.0.0",
                "typesVersions":{"*":{"*":["types/*"]}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/replacement-tokens/types/*.ts",
            b"export const wholeMatch: true;".to_vec(),
        )
        .file(
            "/work/node_modules/replacement-tokens/types/$.ts",
            b"export const escapedDollar: true;".to_vec(),
        )
        .file(
            "/work/node_modules/replacement-tokens/types/types/.ts",
            b"export const matchPrefix: true;".to_vec(),
        )
        .file(
            "/work/node_modules/extension-only/package.json",
            br#"{
                "name":"extension-only","version":"1.0.0",
                "typesVersions":{"*":{"x":[".ts"],"y":[".d.ts"]}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/extension-only/.ts",
            b"export const tsExtensionOnly: true;".to_vec(),
        )
        .file(
            "/work/node_modules/extension-only/.d.ts",
            b"export const dtsExtensionOnly: true;".to_vec(),
        )
        .file(
            "/work/node_modules/infinite-substitution/package.json",
            br#"{/* JSONC fallback */
                "name":"infinite-substitution","version":"1.0.0",
                "typesVersions":{"*":{
                    "positive*":[1e309],"negative*":[-1e309]
                }}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/infinite-substitution/Infinity.ts",
            b"export const positiveInfinity: true;".to_vec(),
        )
        .file(
            "/work/node_modules/infinite-substitution/-Infinity.ts",
            b"export const negativeInfinity: true;".to_vec(),
        )
        .file(
            "/work/node_modules/inf-length/package.json",
            br#"{
                "name":"inf-length","version":"1.0.0",
                "typesVersions":{"*":{"x":{"0":"m","length":"inf"}}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/inf-length/m.ts",
            b"export const hiddenInfTarget: true;".to_vec(),
        )
        .file(
            "/work/node_modules/inf-length/x.ts",
            b"export const hiddenInfFallback: true;".to_vec(),
        )
        .file(
            "/work/node_modules/nel-length/package.json",
            br#"{
                "name":"nel-length","version":"1.0.0",
                "typesVersions":{"*":{"x":{"0":"m","length":"\u00851"}}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/nel-length/m.ts",
            b"export const hiddenNelTarget: true;".to_vec(),
        )
        .file(
            "/work/node_modules/nel-length/x.ts",
            b"export const hiddenNelFallback: true;".to_vec(),
        )
        .file(
            "/work/node_modules/boolean-target/package.json",
            br#"{
                "name":"boolean-target","version":"1.0.0",
                "typesVersions":{"*":{"x":true}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/boolean-target/x.ts",
            b"export const hiddenBooleanFallback: true;".to_vec(),
        )
        .file(
            "/work/node_modules/number-target/package.json",
            br#"{
                "name":"number-target","version":"1.0.0",
                "typesVersions":{"*":{"x":1}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/number-target/x.ts",
            b"export const hiddenNumberFallback: true;".to_vec(),
        )
        .file(
            "/work/node_modules/object-miss/package.json",
            br#"{
                "name":"object-miss","version":"1.0.0",
                "typesVersions":{"*":{"x":{"0":"m"}}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/object-miss/x.ts",
            b"export const hiddenObjectFallback: true;".to_vec(),
        )
        .file(
            "/work/node_modules/plain-object-length/package.json",
            br#"{
                "name":"plain-object-length","version":"1.0.0",
                "typesVersions":{"*":{"x":{"0":"m","length":{}}}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/plain-object-length/x.ts",
            b"export const hiddenPlainObjectFallback: true;".to_vec(),
        )
        .file(
            "/work/node_modules/shadowed-to-string/package.json",
            br#"{
                "name":"shadowed-to-string","version":"1.0.0",
                "typesVersions":{"*":{"x":{
                    "0":"m","length":{"toString":null}
                }}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/shadowed-array-to-string/package.json",
            br#"{
                "name":"shadowed-array-to-string","version":"1.0.0",
                "typesVersions":{"*":{"x":{
                    "0":"m","length":[{"toString":"1"}]
                }}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/null-target/package.json",
            br#"{
                "name":"null-target","version":"1.0.0",
                "typesVersions":{"*":{"x":null}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/array-null/package.json",
            br#"{
                "name":"array-null","version":"1.0.0",
                "typesVersions":{"*":{"x":[null]}}
            }"#
            .to_vec(),
        )
        .build()
        .expect("build JavaScript array-like typesVersions packages");
    let options = CompilerOptions {
        module_resolution: Some(2),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &options).expect("create Node10 resolver");

    for package in [
        "string-target",
        "object-target",
        "boolean-length",
        "fractional-length",
        "early-array",
        "early-object",
        "jsonc-object-prototype",
        "jsonc-array-prototype",
    ] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.ts"),
                    &format!("{package}/x"),
                    ResolutionMode::CommonJs,
                )
                .expect("resolve an array-like typesVersions substitution"),
        );
        assert_eq!(
            module.resolved_file().display(),
            PathBuf::from(format!("/work/node_modules/{package}/m.ts"))
        );
    }

    let inherited_array_to_string = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "jsonc-array-string/xa",
                ResolutionMode::CommonJs,
            )
            .expect("coerce an object through inherited generic Array#join"),
    );
    assert_eq!(
        inherited_array_to_string.resolved_file().display(),
        Path::new("/work/node_modules/jsonc-array-string/m.ts")
    );

    let shadowed_join = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "jsonc-array-string/joinx",
                ResolutionMode::CommonJs,
            )
            .expect("fall back to Object#toString when generic Array#join is shadowed"),
    );
    assert_eq!(
        shadowed_join.resolved_file().display(),
        Path::new("/work/node_modules/jsonc-array-string/[object Object].ts")
    );

    for (request, expected) in [("ownx", "o.ts"), ("gapx", "a,,.ts")] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.ts"),
                    &format!("jsonc-array-string/{request}"),
                    ResolutionMode::CommonJs,
                )
                .expect("stringify an effective sparse generic Array#join projection"),
        );
        assert_eq!(
            module.resolved_file().display(),
            PathBuf::from(format!("/work/node_modules/jsonc-array-string/{expected}"))
        );
    }

    for (request, expected) in [("matchx", "a,.ts,b,c"), ("missx", "a,b,c,d.ts")] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.ts"),
                    &format!("jsonc-raw-array-methods/{request}"),
                    ResolutionMode::CommonJs,
                )
                .expect("apply inherited generic Array#indexOf to a raw substitution"),
        );
        assert_eq!(
            module.resolved_file().display(),
            PathBuf::from(format!(
                "/work/node_modules/jsonc-raw-array-methods/{expected}"
            ))
        );
    }
    assert!(matches!(
        resolver.resolve(
            Path::new("/work/main.ts"),
            "jsonc-raw-array-methods/shadowx",
            ResolutionMode::CommonJs,
        ),
        Err(ResolutionError::InvalidData(_))
    ));

    for package in ["exact-false", "exact-zero"] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.ts"),
                    &format!("{package}/x"),
                    ResolutionMode::CommonJs,
                )
                .expect("resolve a falsy exact substitution through the package root"),
        );
        assert_eq!(
            module.resolved_file().display(),
            PathBuf::from(format!("/work/node_modules/{package}/root.d.ts"))
        );
    }

    for (request, expected) in [
        ("falsex", "false.ts"),
        ("truex", "true.ts"),
        ("zerox", "0.ts"),
        ("onex", "1.ts"),
        ("objectx", "[object Object].ts"),
        ("arrayx", "m.ts"),
    ] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.ts"),
                    &format!("wildcard-coercion/{request}"),
                    ResolutionMode::CommonJs,
                )
                .expect("resolve a JavaScript-coerced wildcard substitution"),
        );
        assert_eq!(
            module.resolved_file().display(),
            PathBuf::from(format!("/work/node_modules/wildcard-coercion/{expected}"))
        );
    }

    for (request, expected) in [
        ("$&", "types/*.ts"),
        ("$$", "types/$.ts"),
        ("$`", "types/types/.ts"),
    ] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.ts"),
                    &format!("replacement-tokens/{request}"),
                    ResolutionMode::CommonJs,
                )
                .expect("resolve a JavaScript replacement-string token"),
        );
        assert_eq!(
            module.resolved_file().display(),
            PathBuf::from(format!("/work/node_modules/replacement-tokens/{expected}"))
        );
    }

    let extension_only_ts = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "extension-only/x",
                ResolutionMode::CommonJs,
            )
            .expect("resolve an extension-only substitution through the ordinary loader"),
    );
    assert_eq!(
        extension_only_ts.resolved_file().display(),
        Path::new("/work/node_modules/extension-only/.ts")
    );
    assert_eq!(extension_only_ts.extension(), &ModuleExtension::Ts);
    assert!(extension_only_ts.resolved_using_ts_extension());
    assert!(extension_only_ts.package_id().is_some());

    let extension_only_dts = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "extension-only/y",
                ResolutionMode::CommonJs,
            )
            .expect("resolve a declaration-looking extension through the ordinary loader"),
    );
    assert_eq!(
        extension_only_dts.resolved_file().display(),
        Path::new("/work/node_modules/extension-only/.d.ts")
    );
    assert_eq!(extension_only_dts.extension(), &ModuleExtension::Ts);
    assert!(!extension_only_dts.resolved_using_ts_extension());
    assert_eq!(extension_only_dts.package_id(), None);

    for (request, expected) in [("positivex", "Infinity.ts"), ("negativex", "-Infinity.ts")] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.ts"),
                    &format!("infinite-substitution/{request}"),
                    ResolutionMode::CommonJs,
                )
                .expect("coerce an overflowing JSON number like JavaScript"),
        );
        assert_eq!(
            module.resolved_file().display(),
            PathBuf::from(format!(
                "/work/node_modules/infinite-substitution/{expected}"
            ))
        );
    }

    for package in [
        "boolean-target",
        "number-target",
        "object-miss",
        "inf-length",
        "nel-length",
        "plain-object-length",
        "strict-proto-own",
    ] {
        assert_eq!(
            resolver
                .resolve(
                    Path::new("/work/main.ts"),
                    &format!("{package}/x"),
                    ResolutionMode::CommonJs,
                )
                .expect("an array-like value without length owns a miss"),
            ResolutionOutcome::NotFound
        );
    }

    for package in [
        "null-target",
        "array-null",
        "shadowed-to-string",
        "shadowed-array-to-string",
    ] {
        assert!(matches!(
            resolver.resolve(
                Path::new("/work/main.ts"),
                &format!("{package}/x"),
                ResolutionMode::CommonJs,
            ),
            Err(ResolutionError::InvalidData(_))
        ));
    }
    assert!(matches!(
        resolver.resolve(
            Path::new("/work/main.ts"),
            "wildcard-coercion/badObjectx",
            ResolutionMode::CommonJs,
        ),
        Err(ResolutionError::InvalidData(_))
    ));
}

#[test]
fn package_maps_apply_javascript_replacement_tokens_after_absolute_join() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.mts", b"export {};".to_vec())
        .file(
            "/work/node_modules/export-tokens/package.json",
            br#"{
                "name":"export-tokens","version":"1.0.0",
                "exports":{"./*":"./types/*.js"}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/export-tokens/types/*.ts",
            b"export const wholeMatch: true;".to_vec(),
        )
        .file(
            "/work/node_modules/export-tokens/types/$.ts",
            b"export const escapedDollar: true;".to_vec(),
        )
        .file(
            "/work/node_modules/export-tokens/types/work/node_modules/export-tokens/types/.ts",
            b"export const absolutePrefix: true;".to_vec(),
        )
        .build()
        .expect("build package-map replacement-token host");
    let options = CompilerOptions {
        module: Some(199),
        module_resolution: Some(99),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &options).expect("create NodeNext resolver");

    for (request, expected) in [
        ("$&", "/work/node_modules/export-tokens/types/*.ts"),
        ("$$", "/work/node_modules/export-tokens/types/$.ts"),
        (
            "$`",
            "/work/node_modules/export-tokens/types/work/node_modules/export-tokens/types/.ts",
        ),
    ] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.mts"),
                    &format!("export-tokens/{request}"),
                    ResolutionMode::EsNext,
                )
                .expect("resolve an exports replacement-string token"),
        );
        assert_eq!(module.resolved_file().display(), Path::new(expected));
    }
}

#[test]
fn javascript_replacement_expansion_fails_before_quadratic_allocation() {
    let target = format!("{}*", "a".repeat(2_048));
    let package_json = format!(
        r#"{{
            "name":"replacement-limit","version":"1.0.0",
            "typesVersions":{{"*":{{"*":["{target}"]}}}}
        }}"#
    );
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/replacement-limit/package.json",
            package_json.into_bytes(),
        )
        .build()
        .expect("build replacement expansion limit host");
    let options = CompilerOptions {
        module_resolution: Some(2),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &options).expect("create Node10 resolver");
    let capture = "$`".repeat(600);

    assert!(matches!(
        resolver.resolve(
            Path::new("/work/main.ts"),
            &format!("replacement-limit/{capture}"),
            ResolutionMode::CommonJs,
        ),
        Err(ResolutionError::ResourceLimit(_))
    ));
}

#[test]
fn malformed_types_versions_objects_fall_back_to_legacy_package_fields() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/top/package.json",
            br#"{
                "name":"top","version":"1.0.0",
                "types":"index.d.ts","typesVersions":false
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/top/index.d.ts",
            b"export const top: true;".to_vec(),
        )
        .file(
            "/work/node_modules/range/package.json",
            br#"{
                "name":"range","version":"1.0.0",
                "types":"index.d.ts","typesVersions":{"*":false}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/range/index.d.ts",
            b"export const range: true;".to_vec(),
        )
        .file(
            "/work/node_modules/array/package.json",
            br#"{
                "name":"array","version":"1.0.0",
                "typesVersions":[null,null,null,null,null,null,{"x":["mapped.d.ts"]}]
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/array/mapped.d.ts",
            b"export const array: true;".to_vec(),
        )
        .file(
            "/work/node_modules/inner-array/package.json",
            br#"{
                "name":"inner-array","version":"1.0.0",
                "typesVersions":{"*":[null,null,null,null,null,null,["mapped.d.ts"]]}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/inner-array/mapped.d.ts",
            b"export const innerArray: true;".to_vec(),
        )
        .build()
        .expect("build malformed typesVersions packages");
    let options = CompilerOptions {
        module_resolution: Some(2),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    for (specifier, expected) in [
        ("top", "/work/node_modules/top/index.d.ts"),
        ("range", "/work/node_modules/range/index.d.ts"),
        ("array/x", "/work/node_modules/array/mapped.d.ts"),
        (
            "inner-array/6",
            "/work/node_modules/inner-array/mapped.d.ts",
        ),
    ] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.ts"),
                    specifier,
                    ResolutionMode::CommonJs,
                )
                .expect("malformed typesVersions must not abort legacy resolution"),
        );
        assert_eq!(module.resolved_file().display(), Path::new(expected));
    }
}

#[test]
fn legacy_subpaths_honor_nested_packages_and_nested_types_versions_workers() {
    let host = MemoryCompilerHost::builder("/")
        .file("/main.ts", b"export {};".to_vec())
        .file(
            "/node_modules/nested/package.json",
            br#"{"name":"root","version":"1.0.0"}"#.to_vec(),
        )
        .file(
            "/node_modules/nested/sub/package.json",
            br#"{"name":"nested","version":"2.0.0","types":"entry.d.ts"}"#.to_vec(),
        )
        .file(
            "/node_modules/nested/sub/entry.d.ts",
            b"export const nested: true;".to_vec(),
        )
        .file(
            "/node_modules/mapped/package.json",
            br#"{
                "name":"mapped","version":"1.0.0",
                "typesVersions":{"*":{"sub":["mapped"],"index":["nested.d.ts"]}}
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/mapped/mapped/nested.d.ts",
            b"export const mapped: true;".to_vec(),
        )
        .file(
            "/node_modules/final-worker/package.json",
            br#"{
                "name":"final-worker","version":"1.0.0",
                "typesVersions":{"*":{"index":["nested.d.ts"]}}
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/final-worker/sub/nested.d.ts",
            b"export const finalWorker: true;".to_vec(),
        )
        .file(
            "/node_modules/owned-miss/package.json",
            br#"{
                "name":"owned-miss","version":"1.0.0",
                "typesVersions":{"*":{"sub":["mapped"],"index":["missing"]}}
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/owned-miss/mapped/index.d.ts",
            b"export const mustStayHidden: true;".to_vec(),
        )
        .build()
        .expect("build nested package and typesVersions workers");

    for options in [options_for_module(1), options_for_module(199)] {
        let mode = ResolutionMode::CommonJs;
        let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
        let nested = resolved(
            resolver
                .resolve(Path::new("/main.ts"), "nested/sub", mode)
                .expect("resolve a nested package boundary"),
        );
        assert_eq!(
            nested.resolved_file().display(),
            Path::new("/node_modules/nested/sub/entry.d.ts")
        );
        assert_eq!(nested.package_id().map(PackageId::name), Some("nested"));
    }

    let options = options_for_module(1);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
    for (specifier, expected) in [
        ("mapped/sub", "/node_modules/mapped/mapped/nested.d.ts"),
        (
            "final-worker/sub",
            "/node_modules/final-worker/sub/nested.d.ts",
        ),
    ] {
        let module = resolved(
            resolver
                .resolve(Path::new("/main.ts"), specifier, ResolutionMode::CommonJs)
                .expect("resolve through the subpath directory worker"),
        );
        assert_eq!(module.resolved_file().display(), Path::new(expected));
    }
    assert_eq!(
        resolver
            .resolve(
                Path::new("/main.ts"),
                "owned-miss/sub",
                ResolutionMode::CommonJs,
            )
            .expect("an inner mapping owns its miss"),
        ResolutionOutcome::NotFound
    );
}

#[test]
fn direct_legacy_files_use_actual_node_package_roots_without_local_scope_reads() {
    let forbidden_scope_read = HostError::new(
        HostErrorKind::Other,
        HostOperation::ReadFile,
        Some(PathBuf::from("/work/package.json")),
        "a local direct-file resolution must not read its ancestor manifest",
    );
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/package.json",
            br#"{"name":"workspace","version":"1.0.0"}"#.to_vec(),
        )
        .file("/work/main.ts", b"export {};".to_vec())
        .file("/work/local.ts", b"export const local: true;".to_vec())
        .file(
            "/work/node_modules/outer/package.json",
            br#"{"name":"outer","version":"1.0.0"}"#.to_vec(),
        )
        .file("/work/node_modules/outer/index.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/outer/nested/package.json",
            br#"{"name":"nested","version":"2.0.0"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/outer/nested/value.ts",
            b"export const value: true;".to_vec(),
        )
        .file(
            "/work/node_modules/direct/package.json",
            br#"{"name":"direct","version":"1.0.0"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/direct.ts",
            b"export const sibling: true;".to_vec(),
        )
        .file(
            "/work/node_modules/direct/index.d.ts",
            b"export const wrongIndex: true;".to_vec(),
        )
        .failure(forbidden_scope_read)
        .build()
        .expect("build direct-file provenance host");
    let options = CompilerOptions {
        module: Some(1),
        module_resolution: Some(2),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    let local = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "./local",
                ResolutionMode::CommonJs,
            )
            .expect("resolve a local direct file without reading package.json"),
    );
    assert_eq!(local.package_id(), None);

    let nested = resolved(
        resolver
            .resolve(
                Path::new("/work/node_modules/outer/index.ts"),
                "./nested/value",
                ResolutionMode::CommonJs,
            )
            .expect("attach the actual node_modules package root after a direct hit"),
    );
    assert_eq!(nested.package_id().map(PackageId::name), Some("outer"));
    assert_eq!(
        nested.package_id().map(PackageId::submodule_name),
        Some("nested/value.ts")
    );

    let sibling = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "direct",
                ResolutionMode::Unspecified,
            )
            .expect("root direct file precedes the package directory worker"),
    );
    assert_eq!(
        sibling.resolved_file().display(),
        Path::new("/work/node_modules/direct.ts")
    );
    assert_eq!(sibling.package_id().map(PackageId::name), Some("direct"));
}

#[test]
fn optional_direct_files_preserve_parse_node_module_package_slices() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/package.json",
            br#"{"name":"root-package","version":"1.0.0"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/foo.ts",
            b"export const foo: true;".to_vec(),
        )
        .file(
            "/work/node_modules/@scope/package.json",
            br#"{"name":"scope-package","version":"2.0.0"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/@scope/pkg.ts",
            b"export const pkg: true;".to_vec(),
        )
        .file(
            "/work/node_modules/@scope.ts",
            b"export const scope: true;".to_vec(),
        )
        .build()
        .expect("build parseNodeModuleFromPath direct-file host");
    let options = CompilerOptions {
        module_resolution: Some(2),
        ..CompilerOptions::default()
    };
    let program_options = ProgramOptions::default().with_paths(vec![
        PathMapping::new("foo-file", vec!["node_modules/foo".to_owned()]),
        PathMapping::new(
            "scoped-package-file",
            vec!["node_modules/@scope/pkg".to_owned()],
        ),
        PathMapping::new("scope-file", vec!["node_modules/@scope".to_owned()]),
    ]);
    let mut resolver = ModuleResolver::new_with_program_options(&host, &options, &program_options)
        .expect("create direct-file package-slice resolver");

    for (specifier, expected_path, package_name, submodule) in [
        (
            "foo-file",
            "/work/node_modules/foo.ts",
            "root-package",
            "oo.ts",
        ),
        (
            "scoped-package-file",
            "/work/node_modules/@scope/pkg.ts",
            "scope-package",
            "pkg.ts",
        ),
        (
            "scope-file",
            "/work/node_modules/@scope.ts",
            "root-package",
            "scope.ts",
        ),
    ] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.ts"),
                    specifier,
                    ResolutionMode::Unspecified,
                )
                .expect("resolve an optional direct node_modules file"),
        );
        assert_eq!(module.resolved_file().display(), Path::new(expected_path));
        let package_id = module.package_id().expect("attach the parsed package id");
        assert_eq!(package_id.name(), package_name);
        assert_eq!(package_id.submodule_name(), submodule);
    }
}

#[test]
fn bare_package_trailing_separators_retain_candidate_and_package_id_spelling() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .directory("/work/node_modules/pkg/")
        .file(
            "/work/node_modules/pkg/package.json",
            br#"{"name":"pkg","version":"1.0.0","types":"index.d.ts"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/pkg/index.d.ts",
            b"export const pkg: true;".to_vec(),
        )
        .build()
        .expect("build trailing bare-package host");

    for module_resolution in [2, 100] {
        let options = CompilerOptions {
            module_resolution: Some(module_resolution),
            ..CompilerOptions::default()
        };
        let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.ts"),
                    "pkg/",
                    ResolutionMode::Unspecified,
                )
                .expect("resolve a bare package with a trailing separator"),
        );
        assert_eq!(
            module.resolved_file().display(),
            Path::new("/work/node_modules/pkg/index.d.ts")
        );
        let package_id = module.package_id().expect("attach package id");
        assert_eq!(package_id.name(), "pkg");
        assert_eq!(package_id.submodule_name(), "ndex.d.ts");
    }
}

#[test]
fn external_relative_root_dirs_keep_lexical_paths_in_node_and_classic_modes() {
    let source = b"export const linked: true;".to_vec();
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/src/main.ts", b"export {};".to_vec())
        .file("/work/node_modules/pkg/value.ts", source.clone())
        .file("/store/value.ts", source)
        .realpath("/work/node_modules/pkg/value.ts", "/store/value.ts")
        .build()
        .expect("build rootDirs realpath host");
    let program_options = ProgramOptions::default().with_root_dirs(vec![
        program_path("/work/src"),
        program_path("/work/node_modules/pkg"),
    ]);

    for resolution_kind in [1, 2] {
        let options = CompilerOptions {
            module: Some(1),
            module_resolution: Some(resolution_kind),
            ..CompilerOptions::default()
        };
        let mut resolver =
            ModuleResolver::new_with_program_options(&host, &options, &program_options)
                .expect("create rootDirs resolver");
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/src/main.ts"),
                    "./value",
                    ResolutionMode::CommonJs,
                )
                .expect("resolve an external-relative rootDirs candidate"),
        );
        assert_eq!(
            module.resolved_file().display(),
            Path::new("/work/node_modules/pkg/value.ts"),
            "moduleResolution={resolution_kind}"
        );
        assert_eq!(module.original_path(), None);
        assert!(module.is_external_library_import());
    }
}

#[test]
fn package_manifest_read_json_accepts_jsonc_fields_and_retains_exact_text() {
    let exports_manifest = r#"{
        // TypeScript's readJson fallback accepts comments and trailing commas.
        "name": "jsonc-exports",
        "version": "1.2.3",
        "type": "module",
        "exports": { ".": "./dist/index.js", },
    }"#;
    let types_manifest = r#"{
        "name": "jsonc-types",
        "version": "2.0.0",
        "types": "./missing.d.ts",
        "types": "./actual.d.ts",
    }"#;
    let inherited_legacy_manifest = r#"{/* force convertToJson */
        "name":"inherited-legacy",
        "__proto__":{
            "typings":"./fake.d.ts","types":"./fake.d.ts","main":"./fake.js",
            "typesVersions":{"*":{"*":["fake"]}}
        },
    }"#;
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.mts", b"export {};".to_vec())
        .file(
            "/work/node_modules/jsonc-exports/package.json",
            exports_manifest.as_bytes().to_vec(),
        )
        .file(
            "/work/node_modules/jsonc-exports/dist/index.d.ts",
            b"export const value: true;".to_vec(),
        )
        .file(
            "/work/node_modules/jsonc-types/package.json",
            types_manifest.as_bytes().to_vec(),
        )
        .file(
            "/work/node_modules/jsonc-types/actual.d.ts",
            b"export const value: true;".to_vec(),
        )
        .file(
            "/work/node_modules/inherited-legacy/package.json",
            inherited_legacy_manifest.as_bytes().to_vec(),
        )
        .file(
            "/work/node_modules/inherited-legacy/fake.d.ts",
            b"export const forbiddenInheritedField: true;".to_vec(),
        )
        .file(
            "/work/node_modules/inherited-legacy/index.d.ts",
            b"export const ownFieldsOnly: true;".to_vec(),
        )
        .build()
        .expect("build JSONC package host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create JSONC resolver");

    for (specifier, expected, manifest, name, module_type) in [
        (
            "jsonc-exports",
            "/work/node_modules/jsonc-exports/dist/index.d.ts",
            exports_manifest,
            "jsonc-exports",
            PackageJsonType::Module,
        ),
        (
            "jsonc-types",
            "/work/node_modules/jsonc-types/actual.d.ts",
            types_manifest,
            "jsonc-types",
            PackageJsonType::Unspecified,
        ),
        (
            "inherited-legacy",
            "/work/node_modules/inherited-legacy/index.d.ts",
            inherited_legacy_manifest,
            "inherited-legacy",
            PackageJsonType::Unspecified,
        ),
    ] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.mts"),
                    specifier,
                    ResolutionMode::EsNext,
                )
                .expect("resolve JSONC package"),
        );
        assert_eq!(
            module.resolved_file().canonical().as_path(),
            Path::new(expected)
        );
        let metadata = module
            .package_metadata()
            .expect("JSONC package retains its manifest observation");
        assert_eq!(metadata.text(), manifest);
        assert_eq!(metadata.name(), Some(name));
        assert_eq!(metadata.module_type(), module_type);
    }
}

#[test]
fn exports_shape_uses_own_keys_without_rejecting_mixed_objects() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.mts", b"export {};".to_vec())
        .file(
            "/work/node_modules/mixed-root/package.json",
            br#"{
                "name":"mixed-root",
                "exports":{".":"./root.js","default":"./bad.js"}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/mixed-root/root.d.ts",
            b"export const mixedRoot: true;".to_vec(),
        )
        .file(
            "/work/node_modules/mixed-subpath/package.json",
            br#"{
                "name":"mixed-subpath",
                "exports":{"./x":"./x.js","default":"./bad.js"}
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/mixed-subpath/x.d.ts",
            b"export const forbiddenMixedSubpath: true;".to_vec(),
        )
        .file(
            "/work/node_modules/prototype-map/package.json",
            br#"{/* force convertToJson */
                "name":"prototype-map","exports":{
                    "__proto__":{"default":"./bad.js"},
                    "./x":"./x.js"
                },
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/prototype-map/x.d.ts",
            b"export const ownSubpath: true;".to_vec(),
        )
        .file(
            "/work/node_modules/nested-own-gate/package.json",
            br#"{/* force convertToJson */
                "name":"nested-own-gate",
                "__proto__":{"exports":{"./x":"./root.js"}},
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/nested-own-gate/root.d.ts",
            b"export const forbiddenInheritedRoot: true;".to_vec(),
        )
        .file(
            "/work/node_modules/nested-own-gate/x/package.json",
            br#"{"name":"nested","types":"./nested.d.ts"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/nested-own-gate/x/nested.d.ts",
            b"export const nestedPackage: true;".to_vec(),
        )
        .build()
        .expect("build exports own-key shape host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create NodeNext resolver");

    for (specifier, expected) in [
        ("mixed-root", "/work/node_modules/mixed-root/root.d.ts"),
        ("prototype-map/x", "/work/node_modules/prototype-map/x.d.ts"),
        (
            "nested-own-gate/x",
            "/work/node_modules/nested-own-gate/x/nested.d.ts",
        ),
    ] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.mts"),
                    specifier,
                    ResolutionMode::EsNext,
                )
                .expect("resolve an exports own-key shape"),
        );
        assert_eq!(module.resolved_file().display(), Path::new(expected));
    }

    assert_eq!(
        resolver
            .resolve(
                Path::new("/work/main.mts"),
                "mixed-subpath/x",
                ResolutionMode::EsNext,
            )
            .expect("a mixed subpath map is a regular miss, not invalid data"),
        ResolutionOutcome::NotFound
    );
}

#[test]
fn invalid_and_non_object_manifests_are_found_empty_package_scopes() {
    let manifests = [
        ("empty", ""),
        ("null", "null"),
        ("array", "[]"),
        ("primitive", "true"),
        ("malformed", r#"{"name":"malformed""#),
        ("unquoted", r#"{name:"unquoted"}"#),
        (
            "nested-invalid",
            r#"{"name":"nested-invalid","nested":{bad:true}}"#,
        ),
    ];
    let mut builder =
        MemoryCompilerHost::builder("/work").file("/work/main.ts", b"export {};".to_vec());
    for (package, manifest) in manifests {
        builder = builder
            .file(
                format!("/work/node_modules/{package}/package.json"),
                manifest.as_bytes().to_vec(),
            )
            .file(
                format!("/work/node_modules/{package}/index.d.ts"),
                b"export const value: true;".to_vec(),
            );
    }
    let host = builder.build().expect("build empty-semantics package host");
    let options = options_for_module(1);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create legacy resolver");

    for (package, manifest) in manifests {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.ts"),
                    package,
                    ResolutionMode::CommonJs,
                )
                .expect("an unreadable semantic object falls through to legacy index"),
        );
        assert_eq!(
            module.resolved_file().canonical().as_path(),
            PathBuf::from(format!("/work/node_modules/{package}/index.d.ts"))
        );
        let metadata = module
            .package_metadata()
            .expect("present manifest remains the nearest package scope");
        assert_eq!(metadata.text(), manifest);
        assert_eq!(metadata.name(), None);
        assert_eq!(metadata.version(), None);
        assert_eq!(metadata.module_type(), PackageJsonType::Unspecified);
    }
}

#[test]
fn invalid_manifest_text_does_not_hide_a_later_target_host_failure() {
    let target_failure = HostError::new(
        HostErrorKind::PermissionDenied,
        HostOperation::FileExists,
        Some(PathBuf::from("/work/node_modules/broken/index.d.ts")),
        "legacy index probe denied",
    );
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/broken/package.json",
            br#"{"name":"broken""#.to_vec(),
        )
        .file(
            "/work/node_modules/broken/index.d.ts",
            b"export const value: true;".to_vec(),
        )
        .failure(target_failure.clone())
        .build()
        .expect("build invalid-manifest failure host");
    let options = options_for_module(1);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create legacy resolver");
    let error = resolver
        .resolve(
            Path::new("/work/main.ts"),
            "broken",
            ResolutionMode::CommonJs,
        )
        .expect_err("the later legacy-index host failure must win");
    let ResolutionError::Host(actual) = error else {
        panic!("expected target host error, got {error:?}");
    };
    assert_eq!(actual, target_failure);
}

#[test]
fn a_manifest_that_disappears_after_file_exists_is_a_found_empty_scope() {
    let manifest = PathBuf::from("/work/node_modules/racy/package.json");
    let inner = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            manifest.clone(),
            br#"{"name":"racy","types":"./missing.d.ts"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/racy/index.d.ts",
            b"export const value: true;".to_vec(),
        )
        .build()
        .expect("build racy package host");
    let host = MissingManifestContentsHost { inner, manifest };
    let options = options_for_module(1);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create legacy resolver");
    let module = resolved(
        resolver
            .resolve(Path::new("/work/main.ts"), "racy", ResolutionMode::CommonJs)
            .expect("missing contents expose empty manifest fields"),
    );
    assert_eq!(
        module.resolved_file().canonical().as_path(),
        Path::new("/work/node_modules/racy/index.d.ts")
    );
    let metadata = module
        .package_metadata()
        .expect("fileExists keeps a package boundary despite the absent read");
    assert_eq!(metadata.text(), "");
    assert_eq!(metadata.name(), None);
}

#[test]
fn relative_json_requires_an_explicit_suffix_and_effective_json_resolution() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", b"export {};".to_vec())
        .file("/work/data.json", br#"{"value":1}"#.to_vec())
        .build()
        .expect("build relative JSON host");
    let enabled = CompilerOptions {
        module: Some(99),
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &enabled).expect("create Bundler resolver");

    let module = resolved(
        resolver
            .resolve(
                Path::new("/work/root.ts"),
                "./data.json",
                ResolutionMode::EsNext,
            )
            .expect("resolve an explicitly named JSON module"),
    );
    assert_eq!(module.extension(), &ModuleExtension::Json);
    assert_eq!(
        module.resolved_file().display(),
        Path::new("/work/data.json")
    );
    assert!(!module.is_external_library_import());
    assert!(!module.resolved_using_ts_extension());
    assert_eq!(module.original_path(), None);
    assert_eq!(
        resolver
            .resolve(Path::new("/work/root.ts"), "./data", ResolutionMode::EsNext,)
            .expect("extensionless relative requests exclude JSON"),
        ResolutionOutcome::NotFound
    );

    let disabled = CompilerOptions {
        resolve_json_module: Some(false),
        ..enabled
    };
    let mut resolver = ModuleResolver::new(&host, &disabled).expect("create disabled resolver");
    assert_eq!(
        resolver
            .resolve(
                Path::new("/work/root.ts"),
                "./data.json",
                ResolutionMode::EsNext,
            )
            .expect("disabled JSON resolution is an authoritative miss"),
        ResolutionOutcome::NotFound
    );
}

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use tsc_host::{CompilerHost, HostError, HostErrorKind, HostOperation, MemoryCompilerHost};
use tsc_program::{
    load_no_lib_program, plan_source_requests, CompilerOptions, HostResolvedModule, ModuleResolver,
    ModuleSuffix, PathMapping, ProgramLoadLimits, ProgramOptions, ProgramPath, ResolutionError,
    ResolutionMode, ResolutionOutcome,
};

struct RecordingFileHost {
    inner: MemoryCompilerHost,
    calls: RefCell<Vec<PathBuf>>,
    realpath_calls: RefCell<Vec<PathBuf>>,
}

struct FailingFileHost {
    inner: MemoryCompilerHost,
    fail_path: PathBuf,
    calls: RefCell<Vec<PathBuf>>,
    failure: HostError,
}

impl CompilerHost for RecordingFileHost {
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
        self.realpath_calls.borrow_mut().push(path.to_path_buf());
        self.inner.realpath(path)
    }
}

impl CompilerHost for FailingFileHost {
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
        if path == self.fail_path {
            return Err(self.failure.clone());
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
}

fn options(module_suffixes: Option<Vec<ModuleSuffix>>) -> CompilerOptions {
    CompilerOptions {
        module: Some(1),
        module_resolution: Some(2),
        resolve_json_module: Some(true),
        module_suffixes,
        ..CompilerOptions::default()
    }
}

fn program_path(path: &str) -> ProgramPath {
    ProgramPath::from_trusted_parts(path, path).expect("case-sensitive program path")
}

fn resolved_path(outcome: ResolutionOutcome<HostResolvedModule>) -> PathBuf {
    let ResolutionOutcome::Resolved(module) = outcome else {
        panic!("expected a resolved module")
    };
    module.resolved_file().display().to_path_buf()
}

fn resolve(
    host: &dyn CompilerHost,
    compiler_options: &CompilerOptions,
    program_options: &ProgramOptions,
    specifier: &str,
) -> ResolutionOutcome<HostResolvedModule> {
    ModuleResolver::new_with_program_options(host, compiler_options, program_options)
        .expect("construct suffix-aware resolver")
        .resolve(Path::new("/index.ts"), specifier, ResolutionMode::CommonJs)
        .expect("resolve suffix fixture")
}

#[test]
fn absent_empty_and_blank_lists_preserve_their_distinct_runtime_boundaries() {
    let host = MemoryCompilerHost::builder("/")
        .file("/index.ts", b"export {};".to_vec())
        .file("/foo.ts", b"export const base = 0;".to_vec())
        .build()
        .expect("ordinary suffix fixture");
    let program_options = ProgramOptions::default();

    for module_suffixes in [None, Some(Vec::new()), Some(vec![ModuleSuffix::value("")])] {
        assert_eq!(
            resolved_path(resolve(
                &host,
                &options(module_suffixes),
                &program_options,
                "./foo",
            )),
            Path::new("/foo.ts")
        );
    }

    assert!(matches!(
        resolve(
            &host,
            &options(Some(vec![ModuleSuffix::value(".ios")])),
            &program_options,
            "./foo",
        ),
        ResolutionOutcome::NotFound
    ));
}

#[test]
fn suffix_order_is_nested_inside_each_extension_candidate() {
    let host = RecordingFileHost {
        inner: MemoryCompilerHost::builder("/")
            .file("/index.ts", b"export {};".to_vec())
            .file("/foo.ts", b"export const base = 0;".to_vec())
            .file("/foo.ios.tsx", b"export const wrong = 0;".to_vec())
            .build()
            .expect("extension-major suffix fixture"),
        calls: RefCell::new(Vec::new()),
        realpath_calls: RefCell::new(Vec::new()),
    };
    let compiler_options = options(Some(vec![
        ModuleSuffix::value(".ios"),
        ModuleSuffix::value(""),
    ]));

    assert_eq!(
        resolved_path(resolve(
            &host,
            &compiler_options,
            &ProgramOptions::default(),
            "./foo",
        )),
        Path::new("/foo.ts")
    );
    assert_eq!(
        host.calls
            .borrow()
            .iter()
            .filter(|path| path.to_string_lossy().starts_with("/foo"))
            .cloned()
            .collect::<Vec<_>>(),
        [PathBuf::from("/foo.ios.ts"), PathBuf::from("/foo.ts")]
    );
}

#[test]
fn suffix_text_preserves_leading_and_trailing_whitespace() {
    let host = RecordingFileHost {
        inner: MemoryCompilerHost::builder("/")
            .file("/index.ts", b"export {};".to_vec())
            .file("/foo  .ios  .ts", b"export const selected = 0;".to_vec())
            .build()
            .expect("whitespace suffix fixture"),
        calls: RefCell::new(Vec::new()),
        realpath_calls: RefCell::new(Vec::new()),
    };

    assert_eq!(
        resolved_path(resolve(
            &host,
            &options(Some(vec![ModuleSuffix::value("  .ios  ")])),
            &ProgramOptions::default(),
            "./foo",
        )),
        Path::new("/foo  .ios  .ts")
    );
    assert!(host
        .calls
        .borrow()
        .contains(&PathBuf::from("/foo  .ios  .ts")));
}

#[test]
fn first_suffix_file_exists_error_stops_later_suffix_probes() {
    let fail_path = PathBuf::from("/foo.first.ts");
    let failure = HostError::new(
        HostErrorKind::PermissionDenied,
        HostOperation::FileExists,
        Some(fail_path.clone()),
        "first suffix probe is denied",
    );
    let host = FailingFileHost {
        inner: MemoryCompilerHost::builder("/")
            .file("/index.ts", b"export {};".to_vec())
            .file("/foo.second.ts", b"export const wrong = 0;".to_vec())
            .build()
            .expect("failing suffix fixture"),
        fail_path: fail_path.clone(),
        calls: RefCell::new(Vec::new()),
        failure: failure.clone(),
    };
    let compiler_options = options(Some(vec![
        ModuleSuffix::value(".first"),
        ModuleSuffix::value(".second"),
    ]));
    let error = ModuleResolver::new_with_program_options(
        &host,
        &compiler_options,
        &ProgramOptions::default(),
    )
    .expect("construct suffix-aware resolver")
    .resolve(Path::new("/index.ts"), "./foo", ResolutionMode::CommonJs)
    .expect_err("host failures must not be treated as missing suffixes");

    assert_eq!(error, ResolutionError::Host(failure));
    assert_eq!(
        host.calls
            .borrow()
            .iter()
            .filter(|path| path.to_string_lossy().starts_with("/foo"))
            .cloned()
            .collect::<Vec<_>>(),
        [fail_path]
    );
}

#[test]
fn ordered_suffixes_select_first_second_and_explicit_unsuffixed_fallbacks() {
    for (files, expected) in [
        (
            vec!["/foo-ios.ts", "/foo__native.ts", "/foo.ts"],
            "/foo-ios.ts",
        ),
        (vec!["/foo__native.ts", "/foo.ts"], "/foo__native.ts"),
        (vec!["/foo.ts"], "/foo.ts"),
    ] {
        let mut builder =
            MemoryCompilerHost::builder("/").file("/index.ts", b"export {};".to_vec());
        for file in files {
            builder = builder.file(file, b"export {};".to_vec());
        }
        let host = builder.build().expect("ordered suffix fixture");
        let compiler_options = options(Some(vec![
            ModuleSuffix::value("-ios"),
            ModuleSuffix::value("__native"),
            ModuleSuffix::value(""),
        ]));
        assert_eq!(
            resolved_path(resolve(
                &host,
                &compiler_options,
                &ProgramOptions::default(),
                "./foo",
            )),
            Path::new(expected)
        );
    }
}

#[test]
fn preserved_undefined_slots_use_javascript_string_coercion() {
    let host = MemoryCompilerHost::builder("/")
        .file("/index.ts", b"export {};".to_vec())
        .file("/fooundefined.ts", b"export const recovered = 0;".to_vec())
        .file("/foo.ts", b"export const base = 0;".to_vec())
        .build()
        .expect("undefined suffix fixture");
    assert_eq!(
        resolved_path(resolve(
            &host,
            &options(Some(vec![ModuleSuffix::Undefined])),
            &ProgramOptions::default(),
            "./foo",
        )),
        Path::new("/fooundefined.ts")
    );
}

#[test]
fn suffixes_cover_written_js_json_directory_package_and_paths_candidates() {
    let host = MemoryCompilerHost::builder("/")
        .file("/index.ts", b"export {};".to_vec())
        .file("/foo.ios.js", b"module.exports = {};".to_vec())
        .file("/data.ios.json", br#"{"ios":true}"#.to_vec())
        .file("/dir/index.ios.ts", b"export {};".to_vec())
        .file("/node_modules/pkg/index.ios.ts", b"export {};".to_vec())
        .file("/mapped/lib/index.ios.ts", b"export {};".to_vec())
        .build()
        .expect("official suffix surfaces");
    let compiler_options = options(Some(vec![ModuleSuffix::value(".ios")]));
    let program_options = ProgramOptions::default().with_paths(vec![PathMapping::new(
        "mapped",
        vec!["mapped/lib".to_owned()],
    )]);

    for (specifier, expected) in [
        ("./foo.js", "/foo.ios.js"),
        ("./data.json", "/data.ios.json"),
        ("./dir", "/dir/index.ios.ts"),
        ("pkg", "/node_modules/pkg/index.ios.ts"),
        ("mapped", "/mapped/lib/index.ios.ts"),
    ] {
        assert_eq!(
            resolved_path(resolve(
                &host,
                &compiler_options,
                &program_options,
                specifier,
            )),
            Path::new(expected),
            "{specifier}"
        );
    }
}

#[test]
fn explicit_paths_substitutions_publish_the_suffix_hit() {
    let host = MemoryCompilerHost::builder("/")
        .file("/index.ts", b"export {};".to_vec())
        .file("/mapped/foo.ios.ts", b"export {};".to_vec())
        .build()
        .expect("exact paths suffix fixture");
    let program_options = ProgramOptions::default().with_paths(vec![PathMapping::new(
        "mapped",
        vec!["mapped/foo.ts".to_owned()],
    )]);
    assert_eq!(
        resolved_path(resolve(
            &host,
            &options(Some(vec![ModuleSuffix::value(".ios")])),
            &program_options,
            "mapped",
        )),
        Path::new("/mapped/foo.ios.ts")
    );
}

#[test]
fn exact_package_json_types_field_keeps_typescripts_unsuffixed_result_quirk() {
    let host = RecordingFileHost {
        inner: MemoryCompilerHost::builder("/")
            .file("/index.ts", b"export {};".to_vec())
            .file(
                "/node_modules/typed/package.json",
                br#"{"name":"typed","types":"types/index.d.ts"}"#.to_vec(),
            )
            .file(
                "/node_modules/typed/types/index.ios.d.ts",
                b"export {};".to_vec(),
            )
            .build()
            .expect("package-field suffix fixture"),
        calls: RefCell::new(Vec::new()),
        realpath_calls: RefCell::new(Vec::new()),
    };
    let outcome = resolve(
        &host,
        &options(Some(vec![ModuleSuffix::value(".ios")])),
        &ProgramOptions::default(),
        "typed",
    );
    assert_eq!(
        resolved_path(outcome),
        Path::new("/node_modules/typed/types/index.d.ts"),
        "loadFileNameFromPackageJsonField uses tryFile only as an existence predicate"
    );
    assert!(host
        .calls
        .borrow()
        .contains(&PathBuf::from("/node_modules/typed/types/index.ios.d.ts")));
    assert!(host
        .calls
        .borrow()
        .contains(&PathBuf::from("/node_modules/typed/package.json")));
    assert!(host.calls.borrow().iter().all(|path| {
        path.file_name().and_then(|name| name.to_str()) != Some("package.ios.json")
    }));
    assert_eq!(
        host.realpath_calls.borrow().as_slice(),
        [PathBuf::from("/node_modules/typed/types/index.d.ts")],
        "the suffix hit is only an existence predicate; realpath sees the published unsuffixed candidate"
    );
}

#[test]
fn type_reference_primary_roots_use_the_same_suffix_probe() {
    let host = MemoryCompilerHost::builder("/")
        .file("/index.ts", b"export {};".to_vec())
        .file("/types/pkg/index.ios.d.ts", b"export {};".to_vec())
        .build()
        .expect("type-reference suffix fixture");
    let compiler_options = options(Some(vec![ModuleSuffix::value(".ios")]));
    let mut resolver = ModuleResolver::new(&host, &compiler_options).expect("type resolver");
    let roots = [program_path("/types")];
    let outcome = resolver
        .resolve_type_reference(
            Path::new("/index.ts"),
            "pkg",
            ResolutionMode::CommonJs,
            Some(&roots),
        )
        .expect("resolve suffixed type reference");
    let ResolutionOutcome::Resolved(reference) = outcome else {
        panic!("suffixed type reference must resolve")
    };
    assert_eq!(
        reference.resolved_file().display(),
        Path::new("/types/pkg/index.ios.d.ts")
    );
}

#[test]
fn recursive_loader_admits_only_the_selected_suffixed_source() {
    let host = MemoryCompilerHost::builder("/")
        .file(
            "/index.ts",
            b"import { selected } from './dep'; selected;".to_vec(),
        )
        .file(
            "/dep.native.ts",
            b"export const selected = 'native';".to_vec(),
        )
        .file("/dep.ts", b"export const selected = 'base';".to_vec())
        .build()
        .expect("loader moduleSuffixes fixture");
    let compiler_options = CompilerOptions {
        no_emit: Some(true),
        module: Some(1),
        module_resolution: Some(2),
        module_suffixes: Some(vec![ModuleSuffix::value(".native")]),
        ..CompilerOptions::default()
    };
    let program = load_no_lib_program(
        &host,
        &[PathBuf::from("/index.ts")],
        compiler_options,
        ProgramOptions::default()
            .with_no_lib(true)
            .with_types(Vec::new()),
        ProgramLoadLimits::new(16, 16, 8, 1 << 20, 1 << 20),
    )
    .expect("load suffixed dependency graph");

    assert_eq!(
        program
            .source_files()
            .iter()
            .map(|source| source.path().display().to_path_buf())
            .collect::<Vec<_>>(),
        [PathBuf::from("/dep.native.ts"), PathBuf::from("/index.ts")]
    );
    assert!(program
        .source_files()
        .iter()
        .all(|source| source.path().display() != Path::new("/dep.ts")));
}

#[test]
fn separator_suffix_classifies_the_selected_classic_path_before_realpath() {
    let selected = PathBuf::from("/foo/node_modules/native.ts");
    let host = RecordingFileHost {
        inner: MemoryCompilerHost::builder("/")
            .file("/index.ts", b"import 'foo';".to_vec())
            .file(&selected, b"export const native = true;".to_vec())
            .build()
            .expect("Classic separator suffix fixture"),
        calls: RefCell::new(Vec::new()),
        realpath_calls: RefCell::new(Vec::new()),
    };
    let compiler_options = CompilerOptions {
        module: Some(1),
        module_resolution: Some(1),
        module_suffixes: Some(vec![ModuleSuffix::value("/node_modules/native")]),
        ..CompilerOptions::default()
    };
    let outcome = resolve(&host, &compiler_options, &ProgramOptions::default(), "foo");
    let ResolutionOutcome::Resolved(module) = outcome else {
        panic!("separator-bearing Classic suffix must resolve")
    };
    assert_eq!(module.resolved_file().display(), selected);
    assert!(module.is_external_library_import());
    assert_eq!(host.realpath_calls.borrow().as_slice(), [selected]);
}

#[test]
fn relative_node_resolution_attaches_package_facts_from_the_selected_suffix_path() {
    let host = MemoryCompilerHost::builder("/")
        .file("/index.ts", b"import './dep';".to_vec())
        .file(
            "/dep/node_modules/pkg/package.json",
            br#"{"name":"pkg","version":"1.2.3"}"#.to_vec(),
        )
        .file(
            "/dep/node_modules/pkg/index.ts",
            b"export const selected = true;".to_vec(),
        )
        .build()
        .expect("relative package suffix fixture");
    let outcome = resolve(
        &host,
        &options(Some(vec![ModuleSuffix::value("/node_modules/pkg/index")])),
        &ProgramOptions::default(),
        "./dep",
    );
    let ResolutionOutcome::Resolved(module) = outcome else {
        panic!("relative separator-bearing suffix must resolve")
    };
    assert_eq!(
        module.resolved_file().display(),
        Path::new("/dep/node_modules/pkg/index.ts")
    );
    assert!(
        !module.is_external_library_import(),
        "Node relative externality is classified from the request path components"
    );
    let package_id = module
        .package_id()
        .expect("the selected node_modules path supplies package facts");
    assert_eq!(package_id.name(), "pkg");
    assert_eq!(package_id.version(), "1.2.3");
}

#[test]
fn type_reference_externality_and_package_facts_use_the_selected_suffix_path() {
    let selected = PathBuf::from("/types/pkg/node_modules/real/index.d.ts");
    let host = RecordingFileHost {
        inner: MemoryCompilerHost::builder("/")
            .file("/index.ts", b"export {};".to_vec())
            .file(
                "/types/pkg/node_modules/real/package.json",
                br#"{"name":"real","version":"4.5.6"}"#.to_vec(),
            )
            .file(&selected, b"export {};".to_vec())
            .build()
            .expect("type-reference separator suffix fixture"),
        calls: RefCell::new(Vec::new()),
        realpath_calls: RefCell::new(Vec::new()),
    };
    let compiler_options = options(Some(vec![ModuleSuffix::value("/node_modules/real/index")]));
    let mut resolver = ModuleResolver::new(&host, &compiler_options).expect("type resolver");
    let roots = [program_path("/types")];
    let outcome = resolver
        .resolve_type_reference(
            Path::new("/index.ts"),
            "pkg",
            ResolutionMode::CommonJs,
            Some(&roots),
        )
        .expect("resolve selected type-reference suffix path");
    let ResolutionOutcome::Resolved(reference) = outcome else {
        panic!("separator-bearing type-reference suffix must resolve")
    };
    assert_eq!(reference.resolved_file().display(), selected);
    assert!(reference.is_external_library_import());
    assert_eq!(
        reference
            .package_id()
            .expect("selected type-reference path supplies package facts")
            .name(),
        "real"
    );
    assert!(host.realpath_calls.borrow().contains(&selected));
}

#[test]
fn type_reference_directory_result_reclassifies_the_selected_index_suffix() {
    let selected = PathBuf::from("/types/pkg/index/node_modules/real.d.ts");
    let host = MemoryCompilerHost::builder("/")
        .file("/index.ts", b"export {};".to_vec())
        .file(&selected, b"export {};".to_vec())
        .build()
        .expect("type-reference index suffix fixture");
    let compiler_options = options(Some(vec![ModuleSuffix::value("/node_modules/real")]));
    let mut resolver = ModuleResolver::new(&host, &compiler_options).expect("type resolver");
    let roots = [program_path("/types")];
    let outcome = resolver
        .resolve_type_reference(
            Path::new("/index.ts"),
            "pkg",
            ResolutionMode::CommonJs,
            Some(&roots),
        )
        .expect("resolve selected type-reference index suffix path");
    let ResolutionOutcome::Resolved(reference) = outcome else {
        panic!("separator-bearing type-reference index suffix must resolve")
    };
    assert_eq!(reference.resolved_file().display(), selected);
    assert!(reference.is_external_library_import());
}

#[test]
fn loader_deduplicates_dot_segment_suffix_spellings_by_normalized_program_identity() {
    let shared_source = b"export const selected = true;".to_vec();
    let host = MemoryCompilerHost::builder("/")
        .file("/index.ts", b"import './foo'; import './native';".to_vec())
        .file("/foo/../native.ts", shared_source.clone())
        .file("/native.ts", shared_source)
        .build()
        .expect("dot-segment suffix fixture");
    let compiler_options = CompilerOptions {
        no_emit: Some(true),
        module: Some(1),
        module_resolution: Some(2),
        module_suffixes: Some(vec![
            ModuleSuffix::value("/../native"),
            ModuleSuffix::value(""),
        ]),
        ..CompilerOptions::default()
    };
    let program = load_no_lib_program(
        &host,
        &[PathBuf::from("/index.ts")],
        compiler_options,
        ProgramOptions::default()
            .with_no_lib(true)
            .with_types(Vec::new()),
        ProgramLoadLimits::new(16, 16, 8, 1 << 20, 1 << 20),
    )
    .expect("load canonicalized suffix aliases");

    assert_eq!(
        program
            .source_files()
            .iter()
            .filter(|source| source.path().canonical().as_path() == Path::new("/native.ts"))
            .count(),
        1
    );
    let root = program
        .source_files()
        .iter()
        .find(|source| source.path().display() == Path::new("/index.ts"))
        .expect("root source");
    let requests = plan_source_requests(root, program.compiler_options())
        .expect("re-plan root requests")
        .module_requests()
        .to_vec();
    assert_eq!(requests.len(), 2);
    let source_ids = requests
        .iter()
        .map(|key| {
            let ResolutionOutcome::Resolved(module) = program
                .resolutions()
                .require_module(key)
                .expect("module resolution row")
                .outcome()
            else {
                panic!("both aliases must resolve")
            };
            module.target().source().expect("loaded target source")
        })
        .collect::<Vec<_>>();
    assert_eq!(source_ids[0], source_ids[1]);
}

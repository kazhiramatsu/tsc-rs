//! One resolution per target of a path-relative request: requests from
//! several directories that name one file share the first's resolution (the
//! loader's per-target memo over `ModuleResolver::relative_request_target`),
//! and requests whose outcome can depend on more than the target resolve
//! one by one.

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../host/tests/support/scalar_query_bridge.rs"
));

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use tsc_host::{CompilerHost, HostError, MemoryCompilerHost};
use tsc_program::{
    load_no_lib_program, CompilerOptions, PreparedProgram, ProgramLoadLimits, ProgramOptions,
    ProgramPath, ResolutionMode, ResolutionOutcome, ResolvedModuleTarget,
};

use super::utf16_scalar_path::ScalarTestPath as _;

/// Records every existence question the loader and its resolver ask.
struct ProbeRecordingHost {
    inner: MemoryCompilerHost,
    file_probes: RefCell<Vec<PathBuf>>,
    directory_probes: RefCell<Vec<PathBuf>>,
}

impl CompilerHost for ProbeRecordingHost {
    scalar_host_query_bridge!();
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
        self.file_probes.borrow_mut().push(path.to_path_buf());
        self.inner.file_exists(path)
    }

    fn directory_exists(&self, path: &Path) -> Result<bool, HostError> {
        self.directory_probes.borrow_mut().push(path.to_path_buf());
        self.inner.directory_exists(path)
    }

    fn read_directory(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        self.inner.read_directory(path)
    }

    fn realpath(&self, path: &Path) -> Result<Option<PathBuf>, HostError> {
        self.inner.realpath(path)
    }
}

const ROOTS: [&str; 5] = [
    "/work/src/a/first.ts",
    "/work/src/b/second.ts",
    "/work/src/c/d/third.ts",
    "/work/src/e/fourth.ts",
    "/work/src/f/fifth.ts",
];

/// Three directories name `/work/src/lib/util` (two of them with the same
/// specifier text), two name a file that does not exist, and two name both
/// the file `/work/src/lib/mod` and the directory `/work/src/lib/mod/`, in
/// opposite orders.
fn host() -> ProbeRecordingHost {
    ProbeRecordingHost {
        inner: MemoryCompilerHost::builder("/work")
            .file(
                "/work/src/a/first.ts",
                b"import \"../lib/util\";\nimport \"../lib/missing\";\nexport {};\n".to_vec(),
            )
            .file(
                "/work/src/b/second.ts",
                b"import \"../lib/util\";\nimport \"../lib/missing\";\nexport {};\n".to_vec(),
            )
            .file(
                "/work/src/c/d/third.ts",
                b"import \"../../lib/util\";\nexport {};\n".to_vec(),
            )
            .file(
                "/work/src/e/fourth.ts",
                b"import \"../lib/mod\";\nimport \"../lib/mod/\";\nexport {};\n".to_vec(),
            )
            .file(
                "/work/src/f/fifth.ts",
                b"import \"../lib/mod/\";\nimport \"../lib/mod\";\nexport {};\n".to_vec(),
            )
            .file("/work/src/lib/util.ts", b"export {};\n".to_vec())
            .file("/work/src/lib/mod.ts", b"export {};\n".to_vec())
            .file("/work/src/lib/mod/index.ts", b"export {};\n".to_vec())
            .build()
            .expect("build the shared-target fixture"),
        file_probes: RefCell::new(Vec::new()),
        directory_probes: RefCell::new(Vec::new()),
    }
}

fn load(
    host: &ProbeRecordingHost,
    compiler_options: CompilerOptions,
    program_options: ProgramOptions,
) -> PreparedProgram {
    let roots = ROOTS.iter().map(PathBuf::from).collect::<Vec<_>>();
    load_no_lib_program(
        host,
        &roots,
        compiler_options,
        program_options.with_no_lib(true).with_types(Vec::new()),
        ProgramLoadLimits::new(64, 64, 16, 1 << 20, 1 << 20),
    )
    .expect("load the shared-target fixture")
}

fn compiler_options() -> CompilerOptions {
    CompilerOptions {
        no_emit: Some(true),
        ..CompilerOptions::default()
    }
}

fn program_path(value: &str) -> ProgramPath {
    ProgramPath::from_trusted_parts(value, value).expect("a case-sensitive fixture path")
}

fn probes_of(probes: &RefCell<Vec<PathBuf>>, path: &str) -> usize {
    probes
        .borrow()
        .iter()
        .filter(|probe| probe.as_path() == Path::new(path))
        .count()
}

/// Every source's resolution of `specifier`, as the selected file's path
/// (or `None` for an unresolved request).
fn resolved(program: &PreparedProgram, source: &str, specifier: &str) -> Option<PathBuf> {
    let path = program_path(source);
    let view = program
        .module_resolution_by_path(path.canonical(), specifier.into(), ResolutionMode::CommonJs)
        .or_else(|| {
            program.module_resolution_by_path(
                path.canonical(),
                specifier.into(),
                ResolutionMode::EsNext,
            )
        })
        .or_else(|| {
            program.module_resolution_by_path(
                path.canonical(),
                specifier.into(),
                ResolutionMode::Unspecified,
            )
        })
        .unwrap_or_else(|| panic!("{source} has a row for {specifier}"));
    match view.outcome() {
        ResolutionOutcome::Resolved(module) => match module.target() {
            ResolvedModuleTarget::Source { resolved_file, .. }
            | ResolvedModuleTarget::Unloaded { resolved_file, .. } => {
                Some(resolved_file.display().scalar_test_path().to_path_buf())
            }
        },
        ResolutionOutcome::NotFound => None,
    }
}

fn assert_outcomes(program: &PreparedProgram) {
    let util = Some(PathBuf::from("/work/src/lib/util.ts"));
    assert_eq!(resolved(program, ROOTS[0], "../lib/util"), util);
    assert_eq!(resolved(program, ROOTS[1], "../lib/util"), util);
    assert_eq!(resolved(program, ROOTS[2], "../../lib/util"), util);
    assert_eq!(resolved(program, ROOTS[0], "../lib/missing"), None);
    assert_eq!(resolved(program, ROOTS[1], "../lib/missing"), None);
    let module = Some(PathBuf::from("/work/src/lib/mod.ts"));
    assert_eq!(resolved(program, ROOTS[3], "../lib/mod"), module);
    assert_eq!(resolved(program, ROOTS[4], "../lib/mod"), module);
}

#[test]
fn directories_naming_one_target_share_its_resolution() {
    let shared = host();
    let program = load(&shared, compiler_options(), ProgramOptions::default());
    assert_outcomes(&program);

    // The same program with a `rootDirs` entry: a relative name then starts
    // from its containing directory, so every (directory, specifier) pair
    // resolves on its own. The root holds none of the fixture's files, so the
    // outcomes are the same.
    let separate = host();
    let with_root_dirs = load(
        &separate,
        compiler_options(),
        ProgramOptions::default().with_root_dirs(vec![program_path("/work/generated")]),
    );
    assert_outcomes(&with_root_dirs);

    // `util.ts` is the first candidate of the target `/work/src/lib/util`:
    // three directories ask for it, and one resolution probes it.
    let shared_util = probes_of(&shared.file_probes, "/work/src/lib/util.ts");
    let separate_util = probes_of(&separate.file_probes, "/work/src/lib/util.ts");
    assert!(
        separate_util >= shared_util + 2,
        "three directories resolve the target separately under rootDirs \
         ({separate_util} probes) and once without ({shared_util})"
    );
    // A miss is shared as a hit is: the second directory does not probe the
    // missing target's candidates again.
    let shared_missing = probes_of(&shared.file_probes, "/work/src/lib/missing.ts");
    let separate_missing = probes_of(&separate.file_probes, "/work/src/lib/missing.ts");
    assert_eq!(shared_missing, 1, "one resolution probes the missing file");
    assert!(
        separate_missing >= 2,
        "each directory probes the missing file under rootDirs ({separate_missing})"
    );
    // The parent directory of the target is asked about once per resolution.
    assert!(
        probes_of(&separate.directory_probes, "/work/src/lib")
            > probes_of(&shared.directory_probes, "/work/src/lib")
    );
}

#[test]
fn a_directory_spelling_is_its_own_target() {
    // `../lib/mod/` names the directory and `../lib/mod` the file beside it.
    // The memo's targets keep the trailing separator, so neither request
    // takes the other's resolution, whichever a directory asks first: the
    // shared program answers as the one that resolves every pair.
    let shared = host();
    let shared_program = load(&shared, compiler_options(), ProgramOptions::default());
    let separate = host();
    let separate_program = load(
        &separate,
        compiler_options(),
        ProgramOptions::default().with_root_dirs(vec![program_path("/work/generated")]),
    );
    let file = Some(PathBuf::from("/work/src/lib/mod.ts"));
    for root in [ROOTS[3], ROOTS[4]] {
        assert_eq!(resolved(&shared_program, root, "../lib/mod"), file);
        let directory = resolved(&separate_program, root, "../lib/mod/");
        assert_ne!(directory, file, "{root}: the directory is not the file");
        assert_eq!(
            resolved(&shared_program, root, "../lib/mod/"),
            directory,
            "{root}: the directory request keeps its own resolution"
        );
    }
}

#[test]
fn a_traced_program_resolves_every_request() {
    // A trace names the containing file of its request, so traced requests
    // are never shared: both directories probe the missing file.
    let traced = host();
    let program = load(
        &traced,
        CompilerOptions {
            trace_resolution: Some(true),
            ..compiler_options()
        },
        ProgramOptions::default(),
    );
    assert_outcomes(&program);
    assert!(
        probes_of(&traced.file_probes, "/work/src/lib/missing.ts") >= 2,
        "each traced request probes the missing file"
    );
}

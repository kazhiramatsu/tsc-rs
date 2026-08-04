#![cfg(unix)]

use std::fs;
use std::io;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tsc_compiler::ProgramSession;
use tsc_host::{FsCompilerHost, MemoryCompilerHost};
use tsc_program::{
    load_program, plan_source_requests, CompilerOptions, LibraryCatalog, PreparedProgram,
    ProgramLoadLimits, ProgramOptions, ProgramPath, ResolutionMode, ResolutionOutcome,
    ResolvedModuleTarget,
};

const UPSTREAM_FIXTURE_PATH: &str =
    "ts-tests/tests/cases/compiler/moduleResolutionWithSymlinks_preserveSymlinks.ts";
const UPSTREAM_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ts-tests/tests/cases/compiler/moduleResolutionWithSymlinks_preserveSymlinks.ts"
));
const PINNED_FIXTURE: &str = concat!(
    "// @target: es2015\n",
    "// @noImplicitReferences: true\n",
    "\n",
    "// @traceResolution: true\n",
    "// @preserveSymlinks: true\n",
    "// @moduleResolution: bundler\n",
    "// @filename: /linked/index.d.ts\n",
    "// @symlink: /app/node_modules/linked/index.d.ts,/app/node_modules/linked2/index.d.ts\n",
    "export { real } from \"real\";\n",
    "export class C { private x; }\n",
    "\n",
    "// @filename: /app/node_modules/real/index.d.ts\n",
    "export const real: string;\n",
    "\n",
    "// @filename: /app/app.ts\n",
    "// We shouldn't resolve symlinks for references either. See the trace.\n",
    "/// <reference types=\"linked\" />\n",
    "\n",
    "import { C as C1 } from \"linked\";\n",
    "import { C as C2 } from \"linked2\";\n",
    "\n",
    "let x = new C1();\n",
    "// Should fail. We no longer resolve any symlinks.\n",
    "x = new C2();\n",
);
const LINKED_SOURCE: &str = "export { real } from \"real\";\nexport class C { private x; }\n";
const REAL_SOURCE: &str = "export const real: string;\n";
const APP_SOURCE: &str = concat!(
    "// We shouldn't resolve symlinks for references either. See the trace.\n",
    "/// <reference types=\"linked\" />\n",
    "\n",
    "import { C as C1 } from \"linked\";\n",
    "import { C as C2 } from \"linked2\";\n",
    "\n",
    "let x = new C1();\n",
    "// Should fail. We no longer resolve any symlinks.\n",
    "x = new C2();\n",
);
static NEXT_TEMP_TREE: AtomicU64 = AtomicU64::new(0);

struct TempTree {
    root: PathBuf,
    cleanup_parent: PathBuf,
}

impl TempTree {
    fn new() -> Self {
        loop {
            let sequence = NEXT_TEMP_TREE.fetch_add(1, Ordering::Relaxed);
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock is after the Unix epoch")
                .as_nanos();
            let candidate = std::env::temp_dir().join(format!(
                "tsc-rs-preserve-symlinks-canary-{}-{timestamp}-{sequence}",
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

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let file_name = self.root.file_name().and_then(|name| name.to_str());
        assert_eq!(self.root.parent(), Some(self.cleanup_parent.as_path()));
        assert!(file_name.is_some_and(|name| name.starts_with("tsc-rs-preserve-symlinks-canary-")));
        if let Err(error) = fs::remove_dir_all(&self.root) {
            if !std::thread::panicking() {
                panic!("remove temp tree {}: {error}", self.root.display());
            }
        }
    }
}

fn temp_volume_is_case_sensitive(tree: &TempTree) -> bool {
    let lowercase = tree.path("case-profile-probe");
    let uppercase = tree.path("CASE-PROFILE-PROBE");
    fs::write(&lowercase, []).expect("write temp-volume case probe");
    let case_sensitive = !uppercase.exists();
    fs::remove_file(lowercase).expect("remove temp-volume case probe");
    case_sensitive
}

fn limits() -> ProgramLoadLimits {
    ProgramLoadLimits::new(256, 4_096, 64, 8 * 1_024 * 1_024, 64 * 1_024 * 1_024)
}

fn compiler_options() -> CompilerOptions {
    CompilerOptions {
        no_emit: Some(true),
        target: Some(2),
        module_resolution: Some(100),
        ..CompilerOptions::default()
    }
}

fn program_options(preserve_symlinks: bool) -> ProgramOptions {
    ProgramOptions::default()
        .with_preserve_symlinks(preserve_symlinks)
        .with_types(Vec::new())
}

fn workspace_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn declaration_library_files(directory: &Path) -> Vec<PathBuf> {
    let mut files = fs::read_dir(directory)
        .expect("read vendored TypeScript library directory")
        .map(|entry| entry.expect("read vendored library entry").path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".d.ts"))
        })
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn source_paths(program: &PreparedProgram) -> Vec<PathBuf> {
    program
        .source_files()
        .iter()
        .map(|source| source.path().display().to_path_buf())
        .collect()
}

fn declaration_module_name(path: &Path) -> &str {
    path.to_str()
        .expect("fixture paths are UTF-8")
        .strip_suffix(".d.ts")
        .expect("fixture package entry is a declaration file")
}

fn assert_module_resolution(
    program: &PreparedProgram,
    containing_file: &Path,
    specifier: &str,
    expected_target: &Path,
    expected_original_path: Option<&Path>,
) -> tsc_program::SourceFileId {
    let source = program
        .source_files()
        .iter()
        .find(|source| source.path().display() == containing_file)
        .expect("containing source is owned");
    let key = plan_source_requests(source, program.compiler_options())
        .expect("plan source requests")
        .module_requests()
        .iter()
        .find(|key| key.specifier() == specifier)
        .expect("module request exists")
        .clone();
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
    assert_eq!(resolved_file.display(), expected_target);
    assert_eq!(
        program.source_file(*source).unwrap().path().display(),
        expected_target
    );
    assert_eq!(
        module.original_path().map(ProgramPath::display),
        expected_original_path
    );
    *source
}

fn assert_type_reference_resolution(
    program: &PreparedProgram,
    containing_file: &Path,
    specifier: &str,
    expected_target: &Path,
    expected_original_path: Option<&Path>,
) -> tsc_program::SourceFileId {
    let source = program
        .source_files()
        .iter()
        .find(|source| source.path().display() == containing_file)
        .expect("containing source is owned");
    let plan =
        plan_source_requests(source, program.compiler_options()).expect("plan source requests");
    let directive = plan
        .type_reference_directives()
        .iter()
        .find(|directive| directive.key().specifier() == specifier)
        .expect("type-reference request exists");
    assert_eq!(directive.key().mode(), ResolutionMode::Unspecified);
    let resolution = program
        .resolutions()
        .require_type_reference(directive.key())
        .expect("type-reference request has an authoritative row");
    let ResolutionOutcome::Resolved(directive) = resolution.outcome() else {
        panic!("type-reference request must resolve: {specifier}");
    };
    assert_eq!(directive.target().display(), expected_target);
    assert_eq!(
        program
            .source_file(directive.source())
            .unwrap()
            .path()
            .display(),
        expected_target
    );
    assert_eq!(
        directive.original_path().map(ProgramPath::display),
        expected_original_path
    );
    directive.source()
}

#[test]
fn preserve_symlinks_matches_the_upstream_session_contract_for_memory_and_filesystem_hosts() {
    assert_eq!(
        Path::new(UPSTREAM_FIXTURE_PATH)
            .file_name()
            .and_then(|name| name.to_str()),
        Some("moduleResolutionWithSymlinks_preserveSymlinks.ts")
    );
    assert_eq!(UPSTREAM_FIXTURE, PINNED_FIXTURE);

    let tree = TempTree::new();
    let physical_linked = tree.path("linked/index.d.ts");
    let app = tree.path("app");
    let app_source = tree.path("app/app.ts");
    let lexical_linked = tree.path("app/node_modules/linked/index.d.ts");
    let lexical_linked2 = tree.path("app/node_modules/linked2/index.d.ts");
    let real = tree.path("app/node_modules/real/index.d.ts");
    let vendored_library_directory =
        fs::canonicalize(workspace_path("vendor/typescript-6.0.3/lib"))
            .expect("canonicalize vendored TypeScript library directory");
    let library_directory = tree.path("typescript/lib");
    let library_package = tree.path("typescript/package.json");
    let fixture_package = tree.path("package.json");
    let library_catalog = LibraryCatalog::typescript_6_0_3(&library_directory);

    for directory in [
        tree.path("linked"),
        tree.path("app/node_modules/linked"),
        tree.path("app/node_modules/linked2"),
        tree.path("app/node_modules/real"),
        library_directory.clone(),
    ] {
        fs::create_dir_all(directory).expect("create official fixture directory");
    }
    for (path, text) in [
        (&physical_linked, LINKED_SOURCE),
        (&real, REAL_SOURCE),
        (&app_source, APP_SOURCE),
    ] {
        fs::write(path, text).expect("write official fixture source");
    }
    for source in declaration_library_files(&vendored_library_directory) {
        let destination = library_directory.join(
            source
                .file_name()
                .expect("vendored declaration has a file name"),
        );
        fs::copy(&source, destination).expect("copy vendored library into isolated temp tree");
    }
    fs::write(&library_package, b"{}\n").expect("write deterministic library package scope");
    fs::write(&fixture_package, b"{}\n").expect("isolate the fixture package scope");
    symlink(&physical_linked, &lexical_linked).expect("create linked package symlink");
    symlink(&physical_linked, &lexical_linked2).expect("create linked2 package symlink");

    let case_sensitive = temp_volume_is_case_sensitive(&tree);
    let filesystem = FsCompilerHost::new(&app, case_sensitive).expect("construct filesystem host");
    let mut memory = MemoryCompilerHost::builder(&app)
        .case_sensitive(case_sensitive)
        .file(&physical_linked, LINKED_SOURCE.as_bytes().to_vec())
        .file(&lexical_linked, LINKED_SOURCE.as_bytes().to_vec())
        .file(&lexical_linked2, LINKED_SOURCE.as_bytes().to_vec())
        .file(&real, REAL_SOURCE.as_bytes().to_vec())
        .file(&app_source, APP_SOURCE.as_bytes().to_vec())
        .realpath(&lexical_linked, &physical_linked)
        .realpath(&lexical_linked2, &physical_linked);
    for path in declaration_library_files(&library_directory) {
        memory = memory.file(
            &path,
            fs::read(&path).expect("read vendored library source"),
        );
    }
    memory = memory
        .file(&library_package, b"{}\n".to_vec())
        .file(&fixture_package, b"{}\n".to_vec());
    let memory = memory.build().expect("construct mirrored memory host");
    let roots = [app_source.clone()];

    let preserved_memory = load_program(
        &memory,
        &roots,
        compiler_options(),
        program_options(true),
        &library_catalog,
        limits(),
    )
    .expect("load preserved MemoryHost program");
    let preserved_filesystem = load_program(
        &filesystem,
        &roots,
        compiler_options(),
        program_options(true),
        &library_catalog,
        limits(),
    )
    .expect("load preserved FsHost program");
    assert_eq!(preserved_memory, preserved_filesystem);
    assert_eq!(preserved_memory.library_files().len(), 19);
    assert!(preserved_memory
        .library_files()
        .iter()
        .enumerate()
        .all(|(position, source)| source.index() == position));
    assert_eq!(
        &source_paths(&preserved_memory)[19..],
        [
            real.clone(),
            lexical_linked.clone(),
            lexical_linked2.clone(),
            app_source.clone(),
        ]
    );

    let linked_source = assert_module_resolution(
        &preserved_memory,
        &app_source,
        "linked",
        &lexical_linked,
        None,
    );
    let linked2_source = assert_module_resolution(
        &preserved_memory,
        &app_source,
        "linked2",
        &lexical_linked2,
        None,
    );
    assert_ne!(linked_source, linked2_source);
    assert_eq!(
        assert_type_reference_resolution(
            &preserved_memory,
            &app_source,
            "linked",
            &lexical_linked,
            None,
        ),
        linked_source
    );
    assert_module_resolution(&preserved_memory, &lexical_linked, "real", &real, None);
    assert_module_resolution(&preserved_memory, &lexical_linked2, "real", &real, None);

    let preserved_memory_outcome = ProgramSession::new(preserved_memory)
        .run()
        .expect("run preserved MemoryHost program");
    let preserved_filesystem_outcome = ProgramSession::new(preserved_filesystem)
        .run()
        .expect("run preserved FsHost program");
    assert_eq!(preserved_memory_outcome, preserved_filesystem_outcome);
    assert!(preserved_memory_outcome.config_diagnostics().is_empty());
    assert!(preserved_memory_outcome.syntactic_diagnostics().is_empty());
    assert!(preserved_memory_outcome.options_diagnostics().is_empty());
    assert!(preserved_memory_outcome.global_diagnostics().is_empty());
    assert_eq!(
        preserved_memory_outcome
            .semantic_diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [2322],
        "preserved semantic diagnostics: {:#?}",
        preserved_memory_outcome.semantic_diagnostics()
    );
    assert_eq!(
        preserved_memory_outcome
            .conformance_diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [6133, 2322]
    );
    let incompatibility = &preserved_memory_outcome.semantic_diagnostics()[0];
    assert_eq!(incompatibility.file_name.as_deref(), app_source.to_str());
    assert_eq!(
        (incompatibility.start, incompatibility.length),
        (
            Some(APP_SOURCE.rfind("x = new C2();").unwrap() as u32),
            Some(1),
        )
    );
    assert_eq!(
        incompatibility.message_text(),
        format!(
            "Type 'import(\"{}\").C' is not assignable to type 'import(\"{}\").C'.",
            declaration_module_name(&lexical_linked2),
            declaration_module_name(&lexical_linked),
        )
    );
    assert!(incompatibility.message.next_present);
    assert_eq!(incompatibility.message.next.len(), 1);
    assert_eq!(incompatibility.message.next[0].code, 2442);
    assert_eq!(
        incompatibility.message.next[0].text,
        "Types have separate declarations of a private property 'x'."
    );
    assert!(incompatibility.message.next[0].next.is_empty());
    assert!(incompatibility.related.is_empty());

    let followed_memory = load_program(
        &memory,
        &roots,
        compiler_options(),
        program_options(false),
        &library_catalog,
        limits(),
    )
    .expect("load followed MemoryHost program");
    let followed_filesystem = load_program(
        &filesystem,
        &roots,
        compiler_options(),
        program_options(false),
        &library_catalog,
        limits(),
    )
    .expect("load followed FsHost program");
    assert_eq!(followed_memory, followed_filesystem);
    assert_eq!(followed_memory.library_files().len(), 19);
    assert_eq!(
        &source_paths(&followed_memory)[19..],
        [physical_linked.clone(), app_source.clone()]
    );

    let linked_source = assert_module_resolution(
        &followed_memory,
        &app_source,
        "linked",
        &physical_linked,
        Some(&lexical_linked),
    );
    let linked2_source = assert_module_resolution(
        &followed_memory,
        &app_source,
        "linked2",
        &physical_linked,
        Some(&lexical_linked2),
    );
    assert_eq!(linked_source, linked2_source);
    assert_eq!(
        assert_type_reference_resolution(
            &followed_memory,
            &app_source,
            "linked",
            &physical_linked,
            Some(&lexical_linked),
        ),
        linked_source
    );

    let followed_memory_outcome = ProgramSession::new(followed_memory)
        .run()
        .expect("run followed MemoryHost program");
    let followed_filesystem_outcome = ProgramSession::new(followed_filesystem)
        .run()
        .expect("run followed FsHost program");
    assert_eq!(followed_memory_outcome, followed_filesystem_outcome);
    assert!(followed_memory_outcome.config_diagnostics().is_empty());
    assert!(followed_memory_outcome.syntactic_diagnostics().is_empty());
    assert!(followed_memory_outcome.options_diagnostics().is_empty());
    assert!(followed_memory_outcome.global_diagnostics().is_empty());
    assert_eq!(
        followed_memory_outcome
            .semantic_diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [2307]
    );
    let missing_real = &followed_memory_outcome.semantic_diagnostics()[0];
    assert_eq!(
        (
            missing_real.file_name.as_deref(),
            missing_real.start,
            missing_real.length,
            missing_real.message_text(),
        ),
        (
            physical_linked.to_str(),
            Some(LINKED_SOURCE.find("\"real\"").unwrap() as u32),
            Some("\"real\"".len() as u32),
            "Cannot find module 'real' or its corresponding type declarations.",
        )
    );
    assert!(!missing_real.message.next_present);
    assert!(missing_real.message.next.is_empty());
    assert!(!missing_real.related_information_present);
    assert!(missing_real.related.is_empty());
    assert_eq!(
        followed_memory_outcome
            .conformance_diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [6133, 2307]
    );
}

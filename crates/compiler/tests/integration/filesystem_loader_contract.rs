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
    load_no_lib_program, plan_source_requests, CompilerOptionNumber, CompilerOptions, PathMapping,
    ProgramLoadLimits, ProgramOptions, ProgramPath, ResolutionOutcome, ResolvedModuleTarget,
    UnloadedModuleReason,
};

const GENEROUS_LIMIT: usize = 1_024 * 1_024;
const MINIMAL_GLOBALS: &str = r#"
interface IArguments { length: number; callee: Function; }
interface Array<T> { length: number; [index: number]: T; }
interface Object {}
interface Function {}
interface CallableFunction extends Function {}
interface NewableFunction extends Function {}
interface String {}
interface Number {}
interface Boolean {}
interface RegExp {}
"#;

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
                "tsc-rs-filesystem-loader-{}-{timestamp}-{sequence}",
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

impl Drop for TempTree {
    fn drop(&mut self) {
        let file_name = self.root.file_name().and_then(|name| name.to_str());
        assert_eq!(self.root.parent(), Some(self.cleanup_parent.as_path()));
        assert!(file_name.is_some_and(|name| name.starts_with("tsc-rs-filesystem-loader-")));
        if let Err(error) = fs::remove_dir_all(&self.root) {
            if !std::thread::panicking() {
                panic!("remove temp tree {}: {error}", self.root.display());
            }
        }
    }
}

fn limits() -> ProgramLoadLimits {
    ProgramLoadLimits::new(
        GENEROUS_LIMIT,
        GENEROUS_LIMIT,
        256,
        GENEROUS_LIMIT,
        GENEROUS_LIMIT,
    )
}

#[test]
fn extensionless_roots_preserve_memory_filesystem_and_session_equivalence() {
    let tree = TempTree::new();
    let entry = concat!(
        "/// <reference path=\"./globals.d.ts\" />\n",
        "const value: number = 'not a number';\n",
        "export { value };\n",
    );
    let files = [
        ("entry.tsx", entry.as_bytes()),
        ("globals.d.ts", MINIMAL_GLOBALS.as_bytes()),
    ];
    for (relative, bytes) in files {
        fs::write(tree.path(relative), bytes).expect("write extensionless source tree");
    }

    let filesystem = FsCompilerHost::new(tree.root(), true).expect("construct filesystem host");
    let mut memory = MemoryCompilerHost::builder(tree.root()).case_sensitive(true);
    for (relative, bytes) in files {
        memory = memory.file(tree.path(relative), bytes.to_vec());
    }
    let memory = memory.build().expect("construct memory host");
    let compiler_options = CompilerOptions {
        no_emit: Some(true),
        ..CompilerOptions::default()
    };
    let program_options = ProgramOptions::default()
        .with_no_lib(true)
        .with_types(Vec::new());

    let roots = [tree.path("entry")];
    let from_memory = load_no_lib_program(
        &memory,
        &roots,
        compiler_options.clone(),
        program_options.clone(),
        limits(),
    )
    .expect("load extensionless root from MemoryHost");
    let from_filesystem = load_no_lib_program(
        &filesystem,
        &roots,
        compiler_options.clone(),
        program_options.clone(),
        limits(),
    )
    .expect("load extensionless root from FsHost");
    assert_eq!(from_memory, from_filesystem);
    assert_eq!(from_memory.roots()[0].path().display(), tree.path("entry"));
    let root_source = from_memory.roots()[0]
        .source()
        .expect("extensionless root has a selected source");
    assert_eq!(
        from_memory
            .source_file(root_source)
            .unwrap()
            .path()
            .display(),
        tree.path("entry.tsx")
    );

    let memory_outcome = ProgramSession::new(from_memory)
        .run()
        .expect("run MemoryHost extensionless program");
    let filesystem_outcome = ProgramSession::new(from_filesystem)
        .run()
        .expect("run FsHost extensionless program");
    assert_eq!(memory_outcome, filesystem_outcome);
    assert!(memory_outcome.options_diagnostics().is_empty());
    assert!(memory_outcome.global_diagnostics().is_empty());
    assert_eq!(
        memory_outcome
            .semantic_diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [2322]
    );

    let missing_roots = [tree.path("missing")];
    let missing_memory = load_no_lib_program(
        &memory,
        &missing_roots,
        compiler_options.clone(),
        program_options.clone(),
        limits(),
    )
    .expect("retain a MemoryHost extensionless miss as TS6231");
    let missing_filesystem = load_no_lib_program(
        &filesystem,
        &missing_roots,
        compiler_options,
        program_options,
        limits(),
    )
    .expect("retain an FsHost extensionless miss as TS6231");
    assert_eq!(missing_memory, missing_filesystem);
    let missing_memory = ProgramSession::new(missing_memory)
        .run()
        .expect("run MemoryHost extensionless miss");
    let missing_filesystem = ProgramSession::new(missing_filesystem)
        .run()
        .expect("run FsHost extensionless miss");
    assert_eq!(missing_memory, missing_filesystem);
    assert_eq!(
        missing_memory
            .options_diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [6231]
    );
    assert_eq!(
        missing_memory
            .global_diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [2318; 10]
    );
    assert!(missing_memory.semantic_diagnostics().is_empty());
}

#[test]
fn external_symlink_preserves_memory_filesystem_and_session_equivalence() {
    let tree = TempTree::new();
    fs::create_dir_all(tree.path("node_modules/pkg")).expect("create lexical package directory");
    fs::create_dir_all(tree.path("store/pkg")).expect("create physical package directory");

    let root = concat!(
        "/// <reference path=\"./globals.d.ts\" />\n",
        "import { value } from 'pkg';\n",
        "const checked: number = value;\n",
        "export { checked };\n",
    );
    let package = b"export const value = 1;\n";
    let lexical_package = tree.path("node_modules/pkg/index.ts");
    let physical_package = tree.path("store/pkg/index.ts");
    let regular_files = [
        (tree.path("root.ts"), root.as_bytes()),
        (tree.path("globals.d.ts"), MINIMAL_GLOBALS.as_bytes()),
        (
            tree.path("node_modules/pkg/package.json"),
            br#"{"name":"pkg","version":"1.0.0","exports":"./index.ts"}"#.as_slice(),
        ),
        (physical_package.clone(), package.as_slice()),
    ];
    for (path, bytes) in &regular_files {
        fs::write(path, bytes).expect("write external-symlink source tree");
    }
    symlink(&physical_package, &lexical_package).expect("create package entry symlink");

    let filesystem = FsCompilerHost::new(tree.root(), true).expect("construct filesystem host");
    let mut memory = MemoryCompilerHost::builder(tree.root()).case_sensitive(true);
    for (path, bytes) in &regular_files {
        memory = memory.file(path, bytes.to_vec());
    }
    let memory = memory
        .file(&lexical_package, package.to_vec())
        .realpath(&lexical_package, &physical_package)
        .build()
        .expect("construct mirrored memory symlink host");
    let compiler_options = CompilerOptions {
        no_emit: Some(true),
        module: Some(199),
        module_resolution: Some(99),
        ..CompilerOptions::default()
    };
    let program_options = ProgramOptions::default()
        .with_no_lib(true)
        .with_types(Vec::new());
    let roots = [tree.path("root.ts")];

    let from_memory = load_no_lib_program(
        &memory,
        &roots,
        compiler_options.clone(),
        program_options.clone(),
        limits(),
    )
    .expect("load external symlink from MemoryHost");
    let from_filesystem = load_no_lib_program(
        &filesystem,
        &roots,
        compiler_options,
        program_options,
        limits(),
    )
    .expect("load external symlink from FsHost");
    assert_eq!(from_memory, from_filesystem);
    assert_eq!(
        from_memory
            .source_files()
            .iter()
            .map(|source| source.path().display().to_path_buf())
            .collect::<Vec<_>>(),
        [
            tree.path("globals.d.ts"),
            physical_package.clone(),
            tree.path("root.ts"),
        ]
    );

    let root_source = from_memory
        .source_files()
        .iter()
        .find(|source| source.path().display() == tree.path("root.ts"))
        .expect("root source is owned");
    let key = plan_source_requests(root_source, from_memory.compiler_options())
        .expect("plan package request")
        .module_requests()[0]
        .clone();
    let resolution = from_memory
        .resolutions()
        .require_module(&key)
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
    assert_eq!(resolved_file.display(), physical_package);
    assert_eq!(
        from_memory.source_file(*source).unwrap().path().display(),
        physical_package
    );
    assert_eq!(
        module.original_path().map(ProgramPath::display),
        Some(lexical_package.as_path())
    );

    let memory_outcome = ProgramSession::new(from_memory)
        .run()
        .expect("run MemoryHost symlink program");
    let filesystem_outcome = ProgramSession::new(from_filesystem)
        .run()
        .expect("run FsHost symlink program");
    assert_eq!(memory_outcome, filesystem_outcome);
    assert!(memory_outcome.options_diagnostics().is_empty());
    assert!(memory_outcome.global_diagnostics().is_empty());
    assert!(memory_outcome.semantic_diagnostics().is_empty());
}

#[test]
fn paths_base_url_and_root_dirs_produce_identical_filesystem_backed_diagnostics() {
    let tree = TempTree::new();
    fs::create_dir(tree.path("src")).expect("create paths directory");
    fs::create_dir(tree.path("base")).expect("create baseUrl directory");
    fs::create_dir(tree.path("generated")).expect("create rootDirs directory");
    let root = concat!(
        "/// <reference path=\"./globals.d.ts\" />\n",
        "import { mapped } from '@app/mapped';\n",
        "import { based } from 'based';\n",
        "import { rooted } from './rooted';\n",
        "const result: number = mapped;\n",
        "export { result, based, rooted };\n",
    );
    let files = [
        ("root.ts", root.as_bytes()),
        ("globals.d.ts", MINIMAL_GLOBALS.as_bytes()),
        (
            "src/mapped.ts",
            b"export const mapped = 'mapped';".as_slice(),
        ),
        ("base/based.ts", b"export const based = 1;".as_slice()),
        (
            "generated/rooted.ts",
            b"export const rooted = 1;".as_slice(),
        ),
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
    let compiler_options = CompilerOptions {
        no_emit: Some(true),
        base_url: Some("base".to_owned()),
        ignore_deprecations: Some("6.0".to_owned()),
        ..CompilerOptions::default()
    };
    let root_dirs = [tree.root().to_path_buf(), tree.path("generated")]
        .into_iter()
        .map(|path| {
            ProgramPath::from_trusted_parts(path.clone(), path)
                .expect("construct normalized rootDirs identity")
        })
        .collect();
    let program_options = ProgramOptions::default()
        .with_no_lib(true)
        .with_types(Vec::new())
        .with_root_dirs(root_dirs)
        .with_paths(vec![PathMapping::new(
            "@app/*",
            vec!["../src/*".to_owned()],
        )]);
    let roots = [tree.path("root.ts")];

    let from_memory = load_no_lib_program(
        &memory,
        &roots,
        compiler_options.clone(),
        program_options.clone(),
        limits(),
    )
    .expect("load MemoryHost program");
    let from_filesystem = load_no_lib_program(
        &filesystem,
        &roots,
        compiler_options,
        program_options,
        limits(),
    )
    .expect("load FsHost program");
    assert_eq!(from_memory, from_filesystem);

    let memory_outcome = ProgramSession::new(from_memory)
        .run()
        .expect("run MemoryHost prepared program");
    let filesystem_outcome = ProgramSession::new(from_filesystem)
        .run()
        .expect("run FsHost prepared program");
    assert_eq!(memory_outcome, filesystem_outcome);
    assert!(memory_outcome.syntactic_diagnostics().is_empty());
    assert!(memory_outcome.options_diagnostics().is_empty());
    assert!(memory_outcome.global_diagnostics().is_empty());
    assert_eq!(
        memory_outcome
            .semantic_diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [2322]
    );
}

#[test]
fn allow_js_local_closure_produces_identical_filesystem_backed_diagnostics() {
    let tree = TempTree::new();
    let root = concat!(
        "/// <reference path=\"./globals.d.ts\" />\n",
        "import { checked } from './dependency.js';\n",
        "import packageValue from 'pkg';\n",
        "const numeric: number = checked;\n",
        "packageValue;\n",
        "export { numeric };\n",
    );
    let dependency = concat!(
        "// @ts-check\n",
        "import './leaf.cjs';\n",
        "export const checked = 'text';\n",
        "checked.missing;\n",
    );
    let files = [
        ("root.ts", root.as_bytes()),
        ("globals.d.ts", MINIMAL_GLOBALS.as_bytes()),
        ("dependency.js", dependency.as_bytes()),
        ("leaf.cjs", b"exports.leaf = 1;".as_slice()),
        (
            "node_modules/pkg/package.json",
            br#"{"name":"pkg","version":"1.0.0","main":"index.js"}"#.as_slice(),
        ),
        (
            "node_modules/pkg/index.js",
            b"module.exports = 1;".as_slice(),
        ),
    ];
    fs::create_dir_all(tree.path("node_modules/pkg")).expect("create JavaScript package directory");
    for (relative, bytes) in files {
        fs::write(tree.path(relative), bytes).expect("write JavaScript source tree");
    }

    let filesystem = FsCompilerHost::new(tree.root(), true).expect("construct filesystem host");
    let mut memory = MemoryCompilerHost::builder(tree.root()).case_sensitive(true);
    for (relative, bytes) in files {
        memory = memory.file(tree.path(relative), bytes.to_vec());
    }
    let memory = memory.build().expect("construct memory host");
    let compiler_options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        no_emit: Some(true),
        ..CompilerOptions::default()
    };
    let program_options = ProgramOptions::default()
        .with_no_lib(true)
        .with_types(Vec::new());
    let roots = [tree.path("root.ts")];

    let from_memory = load_no_lib_program(
        &memory,
        &roots,
        compiler_options.clone(),
        program_options.clone(),
        limits(),
    )
    .expect("load JavaScript closure from MemoryHost");
    let from_filesystem = load_no_lib_program(
        &filesystem,
        &roots,
        compiler_options,
        program_options,
        limits(),
    )
    .expect("load JavaScript closure from FsHost");
    assert_eq!(from_memory, from_filesystem);
    assert_eq!(
        from_memory
            .source_files()
            .iter()
            .map(|source| source.path().display().to_path_buf())
            .collect::<Vec<_>>(),
        [
            tree.path("globals.d.ts"),
            tree.path("leaf.cjs"),
            tree.path("dependency.js"),
            tree.path("root.ts"),
        ]
    );
    let root_source = from_memory
        .source_files()
        .iter()
        .find(|source| source.path().display() == tree.path("root.ts"))
        .expect("root source is owned");
    let root_plan = plan_source_requests(root_source, from_memory.compiler_options())
        .expect("plan root requests");
    let package_key = root_plan
        .module_requests()
        .iter()
        .find(|key| key.specifier() == "pkg")
        .expect("package request exists");
    let package_resolution = from_memory
        .resolutions()
        .require_module(package_key)
        .expect("package request has an authoritative row");
    let ResolutionOutcome::Resolved(package) = package_resolution.outcome() else {
        panic!("package JavaScript resolves");
    };
    let ResolvedModuleTarget::Unloaded { reason, .. } = package.target() else {
        panic!("default depth keeps package JavaScript unloaded");
    };
    assert_eq!(*reason, UnloadedModuleReason::NodeModulesDepth);

    let memory_outcome = ProgramSession::new(from_memory)
        .run()
        .expect("run MemoryHost JavaScript program");
    let filesystem_outcome = ProgramSession::new(from_filesystem)
        .run()
        .expect("run FsHost JavaScript program");
    assert_eq!(memory_outcome, filesystem_outcome);
    assert!(memory_outcome.syntactic_diagnostics().is_empty());
    assert!(memory_outcome.options_diagnostics().is_empty());
    assert!(memory_outcome.global_diagnostics().is_empty());
    assert_eq!(
        memory_outcome
            .semantic_diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [2339, 7016, 2322]
    );
}

#[test]
fn positive_node_module_js_depth_admits_package_and_gates_its_nested_javascript() {
    let tree = TempTree::new();
    fs::create_dir_all(tree.path("node_modules/pkg"))
        .expect("create nested JavaScript package directory");
    let root = concat!(
        "/// <reference path=\"./globals.d.ts\" />\n",
        "import { value } from 'pkg';\n",
        "const numeric: number = value;\n",
        "export { numeric };\n",
    );
    let package = concat!(
        "import { leaf } from './leaf.js';\n",
        "export const value = 1;\n",
        "leaf;\n",
    );
    let files = [
        ("root.ts", root.as_bytes()),
        ("globals.d.ts", MINIMAL_GLOBALS.as_bytes()),
        (
            "node_modules/pkg/package.json",
            br#"{"name":"pkg","version":"1.0.0","main":"index.js"}"#.as_slice(),
        ),
        ("node_modules/pkg/index.js", package.as_bytes()),
        (
            "node_modules/pkg/leaf.js",
            b"export const leaf = 2;".as_slice(),
        ),
    ];
    for (relative, bytes) in files {
        fs::write(tree.path(relative), bytes).expect("write nested JavaScript package tree");
    }

    let filesystem = FsCompilerHost::new(tree.root(), true).expect("construct filesystem host");
    let mut memory = MemoryCompilerHost::builder(tree.root()).case_sensitive(true);
    for (relative, bytes) in files {
        memory = memory.file(tree.path(relative), bytes.to_vec());
    }
    let memory = memory.build().expect("construct memory host");
    let compiler_options = CompilerOptions {
        allow_js: true,
        max_node_module_js_depth: Some(1.into()),
        no_emit: Some(true),
        ..CompilerOptions::default()
    };
    let program_options = ProgramOptions::default()
        .with_no_lib(true)
        .with_types(Vec::new());
    let roots = [tree.path("root.ts")];

    let from_memory = load_no_lib_program(
        &memory,
        &roots,
        compiler_options.clone(),
        program_options.clone(),
        limits(),
    )
    .expect("load depth-bounded JavaScript package from MemoryHost");
    let from_filesystem = load_no_lib_program(
        &filesystem,
        &roots,
        compiler_options,
        program_options,
        limits(),
    )
    .expect("load depth-bounded JavaScript package from FsHost");
    assert_eq!(from_memory, from_filesystem);
    assert_eq!(
        from_memory
            .source_files()
            .iter()
            .map(|source| source.path().display().to_path_buf())
            .collect::<Vec<_>>(),
        [
            tree.path("globals.d.ts"),
            tree.path("node_modules/pkg/index.js"),
            tree.path("root.ts"),
        ]
    );

    let root_source = from_memory
        .source_files()
        .iter()
        .find(|source| source.path().display() == tree.path("root.ts"))
        .expect("root source is owned");
    let package_key = plan_source_requests(root_source, from_memory.compiler_options())
        .expect("plan package request")
        .module_requests()[0]
        .clone();
    let package_resolution = from_memory
        .resolutions()
        .require_module(&package_key)
        .expect("package request has an authoritative row");
    let ResolutionOutcome::Resolved(package_resolution) = package_resolution.outcome() else {
        panic!("package JavaScript resolves");
    };
    let ResolvedModuleTarget::Source {
        source: package_source,
        resolved_file: package_file,
    } = package_resolution.target()
    else {
        panic!("depth one admits the package entry point");
    };
    assert_eq!(
        package_file.display(),
        tree.path("node_modules/pkg/index.js")
    );

    let package_source = from_memory
        .source_file(*package_source)
        .expect("package source id is owned");
    let leaf_key = plan_source_requests(package_source, from_memory.compiler_options())
        .expect("plan nested package request")
        .module_requests()[0]
        .clone();
    let leaf_resolution = from_memory
        .resolutions()
        .require_module(&leaf_key)
        .expect("nested request has an authoritative row");
    let ResolutionOutcome::Resolved(leaf_resolution) = leaf_resolution.outcome() else {
        panic!("nested package JavaScript resolves");
    };
    let ResolvedModuleTarget::Unloaded {
        resolved_file,
        reason,
    } = leaf_resolution.target()
    else {
        panic!("depth two remains outside the configured depth one boundary");
    };
    assert_eq!(
        resolved_file.display(),
        tree.path("node_modules/pkg/leaf.js")
    );
    assert_eq!(*reason, UnloadedModuleReason::NodeModulesDepth);

    let memory_outcome = ProgramSession::new(from_memory)
        .run()
        .expect("run MemoryHost depth-bounded JavaScript program");
    let filesystem_outcome = ProgramSession::new(from_filesystem)
        .run()
        .expect("run FsHost depth-bounded JavaScript program");
    assert_eq!(memory_outcome, filesystem_outcome);
    assert!(memory_outcome.syntactic_diagnostics().is_empty());
    assert!(memory_outcome.options_diagnostics().is_empty());
    assert!(memory_outcome.global_diagnostics().is_empty());
    assert!(memory_outcome.semantic_diagnostics().is_empty());
}

#[test]
fn allow_js_false_precedes_positive_node_module_depth_in_both_authoritative_loaders() {
    let tree = TempTree::new();
    fs::create_dir_all(tree.path("node_modules/pkg"))
        .expect("create non-admitted JavaScript package directory");
    let root = concat!(
        "/// <reference path=\"./globals.d.ts\" />\n",
        "import packageValue from 'pkg';\n",
        "packageValue;\n",
        "export {};\n",
    );
    let files = [
        ("root.ts", root.as_bytes()),
        ("globals.d.ts", MINIMAL_GLOBALS.as_bytes()),
        (
            "node_modules/pkg/package.json",
            br#"{"name":"pkg","version":"1.0.0","main":"index.js"}"#.as_slice(),
        ),
        (
            "node_modules/pkg/index.js",
            b"module.exports = 1;".as_slice(),
        ),
    ];
    for (relative, bytes) in files {
        fs::write(tree.path(relative), bytes).expect("write non-admitted JavaScript package tree");
    }

    let filesystem = FsCompilerHost::new(tree.root(), true).expect("construct filesystem host");
    let mut memory = MemoryCompilerHost::builder(tree.root()).case_sensitive(true);
    for (relative, bytes) in files {
        memory = memory.file(tree.path(relative), bytes.to_vec());
    }
    let memory = memory.build().expect("construct memory host");
    let compiler_options = CompilerOptions {
        allow_js: false,
        max_node_module_js_depth: Some(1.into()),
        no_emit: Some(true),
        no_implicit_any: Some(true),
        ..CompilerOptions::default()
    };
    let program_options = ProgramOptions::default()
        .with_no_lib(true)
        .with_types(Vec::new());
    let roots = [tree.path("root.ts")];

    let from_memory = load_no_lib_program(
        &memory,
        &roots,
        compiler_options.clone(),
        program_options.clone(),
        limits(),
    )
    .expect("load non-admitted JavaScript row from MemoryHost");
    let from_filesystem = load_no_lib_program(
        &filesystem,
        &roots,
        compiler_options,
        program_options,
        limits(),
    )
    .expect("load non-admitted JavaScript row from FsHost");
    assert_eq!(from_memory, from_filesystem);
    assert_eq!(
        from_memory
            .source_files()
            .iter()
            .map(|source| source.path().display().to_path_buf())
            .collect::<Vec<_>>(),
        [tree.path("globals.d.ts"), tree.path("root.ts")]
    );

    let root_source = from_memory
        .source_files()
        .iter()
        .find(|source| source.path().display() == tree.path("root.ts"))
        .expect("root source is owned");
    let package_key = plan_source_requests(root_source, from_memory.compiler_options())
        .expect("plan non-admitted package request")
        .module_requests()[0]
        .clone();
    let package_resolution = from_memory
        .resolutions()
        .require_module(&package_key)
        .expect("non-admitted package request has an authoritative row");
    let ResolutionOutcome::Resolved(package_resolution) = package_resolution.outcome() else {
        panic!("non-admitted package JavaScript still resolves");
    };
    let ResolvedModuleTarget::Unloaded {
        resolved_file,
        reason,
    } = package_resolution.target()
    else {
        panic!("allowJs=false keeps package JavaScript outside source membership");
    };
    assert_eq!(
        resolved_file.display(),
        tree.path("node_modules/pkg/index.js")
    );
    assert_eq!(*reason, UnloadedModuleReason::JavaScriptNotAdmitted);

    let memory_outcome = ProgramSession::new(from_memory)
        .run()
        .expect("run MemoryHost non-admitted JavaScript program");
    let filesystem_outcome = ProgramSession::new(from_filesystem)
        .run()
        .expect("run FsHost non-admitted JavaScript program");
    assert_eq!(memory_outcome, filesystem_outcome);
    assert!(memory_outcome.syntactic_diagnostics().is_empty());
    assert!(memory_outcome.options_diagnostics().is_empty());
    assert!(memory_outcome.global_diagnostics().is_empty());
    assert_eq!(
        memory_outcome
            .semantic_diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [7016]
    );
}

#[test]
fn fractional_and_nan_depths_preserve_elision_precedence_through_program_session() {
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/root.ts",
            concat!(
                "/// <reference path=\"./globals.d.ts\" />\n",
                "import packageValue from 'pkg';\n",
                "packageValue;\n",
                "export {};\n",
            )
            .as_bytes()
            .to_vec(),
        )
        .file("/work/globals.d.ts", MINIMAL_GLOBALS.as_bytes().to_vec())
        .file(
            "/work/node_modules/pkg/package.json",
            br#"{"name":"pkg","version":"1.0.0","main":"index.js"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/pkg/index.js",
            b"module.exports = 1;".to_vec(),
        )
        .build()
        .expect("construct fractional/NaN precedence host");

    for (label, maximum, expected_reason) in [
        (
            "fractional first-layer elision",
            0.5,
            UnloadedModuleReason::NodeModulesDepth,
        ),
        (
            "NaN disables ordered depth elision",
            f64::NAN,
            UnloadedModuleReason::JavaScriptNotAdmitted,
        ),
    ] {
        let program = load_no_lib_program(
            &host,
            &[PathBuf::from("/work/root.ts")],
            CompilerOptions {
                allow_js: false,
                max_node_module_js_depth: Some(CompilerOptionNumber::new(maximum)),
                no_emit: Some(true),
                no_implicit_any: Some(true),
                ..CompilerOptions::default()
            },
            ProgramOptions::default()
                .with_no_lib(true)
                .with_types(Vec::new()),
            limits(),
        )
        .unwrap_or_else(|error| panic!("load {label}: {error}"));
        let root = program
            .source_files()
            .iter()
            .find(|source| source.path().display() == Path::new("/work/root.ts"))
            .expect("root source is owned");
        let key = plan_source_requests(root, program.compiler_options())
            .expect("plan package request")
            .module_requests()[0]
            .clone();
        let resolution = program
            .resolutions()
            .require_module(&key)
            .expect("package request has an authoritative row");
        let ResolutionOutcome::Resolved(resolution) = resolution.outcome() else {
            panic!("{label} package request must resolve");
        };
        let ResolvedModuleTarget::Unloaded { reason, .. } = resolution.target() else {
            panic!("allowJs=false must keep the {label} target unloaded");
        };
        assert_eq!(*reason, expected_reason, "{label}");

        let outcome = ProgramSession::new(program)
            .run()
            .unwrap_or_else(|error| panic!("run {label}: {error}"));
        assert_eq!(
            outcome
                .semantic_diagnostics()
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            [7016],
            "{label}"
        );
    }
}

#[test]
fn jsx_without_mode_flows_from_both_loaders_to_exact_ts6142_diagnostics() {
    let tree = TempTree::new();
    fs::create_dir_all(tree.path("node_modules/pkg")).expect("create JSX package directory");
    let root = concat!(
        "/// <reference path=\"./globals.d.ts\" />\n",
        "import './dependency.jsx';\n",
        "import 'pkg';\n",
        "export {};\n",
    );
    let files = [
        ("root.ts", root.as_bytes()),
        ("globals.d.ts", MINIMAL_GLOBALS.as_bytes()),
        ("dependency.jsx", b"export const local = 1;".as_slice()),
        (
            "node_modules/pkg/package.json",
            br#"{"name":"pkg","version":"1.0.0","main":"index.jsx"}"#.as_slice(),
        ),
        (
            "node_modules/pkg/index.jsx",
            b"exports.package = 1;".as_slice(),
        ),
    ];
    for (relative, bytes) in files {
        fs::write(tree.path(relative), bytes).expect("write JSX source tree");
    }

    let filesystem = FsCompilerHost::new(tree.root(), true).expect("construct filesystem host");
    let mut memory = MemoryCompilerHost::builder(tree.root()).case_sensitive(true);
    for (relative, bytes) in files {
        memory = memory.file(tree.path(relative), bytes.to_vec());
    }
    let memory = memory.build().expect("construct memory host");
    let compiler_options = CompilerOptions {
        allow_js: true,
        no_emit: Some(true),
        ..CompilerOptions::default()
    };
    let program_options = ProgramOptions::default()
        .with_no_lib(true)
        .with_types(Vec::new());
    let roots = [tree.path("root.ts")];

    let from_memory = load_no_lib_program(
        &memory,
        &roots,
        compiler_options.clone(),
        program_options.clone(),
        limits(),
    )
    .expect("load JSX rows from MemoryHost");
    let from_filesystem = load_no_lib_program(
        &filesystem,
        &roots,
        compiler_options,
        program_options,
        limits(),
    )
    .expect("load JSX rows from FsHost");
    assert_eq!(from_memory, from_filesystem);
    assert!(from_memory.diagnostics().program().is_empty());
    assert_eq!(
        from_memory
            .source_files()
            .iter()
            .map(|source| source.path().display().to_path_buf())
            .collect::<Vec<_>>(),
        [tree.path("globals.d.ts"), tree.path("root.ts")]
    );
    let root_source = from_memory
        .source_files()
        .iter()
        .find(|source| source.path().display() == tree.path("root.ts"))
        .expect("root source is owned");
    let root_plan = plan_source_requests(root_source, from_memory.compiler_options())
        .expect("plan JSX root requests");
    for key in root_plan.module_requests() {
        let resolution = from_memory
            .resolutions()
            .require_module(key)
            .expect("JSX request has an authoritative row");
        let ResolutionOutcome::Resolved(module) = resolution.outcome() else {
            panic!("JSX request must resolve: {}", key.specifier());
        };
        let ResolvedModuleTarget::Unloaded { reason, .. } = module.target() else {
            panic!(
                "JSX without a mode must remain unloaded: {}",
                key.specifier()
            );
        };
        assert_eq!(*reason, UnloadedModuleReason::JsxWithoutJsxOption);
    }

    let memory_outcome = ProgramSession::new(from_memory)
        .run()
        .expect("run MemoryHost JSX program");
    let filesystem_outcome = ProgramSession::new(from_filesystem)
        .run()
        .expect("run FsHost JSX program");
    assert_eq!(memory_outcome, filesystem_outcome);
    assert!(memory_outcome.syntactic_diagnostics().is_empty());
    assert!(memory_outcome.options_diagnostics().is_empty());
    assert!(memory_outcome.global_diagnostics().is_empty());
    assert_eq!(
        memory_outcome
            .semantic_diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [6142, 6142]
    );
}

#[test]
fn arbitrary_declaration_membership_keeps_importer_specific_ts6263() {
    let tree = TempTree::new();
    let root = concat!(
        "/// <reference path=\"./globals.d.ts\" />\n",
        "import './data.json';\n",
        "export {};\n",
    );
    let files = [
        ("root.ts", root.as_bytes()),
        (
            "ambient.d.ts",
            b"import './data.json';\nexport {};\n".as_slice(),
        ),
        (
            "data.d.json.ts",
            b"declare const data: true;\nexport default data;\n".as_slice(),
        ),
        ("globals.d.ts", MINIMAL_GLOBALS.as_bytes()),
    ];
    for (relative, bytes) in files {
        fs::write(tree.path(relative), bytes).expect("write arbitrary declaration source tree");
    }

    let filesystem = FsCompilerHost::new(tree.root(), true).expect("construct filesystem host");
    let mut memory = MemoryCompilerHost::builder(tree.root()).case_sensitive(true);
    for (relative, bytes) in files {
        memory = memory.file(tree.path(relative), bytes.to_vec());
    }
    let memory = memory.build().expect("construct memory host");
    let compiler_options = CompilerOptions {
        no_emit: Some(true),
        module: Some(1),
        module_resolution: Some(2),
        resolve_json_module: Some(false),
        ..CompilerOptions::default()
    };
    let program_options = ProgramOptions::default()
        .with_no_lib(true)
        .with_types(Vec::new());
    let roots = [tree.path("root.ts"), tree.path("ambient.d.ts")];

    let from_memory = load_no_lib_program(
        &memory,
        &roots,
        compiler_options.clone(),
        program_options.clone(),
        limits(),
    )
    .expect("load arbitrary rows from MemoryHost");
    let from_filesystem = load_no_lib_program(
        &filesystem,
        &roots,
        compiler_options,
        program_options,
        limits(),
    )
    .expect("load arbitrary rows from FsHost");
    assert_eq!(from_memory, from_filesystem);
    assert!(from_memory
        .source_files()
        .iter()
        .any(|source| { source.path().display() == tree.path("data.d.json.ts") }));
    let root_source = from_memory
        .source_files()
        .iter()
        .find(|source| source.path().display() == tree.path("root.ts"))
        .expect("root source is owned");
    let root_key = plan_source_requests(root_source, from_memory.compiler_options())
        .expect("plan arbitrary root request")
        .module_requests()[0]
        .clone();
    let root_resolution = from_memory
        .resolutions()
        .require_module(&root_key)
        .expect("root arbitrary request has an authoritative row");
    assert!(matches!(
        root_resolution.outcome(),
        ResolutionOutcome::Resolved(module)
            if matches!(module.target(), ResolvedModuleTarget::Source { .. })
    ));

    let memory_outcome = ProgramSession::new(from_memory)
        .run()
        .expect("run MemoryHost arbitrary program");
    let filesystem_outcome = ProgramSession::new(from_filesystem)
        .run()
        .expect("run FsHost arbitrary program");
    assert_eq!(memory_outcome, filesystem_outcome);
    assert_eq!(
        memory_outcome
            .semantic_diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [6263]
    );
}

#[test]
fn declaration_augmentation_allows_a_resolution_only_arbitrary_target() {
    let tree = TempTree::new();
    let root = concat!(
        "/// <reference path=\"./globals.d.ts\" />\n",
        "export {};\n",
        "declare module './data.json' { export const value: true; }\n",
    );
    let files = [
        ("root.d.ts", root.as_bytes()),
        (
            "data.d.json.ts",
            b"declare const data: true;\nexport default data;\n".as_slice(),
        ),
        ("globals.d.ts", MINIMAL_GLOBALS.as_bytes()),
    ];
    for (relative, bytes) in files {
        fs::write(tree.path(relative), bytes).expect("write declaration augmentation source tree");
    }

    let filesystem = FsCompilerHost::new(tree.root(), true).expect("construct filesystem host");
    let mut memory = MemoryCompilerHost::builder(tree.root()).case_sensitive(true);
    for (relative, bytes) in files {
        memory = memory.file(tree.path(relative), bytes.to_vec());
    }
    let memory = memory.build().expect("construct memory host");
    let compiler_options = CompilerOptions {
        no_emit: Some(true),
        module: Some(1),
        module_resolution: Some(2),
        ..CompilerOptions::default()
    };
    let program_options = ProgramOptions::default()
        .with_no_lib(true)
        .with_types(Vec::new());
    let roots = [tree.path("root.d.ts")];

    let from_memory = load_no_lib_program(
        &memory,
        &roots,
        compiler_options.clone(),
        program_options.clone(),
        limits(),
    )
    .expect("load declaration augmentation from MemoryHost");
    let from_filesystem = load_no_lib_program(
        &filesystem,
        &roots,
        compiler_options,
        program_options,
        limits(),
    )
    .expect("load declaration augmentation from FsHost");
    assert_eq!(from_memory, from_filesystem);
    assert!(from_memory
        .source_files()
        .iter()
        .all(|source| source.path().display() != tree.path("data.d.json.ts")));
    let root_source = from_memory
        .source_files()
        .iter()
        .find(|source| source.path().display() == tree.path("root.d.ts"))
        .expect("declaration root is owned");
    let key = plan_source_requests(root_source, from_memory.compiler_options())
        .expect("plan declaration augmentation")
        .module_requests()[0]
        .clone();
    let resolution = from_memory
        .resolutions()
        .require_module(&key)
        .expect("augmentation has an authoritative row");
    assert!(matches!(
        resolution.outcome(),
        ResolutionOutcome::Resolved(module)
            if matches!(
                module.target(),
                ResolvedModuleTarget::Unloaded {
                    reason: UnloadedModuleReason::ResolutionOnly,
                    ..
                }
            )
    ));

    let memory_outcome = ProgramSession::new(from_memory)
        .run()
        .expect("run MemoryHost declaration augmentation");
    let filesystem_outcome = ProgramSession::new(from_filesystem)
        .run()
        .expect("run FsHost declaration augmentation");
    assert_eq!(memory_outcome, filesystem_outcome);
    assert!(memory_outcome.semantic_diagnostics().is_empty());
}

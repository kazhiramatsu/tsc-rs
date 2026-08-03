#![cfg(unix)]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tsc_compiler::ProgramSession;
use tsc_host::{FsCompilerHost, MemoryCompilerHost};
use tsc_program::{
    load_no_lib_program, CompilerOptions, PreparedProgram, ProgramLoadLimits, ProgramOptions,
};

const GENEROUS_LIMIT: usize = 1_024 * 1_024;
const ROOT_SOURCE: &[u8] = b"const received: number = automaticValue;\n";
const PACKAGE_JSON: &[u8] = br#"{"name":"@types/automatic","types":"index.d.ts"}"#;
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
                "tsc-rs-automatic-types-session-{}-{timestamp}-{sequence}",
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
        assert!(
            file_name.is_some_and(|name| { name.starts_with("tsc-rs-automatic-types-session-") })
        );
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

fn compiler_options() -> CompilerOptions {
    CompilerOptions {
        no_emit: Some(true),
        ..CompilerOptions::default()
    }
}

fn automatic_options() -> ProgramOptions {
    ProgramOptions::default()
        .with_no_lib(true)
        .with_types(vec!["*".to_owned()])
}

fn source_paths(program: &PreparedProgram) -> Vec<&Path> {
    program
        .source_files()
        .iter()
        .map(|source| source.path().display())
        .collect()
}

#[test]
fn automatic_type_declaration_reaches_the_session_identically_from_both_hosts() {
    let tree = TempTree::new();
    let root = tree.path("root.ts");
    let package_directory = tree.path("node_modules/@types/automatic");
    let package_json = package_directory.join("package.json");
    let declaration = package_directory.join("index.d.ts");
    let declaration_text = format!("{MINIMAL_GLOBALS}\ndeclare const automaticValue: number;\n");

    fs::create_dir_all(&package_directory).expect("create automatic type package");
    fs::write(&root, ROOT_SOURCE).expect("write root source");
    fs::write(&package_json, PACKAGE_JSON).expect("write automatic type package manifest");
    fs::write(&declaration, declaration_text.as_bytes()).expect("write automatic type declaration");

    let filesystem = FsCompilerHost::new(tree.root(), true).expect("construct filesystem host");
    let memory = MemoryCompilerHost::builder(tree.root())
        .case_sensitive(true)
        .file(root.clone(), ROOT_SOURCE.to_vec())
        .file(package_json.clone(), PACKAGE_JSON.to_vec())
        .file(declaration.clone(), declaration_text.into_bytes())
        .build()
        .expect("construct memory host");
    let roots = [root.clone()];

    let from_memory = load_no_lib_program(
        &memory,
        &roots,
        compiler_options(),
        automatic_options(),
        limits(),
    )
    .expect("load automatic types through MemoryCompilerHost");
    let from_filesystem = load_no_lib_program(
        &filesystem,
        &roots,
        compiler_options(),
        automatic_options(),
        limits(),
    )
    .expect("load automatic types through FsCompilerHost");

    assert_eq!(from_memory, from_filesystem);
    assert_eq!(
        source_paths(&from_memory),
        [root.as_path(), declaration.as_path()]
    );

    let memory_outcome = ProgramSession::new(from_memory)
        .run()
        .expect("run memory-backed automatic-types session");
    let filesystem_outcome = ProgramSession::new(from_filesystem)
        .run()
        .expect("run filesystem-backed automatic-types session");
    assert_eq!(memory_outcome, filesystem_outcome);
    assert_eq!(memory_outcome.diagnostics().count(), 0);
}

#[test]
fn missing_automatic_types_flow_to_deduplicated_options_diagnostics() {
    let root_text = format!("{MINIMAL_GLOBALS}\nconst value: number = 1;\n");
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", root_text.into_bytes())
        .build()
        .expect("construct missing-automatic-types host");
    let prepared = load_no_lib_program(
        &host,
        &[PathBuf::from("/work/root.ts")],
        compiler_options(),
        ProgramOptions::default()
            .with_no_lib(true)
            .with_types(vec!["missing".to_owned(), "missing".to_owned()])
            .with_type_roots(Vec::new()),
        limits(),
    )
    .expect("load program with missing automatic types");

    let outcome = ProgramSession::new(prepared)
        .run()
        .expect("run missing-automatic-types session");
    assert!(outcome.config_diagnostics().is_empty());
    assert!(outcome.syntactic_diagnostics().is_empty());
    assert!(outcome.global_diagnostics().is_empty());
    assert!(outcome.semantic_diagnostics().is_empty());
    assert_eq!(
        outcome
            .options_diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [2688]
    );
    assert_eq!(outcome.options_diagnostics()[0].file_name, None);
    assert_eq!(
        outcome.options_diagnostics()[0].message.text,
        "Cannot find type definition file for 'missing'."
    );
}

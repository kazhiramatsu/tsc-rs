#![cfg(unix)]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tsc_compiler::ProgramSession;
use tsc_host::{FsCompilerHost, MemoryCompilerHost};
use tsc_program::{
    load_no_lib_program, CompilerOptions, PathMapping, ProgramLoadLimits, ProgramOptions,
    ProgramPath,
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

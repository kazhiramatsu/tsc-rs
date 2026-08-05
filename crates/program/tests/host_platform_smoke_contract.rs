use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tsc_host::{CompilerHost, FsCompilerHost, MemoryCompilerHost};
use tsc_program::{load_no_lib_program, CompilerOptions, ProgramLoadLimits, ProgramOptions};

static NEXT_TEMP_TREE: AtomicU64 = AtomicU64::new(0);

struct TempTree {
    root: PathBuf,
}

impl TempTree {
    fn new() -> Self {
        loop {
            let sequence = NEXT_TEMP_TREE.fetch_add(1, Ordering::Relaxed);
            let candidate = std::env::temp_dir().join(format!(
                "tsc-rs-program-host-smoke-{}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&candidate) {
                Ok(()) => {
                    let root = fs::canonicalize(candidate).expect("physicalize temp tree root");
                    return Self { root };
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
        if let Err(error) = fs::remove_dir_all(&self.root) {
            if !std::thread::panicking() {
                panic!("remove temp tree {}: {error}", self.root.display());
            }
        }
    }
}

fn normalized_display(path: &Path) -> String {
    path.to_str()
        .expect("temporary path is Unicode")
        .replace('\\', "/")
}

#[test]
fn native_filesystem_and_memory_hosts_build_the_same_small_program() {
    let tree = TempTree::new();
    fs::create_dir(tree.path("src")).expect("create source directory");
    let main_text = b"import { child } from './child';\nvoid child;\n";
    let child_text = b"export const child = 1;\n";
    let main = tree.path("src/main.ts");
    let child = tree.path("src/child.ts");
    fs::write(&main, main_text).expect("write root source");
    fs::write(&child, child_text).expect("write dependency source");

    let case_sensitive = FsCompilerHost::from_process()
        .expect("detect native filesystem profile")
        .use_case_sensitive_file_names();
    let filesystem =
        FsCompilerHost::new(tree.root(), case_sensitive).expect("construct filesystem host");
    let memory = MemoryCompilerHost::builder(tree.root())
        .case_sensitive(case_sensitive)
        .file(&main, main_text.to_vec())
        .file(&child, child_text.to_vec())
        .build()
        .expect("construct equivalent memory host");
    let options = CompilerOptions {
        no_emit: Some(true),
        module: Some(1),
        module_resolution: Some(2),
        ..CompilerOptions::default()
    };
    let program_options = ProgramOptions::default()
        .with_no_lib(true)
        .with_types(Vec::new());
    let limits = ProgramLoadLimits::new(16, 16, 16, 64 * 1024, 128 * 1024);

    let from_filesystem = load_no_lib_program(
        &filesystem,
        std::slice::from_ref(&main),
        options.clone(),
        program_options.clone(),
        limits,
    )
    .expect("load native filesystem program");
    let from_memory = load_no_lib_program(
        &memory,
        std::slice::from_ref(&main),
        options,
        program_options,
        limits,
    )
    .expect("load equivalent memory program");

    assert_eq!(from_filesystem, from_memory);
    assert!(from_filesystem.diagnostics().program().is_empty());
    assert_eq!(
        from_filesystem
            .source_files()
            .iter()
            .map(|source| source
                .path()
                .display()
                .to_str()
                .expect("source path is Unicode"))
            .collect::<Vec<_>>(),
        [normalized_display(&child), normalized_display(&main)]
    );
}

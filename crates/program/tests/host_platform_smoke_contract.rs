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
    // ModuleResolver deliberately presents TypeScript-normalized `/` path
    // spellings to its host. MemoryCompilerHost preserves caller spelling
    // exactly, so seed the equivalent logical tree in that boundary spelling
    // instead of Windows' native `\\?\C:\...` PathBuf spelling.
    let memory = MemoryCompilerHost::builder(normalized_display(tree.root()))
        .case_sensitive(case_sensitive)
        .file(normalized_display(&main), main_text.to_vec())
        .file(normalized_display(&child), child_text.to_vec())
        .build()
        .expect("construct equivalent memory host");
    let options = CompilerOptions {
        no_emit: Some(true),
        module: Some(1),
        module_resolution: Some(100),
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
                .as_str()
                .expect("source path is Unicode"))
            .collect::<Vec<_>>(),
        [normalized_display(&child), normalized_display(&main)]
    );
}
#[test]
fn native_config_discovery_preserves_include_buckets_and_native_order() {
    use tsc_diagnostics::JsString;
    use tsc_program::{CompilerConfigHost, ConfigParseHost};

    let tree = TempTree::new();
    let names = [
        "root.ts",
        "a/first.ts",
        "a/deep/leaf.ts",
        "b/last.ts",
        "a/skip.ts",
        "a/readme.md",
        "node_modules/pkg/explicit.ts",
        "unicode/\u{e000}.ts",
        "unicode/\u{10000}.ts",
    ];
    let mut memory = MemoryCompilerHost::builder(normalized_display(tree.root()));
    for name in names {
        let path = tree.path(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"export {};\n").unwrap();
        memory = memory.file(normalized_display(&path), b"export {};\n".to_vec());
    }
    let base = JsString::from(normalized_display(tree.root()));
    let includes = ["b/**/*.ts", "**/*.ts", "node_modules/**/*.ts"].map(JsString::from);
    let excludes = [JsString::from("**/skip.ts")];
    let expected = [
        "b/last.ts",
        "root.ts",
        "a/first.ts",
        "a/deep/leaf.ts",
        "unicode/\u{e000}.ts",
        "unicode/\u{10000}.ts",
        "node_modules/pkg/explicit.ts",
    ]
    .map(|name| JsString::from(normalized_display(&tree.path(name))));
    for case_sensitive in [true, false] {
        let filesystem = FsCompilerHost::new(tree.root(), case_sensitive).unwrap();
        let memory = memory
            .clone()
            .case_sensitive(case_sensitive)
            .build()
            .unwrap();
        for host in [&filesystem as &dyn CompilerHost, &memory] {
            let config = CompilerConfigHost::new(host);
            assert_eq!(
                config
                    .read_directory(
                        base.as_js(),
                        &[".ts"],
                        Some(&excludes),
                        Some(&includes),
                        None
                    )
                    .unwrap(),
                expected
            );
            assert_eq!(
                config
                    .read_directory(
                        base.as_js(),
                        &[".ts"],
                        Some(&excludes),
                        Some(&includes),
                        Some(1)
                    )
                    .unwrap(),
                [JsString::from(normalized_display(&tree.path("root.ts")))]
            );
            assert!(config
                .read_directory(base.as_js(), &[".ts"], None, Some(&includes), Some(0))
                .unwrap()
                .is_empty());
            // An include adds this base's parent after the base itself.
            // Prefetch may reach one directory at two remaining depths.
            let nested = JsString::from(normalized_display(&tree.path("a")));
            let ancestor_include = [JsString::from("../**/*.ts")];
            let overlapping = [
                "a/first.ts",
                "a/skip.ts",
                "a/deep/leaf.ts",
                "root.ts",
                "b/last.ts",
                "unicode/\u{e000}.ts",
                "unicode/\u{10000}.ts",
            ]
            .map(|name| JsString::from(normalized_display(&tree.path(name))));
            assert_eq!(
                config
                    .read_directory(
                        nested.as_js(),
                        &[".ts"],
                        None,
                        Some(&ancestor_include),
                        Some(2)
                    )
                    .unwrap(),
                overlapping
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn native_config_discovery_keeps_the_first_symlink_spelling_and_stops_cycles() {
    use tsc_diagnostics::JsString;
    use tsc_program::{CompilerConfigHost, ConfigParseHost};

    let tree = TempTree::new();
    for name in ["root.ts", "z-real/one.ts", "z-real/deep/two.ts"] {
        let path = tree.path(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"export {};\n").unwrap();
    }
    std::os::unix::fs::symlink(tree.path("z-real"), tree.path("a-link")).unwrap();
    std::os::unix::fs::symlink(tree.root(), tree.path("z-real/back")).unwrap();
    let filesystem = FsCompilerHost::new(tree.root(), true).unwrap();
    let config = CompilerConfigHost::new(&filesystem);
    let base = JsString::from(normalized_display(tree.root()));
    let includes = [JsString::from("**/*.ts")];
    let expected = ["root.ts", "a-link/one.ts", "a-link/deep/two.ts"]
        .map(|name| JsString::from(normalized_display(&tree.path(name))));
    for _ in 0..4 {
        assert_eq!(
            config
                .read_directory(base.as_js(), &[".ts"], None, Some(&includes), None)
                .unwrap(),
            expected
        );
    }
}

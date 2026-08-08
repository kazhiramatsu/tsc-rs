use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_TEMP_TREE: AtomicU64 = AtomicU64::new(0);

struct TempTree {
    root: PathBuf,
}

impl TempTree {
    fn new() -> Self {
        loop {
            let sequence = NEXT_TEMP_TREE.fetch_add(1, Ordering::Relaxed);
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock is after the Unix epoch")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "tsc-rs-cli-{timestamp}-{sequence}-{}",
                std::process::id()
            ));
            match fs::create_dir(&root) {
                Ok(()) => return Self { root },
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create CLI temp tree: {error}"),
            }
        }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.root) {
            if !std::thread::panicking() {
                panic!("remove CLI temp tree {}: {error}", self.root.display());
            }
        }
    }
}

fn run(tree: &TempTree, arguments: &[&str]) -> std::process::Output {
    run_from(tree, ".", arguments)
}

fn run_from(tree: &TempTree, relative_directory: &str, arguments: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_tsc-rs"))
        .current_dir(tree.path(relative_directory))
        .args(arguments)
        .output()
        .expect("run tsc-rs binary")
}

fn run_typescript(tree: &TempTree, arguments: &[&str]) -> std::process::Output {
    run_typescript_from(tree, ".", arguments)
}

fn run_typescript_from(
    tree: &TempTree,
    relative_directory: &str,
    arguments: &[&str],
) -> std::process::Output {
    let bundle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("vendor/typescript-6.0.3/lib/_tsc.js");
    Command::new("node")
        .current_dir(tree.path(relative_directory))
        .arg(bundle)
        .args(arguments)
        .output()
        .expect("run vendored TypeScript compiler")
}

fn run_typescript_no_color(tree: &TempTree, arguments: &[&str]) -> std::process::Output {
    let bundle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("vendor/typescript-6.0.3/lib/_tsc.js");
    Command::new("node")
        .current_dir(&tree.root)
        .env("NO_COLOR", "1")
        .arg(bundle)
        .args(arguments)
        .output()
        .expect("run vendored TypeScript compiler without color")
}

fn utf16le_with_bom(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xff, 0xfe];
    bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    bytes
}

fn snapshot_files(root: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    fn visit(
        root: &std::path::Path,
        current: &std::path::Path,
        files: &mut Vec<(String, Vec<u8>)>,
    ) {
        let mut entries = fs::read_dir(current)
            .expect("read snapshot directory")
            .map(|entry| entry.expect("read snapshot entry").path())
            .collect::<Vec<_>>();
        entries.sort();
        for path in entries {
            let relative = path
                .strip_prefix(root)
                .expect("snapshot entry is under root")
                .to_string_lossy()
                .into_owned();
            if path.is_dir() {
                visit(root, &path, files);
            } else {
                files.push((relative, fs::read(&path).expect("read snapshot file")));
            }
        }
    }

    let mut files = Vec::new();
    visit(root, root, &mut files);
    files
}

fn compiler_current_directory(tree: &TempTree) -> PathBuf {
    fs::canonicalize(&tree.root).expect("canonicalize compiler current directory")
}

fn assert_typescript_parity(tree: &TempTree, rust_arguments: &[&str], ts_arguments: &[&str]) {
    let rust = run(tree, rust_arguments);
    let typescript = run_typescript(tree, ts_arguments);
    assert_eq!(rust.status.code(), typescript.status.code());
    assert_eq!(rust.stdout, typescript.stdout);
    assert_eq!(rust.stderr, typescript.stderr);
}

fn assert_typescript_parity_from(
    tree: &TempTree,
    relative_directory: &str,
    rust_arguments: &[&str],
    ts_arguments: &[&str],
) {
    let rust = run_from(tree, relative_directory, rust_arguments);
    let typescript = run_typescript_from(tree, relative_directory, ts_arguments);
    assert_eq!(rust.status.code(), typescript.status.code());
    assert_eq!(rust.stdout, typescript.stdout);
    assert_eq!(rust.stderr, typescript.stderr);
}

fn strip_ansi_sgr(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut in_escape = false;
    for character in text.chars() {
        if in_escape {
            if character.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else if character == '\u{1b}' {
            in_escape = true;
        } else {
            output.push(character);
        }
    }
    output
}

#[test]
fn config_and_include_discovery_run_through_the_production_binary() {
    let tree = TempTree::new();
    fs::create_dir(tree.path("src")).expect("create source directory");
    fs::write(tree.path("src/main.ts"), "const value: number = 1;\n").expect("write source");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"lib":["es5"]},"include":["src/**/*.ts"]}"#,
    )
    .expect("write config");

    let output = run(&tree, &[]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn config_without_no_emit_emits_and_command_line_no_emit_keeps_the_h0_route() {
    let tree = TempTree::new();
    fs::write(tree.path("main.ts"), "const value: number = 1;\n").expect("write source");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"esnext","module":"preserve","lib":["es5"]},"files":["main.ts"]}"#,
    )
    .expect("write config");

    let emitting = run(&tree, &["-p", "tsconfig.json"]);
    assert_eq!(emitting.status.code(), Some(0));
    assert_eq!(
        fs::read(tree.path("main.js")).expect("read emitted JavaScript"),
        b"const value = 1;\n"
    );
    assert!(emitting.stdout.is_empty());
    assert!(emitting.stderr.is_empty());

    fs::remove_file(tree.path("main.js")).expect("remove first emitted output");
    let with_override = run(&tree, &["--noEmit", "-p", "tsconfig.json"]);
    assert_eq!(with_override.status.code(), Some(0));
    assert!(!tree.path("main.js").exists());
    assert!(with_override.stdout.is_empty());
    assert!(with_override.stderr.is_empty());
}

#[test]
fn command_line_emit_options_override_config_values_before_loading() {
    let tree = TempTree::new();
    fs::write(tree.path("main.ts"), "export const value: number = 1;\n").expect("write source");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"target":"es2025","module":"esnext","lib":["es5"]},"files":["main.ts"]}"#,
    )
    .expect("write config");

    let output = run(
        &tree,
        &[
            "--noEmit=false",
            "--target",
            "esnext",
            "--module",
            "preserve",
            "--emitBOM",
            "--newLine",
            "crlf",
            "--listEmittedFiles",
            "-p",
            "tsconfig.json",
        ],
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        fs::read(tree.path("main.js")).expect("read overridden output"),
        [&[0xEF, 0xBB, 0xBF][..], &b"export const value = 1;\r\n"[..],].concat()
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 status output"),
        format!(
            "TSFILE: {}/main.js\n",
            compiler_current_directory(&tree).display()
        )
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn explicit_files_require_ignore_config_when_a_project_is_present() {
    let tree = TempTree::new();
    fs::write(tree.path("main.ts"), "const value: number = 1;\n").expect("write source");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"lib":["es5"]},"files":["main.ts"]}"#,
    )
    .expect("write config");

    let without_ignore = run(&tree, &["--noEmit", "main.ts"]);
    assert_eq!(without_ignore.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&without_ignore.stdout).contains("TS5112"));

    let with_ignore = run(&tree, &["--noEmit", "--ignoreConfig", "main.ts"]);
    assert_eq!(with_ignore.status.code(), Some(0));
    assert!(with_ignore.stdout.is_empty());
    assert!(with_ignore.stderr.is_empty());
}

#[test]
fn semantic_diagnostics_are_stdout_and_exit_two() {
    let tree = TempTree::new();
    fs::write(tree.path("main.ts"), "const value: number = 'wrong';\n").expect("write source");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"lib":["es5"]},"files":["main.ts"]}"#,
    )
    .expect("write config");

    let output = run(&tree, &[]);
    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("TS2322"), "{stdout}");
    assert!(stdout.contains("main.ts(1,7):"), "{stdout}");
    assert!(
        !stdout.contains('~'),
        "plain output unexpectedly had context: {stdout}"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn no_emit_cli_does_not_write_project_outputs() {
    let tree = TempTree::new();
    fs::write(tree.path("main.ts"), "const value: number = 1;\n").expect("write source");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"lib":["es5"]},"files":["main.ts"]}"#,
    )
    .expect("write config");
    let before = snapshot_files(&tree.root);

    let output = run(&tree, &["-p", "tsconfig.json"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert_eq!(snapshot_files(&tree.root), before);
}

#[test]
fn explicit_root_emit_applies_bom_and_reports_the_absolute_output() {
    let tree = TempTree::new();
    fs::write(tree.path("main.ts"), "export const value: number = 1;\n").expect("write source");

    let output = run(
        &tree,
        &[
            "--ignoreConfig",
            "--target",
            "esnext",
            "--module",
            "preserve",
            "--emitBOM",
            "--listEmittedFiles",
            "main.ts",
        ],
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        fs::read(tree.path("main.js")).expect("read emitted output"),
        [&[0xEF, 0xBB, 0xBF][..], &b"export const value = 1;\n"[..],].concat()
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 status output"),
        format!(
            "TSFILE: {}/main.js\n",
            compiler_current_directory(&tree).display()
        )
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn semantic_diagnostics_still_generate_output_and_exit_two() {
    let tree = TempTree::new();
    fs::write(tree.path("error.ts"), "const value: number = 'wrong';\n").expect("write source");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"esnext","module":"preserve","lib":["es5"],"listEmittedFiles":true},"files":["error.ts"]}"#,
    )
    .expect("write config");

    let output = run(&tree, &["-p", "tsconfig.json"]);
    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 diagnostics");
    assert!(stdout.contains("TS2322"), "{stdout}");
    assert!(
        stdout.ends_with(&format!(
            "TSFILE: {}/error.js\n",
            compiler_current_directory(&tree).display()
        )),
        "{stdout}"
    );
    assert_eq!(
        fs::read(tree.path("error.js")).expect("diagnostic emit output"),
        b"const value = 'wrong';\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn no_emit_on_error_skips_files_and_uses_exit_one() {
    let tree = TempTree::new();
    fs::write(tree.path("error.ts"), "const value: number = 'wrong';\n").expect("write source");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"esnext","module":"preserve","lib":["es5"],"noEmitOnError":true,"listEmittedFiles":true},"files":["error.ts"]}"#,
    )
    .expect("write config");

    let output = run(&tree, &["-p", "tsconfig.json"]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 diagnostics");
    assert!(stdout.contains("TS2322"), "{stdout}");
    assert!(!stdout.contains("TSFILE:"), "{stdout}");
    assert!(!tree.path("error.js").exists());
    assert!(output.stderr.is_empty());
}

#[test]
fn filesystem_write_failure_reports_ts5033_continues_and_lists_attempted_files() {
    let tree = TempTree::new();
    fs::write(tree.path("first.ts"), "export const first: number = 1;\n")
        .expect("write first source");
    fs::write(tree.path("second.ts"), "export const second: number = 2;\n")
        .expect("write second source");
    fs::create_dir(tree.path("first.js")).expect("block first output with a directory");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"esnext","module":"preserve","lib":["es5"],"listEmittedFiles":true},"files":["first.ts","second.ts"]}"#,
    )
    .expect("write config");

    let output = run(&tree, &["-p", "tsconfig.json"]);
    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 diagnostics");
    assert!(stdout.contains("TS5033"), "{stdout}");
    #[cfg(unix)]
    assert!(
        stdout.contains("EISDIR: illegal operation on a directory"),
        "{stdout}"
    );
    let current_directory = compiler_current_directory(&tree);
    let first_status = format!("TSFILE: {}/first.js", current_directory.display());
    let second_status = format!("TSFILE: {}/second.js", current_directory.display());
    let first_status = stdout.find(&first_status).expect("first TSFILE status");
    let second_status = stdout.find(&second_status).expect("second TSFILE status");
    assert!(first_status < second_status, "{stdout}");
    assert!(tree.path("first.js").is_dir());
    assert_eq!(
        fs::read(tree.path("second.js")).expect("later output still written"),
        b"export const second = 2;\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn missing_explicit_root_preserves_command_line_spelling() {
    let tree = TempTree::new();
    let output = run(&tree, &["--ignoreConfig", "--noEmit", "missing.ts"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        output.stdout,
        b"error TS6053: File 'missing.ts' not found.\n  The file is in the program because:\n    Root file specified for compilation\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
#[ignore = "local H0 CLI oracle audit; requires the pinned Node runtime"]
fn no_emit_cli_encoding_matrix_matches_vendored_typescript() {
    let utf8_bom_tree = TempTree::new();
    let source = "const value: number = 'wrong';\n";
    let config = r#"{"compilerOptions":{"noEmit":true,"lib":["es5"]},"files":["main.ts"]}"#;
    let mut source_bytes = vec![0xef, 0xbb, 0xbf];
    source_bytes.extend_from_slice(source.as_bytes());
    fs::write(utf8_bom_tree.path("main.ts"), source_bytes).expect("write UTF-8 BOM source");
    let mut config_bytes = vec![0xef, 0xbb, 0xbf];
    config_bytes.extend_from_slice(config.as_bytes());
    fs::write(utf8_bom_tree.path("tsconfig.json"), config_bytes).expect("write UTF-8 BOM config");
    assert_typescript_parity(
        &utf8_bom_tree,
        &["--noEmit", "-p", "tsconfig.json"],
        &["--noEmit", "-p", "tsconfig.json"],
    );

    let utf16le_tree = TempTree::new();
    fs::write(utf16le_tree.path("main.ts"), utf16le_with_bom(source))
        .expect("write UTF-16LE source");
    fs::write(utf16le_tree.path("tsconfig.json"), utf16le_with_bom(config))
        .expect("write UTF-16LE config");
    assert_typescript_parity(
        &utf16le_tree,
        &["--pretty", "false", "--noEmit", "-p", "tsconfig.json"],
        &["--pretty", "false", "--noEmit", "-p", "tsconfig.json"],
    );
}

#[cfg(unix)]
#[test]
#[ignore = "local H0 CLI oracle audit; requires the pinned Node runtime"]
fn no_emit_cli_symlink_and_package_mode_matrix_matches_vendored_typescript() {
    use std::os::unix::fs::symlink;

    let symlink_tree = TempTree::new();
    fs::create_dir_all(symlink_tree.path("src")).expect("create symlink source directory");
    fs::write(
        symlink_tree.path("src/real.ts"),
        "export const value: number = 1;\n",
    )
    .expect("write symlink target");
    symlink(
        symlink_tree.path("src/real.ts"),
        symlink_tree.path("src/link.ts"),
    )
    .expect("create symlink source");
    fs::write(
        symlink_tree.path("main.ts"),
        "import { value } from './src/link';\nconst checked: number = value;\n",
    )
    .expect("write symlink importer");
    for preserve_symlinks in [false, true] {
        let config = format!(
            r#"{{"compilerOptions":{{"noEmit":true,"lib":["es5"],"preserveSymlinks":{preserve_symlinks}}},"files":["main.ts"]}}"#
        );
        fs::write(symlink_tree.path("tsconfig.json"), config).expect("write symlink config");
        assert_typescript_parity(
            &symlink_tree,
            &["-p", "tsconfig.json"],
            &["-p", "tsconfig.json"],
        );
    }

    let package_tree = TempTree::new();
    fs::create_dir_all(package_tree.path("node_modules/pkg")).expect("create package directory");
    fs::write(
        package_tree.path("main.ts"),
        "import { value } from 'pkg';\nconst checked: number = value;\n",
    )
    .expect("write package importer");
    fs::write(
        package_tree.path("node_modules/pkg/package.json"),
        r#"{"name":"pkg","exports":{".":{"types":"./index.d.ts","default":"./index.js"}}}"#,
    )
    .expect("write package manifest");
    fs::write(
        package_tree.path("node_modules/pkg/index.d.ts"),
        "export declare const value: number;\n",
    )
    .expect("write package declaration");
    fs::write(
        package_tree.path("node_modules/pkg/index.js"),
        "exports.value = 1;\n",
    )
    .expect("write package implementation");
    fs::write(
        package_tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"target":"es2022","module":"node16","moduleResolution":"node16"},"files":["main.ts"]}"#,
    )
    .expect("write package mode config");
    assert_typescript_parity(
        &package_tree,
        &["-p", "tsconfig.json"],
        &["-p", "tsconfig.json"],
    );

    let types_tree = TempTree::new();
    fs::create_dir_all(types_tree.path("node_modules/@types/globals"))
        .expect("create automatic type package");
    fs::write(
        types_tree.path("node_modules/@types/globals/index.d.ts"),
        "declare const fromTypes: number;\n",
    )
    .expect("write automatic type declaration");
    fs::write(
        types_tree.path("main.ts"),
        "const checked: number = fromTypes;\n",
    )
    .expect("write automatic type importer");
    fs::write(
        types_tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"lib":["es5"],"types":["globals"]},"files":["main.ts"]}"#,
    )
    .expect("write automatic type config");
    assert_typescript_parity(
        &types_tree,
        &["-p", "tsconfig.json"],
        &["-p", "tsconfig.json"],
    );
}

#[test]
#[ignore = "local H0 CLI oracle audit; requires the pinned Node runtime"]
fn no_emit_cli_case_only_alias_matrix_matches_vendored_typescript() {
    let tree = TempTree::new();
    fs::write(
        tree.path("main.ts"),
        "import { value } from './Value';\nconst checked: number = value;\n",
    )
    .expect("write case-alias importer");
    fs::write(tree.path("value.ts"), "export const value: number = 1;\n")
        .expect("write case-alias target");

    for root_field in [
        r#", "files":["main.ts","value.ts"]"#,
        r#", "include":["**/*.ts"]"#,
        r#", "files":["main.ts"],"include":["**/*.ts"]"#,
        "",
    ] {
        for casing in [None, Some(false), Some(true)] {
            let casing_option = casing.map_or_else(String::new, |value| {
                format!(",\"forceConsistentCasingInFileNames\":{value}")
            });
            fs::write(
                tree.path("tsconfig.json"),
                format!(
                    r#"{{"compilerOptions":{{"noEmit":true,"lib":["es5"]{casing_option}}}{root_field}}}"#
                ),
            )
            .expect("write case-alias config");
            assert_typescript_parity(&tree, &["-p", "tsconfig.json"], &["-p", "tsconfig.json"]);
            let rust = run(&tree, &["--pretty", "-p", "tsconfig.json"]);
            let typescript =
                run_typescript_no_color(&tree, &["--noEmit", "--pretty", "-p", "tsconfig.json"]);
            assert_eq!(rust.status.code(), typescript.status.code());
            assert_eq!(rust.stdout, typescript.stdout);
            assert_eq!(rust.stderr, typescript.stderr);
        }
    }
}

#[test]
#[ignore = "local H0 CLI oracle audit; requires the pinned Node runtime"]
fn no_emit_cli_current_directory_discovery_matches_vendored_typescript() {
    let tree = TempTree::new();
    fs::create_dir_all(tree.path("src/nested")).expect("create current-directory fixture");
    fs::write(tree.path("src/main.ts"), "const value: number = 'wrong';\n")
        .expect("write current-directory source");
    fs::write(
        tree.path("src/nested/ignored.ts"),
        "const ignored: number = 'wrong';\n",
    )
    .expect("write excluded current-directory source");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"lib":["es5"]},"include":["src/**/*.ts"],"exclude":["src/nested"]}"#,
    )
    .expect("write current-directory config");
    assert_typescript_parity_from(&tree, "src", &[], &[]);
    assert_typescript_parity_from(&tree, "src", &["--pretty", "false"], &["--pretty", "false"]);
    assert_typescript_parity_from(&tree, "src", &["-p", ".."], &["-p", ".."]);
}

#[test]
#[ignore = "local H0 CLI oracle audit; requires the pinned Node runtime"]
fn no_emit_cli_root_dirs_and_node_next_matrix_matches_vendored_typescript() {
    let root_dirs_tree = TempTree::new();
    fs::create_dir_all(root_dirs_tree.path("src/project")).expect("create source root");
    fs::create_dir_all(root_dirs_tree.path("generated/src/project"))
        .expect("create generated root");
    fs::write(
        root_dirs_tree.path("src/file1.ts"),
        "import { x } from './project/file3';\nconst value: string = x;\n",
    )
    .expect("write rootDirs importer");
    fs::write(
        root_dirs_tree.path("src/project/file2.d.ts"),
        "export declare const x: number;\n",
    )
    .expect("write source declaration");
    fs::write(
        root_dirs_tree.path("generated/src/project/file3.ts"),
        "export { x } from '../file2';\n",
    )
    .expect("write generated implementation");
    fs::write(
        root_dirs_tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"lib":["es5"],"rootDirs":["src","generated/src"]},"files":["src/file1.ts"]}"#,
    )
    .expect("write rootDirs config");
    assert_typescript_parity(
        &root_dirs_tree,
        &["-p", "tsconfig.json"],
        &["-p", "tsconfig.json"],
    );

    let node_next_tree = TempTree::new();
    fs::create_dir_all(node_next_tree.path("node_modules/pkg")).expect("create NodeNext package");
    fs::write(
        node_next_tree.path("main.ts"),
        "import { value } from 'pkg';\nconst checked: number = value;\n",
    )
    .expect("write NodeNext importer");
    fs::write(
        node_next_tree.path("node_modules/pkg/package.json"),
        r#"{"name":"pkg","type":"module","exports":{".":{"types":"./index.d.ts","default":"./index.js"}}}"#,
    )
    .expect("write NodeNext package manifest");
    fs::write(
        node_next_tree.path("node_modules/pkg/index.d.ts"),
        "export declare const value: number;\n",
    )
    .expect("write NodeNext declaration");
    fs::write(
        node_next_tree.path("node_modules/pkg/index.js"),
        "export const value = 1;\n",
    )
    .expect("write NodeNext implementation");
    fs::write(
        node_next_tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"target":"es2022","module":"nodenext","moduleResolution":"nodenext"},"files":["main.ts"]}"#,
    )
    .expect("write NodeNext config");
    assert_typescript_parity(
        &node_next_tree,
        &["-p", "tsconfig.json"],
        &["-p", "tsconfig.json"],
    );
    for module in ["node18", "node20"] {
        fs::write(
            node_next_tree.path("tsconfig.json"),
            format!(
                r#"{{"compilerOptions":{{"noEmit":true,"target":"es2022","module":"{module}"}},"files":["main.ts"]}}"#
            ),
        )
        .expect("write Node module-mode config");
        assert_typescript_parity(
            &node_next_tree,
            &["-p", "tsconfig.json"],
            &["-p", "tsconfig.json"],
        );
    }
}

#[test]
fn config_option_diagnostics_are_rendered_alongside_semantic_diagnostics() {
    let tree = TempTree::new();
    fs::write(tree.path("main.ts"), "const value: number = 'wrong';\n").expect("write source");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"lib":["es5"],"moduleResolution":"node"},"files":["main.ts"]}"#,
    )
    .expect("write config");

    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let option = stdout
        .find("tsconfig.json(1,68): error TS5107:")
        .unwrap_or_else(|| panic!("missing TS5107 in output: {stdout}"));
    let semantic = stdout
        .find("main.ts(1,7): error TS2322:")
        .unwrap_or_else(|| panic!("missing TS2322 in output: {stdout}"));
    // The formatter applies TypeScript's global diagnostic sort by file name
    // after the bucket driver has assembled its options-before-semantic view.
    assert!(
        semantic < option,
        "diagnostic rendering order drifted: {stdout}"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn pretty_false_uses_plain_output_and_pretty_true_uses_context() {
    let tree = TempTree::new();
    fs::write(tree.path("main.ts"), "const value: number = 'wrong';\n").expect("write source");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"lib":["es5"]},"files":["main.ts"]}"#,
    )
    .expect("write config");

    let plain = run(&tree, &["--pretty", "false"]);
    assert_eq!(plain.status.code(), Some(2));
    let plain_stdout = String::from_utf8_lossy(&plain.stdout);
    assert!(plain_stdout.starts_with("main.ts(1,7): error TS2322:"));
    assert!(!plain_stdout.contains('~'));

    let pretty = run(&tree, &["--pretty", "true"]);
    assert_eq!(pretty.status.code(), Some(2));
    let pretty_stdout = String::from_utf8_lossy(&pretty.stdout);
    let pretty_text = strip_ansi_sgr(&pretty_stdout);
    assert!(pretty_text.contains("main.ts:1:7 - error TS2322:"));
    assert!(pretty_text.contains('~'));
    assert!(pretty_text.contains("Found 1 error in main.ts:1"));
}

#[test]
fn pretty_configured_type_diagnostic_renders_ts1419_related_context() {
    let tree = TempTree::new();
    fs::write(tree.path("main.ts"), "export {};\n").expect("write source");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"lib":["es5"],"types":["missing"]},"files":["main.ts"]}"#,
    )
    .expect("write configured types");

    let output = run(&tree, &["--pretty", "true", "-p", "tsconfig.json"]);
    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\u{1b}[96mtsconfig.json\u{1b}[0m"));
    assert!(stdout.matches("\u{1b}[96m").count() >= 2);
    let rendered = strip_ansi_sgr(&stdout);
    assert!(rendered.contains("error TS2688:"));
    assert!(rendered.contains("tsconfig.json:1:"));
    assert!(rendered.contains("File is entry point of type library specified here."));
    assert!(rendered.contains("Found 1 error.\n\n"));
    assert!(!rendered.contains("in the same file"));
}

#[test]
fn unsupported_options_are_exit_two_and_version_is_lightweight() {
    let tree = TempTree::new();
    let output = run(&tree, &["--watch"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unsupported option"));

    let version = run(&tree, &["--version"]);
    assert_eq!(version.status.code(), Some(0));
    assert_eq!(version.stdout, b"Version 6.0.3\n");
    assert!(version.stderr.is_empty());
}

#[test]
fn missing_project_selection_uses_typescript_command_line_diagnostics() {
    let tree = TempTree::new();
    fs::create_dir(tree.path("empty")).expect("create empty project directory");

    let missing_file = run(&tree, &["-p", "missing.json"]);
    assert_eq!(missing_file.status.code(), Some(1));
    assert_eq!(
        missing_file.stdout,
        b"error TS5058: The specified path does not exist: 'missing.json'.\n"
    );
    assert!(missing_file.stderr.is_empty());

    let missing_config = run(&tree, &["-p", "empty"]);
    assert_eq!(missing_config.status.code(), Some(1));
    assert_eq!(
        missing_config.stdout,
        b"error TS5057: Cannot find a tsconfig.json file at the specified directory: 'empty'.\n"
    );
    assert!(missing_config.stderr.is_empty());
}

#[test]
#[ignore = "local H0 CLI oracle audit; requires the pinned Node runtime"]
fn no_emit_cli_matches_vendored_typescript_plain_output() {
    let tree = TempTree::new();
    fs::write(tree.path("main.ts"), "const value: number = 'wrong';\n").expect("write source");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"lib":["es5"]},"files":["main.ts"]}"#,
    )
    .expect("write config");

    let rust = run(&tree, &["-p", "tsconfig.json"]);
    let typescript = run_typescript(&tree, &["--noEmit", "-p", "tsconfig.json"]);
    assert_eq!(rust.status.code(), typescript.status.code());
    assert_eq!(rust.stdout, typescript.stdout);
    assert_eq!(rust.stderr, typescript.stderr);
}

#[test]
#[ignore = "local H0 CLI oracle audit; requires the pinned Node runtime"]
fn no_emit_cli_matches_vendored_typescript_pretty_output_without_color() {
    let tree = TempTree::new();
    fs::write(tree.path("main.ts"), "const value: number = 'wrong';\n").expect("write source");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"lib":["es5"]},"files":["main.ts"]}"#,
    )
    .expect("write config");

    let rust = run(&tree, &["--pretty", "-p", "tsconfig.json"]);
    let typescript =
        run_typescript_no_color(&tree, &["--noEmit", "--pretty", "-p", "tsconfig.json"]);
    assert_eq!(rust.status.code(), typescript.status.code());
    assert_eq!(rust.stdout, typescript.stdout);
    assert_eq!(rust.stderr, typescript.stderr);
}

#[test]
#[ignore = "local H0 CLI oracle audit; requires the pinned Node runtime"]
fn no_emit_cli_configured_type_related_information_matches_vendored_typescript() {
    let tree = TempTree::new();
    fs::write(tree.path("main.ts"), "export {};\n").expect("write source");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"lib":["es5"],"types":["missing"]},"files":["main.ts"]}"#,
    )
    .expect("write configured types");

    assert_typescript_parity(
        &tree,
        &["--pretty", "false", "-p", "tsconfig.json"],
        &["--pretty", "false", "-p", "tsconfig.json"],
    );
    let rust = run(&tree, &["--pretty", "-p", "tsconfig.json"]);
    let typescript =
        run_typescript_no_color(&tree, &["--noEmit", "--pretty", "-p", "tsconfig.json"]);
    assert_eq!(rust.status.code(), typescript.status.code());
    assert_eq!(rust.stdout, typescript.stdout);
    assert_eq!(rust.stderr, typescript.stderr);
}

#[test]
#[ignore = "local H0 CLI oracle audit; requires the pinned Node runtime"]
fn no_emit_cli_config_and_selection_matrix_matches_vendored_typescript() {
    let tree = TempTree::new();
    fs::create_dir_all(tree.path("src/generated")).expect("create source directories");
    fs::write(tree.path("src/main.ts"), "const value: number = 'wrong';\n")
        .expect("write included source");
    fs::write(
        tree.path("src/generated/ignored.ts"),
        "const ignored: number = 'wrong';\n",
    )
    .expect("write excluded source");
    fs::write(
        tree.path("base.json"),
        r#"{"compilerOptions":{"noEmit":true,"lib":["es5"]},"include":["src/**/*.ts"],"exclude":["src/generated"]}"#,
    )
    .expect("write base config");
    fs::write(tree.path("tsconfig.json"), r#"{"extends":"./base.json"}"#)
        .expect("write extending config");
    assert_typescript_parity(
        &tree,
        &["-p", "tsconfig.json"],
        &["--noEmit", "-p", "tsconfig.json"],
    );

    let override_tree = TempTree::new();
    fs::write(
        override_tree.path("main.ts"),
        "const value: number = 'wrong';\n",
    )
    .expect("write override source");
    fs::write(
        override_tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":false,"lib":["es5"]},"files":["main.ts"]}"#,
    )
    .expect("write override config");
    assert_typescript_parity(
        &override_tree,
        &["--noEmit", "-p", "tsconfig.json"],
        &["--noEmit", "-p", "tsconfig.json"],
    );
    assert_typescript_parity(
        &override_tree,
        &["--ignoreConfig", "--noEmit", "main.ts"],
        &["--ignoreConfig", "--noEmit", "main.ts"],
    );

    let missing = TempTree::new();
    fs::create_dir(missing.path("empty")).expect("create missing project directory");
    assert_typescript_parity(&missing, &["-p", "missing.json"], &["-p", "missing.json"]);
    assert_typescript_parity(&missing, &["-p", "empty"], &["-p", "empty"]);
    assert_typescript_parity(
        &missing,
        &["--pretty", "-p", "missing.json"],
        &["--pretty", "-p", "missing.json"],
    );

    let malformed = TempTree::new();
    fs::write(malformed.path("tsconfig.json"), "{\"compilerOptions\":}\n")
        .expect("write malformed config");
    assert_typescript_parity(
        &malformed,
        &["--pretty", "-p", "tsconfig.json"],
        &["--pretty", "-p", "tsconfig.json"],
    );
}

#[test]
#[ignore = "local H0 CLI oracle audit; requires the pinned Node runtime"]
fn no_emit_cli_extended_config_matrix_matches_vendored_typescript() {
    let extends_tree = TempTree::new();
    fs::create_dir_all(extends_tree.path("src/nested")).expect("create source directories");
    fs::create_dir_all(extends_tree.path("configs")).expect("create config directory");
    fs::write(
        extends_tree.path("src/main.ts"),
        "const value: number = 'wrong';\n",
    )
    .expect("write source");
    fs::write(
        extends_tree.path("src/nested/ignored.ts"),
        "const ignored: number = 'wrong';\n",
    )
    .expect("write nested source");
    fs::write(
        extends_tree.path("configs/base.json"),
        r#"{"compilerOptions":{"noEmit":true,"lib":["es5"]},"include":["../src/**/*.ts"],"exclude":["../src/nested"]}"#,
    )
    .expect("write base config");
    fs::write(
        extends_tree.path("tsconfig.json"),
        r#"{"extends":"./configs/base.json"}"#,
    )
    .expect("write extending config");
    assert_typescript_parity(
        &extends_tree,
        &["-p", "tsconfig.json"],
        &["-p", "tsconfig.json"],
    );
    assert_typescript_parity(
        &extends_tree,
        &["--noEmit", "-p", "tsconfig.json"],
        &["--noEmit", "-p", "tsconfig.json"],
    );

    let nested_tree = TempTree::new();
    fs::create_dir_all(nested_tree.path("src/deep")).expect("create nested tree");
    fs::write(
        nested_tree.path("src/main.ts"),
        "const value: number = 'wrong';\n",
    )
    .expect("write nested root");
    fs::write(
        nested_tree.path("src/deep/other.ts"),
        "const other: number = 'wrong';\n",
    )
    .expect("write nested file");
    fs::write(
        nested_tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"lib":["es5"]},"include":["src/**/*.ts"],"exclude":["src/deep"]}"#,
    )
    .expect("write discovered config");
    assert_typescript_parity(&nested_tree, &[], &[]);
    assert_typescript_parity(&nested_tree, &["--noEmit"], &["--noEmit"]);

    let files_tree = TempTree::new();
    fs::write(
        files_tree.path("main.ts"),
        "const value: number = 'wrong';\n",
    )
    .expect("write files root");
    fs::write(
        files_tree.path("missing.ts"),
        "const missing: number = 'wrong';\n",
    )
    .expect("write second files root");
    fs::write(
        files_tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"lib":["es5"]},"files":["main.ts","missing.ts"]}"#,
    )
    .expect("write files config");
    assert_typescript_parity(
        &files_tree,
        &["-p", "tsconfig.json"],
        &["-p", "tsconfig.json"],
    );

    let jsconfig_tree = TempTree::new();
    fs::write(
        jsconfig_tree.path("main.js"),
        "/** @type {number} */\nconst value = 'wrong';\n",
    )
    .expect("write JavaScript source");
    fs::write(
        jsconfig_tree.path("jsconfig.json"),
        r#"{"compilerOptions":{"checkJs":true,"noEmit":true,"lib":["es5"]},"include":["**/*.js"]}"#,
    )
    .expect("write jsconfig");
    assert_typescript_parity(
        &jsconfig_tree,
        &["--noEmit", "-p", "jsconfig.json"],
        &["--noEmit", "-p", "jsconfig.json"],
    );

    let mapping_tree = TempTree::new();
    fs::create_dir_all(mapping_tree.path("src")).expect("create mapping source directory");
    fs::write(
        mapping_tree.path("src/main.ts"),
        "import { value } from '@app/value';\nconst checked: number = value;\n",
    )
    .expect("write mapping entry");
    fs::write(
        mapping_tree.path("src/value.ts"),
        "export const value = 1;\n",
    )
    .expect("write mapped module");
    fs::write(
        mapping_tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"lib":["es5"],"baseUrl":".","paths":{"@app/*":["src/*"]}},"include":["src/**/*.ts"]}"#,
    )
    .expect("write paths config");
    assert_typescript_parity(
        &mapping_tree,
        &["-p", "tsconfig.json"],
        &["-p", "tsconfig.json"],
    );

    let missing_roots_tree = TempTree::new();
    fs::write(
        missing_roots_tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"lib":["es5"]},"files":["missing.ts"]}"#,
    )
    .expect("write missing-roots config");
    assert_typescript_parity(
        &missing_roots_tree,
        &["-p", "tsconfig.json"],
        &["-p", "tsconfig.json"],
    );

    let empty_roots_tree = TempTree::new();
    fs::write(
        empty_roots_tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"lib":["es5"]},"files":[]}"#,
    )
    .expect("write empty-roots config");
    assert_typescript_parity(
        &empty_roots_tree,
        &["-p", "tsconfig.json"],
        &["-p", "tsconfig.json"],
    );

    assert_typescript_parity(
        &empty_roots_tree,
        &["--ignoreConfig", "--noEmit", "missing.ts"],
        &["--ignoreConfig", "--noEmit", "missing.ts"],
    );

    let missing_extends_tree = TempTree::new();
    fs::write(
        missing_extends_tree.path("tsconfig.json"),
        r#"{"extends":"./missing-base.json","compilerOptions":{"noEmit":true,"lib":["es5"]},"files":[]}"#,
    )
    .expect("write missing-extends config");
    assert_typescript_parity(
        &missing_extends_tree,
        &["-p", "tsconfig.json"],
        &["-p", "tsconfig.json"],
    );

    let circular_extends_tree = TempTree::new();
    fs::write(
        circular_extends_tree.path("tsconfig.json"),
        r#"{"extends":"./base.json","compilerOptions":{"noEmit":true,"lib":["es5"]},"files":[]}"#,
    )
    .expect("write circular primary config");
    fs::write(
        circular_extends_tree.path("base.json"),
        r#"{"extends":"./tsconfig.json"}"#,
    )
    .expect("write circular base config");
    assert_typescript_parity(
        &circular_extends_tree,
        &["-p", "tsconfig.json"],
        &["-p", "tsconfig.json"],
    );
}

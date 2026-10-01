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
        b"\"use strict\";\nconst value = 1;\n"
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
        b"\"use strict\";\nconst value = 'wrong';\n"
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
fn config_option_diagnostics_suppress_semantic_reporting_like_typescript() {
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
    assert!(
        stdout.contains("tsconfig.json(1,68): error TS5107:"),
        "{stdout}"
    );
    assert!(!stdout.contains("TS2322"), "{stdout}");
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
    assert_eq!(version.stdout, b"Version 7.1.0-dev-19dadef8\n");
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

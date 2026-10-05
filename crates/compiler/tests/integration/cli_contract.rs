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
        stdout.contains(
            "tsconfig.json(1,68): error TS5108: Option 'moduleResolution=node10' has been removed."
        ),
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
fn pretty_output_is_tsgo_diagnostic_writer_output() {
    // tsgo FormatDiagnosticWithColorAndContext for each diagnostic (related
    // locations carry their message, a blank line between them), then
    // WriteErrorSummaryText with the file table. The expected bytes are
    // tsgo's output for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("file1.ts"),
        "class Foo { }
const Bar = 3;
",
    )
    .expect("write file1");
    fs::write(
        tree.path("file2.ts"),
        "type Foo = number;
class Bar {}
",
    )
    .expect("write file2");
    fs::write(
        tree.path("file3.ts"),
        "type Foo = 54;
let Bar = 42
",
    )
    .expect("write file3");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{ "compilerOptions": { "target": "es2015", "noEmit": true, "types": [] }, "files": ["file1.ts", "file2.ts", "file3.ts"] }"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "true"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
    concat!(
            "\u{1b}[96mfile1.ts\u{1b}[0m:\u{1b}[93m1\u{1b}[0m:\u{1b}[93m7\u{1b}[0m - \u{1b}[91merror\u{1b}[0m\u{1b}[90m TS2300: \u{1b}[0mDuplicate identifier 'Foo'.\n",
            "\n",
            "\u{1b}[7m1\u{1b}[0m class Foo { }\n",
            "\u{1b}[7m \u{1b}[0m \u{1b}[91m      ~~~\u{1b}[0m\n",
            "\n",
            "  \u{1b}[96mfile2.ts\u{1b}[0m:\u{1b}[93m1\u{1b}[0m:\u{1b}[93m6\u{1b}[0m - 'Foo' was also declared here.\n",
            "    \u{1b}[7m1\u{1b}[0m type Foo = number;\n",
            "    \u{1b}[7m \u{1b}[0m \u{1b}[96m     ~~~\u{1b}[0m\n",
            "\n",
            "  \u{1b}[96mfile3.ts\u{1b}[0m:\u{1b}[93m1\u{1b}[0m:\u{1b}[93m6\u{1b}[0m - 'Foo' was also declared here.\n",
            "    \u{1b}[7m1\u{1b}[0m type Foo = 54;\n",
            "    \u{1b}[7m \u{1b}[0m \u{1b}[96m     ~~~\u{1b}[0m\n",
            "\n",
            "\u{1b}[96mfile1.ts\u{1b}[0m:\u{1b}[93m2\u{1b}[0m:\u{1b}[93m7\u{1b}[0m - \u{1b}[91merror\u{1b}[0m\u{1b}[90m TS2451: \u{1b}[0mCannot redeclare block-scoped variable 'Bar'.\n",
            "\n",
            "\u{1b}[7m2\u{1b}[0m const Bar = 3;\n",
            "\u{1b}[7m \u{1b}[0m \u{1b}[91m      ~~~\u{1b}[0m\n",
            "\n",
            "  \u{1b}[96mfile2.ts\u{1b}[0m:\u{1b}[93m2\u{1b}[0m:\u{1b}[93m7\u{1b}[0m - 'Bar' was also declared here.\n",
            "    \u{1b}[7m2\u{1b}[0m class Bar {}\n",
            "    \u{1b}[7m \u{1b}[0m \u{1b}[96m      ~~~\u{1b}[0m\n",
            "\n",
            "  \u{1b}[96mfile3.ts\u{1b}[0m:\u{1b}[93m2\u{1b}[0m:\u{1b}[93m5\u{1b}[0m - 'Bar' was also declared here.\n",
            "    \u{1b}[7m2\u{1b}[0m let Bar = 42\n",
            "    \u{1b}[7m \u{1b}[0m \u{1b}[96m    ~~~\u{1b}[0m\n",
            "\n",
            "\u{1b}[96mfile2.ts\u{1b}[0m:\u{1b}[93m1\u{1b}[0m:\u{1b}[93m6\u{1b}[0m - \u{1b}[91merror\u{1b}[0m\u{1b}[90m TS2300: \u{1b}[0mDuplicate identifier 'Foo'.\n",
            "\n",
            "\u{1b}[7m1\u{1b}[0m type Foo = number;\n",
            "\u{1b}[7m \u{1b}[0m \u{1b}[91m     ~~~\u{1b}[0m\n",
            "\n",
            "  \u{1b}[96mfile1.ts\u{1b}[0m:\u{1b}[93m1\u{1b}[0m:\u{1b}[93m7\u{1b}[0m - 'Foo' was also declared here.\n",
            "    \u{1b}[7m1\u{1b}[0m class Foo { }\n",
            "    \u{1b}[7m \u{1b}[0m \u{1b}[96m      ~~~\u{1b}[0m\n",
            "\n",
            "\u{1b}[96mfile2.ts\u{1b}[0m:\u{1b}[93m2\u{1b}[0m:\u{1b}[93m7\u{1b}[0m - \u{1b}[91merror\u{1b}[0m\u{1b}[90m TS2451: \u{1b}[0mCannot redeclare block-scoped variable 'Bar'.\n",
            "\n",
            "\u{1b}[7m2\u{1b}[0m class Bar {}\n",
            "\u{1b}[7m \u{1b}[0m \u{1b}[91m      ~~~\u{1b}[0m\n",
            "\n",
            "  \u{1b}[96mfile1.ts\u{1b}[0m:\u{1b}[93m2\u{1b}[0m:\u{1b}[93m7\u{1b}[0m - 'Bar' was also declared here.\n",
            "    \u{1b}[7m2\u{1b}[0m const Bar = 3;\n",
            "    \u{1b}[7m \u{1b}[0m \u{1b}[96m      ~~~\u{1b}[0m\n",
            "\n",
            "\u{1b}[96mfile3.ts\u{1b}[0m:\u{1b}[93m1\u{1b}[0m:\u{1b}[93m6\u{1b}[0m - \u{1b}[91merror\u{1b}[0m\u{1b}[90m TS2300: \u{1b}[0mDuplicate identifier 'Foo'.\n",
            "\n",
            "\u{1b}[7m1\u{1b}[0m type Foo = 54;\n",
            "\u{1b}[7m \u{1b}[0m \u{1b}[91m     ~~~\u{1b}[0m\n",
            "\n",
            "  \u{1b}[96mfile1.ts\u{1b}[0m:\u{1b}[93m1\u{1b}[0m:\u{1b}[93m7\u{1b}[0m - 'Foo' was also declared here.\n",
            "    \u{1b}[7m1\u{1b}[0m class Foo { }\n",
            "    \u{1b}[7m \u{1b}[0m \u{1b}[96m      ~~~\u{1b}[0m\n",
            "\n",
            "\u{1b}[96mfile3.ts\u{1b}[0m:\u{1b}[93m2\u{1b}[0m:\u{1b}[93m5\u{1b}[0m - \u{1b}[91merror\u{1b}[0m\u{1b}[90m TS2451: \u{1b}[0mCannot redeclare block-scoped variable 'Bar'.\n",
            "\n",
            "\u{1b}[7m2\u{1b}[0m let Bar = 42\n",
            "\u{1b}[7m \u{1b}[0m \u{1b}[91m    ~~~\u{1b}[0m\n",
            "\n",
            "  \u{1b}[96mfile1.ts\u{1b}[0m:\u{1b}[93m2\u{1b}[0m:\u{1b}[93m7\u{1b}[0m - 'Bar' was also declared here.\n",
            "    \u{1b}[7m2\u{1b}[0m const Bar = 3;\n",
            "    \u{1b}[7m \u{1b}[0m \u{1b}[96m      ~~~\u{1b}[0m\n",
            "\n",
            "\n",
            "Found 6 errors in 3 files.\n",
            "\n",
            "Errors  Files\n",
            "     2  file1.ts\u{1b}[90m:1\u{1b}[0m\n",
            "     2  file2.ts\u{1b}[90m:1\u{1b}[0m\n",
            "     2  file3.ts\u{1b}[90m:1\u{1b}[0m\n",
            "\n",
        )
    );
}

#[test]
fn javascript_es_module_declarations_follow_tsgo() {
    // tsgo builds a JavaScript file's declarations with the same transform as
    // TypeScript: `declare` on exported declarations, JSDoc types for
    // parameters and returns, JSDoc comments kept, a literal `const`
    // initializer. The expected bytes are tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("lib.js"),
        concat!(
            "/**\n",
            " * A point.\n",
            " */\n",
            "export class Point {\n",
            "    /**\n",
            "     * @param {number} x\n",
            "     * @param {number} y\n",
            "     */\n",
            "    constructor(x, y) {\n",
            "        void [x, y];\n",
            "    }\n",
            "    /** @returns {number} */\n",
            "    length() {\n",
            "        return 0;\n",
            "    }\n",
            "}\n",
            "\n",
            "/**\n",
            " * Adds two numbers.\n",
            " * @param {number} a\n",
            " * @param {number} [b]\n",
            " * @returns {number}\n",
            " */\n",
            "export function add(a, b = 0) {\n",
            "    return a + b;\n",
            "}\n",
            "\n",
            "/** @type {string[]} */\n",
            "export const names = [];\n",
            "\n",
            "export const answer = 42;\n",
            "\n",
            "export default function main() {}\n",
        ),
    )
    .expect("write lib");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"es2015","allowJs":true,"checkJs":true,"declaration":true,"emitDeclarationOnly":true,"types":[],"outDir":"out"},"files":["lib.js"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        fs::read_to_string(tree.path("out/lib.d.ts")).expect("read declarations"),
        concat!(
            "/**\n",
            " * A point.\n",
            " */\n",
            "export declare class Point {\n",
            "    /**\n",
            "     * @param {number} x\n",
            "     * @param {number} y\n",
            "     */\n",
            "    constructor(x: number, y: number);\n",
            "    /** @returns {number} */\n",
            "    length(): number;\n",
            "}\n",
            "/**\n",
            " * Adds two numbers.\n",
            " * @param {number} a\n",
            " * @param {number} [b]\n",
            " * @returns {number}\n",
            " */\n",
            "export declare function add(a: number, b?: number): number;\n",
            "/** @type {string[]} */\n",
            "export declare const names: string[];\n",
            "export declare const answer = 42;\n",
            "export default function main(): void;\n",
        )
    );
}

#[test]
fn javascript_hosted_tags_and_this_members_follow_tsgo() {
    // tsgo's declaration transform reads what its parser reparses from JSDoc:
    // members from `this.x = …` assignments (only `static` kept, a member a
    // base class already has left out), modifiers from `@private`,
    // `@protected` and `@readonly`, `implements` from `@implements`, type
    // arguments from `@augments` and type parameters from `@template`. The
    // expected bytes are tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("lib.js"),
        concat!(
            "export class Base {\n",
            "    constructor() {\n",
            "        /** @type {number} */\n",
            "        this.shared = 1;\n",
            "    }\n",
            "    get value() {\n",
            "        return 1;\n",
            "    }\n",
            "}\n",
            "\n",
            "export class Point extends Base {\n",
            "    /** @private */\n",
            "    static secret() {}\n",
            "    /** @protected */\n",
            "    static helper() {}\n",
            "    /**\n",
            "     * @readonly\n",
            "     * @type {string}\n",
            "     */\n",
            "    label = \"p\";\n",
            "    constructor() {\n",
            "        super();\n",
            "        /** The x coordinate. */\n",
            "        this.x = 0;\n",
            "        this.y = \"y\";\n",
            "        this.shared = 2;\n",
            "        this.method = function () {};\n",
            "        if (this.x) {\n",
            "            this.later = true;\n",
            "        }\n",
            "        const nested = function () {\n",
            "            this.ignored = 1;\n",
            "        };\n",
            "        const arrow = () => {\n",
            "            this.fromArrow = 1;\n",
            "        };\n",
            "        void [nested, arrow];\n",
            "    }\n",
            "    static init() {\n",
            "        this.count = 0;\n",
            "    }\n",
            "}\n",
            "\n",
            "/** @implements {Base} */\n",
            "export class Copy {\n",
            "    constructor() {\n",
            "        this.shared = 0;\n",
            "    }\n",
            "    get value() {\n",
            "        return 0;\n",
            "    }\n",
            "}\n",
            "\n",
            "/**\n",
            " * @template T\n",
            " * @template {string} K\n",
            " * @param {T} value\n",
            " * @param {K} key\n",
            " * @returns {T}\n",
            " */\n",
            "export function identity(value, key) {\n",
            "    void key;\n",
            "    return value;\n",
            "}\n",
            "\n",
            "/** @template T */\n",
            "export class Box {\n",
            "    /** @param {T} value */\n",
            "    constructor(value) {\n",
            "        this.value = value;\n",
            "    }\n",
            "}\n",
            "\n",
            "/** @augments {Box<string>} */\n",
            "export class StringBox extends Box {}\n",
        ),
    )
    .expect("write lib");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"es2022","allowJs":true,"checkJs":true,"declaration":true,"emitDeclarationOnly":true,"types":[],"outDir":"out"},"files":["lib.js"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        "lib.js(32,13): error TS2683: 'this' implicitly has type 'any' because it does not have a type annotation.\n"
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/lib.d.ts")).expect("read declarations"),
        concat!(
            "export declare class Base {\n",
            "    /** @type {number} */\n",
            "    shared: number;\n",
            "    constructor();\n",
            "    get value(): number;\n",
            "}\n",
            "export declare class Point extends Base {\n",
            "    /** The x coordinate. */\n",
            "    x: number;\n",
            "    y: string;\n",
            "    method: () => void;\n",
            "    later: boolean | undefined;\n",
            "    fromArrow: number;\n",
            "    static count: number | undefined;\n",
            "    /** @private */\n",
            "    private static secret;\n",
            "    /** @protected */\n",
            "    protected static helper(): void;\n",
            "    /**\n",
            "     * @readonly\n",
            "     * @type {string}\n",
            "     */\n",
            "    readonly label: string;\n",
            "    constructor();\n",
            "    static init(): void;\n",
            "}\n",
            "/** @implements {Base} */\n",
            "export declare class Copy implements Base {\n",
            "    shared: number;\n",
            "    constructor();\n",
            "    get value(): number;\n",
            "}\n",
            "/**\n",
            " * @template T\n",
            " * @template {string} K\n",
            " * @param {T} value\n",
            " * @param {K} key\n",
            " * @returns {T}\n",
            " */\n",
            "export declare function identity<T, K extends string>(value: T, key: K): T;\n",
            "/** @template T */\n",
            "export declare class Box<T> {\n",
            "    value: T;\n",
            "    /** @param {T} value */\n",
            "    constructor(value: T);\n",
            "}\n",
            "/** @augments {Box<string>} */\n",
            "export declare class StringBox extends Box<string> {\n",
            "}\n",
        )
    );
}

#[test]
fn javascript_typedef_and_callback_declarations_follow_tsgo() {
    // tsgo's parser turns each `@typedef` and `@callback` tag into a type
    // alias before the top-level statement whose JSDoc (or whose nested
    // node's JSDoc outside a block) holds it, and the declaration transform
    // prints it: `export` in a module, `@property` tags as a type literal
    // keeping their comments, a dotted name as namespaces, `@template` tags
    // as type parameters. The comments stay with the statements that own
    // them. The expected bytes are tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("types.js"),
        concat!(
            "/**\n",
            " * @typedef {Object} Opts\n",
            " * @property {string} name The name.\n",
            " * @property {number} [count]\n",
            " * @property {Object} nested\n",
            " * @property {*} nested.any\n",
            " * @property {?string} nested.maybe\n",
            " * @property {string=} nested.opt Optional nested.\n",
            " */\n",
            "\n",
            "/**\n",
            " * @template T\n",
            " * @template {string} [K=string]\n",
            " * @typedef {{ value: T, key: K }} Pair\n",
            " */\n",
            "\n",
            "/** @typedef {string} NS.A */\n",
            "/** @typedef {number} NS.Sub.B */\n",
            "\n",
            "/**\n",
            " * @callback Handler\n",
            " * @param {string} event\n",
            " * @param {number} rest\n",
            " * @returns {boolean}\n",
            " */\n",
            "\n",
            "/**\n",
            " * @callback NoReturn\n",
            " * @param {Object} opts\n",
            " * @param {string} opts.x\n",
            " * @this {Opts}\n",
            " */\n",
            "\n",
            "/** @typedef {Object} Empty */\n",
            "\n",
            "/**\n",
            " * @param {Opts} foo\n",
            " * @param {string} def\n",
            " * @param {number} other\n",
            " */\n",
            "export function use(foo, def, other) {\n",
            "    /** @typedef {string} Local */\n",
            "    void [foo, def, other];\n",
            "}\n",
        ),
    )
    .expect("write types");
    fs::write(
        tree.path("order.js"),
        concat!(
            "// leading line comment\n",
            "/** @typedef {string} A */\n",
            "\n",
            "/**\n",
            " * Doc for B.\n",
            " * @typedef {number} B\n",
            " * @typedef {boolean} C\n",
            " */\n",
            "/** @param {A} a */\n",
            "export function f(a) {}\n",
            "\n",
            "/** @typedef {A | B} D */\n",
            "export const x = 1;\n",
            "\n",
            "export class K {\n",
            "    /** @typedef {string} Inner */\n",
            "    m() {}\n",
            "}\n",
            "\n",
            "/** Free comment */\n",
            "/** @typedef {C} E */\n",
        ),
    )
    .expect("write order");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"es2022","allowJs":true,"checkJs":true,"declaration":true,"emitDeclarationOnly":true,"types":[],"outDir":"out"},"files":["types.js","order.js"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        fs::read_to_string(tree.path("out/types.d.ts")).expect("read types declarations"),
        concat!(
            "/**\n",
            " * @typedef {Object} Opts\n",
            " * @property {string} name The name.\n",
            " * @property {number} [count]\n",
            " * @property {Object} nested\n",
            " * @property {*} nested.any\n",
            " * @property {?string} nested.maybe\n",
            " * @property {string=} nested.opt Optional nested.\n",
            " */\n",
            "export type Opts = {\n",
            "    /**\n",
            "     * The name.\n",
            "     */\n",
            "    name: string;\n",
            "    count?: number;\n",
            "    nested: {\n",
            "        any: any;\n",
            "        maybe: string | null;\n",
            "        opt?: string | undefined;\n",
            "    };\n",
            "};\n",
            "export type Pair<T, K extends string = string> = {\n",
            "    value: T;\n",
            "    key: K;\n",
            "};\n",
            "export declare namespace NS {\n",
            "    export type A = string;\n",
            "}\n",
            "export declare namespace NS {\n",
            "    namespace Sub {\n",
            "        export type B = number;\n",
            "    }\n",
            "}\n",
            "export type Handler = (event: string, rest: number) => boolean;\n",
            "export type NoReturn = (opts: {\n",
            "    x: string;\n",
            "}) => any;\n",
            "export type Empty = Object;\n",
            "/**\n",
            " * @template T\n",
            " * @template {string} [K=string]\n",
            " * @typedef {{ value: T, key: K }} Pair\n",
            " */\n",
            "/** @typedef {string} NS.A */\n",
            "/** @typedef {number} NS.Sub.B */\n",
            "/**\n",
            " * @callback Handler\n",
            " * @param {string} event\n",
            " * @param {number} rest\n",
            " * @returns {boolean}\n",
            " */\n",
            "/**\n",
            " * @callback NoReturn\n",
            " * @param {Object} opts\n",
            " * @param {string} opts.x\n",
            " * @this {Opts}\n",
            " */\n",
            "/** @typedef {Object} Empty */\n",
            "/**\n",
            " * @param {Opts} foo\n",
            " * @param {string} def\n",
            " * @param {number} other\n",
            " */\n",
            "export declare function use(foo: Opts, def: string, other: number): void;\n",
        )
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/order.d.ts")).expect("read order declarations"),
        concat!(
            "/** @typedef {string} A */\n",
            "export type A = string;\n",
            "export type B = number;\n",
            "export type C = boolean;\n",
            "/**\n",
            " * Doc for B.\n",
            " * @typedef {number} B\n",
            " * @typedef {boolean} C\n",
            " */\n",
            "/** @param {A} a */\n",
            "export declare function f(a: A): void;\n",
            "export type D = A | B;\n",
            "/** @typedef {A | B} D */\n",
            "export declare const x = 1;\n",
            "export type Inner = string;\n",
            "export declare class K {\n",
            "    /** @typedef {string} Inner */\n",
            "    m(): void;\n",
            "}\n",
            "export type E = C;\n",
            "/** Free comment */\n",
            "/** @typedef {C} E */\n",
        )
    );
}

#[test]
fn javascript_dotted_typedef_names_with_reexported_values_follow_tsgo() {
    // A dotted `@typedef` name is a namespace that merges with a value the
    // module re-exports with `export {…}`. tsgo counts that export
    // specifier as a visible declaration and the reparsed namespace as
    // exported, so the alias may refer to the namespace without an error.
    // The expected bytes are tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("file.js"),
        concat!(
            "/**\n",
            " * @namespace myTypes\n",
            " * @global\n",
            " * @type {Object<string,*>}\n",
            " */\n",
            "const myTypes = {\n",
            "    // SOME PROPS HERE\n",
            "};\n",
            "\n",
            "/** @typedef {string|RegExp|Array<string|RegExp>} myTypes.typeA */\n",
            "\n",
            "/**\n",
            " * @typedef myTypes.typeB\n",
            " * @property {myTypes.typeA}    prop1 - Prop 1.\n",
            " * @property {string}           prop2 - Prop 2.\n",
            " */\n",
            "\n",
            "/** @typedef {myTypes.typeB|Function} myTypes.typeC */\n",
            "\n",
            "export {myTypes};\n",
        ),
    )
    .expect("write file");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"es2015","module":"commonjs","allowJs":true,"checkJs":true,"declaration":true,"emitDeclarationOnly":true,"types":[],"outDir":"out"},"files":["file.js"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/file.d.ts")).expect("read declarations"),
        concat!(
            "/**\n",
            " * @namespace myTypes\n",
            " * @global\n",
            " * @type {Object<string,*>}\n",
            " */\n",
            "declare const myTypes: Record<string, any>;\n",
            "export declare namespace myTypes {\n",
            "    export type typeA = string | RegExp | Array<string | RegExp>;\n",
            "}\n",
            "export declare namespace myTypes {\n",
            "    export type typeB = {\n",
            "        /**\n",
            "         * - Prop 1.\n",
            "         */\n",
            "        prop1: myTypes.typeA;\n",
            "        /**\n",
            "         * - Prop 2.\n",
            "         */\n",
            "        prop2: string;\n",
            "    };\n",
            "}\n",
            "export declare namespace myTypes {\n",
            "    export type typeC = myTypes.typeB | Function;\n",
            "}\n",
            "/** @typedef {string|RegExp|Array<string|RegExp>} myTypes.typeA */\n",
            "/**\n",
            " * @typedef myTypes.typeB\n",
            " * @property {myTypes.typeA}    prop1 - Prop 1.\n",
            " * @property {string}           prop2 - Prop 2.\n",
            " */\n",
            "/** @typedef {myTypes.typeB|Function} myTypes.typeC */\n",
            "export { myTypes };\n",
        )
    );
}

#[test]
fn javascript_import_tags_follow_tsgo() {
    // tsgo's parser turns an `@import` tag into a type-only import before
    // the top-level statement that holds it, a class member's included, and
    // the declaration transform keeps the bindings the declarations use
    // (late painting), with the specifier written as in the tag. The
    // expected bytes are tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "export interface Foo { a: number }\n",
            "export default interface Def { b: string }\n",
        ),
    )
    .expect("write module");
    fs::write(
        tree.path("lib.js"),
        concat!(
            "/** @import { Foo } from \"./a\" */\n",
            "/** @import Def, * as all from './a' */\n",
            "/** @import { Foo as Unused } from \"./a\" */\n",
            "\n",
            "/**\n",
            " * @param {Foo} foo\n",
            " * @param {Def} def\n",
            " * @param {all.Foo} other\n",
            " */\n",
            "export function use(foo, def, other) {\n",
            "    void [foo, def, other];\n",
            "}\n",
            "\n",
            "export class K {\n",
            "    /** @import { Foo as Bar } from \"./a\" */\n",
            "    /** @param {Bar} bar */\n",
            "    m(bar) {\n",
            "        void bar;\n",
            "    }\n",
            "}\n",
        ),
    )
    .expect("write lib");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"es2022","allowJs":true,"checkJs":true,"declaration":true,"emitDeclarationOnly":true,"types":[],"outDir":"out"},"files":["lib.js"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/lib.d.ts")).expect("read declarations"),
        concat!(
            "/** @import { Foo } from \"./a\" */\n",
            "/** @import Def, * as all from './a' */\n",
            "/** @import { Foo as Unused } from \"./a\" */\n",
            "import type { Foo } from \"./a\";\n",
            "import type Def, * as all from './a';\n",
            "/**\n",
            " * @param {Foo} foo\n",
            " * @param {Def} def\n",
            " * @param {all.Foo} other\n",
            " */\n",
            "export declare function use(foo: Foo, def: Def, other: all.Foo): void;\n",
            "import type { Foo as Bar } from \"./a\";\n",
            "export declare class K {\n",
            "    /** @import { Foo as Bar } from \"./a\" */\n",
            "    /** @param {Bar} bar */\n",
            "    m(bar: Bar): void;\n",
            "}\n",
        )
    );
}

#[test]
fn javascript_overload_tags_follow_tsgo() {
    // tsgo's parser turns each `@overload` tag of a function, method or
    // constructor into a bodyless declaration before it, and the
    // declaration itself becomes the implementation that declarations leave
    // out. The overload copies the implementation's written modifiers and
    // name with their source positions: when the transform reuses them, the
    // implementation's comments print before the overload. The expected
    // bytes are tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("lib.js"),
        concat!(
            "/**\n",
            " * Adds things.\n",
            " * @overload\n",
            " * @param {number} a\n",
            " * @param {number} b\n",
            " * @returns {number}\n",
            " *\n",
            " * @overload\n",
            " * @param {string} a\n",
            " * @param {string} [b]\n",
            " * @returns {string}\n",
            " *\n",
            " * @param {string | number} a\n",
            " * @param {string | number} [b]\n",
            " * @returns {string | number}\n",
            " */\n",
            "export function add(a, b) {\n",
            "    return a;\n",
            "}\n",
            "\n",
            "export class Box {\n",
            "    /**\n",
            "     * @overload\n",
            "     * @param {string} a\n",
            "     */\n",
            "    /**\n",
            "     * @overload\n",
            "     * @param {number} a\n",
            "     * @param {number} b\n",
            "     */\n",
            "    /**\n",
            "     * @param {string | number} a\n",
            "     * @param {number} [b]\n",
            "     */\n",
            "    constructor(a, b) {\n",
            "        void [a, b];\n",
            "    }\n",
            "\n",
            "    /**\n",
            "     * @overload\n",
            "     * @param {string} key\n",
            "     * @returns {string}\n",
            "     */\n",
            "    /**\n",
            "     * @overload\n",
            "     * @param {number} key\n",
            "     * @returns {number}\n",
            "     */\n",
            "    /**\n",
            "     * @param {string | number} key\n",
            "     */\n",
            "    get(key) {\n",
            "        return key;\n",
            "    }\n",
            "\n",
            "    /**\n",
            "     * @overload\n",
            "     */\n",
            "    static make() {}\n",
            "}\n",
        ),
    )
    .expect("write lib");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"es2022","allowJs":true,"checkJs":true,"declaration":true,"emitDeclarationOnly":true,"types":[],"outDir":"out"},"files":["lib.js"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        "lib.js(57,9): error TS7010: 'make', which lacks return-type annotation, implicitly has an 'any' return type.\n"
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/lib.d.ts")).expect("read declarations"),
        concat!(
            "export declare function add(a: number, b: number): number;\n",
            "export declare function add(a: string, b?: string): string;\n",
            "export declare class Box {\n",
            "    constructor(a: string);\n",
            "    constructor(a: number, b: number);\n",
            "    /**\n",
            "     * @overload\n",
            "     * @param {string} key\n",
            "     * @returns {string}\n",
            "     */\n",
            "    /**\n",
            "     * @overload\n",
            "     * @param {number} key\n",
            "     * @returns {number}\n",
            "     */\n",
            "    /**\n",
            "     * @param {string | number} key\n",
            "     */\n",
            "    get(key: string): string;\n",
            "    /**\n",
            "     * @overload\n",
            "     * @param {string} key\n",
            "     * @returns {string}\n",
            "     */\n",
            "    /**\n",
            "     * @overload\n",
            "     * @param {number} key\n",
            "     * @returns {number}\n",
            "     */\n",
            "    /**\n",
            "     * @param {string | number} key\n",
            "     */\n",
            "    get(key: number): number;\n",
            "    /**\n",
            "     * @overload\n",
            "     */\n",
            "    static make(): any;\n",
            "}\n",
        )
    );
}

#[test]
fn javascript_this_tags_follow_tsgo() {
    // tsgo's parser gives a JavaScript function whose first parameter is not
    // `this` a `this` parameter from its `@this` tag, typed by the tag or
    // `any`. The expected bytes are tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("lib.js"),
        concat!(
            "/** @this {string} */\n",
            "export function f1() {}\n",
            "\n",
            "/** @this */\n",
            "export function f2() {}\n",
            "\n",
            "export class C {\n",
            "    /** @this {C} */\n",
            "    m() {}\n",
            "}\n",
            "\n",
            "/**\n",
            " * @this {Window}\n",
            " * @param {Window} self\n",
            " */\n",
            "export function f3(this_, self) {\n",
            "    void [this_, self];\n",
            "}\n",
        ),
    )
    .expect("write lib");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"es2022","allowJs":true,"checkJs":true,"declaration":true,"emitDeclarationOnly":true,"lib":["es2022","dom"],"types":[],"outDir":"out"},"files":["lib.js"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "lib.js(4,10): error TS1110: Type expected.\n",
            "lib.js(16,20): error TS7006: Parameter 'this_' implicitly has an 'any' type.\n",
        )
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/lib.d.ts")).expect("read declarations"),
        concat!(
            "/** @this {string} */\n",
            "export declare function f1(this: string): void;\n",
            "/** @this */\n",
            "export declare function f2(this: any): void;\n",
            "export declare class C {\n",
            "    /** @this {C} */\n",
            "    m(this: C): void;\n",
            "}\n",
            "/**\n",
            " * @this {Window}\n",
            " * @param {Window} self\n",
            " */\n",
            "export declare function f3(this: Window, this_: any, self: Window): void;\n",
        )
    );
}

#[test]
fn javascript_script_declarations_follow_tsgo() {
    // A JavaScript script (neither an ES module nor CommonJS) goes through
    // the same declaration transform as TypeScript in tsgo: global
    // declarations with `declare`, type aliases without `export`, a function
    // typed by `@type` kept rather than left out as an overload
    // implementation, reused literal types with their quotes, and the
    // text of a multi-line JSDoc type without its ` * ` line prefixes. The
    // expected bytes are tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("global.js"),
        concat!(
            "/**\n",
            " * @typedef {Object} Options\n",
            " * @property {string} name\n",
            " * @property {number} [size] The size.\n",
            " */\n",
            "\n",
            "/**\n",
            " * A global helper.\n",
            " * @param {Options} options\n",
            " * @returns {'ok' | 'failed'}\n",
            " */\n",
            "function run(options) {\n",
            "    void options;\n",
            "    return 'ok';\n",
            "}\n",
            "\n",
            "/** @typedef {(value: string) => void} Callback */\n",
            "\n",
            "/** @type {Callback} */\n",
            "function log(value) {\n",
            "    void value;\n",
            "}\n",
            "\n",
            "class Widget {\n",
            "    constructor() {\n",
            "        /** @type {number} */\n",
            "        this.width = 0;\n",
            "    }\n",
            "    /** @returns {Widget} */\n",
            "    clone() {\n",
            "        return new Widget();\n",
            "    }\n",
            "}\n",
            "\n",
            "const settings = {\n",
            "    verbose: false,\n",
            "    level: 1,\n",
            "};\n",
            "\n",
            "/**\n",
            " * @param {string} text\n",
            " */\n",
            "const shout = (text) => text.toUpperCase();\n",
            "\n",
            "/**\n",
            " * @typedef {'a' |\n",
            " *     'b' |\n",
            " *     'c'} Letter\n",
            " */\n",
        ),
    )
    .expect("write script");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"es2022","allowJs":true,"checkJs":true,"declaration":true,"emitDeclarationOnly":true,"types":[],"outDir":"out"},"files":["global.js"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/global.d.ts")).expect("read declarations"),
        concat!(
            "/**\n",
            " * @typedef {Object} Options\n",
            " * @property {string} name\n",
            " * @property {number} [size] The size.\n",
            " */\n",
            "type Options = {\n",
            "    name: string;\n",
            "    /**\n",
            "     * The size.\n",
            "     */\n",
            "    size?: number;\n",
            "};\n",
            "/**\n",
            " * A global helper.\n",
            " * @param {Options} options\n",
            " * @returns {'ok' | 'failed'}\n",
            " */\n",
            "declare function run(options: Options): 'ok' | 'failed';\n",
            "type Callback = (value: string) => void;\n",
            "/** @typedef {(value: string) => void} Callback */\n",
            "/** @type {Callback} */\n",
            "declare function log(value: string): void;\n",
            "declare class Widget {\n",
            "    /** @type {number} */\n",
            "    width: number;\n",
            "    constructor();\n",
            "    /** @returns {Widget} */\n",
            "    clone(): Widget;\n",
            "}\n",
            "declare const settings: {\n",
            "    verbose: boolean;\n",
            "    level: number;\n",
            "};\n",
            "/**\n",
            " * @param {string} text\n",
            " */\n",
            "declare const shout: (text: string) => string;\n",
            "type Letter = 'a' | 'b' | 'c';\n",
            "/**\n",
            " * @typedef {'a' |\n",
            " *     'b' |\n",
            " *     'c'} Letter\n",
            " */\n",
        )
    );
}

#[test]
fn javascript_template_tags_on_function_expressions_follow_tsgo() {
    // tsgo's reparser gives a function expression, arrow function or object
    // literal method the type parameters of its `@template` tags, so the
    // type written for the variable or property has them; an `@implements`
    // tag without a type leaves no `implements` clause. The expected bytes
    // are tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("lib.js"),
        concat!(
            "/**\n",
            " * @template T\n",
            " * @param {T} a\n",
            " * @returns {(b: T) => T}\n",
            " */\n",
            "const seq = a => b => b;\n",
            "\n",
            "const helpers = {\n",
            "    /**\n",
            "     * @template {string} K\n",
            "     * @param {K} key\n",
            "     * @returns {K}\n",
            "     */\n",
            "    id(key) {\n",
            "        return key;\n",
            "    },\n",
            "};\n",
            "\n",
            "/** @implements */\n",
            "class Empty {}\n",
            "\n",
            "/** @type {string} */\n",
            "var text = seq(\"a\")(\"b\");\n",
        ),
    )
    .expect("write lib");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"es2022","allowJs":true,"checkJs":true,"declaration":true,"emitDeclarationOnly":true,"types":[],"outDir":"out"},"files":["lib.js"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        "lib.js(19,16): error TS1003: Identifier expected.\n"
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/lib.d.ts")).expect("read declarations"),
        concat!(
            "/**\n",
            " * @template T\n",
            " * @param {T} a\n",
            " * @returns {(b: T) => T}\n",
            " */\n",
            "declare const seq: <T>(a: T) => (b: T) => T;\n",
            "declare const helpers: {\n",
            "    /**\n",
            "     * @template {string} K\n",
            "     * @param {K} key\n",
            "     * @returns {K}\n",
            "     */\n",
            "    id<K extends string>(key: K): K;\n",
            "};\n",
            "/** @implements */\n",
            "declare class Empty {\n",
            "}\n",
            "/** @type {string} */\n",
            "declare var text: string;\n",
        )
    );
}

#[test]
fn typescript_expandos_follow_tsgo() {
    // tsgo collects the `F.x = …` assignments of a file before its statements
    // and writes their host as a function declaration followed by a namespace
    // of `var` members (transform.go:2700-2970): an identifier value becomes
    // `export { value as name }`, a keyword or a name that resolves elsewhere
    // gets a generated name exported under the property name, a host that is
    // not visible waits until a type reference paints it, and a non-exported
    // function exported by an assignment is painted too. A variable whose
    // only property has a non-identifier name keeps its type, not `typeof`
    // itself. The expected bytes are tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("expando.ts"),
        concat!(
            "export function A() {\n",
            "    return 'A';\n",
            "}\n",
            "export enum Kind { One }\n",
            "A.kind = Kind;\n",
            "A.count = 1;\n",
            "A.null = \"x\";\n",
            "A.name = 2;\n",
            "\n",
            "export const f = (x: number) => x;\n",
            "f.options = { verbose: true };\n",
            "f[\"list\"] = [1, 2];\n",
            "\n",
            "function hidden() {}\n",
            "hidden.flag = true;\n",
            "export const useHidden: typeof hidden = hidden;\n",
            "\n",
            "function helper() {}\n",
            "export function C() {\n",
            "    return null;\n",
            "}\n",
            "C.helper = helper;\n",
            "\n",
            "export default function D() {}\n",
            "D.enabled = true;\n",
            "\n",
            "export const plain = () => {};\n",
            "plain[1] = 0;\n",
        ),
    )
    .expect("write expando.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"es2022","module":"esnext","strict":true,"declaration":true,"emitDeclarationOnly":true,"types":[],"outDir":"out"},"files":["expando.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/expando.d.ts")).expect("read expando.d.ts"),
        concat!(
            "export declare function A(): string;\n",
            "export declare namespace A {\n",
            "    export { Kind as kind };\n",
            "    export var count: number;\n",
            "    export var _a: string;\n",
            "    export { _a as null };\n",
            "    export var _b: number;\n",
            "    export { _b as name };\n",
            "}\n",
            "export declare enum Kind {\n",
            "    One = 0\n",
            "}\n",
            "export declare function f(x: number): number;\n",
            "export declare namespace f {\n",
            "    var options: {\n",
            "        verbose: boolean;\n",
            "    };\n",
            "    var list: number[];\n",
            "}\n",
            "declare function hidden(): void;\n",
            "declare namespace hidden {\n",
            "    var flag: boolean;\n",
            "}\n",
            "export declare const useHidden: typeof hidden;\n",
            "declare function helper(): void;\n",
            "export declare function C(): null;\n",
            "export declare namespace C {\n",
            "    export { helper };\n",
            "}\n",
            "declare function D(): void;\n",
            "export default D;\n",
            "declare namespace D {\n",
            "    var enabled: boolean;\n",
            "}\n",
            "export declare const plain: {\n",
            "    (): void;\n",
            "    1: number;\n",
            "};\n",
        )
    );
}

#[test]
fn javascript_expandos_follow_tsgo() {
    // JavaScript expandos take the same path as TypeScript ones in tsgo: a
    // variable host becomes a new function declaration (without the
    // variable's JSDoc), a `@type` on an assignment types its member, a class
    // host keeps its class with a namespace after it, a generator keeps its
    // `*`, and a script's hosts are global. The expected bytes are tsgo's for
    // the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("module.js"),
        concat!(
            "/**\n",
            " * Formats a value.\n",
            " * @param {number} value\n",
            " */\n",
            "export const format = (value) => String(value);\n",
            "format.width = 1;\n",
            "/** @type {string | undefined} */\n",
            "format.label = undefined;\n",
            "format.self = format;\n",
            "\n",
            "export function parse() {}\n",
            "parse[\"strict\"] = true;\n",
            "parse.default = 1;\n",
            "\n",
            "export class Registry {}\n",
            "Registry.instance = new Registry();\n",
            "\n",
            "/**\n",
            " * @template T\n",
            " * @param {T} value\n",
            " * @returns {Generator<T>}\n",
            " */\n",
            "export function* repeat(value) { yield value; }\n",
            "repeat.times = 2;\n",
        ),
    )
    .expect("write module.js");
    fs::write(
        tree.path("script.js"),
        concat!(
            "function legacy() {}\n",
            "legacy.version = 1;\n",
            "var make = function () {};\n",
            "make.kind = \"\";\n",
        ),
    )
    .expect("write script.js");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"es2022","module":"esnext","allowJs":true,"checkJs":true,"strict":true,"declaration":true,"emitDeclarationOnly":true,"types":[],"outDir":"out"},"files":["module.js","script.js"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/module.d.ts")).expect("read module.d.ts"),
        concat!(
            "export declare function format(value: number): string;\n",
            "export declare namespace format {\n",
            "    var width: number;\n",
            "    export { undefined as label };\n",
            "    export { format as self };\n",
            "}\n",
            "export declare function parse(): void;\n",
            "export declare namespace parse {\n",
            "    export var strict: boolean;\n",
            "    var _a: number;\n",
            "    export { _a as default };\n",
            "}\n",
            "export declare class Registry {\n",
            "}\n",
            "export declare namespace Registry {\n",
            "    var instance: Registry;\n",
            "}\n",
            "/**\n",
            " * @template T\n",
            " * @param {T} value\n",
            " * @returns {Generator<T>}\n",
            " */\n",
            "export declare function* repeat<T>(value: T): Generator<T>;\n",
            "export declare namespace repeat {\n",
            "    var times: number;\n",
            "}\n",
        )
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/script.d.ts")).expect("read script.d.ts"),
        concat!(
            "declare function legacy(): void;\n",
            "declare namespace legacy {\n",
            "    var version: number;\n",
            "}\n",
            "declare function make(): void;\n",
            "declare namespace make {\n",
            "    var kind: string;\n",
            "}\n",
        )
    );
}

#[test]
fn typescript_expando_declaration_map_follows_tsgo() {
    // tsgo clones the host's name for an expando namespace with its text
    // range, so the declaration map maps the namespace's name to the host's
    // name. The expected bytes are tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("expando.ts"),
        concat!(
            "export function parse(text: string) {\n",
            "    return text.length;\n",
            "}\n",
            "parse.strict = true;\n",
            "\n",
            "function helper() {}\n",
            "helper.version = 1;\n",
            "export const useHelper: typeof helper = helper;\n",
        ),
    )
    .expect("write expando.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"es2022","module":"esnext","strict":true,"declaration":true,"declarationMap":true,"emitDeclarationOnly":true,"types":[],"outDir":"out"},"files":["expando.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/expando.d.ts")).expect("read expando.d.ts"),
        concat!(
            "export declare function parse(text: string): number;\n",
            "export declare namespace parse {\n",
            "    var strict: boolean;\n",
            "}\n",
            "declare function helper(): void;\n",
            "declare namespace helper {\n",
            "    var version: number;\n",
            "}\n",
            "export declare const useHelper: typeof helper;\n",
            "export {};\n",
            "//# sourceMappingURL=expando.d.ts.map",
        )
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/expando.d.ts.map")).expect("read expando.d.ts.map"),
        r#"{"version":3,"file":"expando.d.ts","sourceRoot":"","sources":["../expando.ts"],"names":[],"mappings":"AAAA,wBAAgB,KAAK,CAAC,IAAI,EAAE,MAAM,UAEjC;yBAFe,KAAK;;;AAKrB,iBAAS,MAAM,SAAK;kBAAX,MAAM;;;AAEf,eAAO,MAAM,SAAS,EAAE,OAAO,MAAe,CAAC"}"#
    );
}

#[test]
fn export_assignments_follow_tsgo() {
    // tsgo writes an exported expression by its kind (transformExportAssignment,
    // transform.go:1227-1297): a primitive literal initializes a `const`, an
    // arrow function or function expression becomes a function declaration and a
    // class expression a class declaration, both under the expression's own name
    // or `_default` and written after the export, and any other expression types
    // a `const`. The expected bytes are tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("literal.ts"),
        concat!("/** The answer. */\n", "export default 42;\n",),
    )
    .expect("write literal.ts");
    fs::write(tree.path("unary.ts"), "export default +1;\n").expect("write unary.ts");
    fs::write(
        tree.path("arrow.ts"),
        "export default (a: number, b: string) => ({ a, b });\n",
    )
    .expect("write arrow.ts");
    fs::write(
        tree.path("named.ts"),
        concat!(
            "const format = 1;\n",
            "export default (function format(value: string) { return value; });\n",
        ),
    )
    .expect("write named.ts");
    fs::write(
        tree.path("point.ts"),
        concat!(
            "/** A point. */\n",
            "export = class Point {\n",
            "    x = 1;\n",
            "    constructor(public y: number) {}\n",
            "};\n",
        ),
    )
    .expect("write point.ts");
    fs::write(tree.path("constant.ts"), "export default \"x\" as const;\n")
        .expect("write constant.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"es2022","module":"commonjs","strict":true,"declaration":true,"emitDeclarationOnly":true,"types":[],"outDir":"out"},"files":["literal.ts","unary.ts","arrow.ts","named.ts","point.ts","constant.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/literal.d.ts")).expect("read literal.d.ts"),
        concat!(
            "/** The answer. */\n",
            "declare const _default = 42;\n",
            "export default _default;\n",
        )
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/unary.d.ts")).expect("read unary.d.ts"),
        concat!(
            "declare const _default = 1;\n",
            "export default _default;\n",
        )
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/arrow.d.ts")).expect("read arrow.d.ts"),
        concat!(
            "export default _default;\n",
            "declare function _default(a: number, b: string): {\n",
            "    a: number;\n",
            "    b: string;\n",
            "};\n",
        )
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/named.d.ts")).expect("read named.d.ts"),
        concat!(
            "export default format_1;\n",
            "declare function format_1(value: string): string;\n",
        )
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/point.d.ts")).expect("read point.d.ts"),
        concat!(
            "export = Point;\n",
            "/** A point. */\n",
            "declare class Point {\n",
            "    y: number;\n",
            "    x: number;\n",
            "    constructor(y: number);\n",
            "}\n",
        )
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/constant.d.ts")).expect("read constant.d.ts"),
        concat!(
            "declare const _default: \"x\";\n",
            "export default _default;\n",
        )
    );
}

#[test]
fn common_js_declarations_follow_tsgo() {
    // tsgo collects a CommonJS file's `module.exports =` and `exports.name =`
    // assignments before it visits the statements (visitCJSExportAssignments,
    // transformCommonJSExport, transform.go:1343-1562, 2680-2698): an assignment
    // of an identifier exports it, a value types an exported `var` or a generated
    // `const`, a class becomes an exported class, members assigned after
    // `module.exports =` go in a namespace or follow `export =`, and `require`
    // calls are elided when unused. A typedef namespace after a `return` merges
    // with the variable of its name, as tsgo's reparsed statement does, and an
    // `import()` type in a callback is kept. The expected bytes are tsgo's for
    // the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.js"),
        concat!(
            "const fs = require(\"fs\");\n",
            "const { join, dirname: dir } = require(\"path\");\n",
            "function helper() { return 1; }\n",
            "/** The version. */\n",
            "exports.version = \"1.0\";\n",
            "exports.helper = helper;\n",
            "exports[\"kebab-case\"] = 2;\n",
            "module.exports.count = 3;\n",
            "Object.defineProperty(exports, \"flag\", { value: true });\n",
            "exports.Klass = class Klass {\n",
            "    constructor() { this.x = 1; }\n",
            "};\n",
            "exports.default = 42;\n",
        ),
    )
    .expect("write a.js");
    fs::write(
        tree.path("b.js"),
        concat!(
            "class Thing {\n",
            "    constructor() { this.y = 2; }\n",
            "}\n",
            "module.exports = Thing;\n",
            "module.exports.extra = 1;\n",
        ),
    )
    .expect("write b.js");
    fs::write(
        tree.path("c.js"),
        concat!(
            "module.exports = {\n",
            "    a: 1,\n",
            "    b: \"x\",\n",
            "};\n",
            "module.exports.c = true;\n",
        ),
    )
    .expect("write c.js");
    fs::write(
        tree.path("d.js"),
        "module.exports = function (x) { return x; };\n",
    )
    .expect("write d.js");
    fs::write(
        tree.path("e.js"),
        concat!(
            "const types = {};\n",
            "/** @typedef {boolean} types.flag */\n",
            "/**\n",
            " * @callback Factory\n",
            " * @param {import('./b')} thing\n",
            " * @returns {void}\n",
            " */\n",
            "/** @param {types.flag} flag */\n",
            "function check(flag) { return 1; }\n",
            "module.exports = { check };\n",
        ),
    )
    .expect("write e.js");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"allowJs":true,"declaration":true,"emitDeclarationOnly":true,"outDir":"out","target":"es2022","module":"commonjs","types":[]},"files":["a.js","b.js","c.js","d.js","e.js"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.d.ts")).expect("read a.d.ts"),
        concat!(
            "export declare var version: \"1.0\";\n",
            "export { helper };\n",
            "declare const _exported: 2;\n",
            "export { _exported as \"kebab-case\" };\n",
            "export declare var count: 3;\n",
            "export declare var flag: boolean;\n",
            "export declare class Klass {\n",
            "    x: number;\n",
            "    constructor();\n",
            "}\n",
            "declare const _default: 42;\n",
            "export default _default;\n",
            "declare function helper(): number;\n",
        )
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/b.d.ts")).expect("read b.d.ts"),
        concat!(
            "export = Thing;\n",
            "export declare var extra: 1;\n",
            "declare class Thing {\n",
            "    y: number;\n",
            "    constructor();\n",
            "}\n",
        )
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/c.d.ts")).expect("read c.d.ts"),
        concat!(
            "declare const _exports: {\n",
            "    a: number;\n",
            "    b: string;\n",
            "};\n",
            "export = _exports;\n",
            "declare namespace _exports {\n",
            "    export var c: true;\n",
            "}\n",
        )
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/d.d.ts")).expect("read d.d.ts"),
        concat!(
            "export = _exports;\n",
            "declare function _exports(x: any): any;\n",
        )
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/e.d.ts")).expect("read e.d.ts"),
        concat!(
            "declare const _exports: {\n",
            "    check: typeof check;\n",
            "};\n",
            "export = _exports;\n",
            "declare const types: {};\n",
            "export declare namespace types {\n",
            "    export type flag = boolean;\n",
            "}\n",
            "export type Factory = (thing: import('./b')) => void;\n",
            "/** @typedef {boolean} types.flag */\n",
            "/**\n",
            " * @callback Factory\n",
            " * @param {import('./b')} thing\n",
            " * @returns {void}\n",
            " */\n",
            "/** @param {types.flag} flag */\n",
            "declare function check(flag: types.flag): number;\n",
        )
    );
}

#[test]
fn namespace_export_declarations_follow_tsgo() {
    // tsgo visits a statement it neither keeps nor elides, such as `export as
    // namespace`, as a declaration subtree (transform.go:226-274), so the
    // declaration file keeps it in TypeScript and JavaScript alike. The expected
    // diagnostics and bytes are tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("lib.js"),
        concat!("export const z = 3;\n", "export as namespace Lib;\n",),
    )
    .expect("write lib.js");
    fs::write(
        tree.path("glo.ts"),
        concat!(
            "/** A value. */\n",
            "export const x = 1;\n",
            "export as namespace Glo;\n",
        ),
    )
    .expect("write glo.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"allowJs":true,"declaration":true,"emitDeclarationOnly":true,"outDir":"out","target":"es2022","module":"esnext","types":[]},"files":["lib.js","glo.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "glo.ts(3,1): error TS1315: Global module exports may only appear in declaration files.\n"
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/lib.d.ts")).expect("read lib.d.ts"),
        concat!(
            "export declare const z = 3;\n",
            "export as namespace Lib;\n",
        )
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/glo.d.ts")).expect("read glo.d.ts"),
        concat!(
            "/** A value. */\n",
            "export declare const x = 1;\n",
            "export as namespace Glo;\n",
        )
    );
}

#[test]
fn common_js_define_property_diagnostic_follows_tsgo() {
    // A name that cannot be named while tsgo serializes a signature's
    // parameters is resolved from the signature's synthesized scope, which is
    // not in a JavaScript file, so the error goes where the declaration's
    // diagnostic context puts it: the property name of
    // `Object.defineProperty` (diagnostics.go:200-215). The expected
    // diagnostic is tsgo's for the same project.
    let tree = TempTree::new();
    fs::create_dir_all(tree.path("node_modules/@types/pkg")).expect("create package");
    fs::write(
        tree.path("node_modules/@types/pkg/index.d.ts"),
        concat!(
            "interface Private {}\n",
            "declare const obj: { fn(x: Private): void };\n",
            "export = obj;\n",
        ),
    )
    .expect("write index.d.ts");
    fs::write(
        tree.path("index.cjs"),
        concat!(
            "Object.defineProperty(exports, \"api\", { value: require(\"pkg\") });\n",
            "exports.helper = function (/** @type {number} */ n) { return n; };\n",
        ),
    )
    .expect("write index.cjs");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"allowJs":true,"checkJs":true,"declaration":true,"emitDeclarationOnly":true,"outDir":"out","target":"es2022","module":"commonjs"},"files":["index.cjs"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(1));
    let root = compiler_current_directory(&tree);
    let root = root.to_string_lossy().replace('\\', "/");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!(
            "index.cjs(1,32): error TS4023: Exported variable '\"api\"' has or is using name 'Private' from external module \"{root}/node_modules/@types/pkg/index\" but cannot be named.\n"
        )
    );
    assert!(!tree.path("out/index.d.cts").exists());
}

#[test]
fn common_js_signature_scope_diagnostic_follows_tsgo() {
    // While tsgo builds the type of a signature's parameter, it checks names
    // from the signature's synthesized scope, which is not in a JavaScript
    // file, so the errors go where the export's diagnostic context puts them
    // rather than at the file (nodebuilderscopes.go:142-147,
    // symbolaccessibility.go:861). The expected diagnostics are tsgo's for the
    // same project.
    let tree = TempTree::new();
    fs::create_dir_all(tree.path("node_modules/@types/pkg")).expect("create package");
    fs::write(
        tree.path("node_modules/@types/pkg/index.d.ts"),
        concat!(
            "interface Private { a: number }\n",
            "interface Box<T> { fn(x: T): void }\n",
            "declare const pkg: { make<T>(value: T): Box<T>; value: Private };\n",
            "export = pkg;\n",
        ),
    )
    .expect("write index.d.ts");
    fs::write(
        tree.path("index.cjs"),
        concat!(
            "const pkg = require(\"pkg\");\n",
            "module.exports.fn = function (/** @type {number} */ n) { return pkg.make(pkg.value); };\n",
        ),
    )
    .expect("write index.cjs");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"allowJs":true,"checkJs":true,"declaration":true,"emitDeclarationOnly":true,"outDir":"out","target":"es2022","module":"commonjs"},"files":["index.cjs"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(1));
    let root = compiler_current_directory(&tree);
    let root = root.to_string_lossy().replace('\\', "/");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!(
            "index.cjs(2,1): error TS4032: Property 'fn' of exported interface has or is using name 'Box' from private module '\"{root}/node_modules/@types/pkg/index\"'.\n\
             index.cjs(2,1): error TS4032: Property 'fn' of exported interface has or is using name 'Private' from private module '\"{root}/node_modules/@types/pkg/index\"'.\n"
        )
    );
    assert!(!tree.path("out/index.d.cts").exists());
}

#[test]
fn common_js_exported_destructuring_follows_tsgo() {
    // tsgo writes an exported binding pattern as a destructuring assignment
    // whose leaves are `exports.name` (transformInitializedVariable,
    // commonjsmodule.go:1110-1128), and flattens it only when an `export { ... }`
    // also publishes a leaf under another name (destructuringNeedsFlattening,
    // commonjsmodule.go:1428-1480). The expected bytes are tsgo's for the same
    // project.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare const arr: number[];\n",
            "declare const obj: { x: number; y: string; z: boolean; m: number[] };\n",
            "export const [a, b] = arr;\n",
            "export const { x, y: renamed, ...rest } = obj;\n",
            "export let [c = 1, , ...others] = arr;\n",
            "export const { m: [first] } = obj;\n",
            "export const [aliased] = arr;\n",
            "export { aliased as alias };\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"es2022","module":"commonjs","outDir":"out","types":[],"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.js")).expect("read a.js"),
        concat!(
            "\"use strict\";\n",
            "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
            "exports.alias = exports.aliased = exports.first = exports.others = exports.c = exports.rest = exports.renamed = exports.x = exports.b = exports.a = void 0;\n",
            "[exports.a, exports.b] = arr;\n",
            "({ x: exports.x, y: exports.renamed, ...exports.rest } = obj);\n",
            "[exports.c = 1, , ...exports.others] = arr;\n",
            "({ m: [exports.first] } = obj);\n",
            "exports.alias = exports.aliased = arr[0];\n",
            "exports.alias = exports.aliased;\n",
        )
    );
}

#[test]
fn declaration_types_follow_tsgo_pseudo_types() {
    // tsgo checks a declaration's pseudo type (pseudochecker/lookup.go)
    // against the checker's type and builds the type node from it when they
    // agree (pseudotypenodebuilder.go): literals keep their written spelling,
    // a property name is respelled only where its text calls for it, and an
    // object literal's accessor pair typed on both sides stays a pair. The
    // expected bytes are tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "export const literals = {\n",
            "    single: '1',\n",
            "    template: `1`,\n",
            "    emoji: \"⚠️\",\n",
            "    negative: -1,\n",
            "    big: 10n,\n",
            "    'quoted-name': true,\n",
            "    'ident': null,\n",
            "    3: 'three',\n",
            "    ['computed']: [1, 'a'],\n",
            "} as const;\n",
            "export let tpl = `abc` as const;\n",
            "export const widened = { a: 1, b: 'x', c: [1, 2] };\n",
            "export const methods = {\n",
            "    m(x: number) { return x * 2; },\n",
            "    get both(): number { return 1; },\n",
            "    set both(value: number) {},\n",
            "    get lone(): string { return ''; },\n",
            "} as const;\n",
            "export const arrow = (a = 1, b: string, c?: number) => b;\n",
            "export const generic = function <T>(x: T): T { return x; };\n",
            "export const unique = Symbol();\n",
            "export class C {\n",
            "    p = 1;\n",
            "    readonly q = 'x';\n",
            "    r? = 2;\n",
            "    s = [1, 2] as const;\n",
            "    method(a = 1, b: string) { return a; }\n",
            "}\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"es2022","module":"esnext","declaration":true,"emitDeclarationOnly":true,"outDir":"out","types":[],"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.d.ts")).expect("read a.d.ts"),
        concat!(
            "export declare const literals: {\n",
            "    readonly single: '1';\n",
            "    readonly template: `1`;\n",
            "    readonly emoji: \"⚠️\";\n",
            "    readonly negative: -1;\n",
            "    readonly big: 10n;\n",
            "    readonly 'quoted-name': true;\n",
            "    readonly ident: null;\n",
            "    readonly 3: 'three';\n",
            "    readonly computed: readonly [1, 'a'];\n",
            "};\n",
            "export declare let tpl: `abc`;\n",
            "export declare const widened: {\n",
            "    a: number;\n",
            "    b: string;\n",
            "    c: number[];\n",
            "};\n",
            "export declare const methods: {\n",
            "    readonly m: (x: number) => number;\n",
            "    get both(): number;\n",
            "    set both(value: number);\n",
            "    readonly lone: string;\n",
            "};\n",
            "export declare const arrow: (a: number | undefined, b: string, c?: number) => string;\n",
            "export declare const generic: <T>(x: T) => T;\n",
            "export declare const unique: unique symbol;\n",
            "export declare class C {\n",
            "    p: number;\n",
            "    readonly q = \"x\";\n",
            "    r?: number | undefined;\n",
            "    s: readonly [1, 2];\n",
            "    method(a: number | undefined, b: string): number;\n",
            "}\n",
        )
    );
}

#[test]
fn javascript_declaration_types_follow_tsgo_pseudo_types() {
    // A JavaScript declaration's pseudo type reads the types tsgo's reparser
    // hosts from its JSDoc: a `@type {const}` cast is a const context, and
    // an accessor pair typed on both sides stays a pair. The expected bytes
    // are tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.js"),
        concat!(
            "export const config = /** @type {const} */ ({ mode: 'strict', levels: [1, 2] });\n",
            "export const greet = (name = 'world') => name;\n",
            "/** @param {number} x */\n",
            "export function double(x) { return x * 2; }\n",
            "export const accessors = {\n",
            "    /** @returns {number} */\n",
            "    get value() { return 1; },\n",
            "    /** @param {number} v */\n",
            "    set value(v) {},\n",
            "};\n",
            "export const plain = { a: 'x', b: [true] };\n",
        ),
    )
    .expect("write a.js");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"es2022","module":"esnext","allowJs":true,"declaration":true,"emitDeclarationOnly":true,"outDir":"out","types":[],"strict":true},"files":["a.js"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.d.ts")).expect("read a.d.ts"),
        concat!(
            "export declare const config: {\n",
            "    readonly mode: 'strict';\n",
            "    readonly levels: readonly [1, 2];\n",
            "};\n",
            "export declare const greet: (name?: string) => string;\n",
            "/** @param {number} x */\n",
            "export declare function double(x: number): number;\n",
            "export declare const accessors: {\n",
            "    /** @returns {number} */\n",
            "    get value(): number;\n",
            "    /** @param {number} v */\n",
            "    set value(v: number);\n",
            "};\n",
            "export declare const plain: {\n",
            "    a: string;\n",
            "    b: boolean[];\n",
            "};\n",
        )
    );
}

#[test]
fn isolated_declarations_diagnostics_follow_tsgo() {
    // Under isolatedDeclarations the node builder reports where a pseudo type
    // falls back (pseudotypenodebuilder.go), and the tracker words the error
    // (declarations/tracker.go:86-100, diagnostics.go:701-735): an entity
    // name is a private name, and an assignment to an expando function's
    // property gets only the function's error. The expected diagnostics are
    // tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare function make(): number;\n",
            "export const fromCall = make();\n",
            "export const array = [1, 2];\n",
            "const base = { a: 1 };\n",
            "export const spread = { ...base, b: 2 };\n",
            "export const shorthand = { base };\n",
            "export const isPositive = (x: number) => x > 0;\n",
            "export function identity(a = 1, b: string) { return a; }\n",
            "export const literal = { a: 'x', n: [1, 2] } as const;\n",
            "export function expando() {}\n",
            "expando.value = () => 1;\n",
            "expando.count = 10;\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"es2022","module":"esnext","declaration":true,"isolatedDeclarations":true,"outDir":"out","types":[],"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf-8 stdout"),
        concat!(
            "a.ts(2,14): error TS9010: Variable must have an explicit type annotation with --isolatedDeclarations.\n",
            "a.ts(3,22): error TS9017: Only const arrays can be inferred with --isolatedDeclarations.\n",
            "a.ts(5,25): error TS9015: Objects that contain spread assignments can't be inferred with --isolatedDeclarations.\n",
            "a.ts(6,28): error TS9016: Objects that contain shorthand properties can't be inferred with --isolatedDeclarations.\n",
            "a.ts(7,42): error TS9013: Expression type can't be inferred with --isolatedDeclarations.\n",
            "a.ts(8,53): error TS9039: Type containing private name 'a' can't be used with --isolatedDeclarations.\n",
            "a.ts(10,17): error TS9007: Function must have an explicit return type annotation with --isolatedDeclarations.\n",
            "a.ts(11,1): error TS9023: Assigning properties to functions without declaring them is not supported with --isolatedDeclarations. Add an explicit declaration for the properties assigned to this function.\n",
            "a.ts(12,1): error TS9023: Assigning properties to functions without declaring them is not supported with --isolatedDeclarations. Add an explicit declaration for the properties assigned to this function.\n",
        )
    );
}

#[test]
fn class_extends_expressions_follow_tsgo() {
    // An extends clause with an expression gets a `_base` constant typed from
    // the expression's widened type (nodebuilderimpl.go:1811-1815), and under
    // isolatedDeclarations the transform reports the clause before it asks
    // for that type (declarations/transform.go:2013). The expected bytes and
    // diagnostics are tsgo's for the same projects.
    let source_a = concat!(
        "declare function mixin<T extends new (...args: any[]) => {}>(base: T): T & (new (...args: any[]) => { mixed: true });\n",
        "class Base { x = 1; }\n",
        "export class A extends mixin(Base) {}\n",
        "export const B = class extends mixin(Base) {};\n",
        "export class C extends (Base) {}\n",
    );
    let source_b = concat!(
        "declare function mixin<T extends new (...args: any[]) => {}>(base: T): T & (new (...args: any[]) => { mixed: true });\n",
        "class Base { x = 1; }\n",
        "export default class extends mixin(Base) {}\n",
    );
    let tree = TempTree::new();
    fs::write(tree.path("a.ts"), source_a).expect("write a.ts");
    fs::write(tree.path("b.ts"), source_b).expect("write b.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"es2022","module":"esnext","declaration":true,"outDir":"out","types":[],"strict":true},"files":["a.ts","b.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.d.ts")).expect("read a.d.ts"),
        concat!(
            "declare class Base {\n",
            "    x: number;\n",
            "}\n",
            "declare const A_base: typeof Base & (new (...args: any[]) => {\n",
            "    mixed: true;\n",
            "});\n",
            "export declare class A extends A_base {\n",
            "}\n",
            "export declare const B: {\n",
            "    new (): {\n",
            "        mixed: true;\n",
            "        x: number;\n",
            "    };\n",
            "};\n",
            "declare const C_base: typeof Base;\n",
            "export declare class C extends C_base {\n",
            "}\n",
            "export {};\n",
        )
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/b.d.ts")).expect("read b.d.ts"),
        concat!(
            "declare class Base {\n",
            "    x: number;\n",
            "}\n",
            "declare const default_base: typeof Base & (new (...args: any[]) => {\n",
            "    mixed: true;\n",
            "});\n",
            "export default class extends default_base {\n",
            "}\n",
            "export {};\n",
        )
    );

    let tree = TempTree::new();
    fs::write(tree.path("a.ts"), source_a).expect("write a.ts");
    fs::write(tree.path("b.ts"), source_b).expect("write b.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"es2022","module":"esnext","declaration":true,"isolatedDeclarations":true,"outDir":"out","types":[],"strict":true},"files":["a.ts","b.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf-8 stdout"),
        concat!(
            "a.ts(3,24): error TS9021: Extends clause can't contain an expression with --isolatedDeclarations.\n",
            "a.ts(4,18): error TS9022: Inference from class expressions is not supported with --isolatedDeclarations.\n",
            "a.ts(5,24): error TS9021: Extends clause can't contain an expression with --isolatedDeclarations.\n",
            "b.ts(3,30): error TS9021: Extends clause can't contain an expression with --isolatedDeclarations.\n",
        )
    );
}

#[test]
fn computed_names_in_reused_types_follow_tsgo() {
    // tsgo marks an error for a computed property name whose entity name is
    // not accessible and serializes the type instead (nodecopy.go:751-759);
    // strada's rewriting from the evaluator also tracked the name and
    // reported it as private. The expected bytes are tsgo's for the same
    // project, which reports nothing.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "export function make2() {\n",
            "  const enum E { A = \"a\" }\n",
            "  return (x: { [E.A]: string }) => x;\n",
            "}\n",
            "export function make3() {\n",
            "  const s = \"lit\";\n",
            "  return (x: { [s]: number }) => x;\n",
            "}\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"es2022","module":"esnext","declaration":true,"outDir":"out","types":[],"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.d.ts")).expect("read a.d.ts"),
        concat!(
            "export declare function make2(): (x: {\n",
            "    a: string;\n",
            "}) => {\n",
            "    a: string;\n",
            "};\n",
            "export declare function make3(): (x: {\n",
            "    lit: number;\n",
            "}) => {\n",
            "    lit: number;\n",
            "};\n",
        )
    );
}

#[test]
fn diagnostic_type_display_reuses_returns_only_under_an_enclosing_declaration() {
    // tsgo reuses a signature's written return only when the node builder
    // has an enclosing declaration (nodebuilderimpl.go:2114). Diagnostics
    // give one only to a non-context-sensitive expression's type
    // (getTypeNamesForErrorDisplay, relater.go:1270-1288), so a function
    // declaration's assertion prints as the checker's union. The expected
    // diagnostics are tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "function declared(x: number) { return x as number | string; }\n",
            "const n1: number = declared;\n",
            "const expression = function () { return 1 as number | string; };\n",
            "const n2: number = expression;\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"es2022","module":"esnext","noEmit":true,"types":[],"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf-8 stdout"),
        concat!(
            "a.ts(2,7): error TS2322: Type '(x: number) => string | number' is not assignable to type 'number'.\n",
            "a.ts(4,7): error TS2322: Type '() => number | string' is not assignable to type 'number'.\n",
        )
    );
}

#[test]
fn string_literal_export_names_are_allowed_in_declaration_files_like_tsgo() {
    // tsgo's checkModuleExportName reports TS18057 under es2015/es2020 only
    // outside declaration files (checker.go:5517-5528). The expected
    // diagnostics are tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.d.ts"),
        "declare function D(): void;\nexport { D as \"Does not work yet\" };\n",
    )
    .expect("write a.d.ts");
    fs::write(
        tree.path("b.ts"),
        "function E() {}\nexport { E as \"neither does this\" };\n",
    )
    .expect("write b.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"es2015","module":"es2015","noEmit":true,"types":[]},"files":["a.d.ts","b.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf-8 stdout"),
        "b.ts(2,15): error TS18057: String literal import and export names are not supported when the '--module' flag is set to 'es2015' or 'es2020'.\n"
    );
}

#[test]
fn type_parentheses_follow_the_tsgo_printer() {
    // tsgo's factory does not parenthesize types; its printer parenthesizes a
    // type whose precedence is below its position's (printer.go:2271-2302,
    // ast/precedence.go): a generic function type argument stays bare, an
    // intersection in a union and a constrained `infer` in an `extends` clause
    // are parenthesized, and a reused type keeps its written parentheses
    // (nodecopy.go:452-466) and its parsed `typeof X[K]` (printer.go:1999-2012).
    // The expected declarations are tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare const a: { a: string };\n",
            "interface A { a: { b: number } }\n",
            "interface B { b: 1 }\n",
            "interface X<T> { x: T }\n",
            "export type T = A & B | X<1>;\n",
            "export type U = (A | B)[] | keyof A[];\n",
            "export type W<V> = V extends (infer Y extends string) ? Y : never;\n",
            "export declare var prop: X<<T>() => T>;\n",
            "export const g = () => null! as X<<T>() => T>;\n",
            "export const o3 = (o: typeof a['a']) => {};\n",
            "export const o4 = (o: keyof (A['a'])) => {};\n",
            "export const o5 = (o: (typeof a)['a']) => {};\n",
            "export const o6 = (o: typeof a[]) => {};\n",
            "export const o7 = (o: string | (number & {})) => {};\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"es2022","module":"esnext","declaration":true,"emitDeclarationOnly":true,"outDir":"out","types":[],"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.d.ts")).expect("read a.d.ts"),
        concat!(
            "declare const a: {\n",
            "    a: string;\n",
            "};\n",
            "interface A {\n",
            "    a: {\n",
            "        b: number;\n",
            "    };\n",
            "}\n",
            "interface B {\n",
            "    b: 1;\n",
            "}\n",
            "interface X<T> {\n",
            "    x: T;\n",
            "}\n",
            "export type T = (A & B) | X<1>;\n",
            "export type U = (A | B)[] | keyof A[];\n",
            "export type W<V> = V extends (infer Y extends string) ? Y : never;\n",
            "export declare var prop: X<<T>() => T>;\n",
            "export declare const g: () => X<<T>() => T>;\n",
            "export declare const o3: (o: typeof a['a']) => void;\n",
            "export declare const o4: (o: keyof (A['a'])) => void;\n",
            "export declare const o5: (o: (typeof a)['a']) => void;\n",
            "export declare const o6: (o: typeof a[]) => void;\n",
            "export declare const o7: (o: string | (number & {})) => void;\n",
            "export {};\n",
        )
    );
}

#[test]
fn enum_member_property_names_follow_tsgo() {
    // tsgo writes a property named by an enum member as a computed reference
    // to the member when the enum is accessible as a value from the enclosing
    // declaration (nodebuilderimpl.go:2535-2552), and as the member's value
    // otherwise. The expected declarations are tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "export enum S { A = \"a\", B = \"not-an-identifier\" }\n",
            "export enum N { Zero = 0, One = 1 }\n",
            "export const record = { [S.A]: 1, [S.B]: 2, [N.Zero]: true, [N.One]: false };\n",
            "export function local() {\n",
            "    enum L { X = \"x\" }\n",
            "    return { [L.X]: 1 };\n",
            "}\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"es2022","module":"esnext","declaration":true,"emitDeclarationOnly":true,"outDir":"out","types":[],"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.d.ts")).expect("read a.d.ts"),
        concat!(
            "export declare enum S {\n",
            "    A = \"a\",\n",
            "    B = \"not-an-identifier\"\n",
            "}\n",
            "export declare enum N {\n",
            "    Zero = 0,\n",
            "    One = 1\n",
            "}\n",
            "export declare const record: {\n",
            "    [S.A]: number;\n",
            "    [S.B]: number;\n",
            "    [N.Zero]: boolean;\n",
            "    [N.One]: boolean;\n",
            "};\n",
            "export declare function local(): {\n",
            "    x: number;\n",
            "};\n",
        )
    );
}

#[test]
fn declaration_transform_details_follow_tsgo() {
    // tsgo's declaration transform writes a synthesized setter parameter as
    // `value: any` unless the accessor is private (transform.go:1037-1071) and
    // a missing mapped type template as `any` (transform.go:723-740); it
    // reports a private name in a type query, which its parser accepts
    // (parser.go:3164-3174), as TS7080 (transform.go:668-672), which blocks
    // that file's declarations. The expected diagnostics and bytes are tsgo's
    // for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "export class C {\n",
            "    set foo() { }\n",
            "    private set bar() { }\n",
            "}\n",
            "export type M<T> = {[K in keyof T]};\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("b.ts"),
        "export class P {\n    #a = 1;\n    b: typeof this.#a = 1;\n}\n",
    )
    .expect("write b.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"esnext","module":"esnext","declaration":true,"outDir":"out","types":[],"strict":true},"files":["a.ts","b.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf-8 stdout"),
        concat!(
            "a.ts(2,9): error TS1049: A 'set' accessor must have exactly one parameter.\n",
            "a.ts(2,9): error TS7032: Property 'foo' implicitly has type 'any', because its set accessor lacks a parameter type annotation.\n",
            "a.ts(3,17): error TS1049: A 'set' accessor must have exactly one parameter.\n",
            "a.ts(3,17): error TS7032: Property 'bar' implicitly has type 'any', because its set accessor lacks a parameter type annotation.\n",
            "a.ts(5,20): error TS7039: Mapped object type implicitly has an 'any' template type.\n",
            "b.ts(3,15): error TS7080: Declaration emit elides private members, but '#a' refers to a private member. Write an explicit type here.\n",
        )
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/a.d.ts")).expect("read a.d.ts"),
        concat!(
            "export declare class C {\n",
            "    set foo(value: any);\n",
            "    private set bar(value);\n",
            "}\n",
            "export type M<T> = {\n",
            "    [K in keyof T]: any;\n",
            "};\n",
        )
    );
    assert!(!tree.path("out/b.d.ts").exists());
}

#[test]
fn diagnostic_type_display_details_follow_tsgo() {
    // tsgo's appendReferenceToType keeps only the qualifiers of a class's
    // outer type parameter groups (nodebuilderimpl.go:272-323), its
    // escapeString escapes an unpaired surrogate (printer/utilities.go:84-86),
    // and a reused string literal keeps its written quote
    // (nodecopy.go:810-821). The expected diagnostics are tsgo's for the
    // same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "function mixin<T extends { new (...args: any[]): {} }>(superclass: T) {\n",
            "    return class extends superclass { get name() { return \"\"; } };\n",
            "}\n",
            "class Base { set name(v: string) {} }\n",
            "class Mine extends mixin(Base) { get name() { return \"\"; } }\n",
            "const lone: \"\\uD800\" = \"\\uDC00\";\n",
            "declare function takes(cb: (x: \"hi\") => number): void;\n",
            "takes(function (x: 'bye') { return 1; });\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"esnext","module":"esnext","noEmit":true,"types":[],"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf-8 stdout"),
        concat!(
            "a.ts(5,38): error TS2611: 'name' is defined as a property in class 'mixin.(Anonymous class) & Base', but is overridden here in 'Mine' as an accessor.\n",
            "a.ts(6,7): error TS2322: Type '\"\\uDC00\"' is not assignable to type '\"\\uD800\"'.\n",
            "a.ts(8,7): error TS2345: Argument of type '(x: 'bye') => number' is not assignable to parameter of type '(x: \"hi\") => number'.\n",
            "  Types of parameters 'x' and 'x' are incompatible.\n",
            "    Type '\"hi\"' is not assignable to type '\"bye\"'.\n",
        )
    );
}

#[test]
fn diagnostics_sort_by_file_name_like_tsgo() {
    // tsgo's CompareDiagnostics orders by File().FileName()
    // (ast/diagnostic.go:390-395, 482-520), case-sensitively, where tsc 6.0
    // compared SourceFile.path, which a case-insensitive file system folds
    // to lower case. The expected diagnostics are tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(tree.path("B.ts"), "let x: number = \"s\";\n").expect("write B.ts");
    fs::write(tree.path("a.ts"), "let y: string = 1;\n").expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"types":[]},"files":["a.ts","B.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf-8 stdout"),
        concat!(
            "B.ts(1,5): error TS2322: Type 'string' is not assignable to type 'number'.\n",
            "a.ts(1,5): error TS2322: Type 'number' is not assignable to type 'string'.\n",
        )
    );
}

#[test]
fn path_reference_diagnostics_name_the_reference_as_written() {
    // tsgo's getSourceFileFromReference names the reference text with its
    // slashes normalized (`diagnosticFileName`, compiler/fileloader.go:697),
    // where tsc 6.0 named the resolved path. The expected diagnostics are
    // tsgo's for the same project.
    let tree = TempTree::new();
    fs::create_dir_all(tree.path("src")).expect("create src");
    fs::write(
        tree.path("src/a.ts"),
        concat!(
            "///<reference path='../typescript.ts' />\n",
            "/// <reference path=\"..\\lib\\x.js\" />\n",
            "/// <reference path=\"./y.txt\" />\n",
            "/// <reference path=\"./noext\" />\n",
        ),
    )
    .expect("write src/a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"types":[]},"files":["src/a.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf-8 stdout"),
        concat!(
            "src/a.ts(1,21): error TS6053: File '../typescript.ts' not found.\n",
            "src/a.ts(2,22): error TS6504: File '../lib/x.js' is a JavaScript file. Did you mean to enable the 'allowJs' option?\n",
            "src/a.ts(3,22): error TS6054: File './y.txt' has an unsupported extension. The only supported extensions are '.ts', '.tsx', '.d.ts', '.cts', '.d.cts', '.mts', '.d.mts'.\n",
            "src/a.ts(4,22): error TS6231: Could not resolve the path './noext' with the extensions: '.ts', '.tsx', '.d.ts', '.cts', '.d.cts', '.mts', '.d.mts'.\n",
        )
    );
}

#[test]
fn diagnostic_message_details_follow_tsgo() {
    // tsgo names a missing property by plain symbolToString
    // (relater.go:4394: the source text of an early-bound computed name, the
    // value of a mapped enum key), writes an enum-keyed property of a type
    // as a computed reference when the enum is reachable from the render's
    // enclosing declaration (nodebuilderimpl.go:2535-2552), and names the
    // class of a private member by symbolToString (checker.go:11724). The
    // expected diagnostics are tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "enum E { A, B }\n",
            "interface I4 { [0x10]: number }\n",
            "const x4: I4 = {};\n",
            "type R = Record<E, any>;\n",
            "const x5: R = { [E.B]: 1 };\n",
            "const c1 = class { #m() {} };\n",
            "new c1().#m;\n",
            "const holder = { k: class { #n = 1 } };\n",
            "new holder.k().#n;\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"types":[],"target":"esnext","strict":true},"files":["a.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf-8 stdout"),
        concat!(
            "a.ts(3,7): error TS2741: Property '[0x10]' is missing in type '{}' but required in type 'I4'.\n",
            "a.ts(5,7): error TS2741: Property '0' is missing in type '{ [E.B]: number; }' but required in type 'R'.\n",
            "a.ts(7,10): error TS18013: Property '#m' is not accessible outside class 'c1' because it has a private identifier.\n",
            "a.ts(9,16): error TS18013: Property '#n' is not accessible outside class 'k' because it has a private identifier.\n",
        )
    );
}

#[test]
fn imported_helpers_are_checked_per_file_like_tsgo() {
    // tsgo keeps requestedExternalEmitHelpers in each source file's links
    // (checker.go:29053), so every file reports its own missing helper, and
    // a CommonJS default import asks for `__importDefault`
    // (checker.go:5442-5447). The expected diagnostics are tsgo's for the
    // same project.
    let tree = TempTree::new();
    fs::create_dir_all(tree.path("node_modules/tslib")).expect("create tslib");
    fs::write(
        tree.path("a.ts"),
        concat!(
            "export {};\n",
            "async function foo(): Promise<void> {}\n",
            "async function bar(): Promise<void> {}\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("b.ts"),
        concat!(
            "import greet from \"./dep\";\n",
            "export const m = greet();\n",
            "async function baz(): Promise<void> {}\n",
        ),
    )
    .expect("write b.ts");
    fs::write(
        tree.path("dep.ts"),
        "export default function greet() { return 1; }\n",
    )
    .expect("write dep.ts");
    fs::write(
        tree.path("node_modules/tslib/package.json"),
        r#"{"name":"tslib","main":"tslib.js","typings":"tslib.d.ts"}"#,
    )
    .expect("write package.json");
    fs::write(
        tree.path("node_modules/tslib/tslib.d.ts"),
        "export const notAHelper: any;\n",
    )
    .expect("write tslib.d.ts");
    fs::write(
        tree.path("node_modules/tslib/tslib.js"),
        "module.exports.notAHelper = 3;\n",
    )
    .expect("write tslib.js");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"types":[],"target":"es2016","module":"commonjs","importHelpers":true,"strict":true},"files":["a.ts","b.ts","dep.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf-8 stdout"),
        concat!(
            "a.ts(2,16): error TS2343: This syntax requires an imported helper named '__awaiter' which does not exist in 'tslib'. Consider upgrading your version of 'tslib'.\n",
            "b.ts(1,1): error TS2343: This syntax requires an imported helper named '__importDefault' which does not exist in 'tslib'. Consider upgrading your version of 'tslib'.\n",
            "b.ts(3,16): error TS2343: This syntax requires an imported helper named '__awaiter' which does not exist in 'tslib'. Consider upgrading your version of 'tslib'.\n",
        )
    );
}

#[test]
fn distributed_type_parameters_follow_tsgo() {
    // Inside a conditional type that distributes over `K`, a reference to `K`
    // is K's distributed form, whose constraint is `K` (tsgo
    // getDistributedTypeParameter, checker.go:23440-23470); instantiation
    // maps the original, so another conditional's branches do not relate,
    // and the relater explains a source assignable only to the original
    // (relater.go:4802-4804). The expected diagnostics are tsgo's for the
    // same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare function g<K>(k: K): K extends string[] ? K[number] : K;\n",
            "export function f<K>(tag: K): K extends string[] ? K[number] : K { return g<K>(tag); }\n",
            "type Show<A, B extends A> = [A, B];\n",
            "type Pair<A, B extends A> = A extends unknown ? Show<A, B> : never;\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"types":[],"target":"esnext","strict":true},"files":["a.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf-8 stdout"),
        concat!(
            "a.ts(2,68): error TS2719: Type 'K extends string[] ? K[number] : K' is not assignable to type 'K extends string[] ? K[number] : K'. Two different types with this name exist, but they are unrelated.\n",
            "  Type 'K | K[number]' is not assignable to type 'K extends string[] ? K[number] : K'.\n",
            "    Type 'K' is not assignable to type 'K extends string[] ? K[number] : K'.\n",
            "      Type 'K' is not assignable to type 'K'. Two different types with this name exist, but they are unrelated.\n",
            "        'K' is only assignable to the non-distributed 'K', but 'K' has been distributed here.\n",
            "a.ts(4,57): error TS2344: Type 'B' does not satisfy the constraint 'A'.\n",
            "  'B' is only assignable to the non-distributed 'A', but 'A' has been distributed here.\n",
        )
    );
}

#[test]
fn long_type_strings_are_cut_like_tsgo() {
    // tsgo's typeToStringEx cuts a type string of 320 bytes or more to 317
    // bytes and `...` (checker/printer.go:103-115). The expected diagnostics
    // are tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare function make(): (alphaParameterName: string, betaParameterName: number, gammaParameterName: boolean, deltaParameterName: string, epsilonParameterName: number, zetaParameterName: boolean, etaParameterName: string, thetaParameterName: number, iotaParameterName: boolean, kappaParameterName: string, lambdaParameterName: number, muParameterName: boolean, nuParameterName: string, xiParameterName: number, omicronParameterName: boolean, piParameterName: string) => void;\n",
            "const value: number = make();\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"types":[],"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf-8 stdout"),
        "a.ts(2,7): error TS2322: Type '(alphaParameterName: string, betaParameterName: number, gammaParameterName: boolean, deltaParameterName: string, epsilonParameterName: number, zetaParameterName: boolean, etaParameterName: string, thetaParameterName: number, iotaParameterName: boolean, kappaParameterName: string, lambdaParameterName: number, muParam...' is not assignable to type 'number'.\n"
    );
}

#[test]
fn json_values_are_validated_like_tsgo() {
    // tsgo's JSON parser validates the value (parser/parser.go:233-279):
    // strings and property names take double quotes, values are literals,
    // objects or arrays. A JSON source file is never type checked
    // (canIncludeBindAndCheckDiagnostics), so `[b]` reports no TS2304. The
    // expected diagnostics are tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("data.json"),
        concat!(
            "{\n",
            "    'a': true,\n",
            "    [b]: 1,\n",
            "    \"c\": undefined,\n",
            "    \"d\": [1, -2, 'x', {\"e\": null}]\n",
            "}\n",
        ),
    )
    .expect("write data.json");
    fs::write(
        tree.path("a.ts"),
        "import data = require(\"./data.json\");\nexport const x = data;\n",
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"types":[],"strict":true,"module":"commonjs","resolveJsonModule":true},"files":["a.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf-8 stdout"),
        concat!(
            "data.json(2,5): error TS1327: String literal with double quotes expected.\n",
            "data.json(3,5): error TS1327: String literal with double quotes expected.\n",
            "data.json(4,10): error TS1328: Property value can only be string literal, numeric literal, 'true', 'false', 'null', object literal or array literal.\n",
            "data.json(5,18): error TS1327: String literal with double quotes expected.\n",
        )
    );
}

#[test]
fn config_errors_do_not_stop_the_program_like_tsgo() {
    // tsgo creates the Program whatever the config reports
    // (compiler/program.go:2010-2065): a conversion error (TS5024) and an
    // unknown option (TS5023) are config parsing diagnostics printed with
    // the semantic ones, while an option relation row (TS5053) is a Program
    // diagnostic that suppresses them (GetDiagnosticsOfAnyProgram). The
    // expected diagnostics are tsgo's for the same projects.
    let tree = TempTree::new();
    fs::write(tree.path("a.ts"), "const x: number = \"s\";\nexport {};\n").expect("write a.ts");
    for (options, expected) in [
        (
            r#""strict":"yes""#,
            concat!(
                "a.ts(1,7): error TS2322: Type 'string' is not assignable to type 'number'.\n",
                "tsconfig.json(1,55): error TS5024: Compiler option 'strict' requires a value of type boolean.\n",
            ),
        ),
        (
            r#""notAnOption":true"#,
            concat!(
                "a.ts(1,7): error TS2322: Type 'string' is not assignable to type 'number'.\n",
                "tsconfig.json(1,46): error TS5023: Unknown compiler option 'notAnOption'.\n",
            ),
        ),
        (
            r#""sourceMap":true,"inlineSourceMap":true"#,
            "tsconfig.json(1,46): error TS5053: Option 'sourceMap' cannot be specified with option 'inlineSourceMap'.\n",
        ),
    ] {
        fs::write(
            tree.path("tsconfig.json"),
            format!(
                r#"{{"compilerOptions":{{"noEmit":true,"types":[],{options}}},"files":["a.ts"]}}"#
            ),
        )
        .expect("write config");
        let output = run(&tree, &["-p", ".", "--pretty", "false"]);
        assert_eq!(output.status.code(), Some(2), "{options}");
        assert_eq!(
            String::from_utf8(output.stdout).expect("utf-8 stdout"),
            expected,
            "{options}"
        );
        assert!(output.stderr.is_empty(), "{options}");
    }
}

#[test]
fn config_file_names_are_absolute_and_print_relative_like_tsgo() {
    // tsgo names the config by its normalized absolute path
    // (GetParsedCommandLineOfConfigFile, tsoptions/tsconfigparsing.go:2071):
    // diagnostics print it relative to the current directory however `-p`
    // spells it, and the include-pattern explanation names the absolute
    // path. The expected diagnostics are tsgo's for the same project.
    let tree = TempTree::new();
    fs::create_dir_all(tree.path("proj/src")).expect("create src");
    fs::create_dir_all(tree.path("proj/other")).expect("create other");
    fs::write(
        tree.path("proj/src/a.ts"),
        "export const a: number = \"s\";\n",
    )
    .expect("write a.ts");
    fs::write(tree.path("proj/other/b.ts"), "export const b = 1;\n").expect("write b.ts");
    fs::write(
        tree.path("proj/tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"types":[],"rootDir":"./src","strict":"yes"},"include":["src","other"]}"#,
    )
    .expect("write config");
    let project = compiler_current_directory(&tree).join("proj");
    let project = project.display();
    let expected = |config: &str| {
        format!(
            concat!(
                "error TS6059: File '{project}/other/b.ts' is not under 'rootDir' '{project}/src'. 'rootDir' is expected to contain all source files.\n",
                "  The file is in the program because:\n",
                "    Matched by include pattern 'other' in '{project}/tsconfig.json'\n",
                "{config}(1,73): error TS5024: Compiler option 'strict' requires a value of type boolean.\n",
            ),
            project = project,
            config = config,
        )
    };
    for (directory, arguments, config) in [
        (".", &["-p", "proj"][..], "proj/tsconfig.json"),
        ("proj", &["-p", "./tsconfig.json"][..], "tsconfig.json"),
        ("proj", &["-p", "."][..], "tsconfig.json"),
        ("proj", &[][..], "tsconfig.json"),
    ] {
        let output = run_from(
            &tree,
            directory,
            &[arguments, &["--pretty", "false"]].concat(),
        );
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert_eq!(
            String::from_utf8(output.stdout).expect("utf-8 stdout"),
            expected(config),
            "{arguments:?}"
        );
        assert!(output.stderr.is_empty(), "{arguments:?}");
    }
}

/// Run `tsc-rs --pretty false` over a one-file noEmit project and return its
/// exit status and stdout.
fn check_one_file(file_name: &str, text: &str, options: &str) -> (Option<i32>, String) {
    let tree = TempTree::new();
    fs::write(tree.path(file_name), text).expect("write source");
    fs::write(
        tree.path("tsconfig.json"),
        format!(
            r#"{{"compilerOptions":{{"noEmit":true,"types":[]{options}}},"files":["{file_name}"]}}"#
        ),
    )
    .expect("write config");
    let output = run(&tree, &["--pretty", "false"]);
    assert!(output.stderr.is_empty());
    (
        output.status.code(),
        String::from_utf8(output.stdout).expect("utf-8 stdout"),
    )
}

#[test]
fn assertions_stop_a_binary_expression_like_tsgo() {
    // tsgo parseBinaryExpressionRest (parser.go:4686-4703): in
    // `a ## b as T $$ c` the expression ends after the assertion when `$$`
    // would bind before `##` once the assertion is erased (TypeScript issue
    // 63527); equal precedence and lower-precedence operators continue. The
    // expected diagnostics are tsgo's for the same project.
    let (status, stdout) = check_one_file(
        "a.ts",
        concat!(
            "export const x01 = 1 + 1 as number * 2;\n",
            "export const x02 = 1 >> 1 as any as number + 2;\n",
            "export const x03 = 2 * 3 as number * 2;\n",
            "export const x04 = 1 + 1 as number === 2;\n",
        ),
        r#","strict":true"#,
    );
    assert_eq!(status, Some(2));
    assert_eq!(
        stdout,
        concat!(
            "a.ts(1,36): error TS1005: ',' expected.\n",
            "a.ts(2,44): error TS1005: ',' expected.\n",
        )
    );
}

#[test]
fn jsx_attribute_values_skip_whitespace_like_tsgo() {
    // tsgo ScanJsxAttributeValue (scanner.go:1330-1347) skips whitespace and
    // line breaks after `=`, so a quoted value separated from `=` may span
    // lines. The expected diagnostics are tsgo's for the same project.
    let (status, stdout) = check_one_file(
        "a.tsx",
        "const a = <div className= \"foo\n\n bar\" />;\nconst b = <div className=\n\"foo\n bar\" />;\nconst c = <div id= {1} />;\n",
        r#","jsx":"preserve""#,
    );
    assert_eq!(status, Some(2));
    assert_eq!(
        stdout,
        concat!(
            "a.tsx(1,11): error TS7026: JSX element implicitly has type 'any' because no interface 'JSX.IntrinsicElements' exists.\n",
            "a.tsx(4,11): error TS7026: JSX element implicitly has type 'any' because no interface 'JSX.IntrinsicElements' exists.\n",
            "a.tsx(7,11): error TS7026: JSX element implicitly has type 'any' because no interface 'JSX.IntrinsicElements' exists.\n",
        )
    );
}

#[test]
fn every_construct_signature_decides_constructor_access_like_tsgo() {
    // tsgo getConstructorAccessibilityError (checker.go:8834-8862) visits
    // every construct signature, so an intersection of class types reports
    // the class whose constructor is private or protected wherever it
    // stands. The expected diagnostics are tsgo's for the same project.
    let (status, stdout) = check_one_file(
        "a.ts",
        concat!(
            "class A1 {\n",
            "  private constructor(arg: string) {}\n",
            "}\n",
            "class B1 {\n",
            "  constructor(arg: number) {}\n",
            "}\n",
            "declare const Cls1: typeof A1 & typeof B1;\n",
            "new Cls1(42);\n",
            "class Derived1 extends Cls1 {}\n",
            "\n",
            "class A2 {\n",
            "  constructor(arg: string) {}\n",
            "}\n",
            "class B2 {\n",
            "  protected constructor(arg: number) {}\n",
            "}\n",
            "declare const Cls2: typeof A2 & typeof B2;\n",
            "new Cls2(42);\n",
            "class Derived2 extends Cls2 {}\n",
            "export {};\n",
        ),
        r#","strict":true"#,
    );
    assert_eq!(status, Some(2));
    assert_eq!(
        stdout,
        concat!(
            "a.ts(8,1): error TS2673: Constructor of class 'A1' is private and only accessible within the class declaration.\n",
            "a.ts(9,24): error TS2675: Cannot extend a class 'A1'. Class constructor is marked as private.\n",
            "a.ts(18,1): error TS2674: Constructor of class 'B2' is protected and only accessible within the class declaration.\n",
        )
    );
}

#[test]
fn import_type_attributes_are_checked_like_tsgo() {
    // tsgo checkImportType (checker.go:3372-3381): import type attribute
    // values must be string literals (TS2858), and the attributes' type is
    // checked against the global ImportAttributes type (checkImportAttributes).
    // The expected diagnostics are tsgo's for the same project.
    let (status, stdout) = check_one_file(
        "a.ts",
        concat!(
            "declare module \"dependency\" {\n",
            "    export interface Type {}\n",
            "}\n",
            "type T1 = typeof import(\"dependency\", {\n",
            "    with: {\n",
            "        a: (() => \"value\")(),\n",
            "    },\n",
            "});\n",
            "type T2 = import(\"dependency\", {\n",
            "    with: {\n",
            "        \"resolution-mode\": 0,\n",
            "    },\n",
            "}).Type;\n",
        ),
        r#","strict":true,"module":"esnext","target":"esnext""#,
    );
    assert_eq!(status, Some(2));
    assert_eq!(
        stdout,
        concat!(
            "a.ts(6,12): error TS2858: Import attribute values must be string literal expressions.\n",
            "a.ts(10,11): error TS2322: Type '{ \"resolution-mode\": 0; }' is not assignable to type 'ImportAttributes'.\n",
            "  Property 'resolution-mode' is incompatible with index signature.\n",
            "    Type 'number' is not assignable to type 'string'.\n",
            "a.ts(11,28): error TS2858: Import attribute values must be string literal expressions.\n",
        )
    );
}

#[test]
fn private_identifiers_in_destructuring_are_grammar_errors_like_tsgo() {
    // tsgo checkVariableLikeDeclaration and
    // checkObjectLiteralDestructuringPropertyAssignment (checker.go:5979,
    // 12805) report TS18064 at the private name of a binding element and of
    // a destructuring assignment. The expected diagnostics are tsgo's for
    // the same project.
    let (status, stdout) = check_one_file(
        "a.ts",
        concat!(
            "class A {\n",
            "    #foo = 1;\n",
            "    bar() {\n",
            "        const { #foo: foo } = this;\n",
            "        let bar;\n",
            "        ({ #foo: bar } = this);\n",
            "    }\n",
            "}\n",
        ),
        r#","target":"es2022""#,
    );
    assert_eq!(status, Some(2));
    assert_eq!(
        stdout,
        concat!(
            "a.ts(4,17): error TS18064: Private identifiers cannot be used in destructuring patterns.\n",
            "a.ts(6,12): error TS18064: Private identifiers cannot be used in destructuring patterns.\n",
        )
    );
}

#[test]
fn exported_namespace_classes_take_no_await_context_like_tsgo() {
    // tsgo parseClassDeclarationOrExpression (parser.go:1753-1759) gives an
    // exported class the await context of a module's top level only among
    // the source elements; a namespace body is a block, so `await` in a
    // computed member name there reports TS1308. The expected diagnostics
    // are tsgo's for the same project.
    let (status, stdout) = check_one_file(
        "a.ts",
        concat!(
            "declare const x: string;\n",
            "namespace N {\n",
            "    export class B {\n",
            "        [await x]() {}\n",
            "    }\n",
            "    export class B2 {\n",
            "        [x.length]() {}\n",
            "        m() { const y: number = \"s\"; }\n",
            "    }\n",
            "}\n",
            "export {};\n",
        ),
        r#","target":"esnext","module":"esnext""#,
    );
    assert_eq!(status, Some(2));
    assert_eq!(
        stdout,
        concat!(
            "a.ts(4,10): error TS1308: 'await' expressions are only allowed within async functions and at the top levels of modules.\n",
            "a.ts(8,21): error TS2322: Type 'string' is not assignable to type 'number'.\n",
        )
    );
}

#[test]
fn nullish_coalescing_semantics_follow_both_operands_like_tsgo() {
    // tsgo getSyntacticNullishnessSemantics (checker.go:13185-13194): a `??`
    // or `??=` operand is nullish only through its left operand's nullish
    // path, so `b ?? null` or `x ??= null` on the left of `??` is not always
    // nullish, while `null ?? null` is. The expected diagnostics are tsgo's
    // for the same project.
    let (status, stdout) = check_one_file(
        "a.ts",
        concat!(
            "declare let a: unknown, b: unknown;\n",
            "const p = (a ? b ?? null : null) ?? 0;\n",
            "declare let x: string | null | undefined;\n",
            "const q = (x ??= null) ?? 0;\n",
            "const r = (null ?? null) ?? 0;\n",
            "const s = (x ?? null) ?? 0;\n",
            "export {};\n",
        ),
        r#","strict":true"#,
    );
    assert_eq!(status, Some(2));
    assert_eq!(
        stdout,
        concat!(
            "a.ts(5,12): error TS2871: This expression is always nullish.\n",
            "a.ts(5,12): error TS2871: This expression is always nullish.\n",
        )
    );
}

#[test]
fn binary_files_are_reported_like_tsgo() {
    // tsgo Scan (scanner.go:931-936): a U+FFFD character outside a token
    // reports TS1490 at the start of the file and ends the scan; the pretty
    // writer prints no code snippet for it (diagnosticwriter.go:241). The
    // expected diagnostics are tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        b"const a = 1;\nG@\x04\xef\xbf\xbd\x04\x04;\n",
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"types":[]},"files":["a.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf-8 stdout"),
        concat!(
            "a.ts(1,1): error TS1490: File appears to be binary.\n",
            "a.ts(2,1): error TS1434: Unexpected keyword or identifier.\n",
            "a.ts(2,3): error TS1127: Invalid character.\n",
            "a.ts(2,4): error TS1128: Declaration or statement expected.\n",
        )
    );
    let pretty = run(&tree, &["--pretty", "true"]);
    assert_eq!(pretty.status.code(), Some(2));
    let stdout = strip_ansi_sgr(&String::from_utf8(pretty.stdout).expect("utf-8 stdout"));
    assert!(
        stdout.starts_with(
            "a.ts:1:1 - error TS1490: File appears to be binary.\na.ts:2:1 - error TS1434: Unexpected keyword or identifier.\n\n"
        ),
        "{stdout}"
    );
}

#[test]
fn commonjs_require_destructuring_is_not_esm_syntax_like_tsgo() {
    // tsgo checkAliasSymbol (checker.go:7011): under `--module preserve` a
    // `require` bound to a variable or a destructuring element of a
    // CommonJS file is not ESM syntax; an import declaration is (TS1293).
    // The expected diagnostics are tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("dep.cjs"),
        "module.exports.readFile = function () {};\n",
    )
    .expect("write dep.cjs");
    fs::write(
        tree.path("main.cjs"),
        "const { readFile } = require(\"./dep.cjs\");\nconst dep = require(\"./dep.cjs\");\nreadFile;\ndep;\n",
    )
    .expect("write main.cjs");
    fs::write(
        tree.path("esm.cjs"),
        "import { readFile as fromEsm } from \"./dep.cjs\";\nfromEsm;\n",
    )
    .expect("write esm.cjs");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"types":[],"target":"esnext","module":"preserve","moduleResolution":"bundler","verbatimModuleSyntax":true,"allowJs":true,"checkJs":true},"files":["dep.cjs","main.cjs","esm.cjs"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf-8 stdout"),
        "esm.cjs(1,10): error TS1293: ECMAScript module syntax is not allowed in a CommonJS module when 'module' is set to 'preserve'.\n"
    );
}

#[test]
fn include_processor_diagnostics_follow_their_file_like_tsgo() {
    // tsgo GetIncludeProcessorDiagnostics (compiler/program.go:840-846): a
    // Program row located in a source file (here TS2688 for an unresolved
    // `/// <reference types>`) is that file's semantic diagnostic, dropped
    // when the file skips type checking (`skipLibCheck` and a declaration
    // file) or a comment directive precedes it. The expected diagnostics are
    // tsgo's for the same projects.
    for (reference, options, expected) in [
        (
            "/// <reference types=\"cookie-session\"/>\n",
            r#","skipLibCheck":true"#,
            "index.ts(2,7): error TS2322: Type 'number' is not assignable to type 'string'.\n",
        ),
        (
            "// @ts-ignore\n/// <reference types=\"cookie-session\"/>\n",
            "",
            "index.ts(2,7): error TS2322: Type 'number' is not assignable to type 'string'.\n",
        ),
        (
            "/// <reference types=\"cookie-session\"/>\n",
            "",
            concat!(
                "index.ts(2,7): error TS2322: Type 'number' is not assignable to type 'string'.\n",
                "node_modules/foo/index.d.ts(1,23): error TS2688: Cannot find type definition file for 'cookie-session'.\n",
            ),
        ),
    ] {
        let tree = TempTree::new();
        fs::create_dir_all(tree.path("node_modules/foo")).expect("create package");
        fs::write(
            tree.path("node_modules/foo/package.json"),
            r#"{"name":"foo","version":"1.0.0","types":"index.d.ts"}"#,
        )
        .expect("write package.json");
        fs::write(
            tree.path("node_modules/foo/index.d.ts"),
            format!("{reference}export const foo = 1;\n"),
        )
        .expect("write index.d.ts");
        fs::write(
            tree.path("index.ts"),
            "import { foo } from \"foo\";\nconst y: string = foo;\n",
        )
        .expect("write index.ts");
        fs::write(
            tree.path("tsconfig.json"),
            format!(
                r#"{{"compilerOptions":{{"noEmit":true,"types":[],"strict":true{options}}},"files":["index.ts"]}}"#
            ),
        )
        .expect("write config");
        let output = run(&tree, &["--pretty", "false"]);
        assert_eq!(output.status.code(), Some(2), "{options} {reference}");
        assert_eq!(
            String::from_utf8(output.stdout).expect("utf-8 stdout"),
            expected,
            "{options} {reference}"
        );
    }
}

#[test]
fn binding_patterns_pad_missing_elements_like_tsgo() {
    // tsgo padObjectLiteralType (checker.go:17142-17168) pads every element
    // but a rest element that the initializer lacks, so an element without a
    // default is implicitly `any` (TS7031) rather than a missing property.
    // The expected diagnostics are tsgo's for the same project.
    let (status, stdout) = check_one_file(
        "a.ts",
        concat!(
            "export const typedObject = ({\n",
            "    required,\n",
            "    optional = false,\n",
            "} = {}) => {};\n",
            "export const typedArray = ([\n",
            "    required,\n",
            "    optional = false,\n",
            "] = []) => {};\n",
            "export const typedObjectRest = ({\n",
            "    required,\n",
            "    ...rest\n",
            "} = {}) => {\n",
            "    rest;\n",
            "};\n",
            "typedObject({ required: \"value\" });\n",
        ),
        r#","strict":true"#,
    );
    assert_eq!(status, Some(2));
    assert_eq!(
        stdout,
        concat!(
            "a.ts(2,5): error TS7031: Binding element 'required' implicitly has an 'any' type.\n",
            "a.ts(6,5): error TS7031: Binding element 'required' implicitly has an 'any' type.\n",
            "a.ts(10,5): error TS7031: Binding element 'required' implicitly has an 'any' type.\n",
        )
    );
}

#[test]
fn export_assignment_modules_export_their_types_like_tsgo() {
    // tsgo getExternalModuleMember and getExportsOfModuleWorker
    // (checker.go:14947-14951, 16518-16542): a module defined by `export =`
    // also exports the type and namespace declarations of the original
    // module, so a named import finds them. The expected diagnostics are
    // tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        "type SomeTypeAlias = { x: string };\nclass Foo {}\nexport = Foo;\nexport { SomeTypeAlias };\n",
    )
    .expect("write a.ts");
    fs::write(
        tree.path("b.ts"),
        "import { SomeTypeAlias } from \"./a\";\nconst value: SomeTypeAlias = { x: \"ok\" };\nconst bad: SomeTypeAlias = { x: 1 };\n",
    )
    .expect("write b.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"types":[],"strict":true,"module":"commonjs"},"files":["a.ts","b.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf-8 stdout"),
        "b.ts(3,30): error TS2322: Type 'number' is not assignable to type 'string'.\n"
    );
}

#[test]
fn non_iterable_unions_report_at_every_use_like_tsgo() {
    // tsgo getIterationTypesOfIterable (checker.go:6440-6473): a cached
    // failure is recomputed when errors are reported, so each iteration of a
    // non-iterable union reports TS2488. The expected diagnostics are tsgo's
    // for the same project.
    let (status, stdout) = check_one_file(
        "a.ts",
        concat!(
            "type A = { a: string };\n",
            "type B = { b: string };\n",
            "declare const data: A[] | B;\n",
            "for (const item of data) {\n",
            "    item;\n",
            "}\n",
            "for (const ignoredItem of data) {\n",
            "    ignoredItem;\n",
            "}\n",
            "const [el] = data;\n",
            "export {};\n",
        ),
        r#","strict":true,"lib":["es2015"]"#,
    );
    assert_eq!(status, Some(2));
    assert_eq!(
        stdout,
        concat!(
            "a.ts(4,20): error TS2488: Type 'A[] | B' must have a '[Symbol.iterator]()' method that returns an iterator.\n",
            "a.ts(7,27): error TS2488: Type 'A[] | B' must have a '[Symbol.iterator]()' method that returns an iterator.\n",
            "a.ts(10,7): error TS2488: Type 'A[] | B' must have a '[Symbol.iterator]()' method that returns an iterator.\n",
        )
    );
}

#[test]
fn reachability_is_not_cached_under_a_finally_reduction_like_tsgo() {
    // tsgo isReachableFlowNodeWorker (checker/flow.go:2528-2537) caches a
    // shared node's reachability only while no ReduceLabel narrows a label,
    // so a logical assignment in `finally` does not leave a stale cache that
    // makes an exhaustive switch's end reachable. The expected diagnostics
    // are tsgo's for the same project.
    let (status, stdout) = check_one_file(
        "a.ts",
        concat!(
            "let y = false\n",
            "function test1(x: boolean): number {\n",
            "    try {\n",
            "        switch (x) {\n",
            "            case true: return 1\n",
            "            case false: return 0\n",
            "        }\n",
            "    } finally { y ||= true }\n",
            "}\n",
            "function test3(x: boolean): number {\n",
            "    try {\n",
            "        if (x) { return 1 }\n",
            "    } finally { y ||= true }\n",
            "}\n",
            "export {};\n",
        ),
        r#","strict":true"#,
    );
    assert_eq!(status, Some(2));
    assert_eq!(
        stdout,
        "a.ts(10,29): error TS2366: Function lacks ending return statement and return type does not include 'undefined'.\n"
    );
}

#[test]
fn optional_binding_parameters_skip_the_cached_type_like_tsgo() {
    // tsgo getTypeForBindingElementParent (checker.go:18031-18041) uses a
    // cached parameter type only when it can carry no optionality, so a
    // contextually typed implementation does not leave `| undefined` on the
    // pattern of an optional method parameter. tsgo reports nothing here.
    let (status, stdout) = check_one_file(
        "a.ts",
        concat!(
            "export const mock: I = {\n",
            "    m: (_) => {},\n",
            "};\n",
            "export interface I {\n",
            "    m({ x }?: { x: boolean }): void\n",
            "}\n",
        ),
        "",
    );
    assert_eq!(status, Some(0));
    assert_eq!(stdout, "");
}

#[test]
fn jsx_types_instantiate_only_aliases_and_interfaces_like_tsgo() {
    // tsgo instantiateAliasOrInterfaceWithDefaults (jsx.go:1030-1048)
    // returns no type for any other declared type, so an enum
    // `JSX.ElementType` checks no element type (no TS2786). The expected
    // diagnostics are tsgo's for the same project.
    let (status, stdout) = check_one_file(
        "a.tsx",
        concat!(
            "declare namespace JSX {\n",
            "  enum ElementType {}\n",
            "}\n",
            "declare const C: () => any;\n",
            "const x = <C />;\n",
        ),
        r#","strict":true,"jsx":"react""#,
    );
    assert_eq!(status, Some(2));
    assert_eq!(
        stdout,
        "a.tsx(5,12): error TS2874: This JSX tag requires 'React' to be in scope, but it could not be found.\n"
    );
}

#[test]
fn jsx_namespaced_names_are_string_literal_keys_like_tsgo() {
    // tsgo getLiteralTypeFromPropertyName names a JSX attribute `ns:name` by
    // its text (GetPropertyNameForPropertyNameNode), so a template literal
    // index signature it does not match is not compared. The expected
    // diagnostics are tsgo's for the same project.
    let (status, stdout) = check_one_file(
        "a.tsx",
        concat!(
            "declare global {\n",
            "    namespace JSX {\n",
            "        interface Element {}\n",
            "        interface IntrinsicElements {\n",
            "            div: { [key: `do-${string}`]: Function; \"ns:thing\"?: string };\n",
            "        }\n",
            "    }\n",
            "}\n",
            "export const tag = <div ns:thing=\"a\"/>;\n",
            "export const bad = <div ns:thing={1}/>;\n",
        ),
        r#","strict":true,"jsx":"preserve""#,
    );
    assert_eq!(status, Some(2));
    assert_eq!(
        stdout,
        "a.tsx(10,25): error TS2322: Type 'number' is not assignable to type 'string'.\n"
    );
}

#[test]
fn bigint_literal_unions_order_by_value_like_tsgo() {
    // tsgo CompareTypes (checker/utilities.go:563-566) orders bigint literal
    // types by value. The expected diagnostics are tsgo's for the same
    // project.
    let (status, stdout) = check_one_file(
        "a.ts",
        concat!(
            "declare function f(x: 3n | 1n | 2n): void;\n",
            "f(0n);\n",
            "declare function g(x: -1n | 10n | 9n | 0n): void;\n",
            "g(5n);\n",
            "export {};\n",
        ),
        r#","strict":true,"target":"es2020""#,
    );
    assert_eq!(status, Some(2));
    assert_eq!(
        stdout,
        concat!(
            "a.ts(2,3): error TS2345: Argument of type '0n' is not assignable to parameter of type '1n | 2n | 3n'.\n",
            "a.ts(4,3): error TS2345: Argument of type '5n' is not assignable to parameter of type '-1n | 0n | 9n | 10n'.\n",
        )
    );
}

#[test]
fn declaration_files_bind_in_strict_mode_like_tsgo() {
    // tsgo's binder tracks no strict mode: every strict-mode check applies,
    // declaration files included (binder/binder.go:1436-1439). The expected
    // diagnostics are tsgo's for the same project.
    let (status, stdout) = check_one_file(
        "a.d.ts",
        "declare const foo: any;\nwith (foo) {\n}\n",
        r#","strict":true"#,
    );
    assert_eq!(status, Some(2));
    assert_eq!(
        stdout,
        concat!(
            "a.d.ts(2,1): error TS1036: Statements are not allowed in ambient contexts.\n",
            "a.d.ts(2,1): error TS1101: 'with' statements are not allowed in strict mode.\n",
            "a.d.ts(2,1): error TS2410: The 'with' statement is not supported. All symbols in a 'with' block will have type 'any'.\n",
        )
    );
}

#[test]
fn commonjs_modules_reserve_object_like_tsgo() {
    // tsgo checkCollisionWithGlobalObjectInGeneratedCode
    // (checker.go:10680-10694): a top-level `Object` declaration in a
    // CommonJS module collides with the name its output reserves (an error
    // skipped under noEmit). The expected diagnostics are tsgo's for the
    // same project.
    let tree = TempTree::new();
    fs::write(tree.path("a.ts"), "let Object = 0;\nexport const x = 1;\n").expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"strict":true,"module":"commonjs","outDir":"out"},"files":["a.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf-8 stdout"),
        "a.ts(1,5): error TS2441: Duplicate identifier 'Object'. Compiler reserves name 'Object' in top level scope of a module.\n"
    );
    assert!(tree.path("out/a.js").is_file());
}

#[test]
fn require_declarations_name_their_module_like_tsgo() {
    // tsgo getExternalModuleMember reads a `require` declaration's argument
    // as the module specifier, so TS2305 names the module as written. The
    // expected diagnostics are tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("mod.js"),
        "const donkey = (ast) => ast;\nfunction funky(declaration) {\n    return false;\n}\nmodule.exports = donkey;\nmodule.exports.funky = funky;\n",
    )
    .expect("write mod.js");
    fs::write(
        tree.path("main.js"),
        "const { funky } = require('./mod');\nfunky;\n",
    )
    .expect("write main.js");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"types":[],"allowJs":true,"checkJs":true,"strict":false,"module":"commonjs"},"files":["mod.js","main.js"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf-8 stdout"),
        concat!(
            "main.js(1,9): error TS2305: Module '\"./mod\"' has no exported member 'funky'.\n",
            "mod.js(5,1): error TS2309: An export assignment cannot be used in a module with other exported elements.\n",
            "mod.js(6,16): error TS2339: Property 'funky' does not exist on type '(ast: any) => any'.\n",
        )
    );
}

#[test]
fn conflicting_accessors_mark_every_later_declaration_like_tsgo() {
    // tsgo declareSymbolEx (binder/binder.go:279-285): an accessor that
    // conflicts with another kind of declaration marks the symbol as a full
    // accessor, so a later accessor of the other kind is a duplicate too.
    // The expected diagnostics are tsgo's for the same project.
    let (status, stdout) = check_one_file(
        "a.ts",
        concat!(
            "interface I7 {\n",
            "    get x(): number;\n",
            "    x(): number;\n",
            "    set x(value: number);\n",
            "}\n",
            "export {};\n",
        ),
        r#","strict":true"#,
    );
    assert_eq!(status, Some(2));
    assert_eq!(
        stdout,
        concat!(
            "a.ts(2,9): error TS2300: Duplicate identifier 'x'.\n",
            "a.ts(3,5): error TS2300: Duplicate identifier 'x'.\n",
            "a.ts(4,9): error TS2300: Duplicate identifier 'x'.\n",
        )
    );
}

#[test]
fn comma_sequences_are_parenthesized_like_tsgo() {
    // tsgo emitComputedPropertyName and emitJsxExpression emit their
    // expression at OperatorPrecedenceDisallowComma (printer/printer.go:1221,
    // 4369), so a comma sequence written there is parenthesized. The
    // expected output is tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.tsx"),
        "declare const class1: string, class2: string;\nconst x = { [0, 1]: {} };\nconst elem = <div className={class1, class2}/>;\nexport {};\n",
    )
    .expect("write a.tsx");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"strict":true,"target":"es2015","jsx":"preserve","outDir":"out"},"files":["a.tsx"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        fs::read_to_string(tree.path("out/a.jsx")).expect("read a.jsx"),
        "const x = { [(0, 1)]: {} };\nconst elem = <div className={(class1, class2)}/>;\nexport {};\n"
    );
}

#[test]
fn bigint_metadata_is_guarded_below_es2020_like_tsgo() {
    // tsgo serializeBigIntConstructor (transformers/tstransforms/
    // typeserializer.go:388-399) guards the BigInt constructor below ES2020.
    // The expected output is tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        "declare const dec: any;\nexport class C {\n    @dec\n    x!: bigint;\n}\n",
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"strict":true,"target":"es2015","experimentalDecorators":true,"emitDecoratorMetadata":true,"outDir":"out"},"files":["a.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    let js = fs::read_to_string(tree.path("out/a.js")).expect("read a.js");
    assert!(
        js.ends_with(concat!(
            "export class C {\n",
            "}\n",
            "__decorate([\n",
            "    dec,\n",
            "    __metadata(\"design:type\", typeof BigInt === \"function\" ? BigInt : Object)\n",
            "], C.prototype, \"x\", void 0);\n",
        )),
        "{js}"
    );
}

#[test]
fn javascript_files_keep_empty_imports_and_exports_like_tsgo() {
    // tsgo runs import elision only for TypeScript files without
    // verbatimModuleSyntax (compiler/emitter.go:115), and its type eraser
    // keeps a written `import {}` and `export {}` (typeeraser.go:325-329,
    // 362-367). The expected bytes are tsgo's for the same projects.
    for (module, main, c) in [
        (
            "esnext",
            "import {} from \"./a\";\nexport {};\nexport const y = 1;\n",
            "export {};\n",
        ),
        (
            "commonjs",
            concat!(
                "\"use strict\";\n",
                "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
                "exports.y = void 0;\n",
                "const a_1 = require(\"./a\");\n",
                "exports.y = 1;\n",
            ),
            concat!(
                "\"use strict\";\n",
                "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
            ),
        ),
    ] {
        let tree = TempTree::new();
        fs::write(tree.path("a.ts"), "export const a = \"a\";\n").expect("write a.ts");
        fs::write(
            tree.path("main.js"),
            "import {} from \"./a\";\nexport {};\nexport const y = 1;\n",
        )
        .expect("write main.js");
        fs::write(tree.path("c.ts"), "import {} from \"./a\";\nexport {};\n").expect("write c.ts");
        fs::write(
            tree.path("tsconfig.json"),
            format!(
                r#"{{"compilerOptions":{{"types":[],"allowJs":true,"target":"es2020","module":"{module}","outDir":"out"}},"files":["a.ts","main.js","c.ts"]}}"#
            ),
        )
        .expect("write config");
        let output = run(&tree, &["--pretty", "false"]);
        assert_eq!(output.status.code(), Some(0), "{module}");
        assert!(output.stdout.is_empty(), "{module}");
        assert_eq!(
            fs::read_to_string(tree.path("out/main.js")).expect("read main.js"),
            main,
            "{module}"
        );
        assert_eq!(
            fs::read_to_string(tree.path("out/c.js")).expect("read c.js"),
            c,
            "{module}"
        );
    }
}

#[test]
fn misplaced_module_elements_keep_their_bindings_like_tsgo() {
    // tsgo's import elision visits only the statements of a source file and
    // of namespace bodies (importelision.go:117-125): an import or export
    // that a grammar error placed in a block keeps its bindings, and the
    // runtime syntax transformer drops an alias in a block inside a
    // namespace (runtimesyntax.go:123-125). tsgo never collects the module
    // request of such an import (parser/references.go:11-90), so its lookup
    // finds no resolution. The expected diagnostics and bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "namespace M { }\n",
            "{\n",
            "    export = M;\n",
            "    import I = M;\n",
            "    import { b } from \"./b\";\n",
            "}\n",
        ),
    )
    .expect("write a.ts");
    fs::write(tree.path("b.ts"), "export const b = 1;\n").expect("write b.ts");
    fs::write(
        tree.path("c.ts"),
        concat!(
            "namespace A { export const x = 1; }\n",
            "namespace N {\n",
            "    {\n",
            "        import I = A;\n",
            "        I.x;\n",
            "    }\n",
            "}\n",
        ),
    )
    .expect("write c.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"target":"es2015","module":"commonjs","outDir":"out"},"files":["a.ts","b.ts","c.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf-8 stdout"),
        concat!(
            "a.ts(3,5): error TS1231: An export assignment must be at the top level of a file or module declaration.\n",
            "a.ts(4,5): error TS1232: An import declaration can only be used at the top level of a namespace or module.\n",
            "a.ts(5,5): error TS1232: An import declaration can only be used at the top level of a namespace or module.\n",
            "a.ts(5,23): error TS2307: Cannot find module './b' or its corresponding type declarations.\n",
            "c.ts(4,9): error TS1232: An import declaration can only be used at the top level of a namespace or module.\n",
        )
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/a.js")).expect("read a.js"),
        concat!(
            "\"use strict\";\n",
            "{\n",
            "    export = M;\n",
            "    var I = M;\n",
            "    import { b } from \"./b\";\n",
            "}\n",
        )
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/c.js")).expect("read c.js"),
        concat!(
            "\"use strict\";\n",
            "var A;\n",
            "(function (A) {\n",
            "    A.x = 1;\n",
            "})(A || (A = {}));\n",
            "var N;\n",
            "(function (N) {\n",
            "    {\n",
            "        I.x;\n",
            "    }\n",
            "})(N || (N = {}));\n",
        )
    );
}

#[test]
fn misplaced_module_elements_follow_the_module_transform_like_tsgo() {
    // tsgo's CommonJS transformer converts only top-level imports and
    // exports (commonjsmodule.go:60-128), while its ES module transformer
    // visits every node and drops an import-equals declaration or `export =`
    // wherever it is (esmodule.go:35-53, 117-123, 167-174); an embedded
    // statement left without one prints `;`. The expected diagnostics and
    // bytes are tsgo's for the same projects.
    let cases = [
        (
            "commonjs",
            concat!(
                "export {};\n",
                "function f() {\n",
                "    import x = require(\"./b\");\n",
                "    export * from \"./b\";\n",
                "}\n",
                "switch (1) {\n",
                "    case 1:\n",
                "        import { b } from \"./b\";\n",
                "}\n",
            ),
            concat!(
                "a.ts(3,5): error TS1232: An import declaration can only be used at the top level of a namespace or module.\n",
                "a.ts(4,5): error TS1233: An export declaration can only be used at the top level of a namespace or module.\n",
                "a.ts(8,9): error TS1232: An import declaration can only be used at the top level of a namespace or module.\n",
                "a.ts(8,27): error TS2307: Cannot find module './b' or its corresponding type declarations.\n",
            ),
            concat!(
                "\"use strict\";\n",
                "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
                "function f() {\n",
                "    import x = require(\"./b\");\n",
                "    export * from \"./b\";\n",
                "}\n",
                "switch (1) {\n",
                "    case 1:\n",
                "        import { b } from \"./b\";\n",
                "}\n",
            ),
        ),
        (
            "esnext",
            concat!(
                "export {};\n",
                "declare const c: boolean;\n",
                "if (c) import x = require(\"./b\");\n",
                "while (c) export = 1;\n",
                "{\n",
                "    import z = require(\"./b\");\n",
                "}\n",
            ),
            concat!(
                "a.ts(3,8): error TS1232: An import declaration can only be used at the top level of a namespace or module.\n",
                "a.ts(3,27): error TS2307: Cannot find module './b' or its corresponding type declarations.\n",
                "a.ts(4,11): error TS1231: An export assignment must be at the top level of a file or module declaration.\n",
                "a.ts(6,5): error TS1232: An import declaration can only be used at the top level of a namespace or module.\n",
                "a.ts(6,24): error TS2307: Cannot find module './b' or its corresponding type declarations.\n",
            ),
            "if (c)\n    ;\nwhile (c)\n    ;\n{\n}\nexport {};\n",
        ),
        (
            // EmitContext.VisitEmbeddedStatement (printer/emitcontext.go:
            // 998-1010) puts an empty statement with the removed one's
            // position and comments in its place.
            "esnext",
            concat!(
                "export {};\n",
                "declare const c: boolean;\n",
                "if (c) /*x*/ import x = require(\"./b\"); // t\n",
                "if (c) /*y*/ export type {}; // u\n",
                "while (c) /*z*/ interface I {} // v\n",
            ),
            concat!(
                "a.ts(3,14): error TS1232: An import declaration can only be used at the top level of a namespace or module.\n",
                "a.ts(3,33): error TS2307: Cannot find module './b' or its corresponding type declarations.\n",
                "a.ts(4,14): error TS1233: An export declaration can only be used at the top level of a namespace or module.\n",
                "a.ts(5,27): error TS1156: 'interface' declarations can only be declared inside a block.\n",
            ),
            concat!(
                "if (c) /*x*/\n",
                "    ; // t\n",
                "if (c) /*y*/\n",
                "    ; // u\n",
                "while (c) /*z*/\n",
                "    ; // v\n",
                "export {};\n",
            ),
        ),
    ];
    for (module, text, diagnostics, js) in cases {
        let tree = TempTree::new();
        fs::write(tree.path("a.ts"), text).expect("write a.ts");
        fs::write(tree.path("b.ts"), "export const b = 1;\n").expect("write b.ts");
        fs::write(
            tree.path("tsconfig.json"),
            format!(
                r#"{{"compilerOptions":{{"types":[],"target":"es2020","module":"{module}","outDir":"out"}},"files":["a.ts","b.ts"]}}"#
            ),
        )
        .expect("write config");
        let output = run(&tree, &["--pretty", "false"]);
        assert_eq!(output.status.code(), Some(2), "{module}");
        assert_eq!(
            String::from_utf8(output.stdout).expect("utf-8 stdout"),
            diagnostics,
            "{module}"
        );
        assert_eq!(
            fs::read_to_string(tree.path("out/a.js")).expect("read a.js"),
            js,
            "{module}"
        );
    }
}

/// Compiles `a.ts` with `noEmitHelpers` and returns the emitted `a.js`.
fn emit_one_file(text: &str, target: &str) -> String {
    let tree = TempTree::new();
    fs::write(tree.path("a.ts"), text).expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        format!(
            r#"{{"compilerOptions":{{"types":[],"target":"{target}","module":"esnext","noEmitHelpers":true,"outDir":"out"}},"files":["a.ts"]}}"#
        ),
    )
    .expect("write config");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    fs::read_to_string(tree.path("out/a.js")).expect("read a.js")
}

#[test]
fn async_arrows_in_static_initializers_have_no_lexical_this_like_tsgo() {
    // tsgo's async arrow is no lexical `this` (ast.go:2064-2072), and the
    // class fields transform marks a relocated static initializer and a static
    // block's IIFE EFNoLexicalThis (classfields.go:1417, 2714-2718), so the
    // async transform passes `void 0` (async.go:127-130) and only an explicit
    // `this` needs the class alias. The expected bytes are tsgo's.
    let js = emit_one_file(
        concat!(
            "export class C {\n",
            "    static a = async () => 1;\n",
            "    static b = async () => this.a;\n",
            "    static {\n",
            "        const f = async () => this.b;\n",
            "    }\n",
            "}\n",
            "namespace N {\n",
            "    export class D {\n",
            "        static x = async () => 1;\n",
            "    }\n",
            "}\n",
        ),
        "es2015",
    );
    assert_eq!(
        js,
        concat!(
            "var _a;\n",
            "export class C {\n",
            "}\n",
            "_a = C;\n",
            "C.a = () => __awaiter(void 0, void 0, void 0, function* () { return 1; });\n",
            "C.b = () => __awaiter(void 0, void 0, void 0, function* () { return _a.a; });\n",
            "(() => {\n",
            "    const f = () => __awaiter(void 0, void 0, void 0, function* () { return _a.b; });\n",
            "})();\n",
            "var N;\n",
            "(function (N) {\n",
            "    class D {\n",
            "    }\n",
            "    D.x = () => __awaiter(void 0, void 0, void 0, function* () { return 1; });\n",
            "    N.D = D;\n",
            "})(N || (N = {}));\n",
        )
    );
}

#[test]
fn async_parameters_capture_super_like_tsgo() {
    // tsgo's transformAsyncFunctionBody opens the `_super` capture before it
    // visits the parameters that move into the generator (async.go:713-731),
    // so a `super` use in a parameter initializer is captured; element access
    // alone in an async generator needs no `_super` object (forawait.go:
    // 827-831). The expected bytes are tsgo's.
    let js = emit_one_file(
        concat!(
            "class B { m() { return 1; } }\n",
            "export class C extends B {\n",
            "    async k(b = super.m()) { return b; }\n",
            "    p() {\n",
            "        const g = async (a: number, b = super.m()) => a + b;\n",
            "        return g(1);\n",
            "    }\n",
            "}\n",
        ),
        "es2015",
    );
    assert_eq!(
        js,
        concat!(
            "class B {\n",
            "    m() { return 1; }\n",
            "}\n",
            "export class C extends B {\n",
            "    k() {\n",
            "        const _super = Object.create(null, {\n",
            "            m: { get: () => super.m }\n",
            "        });\n",
            "        return __awaiter(this, arguments, void 0, function* (b = _super.m.call(this)) { return b; });\n",
            "    }\n",
            "    p() {\n",
            "        const _super = Object.create(null, {\n",
            "            m: { get: () => super.m }\n",
            "        });\n",
            "        const g = (a_1, ...args_1) => __awaiter(this, [a_1, ...args_1], void 0, function* (a, b = _super.m.call(this)) { return a + b; });\n",
            "        return g(1);\n",
            "    }\n",
            "}\n",
        )
    );
    let js = emit_one_file(
        concat!(
            "class B { x() { return 1; } }\n",
            "export class C extends B {\n",
            "    async * g() {\n",
            "        super[\"x\"]();\n",
            "        yield 1;\n",
            "    }\n",
            "}\n",
        ),
        "es2017",
    );
    assert_eq!(
        js,
        concat!(
            "class B {\n",
            "    x() { return 1; }\n",
            "}\n",
            "export class C extends B {\n",
            "    g() {\n",
            "        const _superIndex = name => super[name];\n",
            "        return __asyncGenerator(this, arguments, function* g_1() {\n",
            "            _superIndex(\"x\").call(this);\n",
            "            yield yield __await(1);\n",
            "        });\n",
            "    }\n",
            "}\n",
        )
    );
}

#[test]
fn decorated_classes_extending_null_call_no_super_like_tsgo() {
    // tsgo's ES decorator transform synthesizes a constructor without
    // `super(...arguments)` for a class extending `null` (esdecorator.go:737).
    // The expected bytes are tsgo's.
    let js = emit_one_file(
        concat!(
            "declare function dec(target: any, context: any): any;\n",
            "export class C extends null {\n",
            "    @dec x: number = 1;\n",
            "}\n",
        ),
        "es2022",
    );
    assert!(
        js.ends_with(concat!(
            "        x = __runInitializers(this, _x_initializers, 1);\n",
            "        constructor() {\n",
            "            __runInitializers(this, _x_extraInitializers);\n",
            "        }\n",
            "    };\n",
            "})();\n",
            "export { C };\n",
        )),
        "{js}"
    );
}

#[test]
fn lowered_optional_chains_take_one_access_paren_like_tsgo() {
    // tsgo's parenthesizeLeftSideOfAccess leaves a left-hand-side
    // expression such as an instantiation expression bare
    // (ast/utilities.go:396-408), so the lowered chain under `.d` gets one
    // pair of parentheses. The expected diagnostics and bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare namespace A {\n",
            "    export class b<T> {\n",
            "        static d: number;\n",
            "        constructor(x: T);\n",
            "    }\n",
            "}\n",
            "type c = unknown;\n",
            "declare const a: typeof A | undefined;\n",
            "export const x = a?.b<c>.d;\n",
            "export const y = (a?.b)!.d;\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"target":"es2019","module":"esnext","outDir":"out"},"files":["a.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf-8 stdout"),
        "a.ts(9,22): error TS1477: An instantiation expression cannot be followed by a property access.\n"
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/a.js")).expect("read a.js"),
        concat!(
            "export const x = (a === null || a === void 0 ? void 0 : a.b).d;\n",
            "export const y = (a === null || a === void 0 ? void 0 : a.b).d;\n",
        )
    );
}

#[test]
fn nullish_and_optional_chain_temps_are_declared_apart_like_tsgo() {
    // tsgo lowers `??` and optional chains in separate transformers
    // (estransforms/definitions.go:16), and EmitContext.MergeEnvironment puts
    // the later transformer's hoisted `var` statement first, so each scope
    // declares the optional-chain temps and then the nullish ones; the
    // printer names them in that order. The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare const a: { b?: () => number } | undefined;\n",
            "declare const x: number | undefined;\n",
            "declare function g(): number | undefined;\n",
            "export const p = g() ?? 1;\n",
            "export const q = a?.b?.();\n",
            "export function f() {\n",
            "    const r = g() ?? (a?.b?.() ?? 2);\n",
            "    const s = a?.b?.() ?? x;\n",
            "    let t = x;\n",
            "    t ??= g();\n",
            "    return [r, s, t];\n",
            "}\n",
            "export const u = (a?.b ?? g)?.();\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"target":"es2019","module":"esnext","outDir":"out"},"files":["a.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.js")).expect("read a.js"),
        concat!(
            "var _a, _b;\n",
            "var _c, _d;\n",
            "export const p = (_c = g()) !== null && _c !== void 0 ? _c : 1;\n",
            "export const q = (_a = a === null || a === void 0 ? void 0 : a.b) === null || _a === void 0 ? void 0 : _a.call(a);\n",
            "export function f() {\n",
            "    var _a, _b;\n",
            "    var _c, _d, _e;\n",
            "    const r = (_c = g()) !== null && _c !== void 0 ? _c : ((_d = (_a = a === null || a === void 0 ? void 0 : a.b) === null || _a === void 0 ? void 0 : _a.call(a)) !== null && _d !== void 0 ? _d : 2);\n",
            "    const s = (_e = (_b = a === null || a === void 0 ? void 0 : a.b) === null || _b === void 0 ? void 0 : _b.call(a)) !== null && _e !== void 0 ? _e : x;\n",
            "    let t = x;\n",
            "    t !== null && t !== void 0 ? t : (t = g());\n",
            "    return [r, s, t];\n",
            "}\n",
            "export const u = (_b = ((_d = a === null || a === void 0 ? void 0 : a.b) !== null && _d !== void 0 ? _d : g)) === null || _b === void 0 ? void 0 : _b();\n",
        )
    );
}

#[test]
fn namespace_and_enum_names_stay_local_in_commonjs_like_tsgo() {
    // tsgo's CommonJS transform leaves the declaration name of an enum or
    // namespace alone (commonjsmodule.go:2064-2068), so a declaration merged
    // with an exported interface keeps `Foo || (Foo = {})`, while an exported
    // one still assigns `exports.N = N = {}`. The expected diagnostics and
    // bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "export default function Foo() {\n",
            "}\n",
            "namespace Foo {\n",
            "    export var x = 1;\n",
            "}\n",
            "export interface Foo {\n",
            "}\n",
            "export namespace N {\n",
            "    export const y = 2;\n",
            "}\n",
            "export enum E { A }\n",
            "enum F { B }\n",
            "export interface F {}\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"target":"es2015","module":"commonjs","strict":false,"outDir":"out"},"files":["a.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf-8 stdout"),
        concat!(
            "a.ts(1,25): error TS2652: Merged declaration 'Foo' cannot include a default export declaration. Consider adding a separate 'export default Foo' declaration instead.\n",
            "a.ts(3,11): error TS2652: Merged declaration 'Foo' cannot include a default export declaration. Consider adding a separate 'export default Foo' declaration instead.\n",
            "a.ts(12,6): error TS2567: Enum declarations can only merge with namespace or other enum declarations.\n",
            "a.ts(13,18): error TS2567: Enum declarations can only merge with namespace or other enum declarations.\n",
        )
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/a.js")).expect("read a.js"),
        concat!(
            "\"use strict\";\n",
            "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
            "exports.E = exports.N = void 0;\n",
            "exports.default = Foo;\n",
            "function Foo() {\n",
            "}\n",
            "(function (Foo) {\n",
            "    Foo.x = 1;\n",
            "})(Foo || (Foo = {}));\n",
            "var N;\n",
            "(function (N) {\n",
            "    N.y = 2;\n",
            "})(N || (exports.N = N = {}));\n",
            "var E;\n",
            "(function (E) {\n",
            "    E[E[\"A\"] = 0] = \"A\";\n",
            "})(E || (exports.E = E = {}));\n",
            "var F;\n",
            "(function (F) {\n",
            "    F[F[\"B\"] = 0] = \"B\";\n",
            "})(F || (F = {}));\n",
        )
    );
}

#[test]
fn import_equals_require_specifiers_are_rewritten_like_tsgo() {
    // tsgo's ES module transform rewrites the specifier of the require call
    // it creates for an import-equals declaration (createRequireCall,
    // esmodule.go:290-296). The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(tree.path("foo.ts"), "export const x = 1;\n").expect("write foo.ts");
    fs::write(
        tree.path("globals.d.ts"),
        "declare function require(module: string): any;\n",
    )
    .expect("write globals.d.ts");
    fs::write(
        tree.path("main.ts"),
        concat!(
            "import foo = require(\"./foo.ts\");\n",
            "export import bar = require(\"./foo.ts\");\n",
            "export const y = foo.x + bar.x;\n",
        ),
    )
    .expect("write main.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"target":"esnext","module":"preserve","verbatimModuleSyntax":true,"rewriteRelativeImportExtensions":true,"outDir":"out"},"files":["globals.d.ts","foo.ts","main.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/main.js")).expect("read main.js"),
        concat!(
            "const foo = require(\"./foo.js\");\n",
            "const bar = require(\"./foo.js\");\n",
            "export { bar };\n",
            "export const y = foo.x + bar.x;\n",
        )
    );
}

#[test]
fn lowered_nullish_conditionals_keep_their_line_like_tsgo() {
    // tsgo's nullish-coalescing transformer gives the conditional neither a
    // position nor an original (nullishcoalescing.go:34-40), so it prints on
    // the line of the assignment. The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare const a: number | undefined;\n",
            "declare const o: { p?: number };\n",
            "export let x: number;\n",
            "x =\n",
            "    a ?? 1;\n",
            "o.p =\n",
            "    a ?? 2;\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"target":"es2019","module":"esnext","outDir":"out"},"files":["a.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.js")).expect("read a.js"),
        concat!(
            "export let x;\n",
            "x = a !== null && a !== void 0 ? a : 1;\n",
            "o.p = a !== null && a !== void 0 ? a : 2;\n",
        )
    );
}

#[test]
fn jsx_runtime_imports_sort_their_specifiers_like_tsgo() {
    // tsgo's getSortedSpecifiers orders a runtime import's specifiers by
    // imported name (jsx.go:183-195). The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::create_dir_all(tree.path("node_modules/react")).expect("create react");
    fs::write(tree.path("node_modules/react/index.d.ts"), "export {};\n").expect("write index");
    fs::write(
        tree.path("node_modules/react/jsx-runtime.d.ts"),
        concat!(
            "export namespace JSX { interface IntrinsicElements { [k: string]: any } interface Element {} }\n",
            "export function jsx(...a: any[]): any;\n",
            "export function jsxs(...a: any[]): any;\n",
            "export const Fragment: any;\n",
        ),
    )
    .expect("write jsx-runtime");
    fs::write(
        tree.path("node_modules/react/package.json"),
        r#"{"name":"react","version":"1.0.0","types":"index.d.ts"}"#,
    )
    .expect("write package.json");
    fs::write(
        tree.path("a.tsx"),
        concat!(
            "export const a = <div>{1}</div>;\n",
            "export const b = <><span /></>;\n",
            "export const c = <p>{1}{2}</p>;\n",
        ),
    )
    .expect("write a.tsx");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"target":"es2020","module":"esnext","moduleResolution":"bundler","jsx":"react-jsx","outDir":"out"},"files":["a.tsx"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.js")).expect("read a.js"),
        concat!(
            "import { Fragment as _Fragment, jsx as _jsx, jsxs as _jsxs } from \"react/jsx-runtime\";\n",
            "export const a = _jsx(\"div\", { children: 1 });\n",
            "export const b = _jsx(_Fragment, { children: _jsx(\"span\", {}) });\n",
            "export const c = _jsxs(\"p\", { children: [1, 2] });\n",
        )
    );
}

#[test]
fn config_and_paths_follow_tsgo() {
    // tsgo converts the first object of a root array without TS5092
    // (tsoptions/tsconfigparsing.go:319-335) and locates no option syntax in
    // it; a `paths` substitution's own `.ts` extension comes from the
    // configuration, so resolving it to a declaration file is no TS5097
    // (candidateEndingIsFromConfig, module/resolver.go:1265-1283). The
    // expected diagnostics are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("tsconfig.json"),
        r#"[{"compilerOptions": {"types": ["nonexistent"]}, "files": ["index.ts"]}]"#,
    )
    .expect("write config");
    fs::write(tree.path("index.ts"), "export const x = 1;\n").expect("write index.ts");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf-8 stdout"),
        concat!(
            "error TS2688: Cannot find type definition file for 'nonexistent'.\n",
            "  The file is in the program because:\n",
            "    Entry point of type library 'nonexistent' specified in compilerOptions\n",
        )
    );

    let tree = TempTree::new();
    fs::create_dir(tree.path("some-path")).expect("create some-path");
    fs::write(
        tree.path("some-path/index.d.ts"),
        "export declare const blah: 1;\n",
    )
    .expect("write index.d.ts");
    fs::write(
        tree.path("named-import.ts"),
        "import { blah } from \"some-path\";\n\nexport const value = blah;\n",
    )
    .expect("write named-import.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"module":"preserve","moduleResolution":"bundler","noEmit":true,"paths":{"some-path":["./some-path/index.ts"]}},"files":["./named-import.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
}

#[test]
fn optional_tuple_elements_and_template_inference_follow_tsgo() {
    // Under exactOptionalPropertyTypes, tsgo's instantiateMappedTypeTemplate
    // removes only the missing type when `-?` makes an optional tuple element
    // required (checker.go:23073-23074), and template literal inference
    // consumes one code point between adjacent placeholders
    // (checker/relater.go:2462-2486). The expected diagnostics are tsgo's.
    let (status, stdout) = check_one_file(
        "a.ts",
        concat!(
            "type WithArray1 = Required<[(string | undefined)?]>;\n",
            "export const tup1: WithArray1 = [undefined];\n",
            "type ToStringOrUnd<T> = { [P in keyof T]-?: string | undefined };\n",
            "type WithArray2 = ToStringOrUnd<[1, 2?]>;\n",
            "export const tup2: WithArray2 = [\"1\", undefined];\n",
            "export const tup3: Required<[number?]> = [undefined];\n",
        ),
        r#","strict":true,"exactOptionalPropertyTypes":true"#,
    );
    assert_eq!(status, Some(2));
    assert_eq!(
        stdout,
        "a.ts(6,43): error TS2322: Type 'undefined' is not assignable to type 'number'.\n"
    );

    let (status, stdout) = check_one_file(
        "a.ts",
        concat!(
            "type Head<S extends string> = S extends `${infer H}${infer _R}` ? H : never;\n",
            "type Rest<S extends string> = S extends `${infer _H}${infer R}` ? R : never;\n",
            "export const h: \"\\u{1F600}\" = \"x\" as unknown as Head<\"\\u{1F600}abc\">;\n",
            "export const r: \"abc\" = \"x\" as unknown as Rest<\"\\u{1F600}abc\">;\n",
            "export const lone: \"\\uD83D\" = \"x\" as unknown as Head<\"\\uD83Dabc\">;\n",
        ),
        r#","strict":true"#,
    );
    assert_eq!(status, Some(0));
    assert_eq!(stdout, "");
}

#[test]
fn const_tuples_and_empty_objects_follow_tsgo() {
    // tsgo's isMutableArrayLikeType excludes `never` (checker.go:23982-23986),
    // so an `as const` tuple stays readonly through a nested generic call;
    // removeSubtypes keeps `unknown`'s `{}` against a written `{}`
    // (checker.go:26472-26474). The expected diagnostics are tsgo's.
    let (status, stdout) = check_one_file(
        "a.ts",
        concat!(
            "declare function id<T>(x: T): T;\n",
            "declare const a: { a: number };\n",
            "declare const b: { b: number };\n",
            "declare const neverValue: never;\n",
            "export const z = id<typeof neverValue>([a, b] as const);\n",
            "interface Box<T> { readonly value: T; }\n",
            "type Content<R> = R extends Box<infer U> ? U : never;\n",
            "type BoxLike<R> = Box<Content<R>>;\n",
            "declare function box<T>(value: T): Box<T>;\n",
            "declare function f<R extends BoxLike<R>>(v: R): R;\n",
            "const r1 = f(box([a, b] as const));\n",
            "export const check1: Box<[{ a: number }, { b: number }]> = r1;\n",
        ),
        r#","strict":true"#,
    );
    assert_eq!(status, Some(2));
    assert_eq!(
        stdout,
        concat!(
            "a.ts(5,40): error TS2345: Argument of type 'readonly [{ a: number; }, { b: number; }]' is not assignable to parameter of type 'never'.\n",
            "a.ts(12,14): error TS2322: Type 'Box<readonly [{ a: number; }, { b: number; }]>' is not assignable to type 'Box<[{ a: number; }, { b: number; }]>'.\n",
            "  The type 'readonly [{ a: number; }, { b: number; }]' is 'readonly' and cannot be assigned to the mutable type '[{ a: number; }, { b: number; }]'.\n",
        )
    );

    let (status, stdout) = check_one_file(
        "a.ts",
        concat!(
            "declare const v: unknown;\n",
            "const acceptsRecord = (record: Record<string, string>) => {};\n",
            "acceptsRecord(v || {});\n",
            "export const e = {};\n",
            "acceptsRecord(v || e);\n",
            "acceptsRecord({});\n",
        ),
        r#","strict":true"#,
    );
    assert_eq!(status, Some(2));
    assert_eq!(
        stdout,
        concat!(
            "a.ts(3,15): error TS2345: Argument of type '{}' is not assignable to parameter of type 'Record<string, string>'.\n",
            "  Index signature for type 'string' is missing in type '{}'.\n",
            "a.ts(5,15): error TS2345: Argument of type '{}' is not assignable to parameter of type 'Record<string, string>'.\n",
            "  Index signature for type 'string' is missing in type '{}'.\n",
        )
    );
}

#[test]
fn synthetic_accessor_accessibility_follows_tsgo() {
    // tsgo tracks the set-accessor accessibility of union and intersection
    // properties separately (checker.go:21851-21918): a union restricts a
    // write to its most restricted constituent, while an intersection takes
    // the most permissive access. The expected diagnostics are tsgo's.
    let (status, stdout) = check_one_file(
        "a.ts",
        concat!(
            "declare class C1 { get foo(): number; set foo(value: number); }\n",
            "declare class C2 { get foo(): number; protected set foo(value: number); }\n",
            "declare class C3 { protected get foo(): number; protected set foo(value: number); }\n",
            "declare class P2 { get foo(): number; private set foo(value: number); }\n",
            "declare const cu12: C1 | C2;\n",
            "cu12.foo;\n",
            "cu12.foo = 123;\n",
            "declare const cu13: C1 | C3;\n",
            "cu13.foo;\n",
            "declare const pu12: C1 | P2;\n",
            "pu12.foo = 1;\n",
            "declare const ci13: C1 & C3;\n",
            "ci13.foo = 1;\n",
            "declare const ci23: C2 & C3;\n",
            "ci23.foo = 1;\n",
        ),
        "",
    );
    assert_eq!(status, Some(2));
    assert_eq!(
        stdout,
        concat!(
            "a.ts(7,6): error TS2445: Property 'foo' is protected and only accessible within class 'C1 | C2' and its subclasses.\n",
            "a.ts(9,6): error TS2339: Property 'foo' does not exist on type 'C1 | C3'.\n",
            "a.ts(11,6): error TS2341: Property 'foo' is private and only accessible within class 'C1 | P2'.\n",
            "a.ts(15,6): error TS2445: Property 'foo' is protected and only accessible within class 'C2 & C3' and its subclasses.\n",
        )
    );
}

#[test]
fn error_recovery_syntax_is_erased_like_tsgo() {
    // tsgo's type eraser drops type parameters and return types from accessors
    // and constructors, every modifier of a constructor it visits, and `in`/`out`
    // wherever they are not the `in` operator; the `export` keyword counts as
    // TypeScript, while a constructor without TypeScript, such as
    // `accessor constructor() {}`, is kept (typeeraser.go:43-186, ast.go
    // subtree facts). The expected diagnostics and bytes are tsgo's for the same
    // project.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        "class A { accessor constructor() { } }\n",
    )
    .expect("write a.ts");
    fs::write(tree.path("b.ts"), "class B { export constructor() { } }\n").expect("write b.ts");
    fs::write(
        tree.path("c.ts"),
        "class C { get foo<T>() { return 1; } set foo<T>(v): number { } }\n",
    )
    .expect("write c.ts");
    fs::write(
        tree.path("d.ts"),
        concat!(
            "class D { in x = 1; out y = 2; }\n",
            "const isIn = \"x\" in { x: 1 };\n",
        ),
    )
    .expect("write d.ts");
    fs::write(
        tree.path("e.ts"),
        "class E { constructor<T>(): number { } }\n",
    )
    .expect("write e.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"esnext","module":"esnext","outDir":"out","types":[],"noEmitOnError":false},"files":["a.ts","b.ts","c.ts","d.ts","e.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        concat!(
            "a.ts(1,11): error TS1275: 'accessor' modifier can only appear on a property declaration.\n",
            "b.ts(1,11): error TS1031: 'export' modifier cannot appear on class elements of this kind.\n",
            "c.ts(1,15): error TS1094: An accessor cannot have type parameters.\n",
            "c.ts(1,42): error TS1094: An accessor cannot have type parameters.\n",
            "d.ts(1,11): error TS1274: 'in' modifier can only appear on a type parameter of a class, interface or type alias\n",
            "d.ts(1,21): error TS1274: 'out' modifier can only appear on a type parameter of a class, interface or type alias\n",
            "e.ts(1,23): error TS1092: Type parameters cannot appear on a constructor declaration.\n",
        )
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/a.js")).expect("read a.js"),
        concat!(
            "\"use strict\";\n",
            "class A {\n",
            "    accessor constructor() { }\n",
            "}\n",
        )
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/b.js")).expect("read b.js"),
        concat!(
            "\"use strict\";\n",
            "class B {\n",
            "    constructor() { }\n",
            "}\n",
        )
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/c.js")).expect("read c.js"),
        concat!(
            "\"use strict\";\n",
            "class C {\n",
            "    get foo() { return 1; }\n",
            "    set foo(v) { }\n",
            "}\n",
        )
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/d.js")).expect("read d.js"),
        concat!(
            "\"use strict\";\n",
            "class D {\n",
            "    x = 1;\n",
            "    y = 2;\n",
            "}\n",
            "const isIn = \"x\" in { x: 1 };\n",
        )
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/e.js")).expect("read e.js"),
        concat!(
            "\"use strict\";\n",
            "class E {\n",
            "    constructor() { }\n",
            "}\n",
        )
    );
}

#[test]
fn declarations_and_namespace_merges_follow_tsgo() {
    // tsgo keeps a binding pattern without initializers whole in a `.d.ts`,
    // typed as the whole declaration; a variable, function or class already
    // declared in the scope takes the `var` a merging namespace or enum would
    // declare; and a `declare import` is elided. The expected bytes are
    // tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("patterns.ts"),
        concat!(
            "var [] = [1, \"hello\"];\n",
            "var [x] = [1, \"hello\"];\n",
            "var [x1, y1] = [1, \"hello\"];\n",
            "var [, , z1] = [0, 1, 2];\n",
            "var a = [1, \"hello\"];\n",
            "var [x2] = a;\n",
            "var { p, q: [r2] } = { p: 1, q: [true] };\n",
            "var [d = 1] = [2];\n",
            "var { f: { g = 2 } } = { f: { g: 3 } };\n",
            "let [...rest] = [1, 2];\n",
            "declare const tuple: [number, string];\n",
            "const [t1, t2] = tuple;\n",
        ),
    )
    .expect("write patterns");
    fs::write(
        tree.path("merges.ts"),
        concat!(
            "var x5 = 1;\n",
            "namespace x5 { export var y = 2; }\n",
            "function f() {}\n",
            "namespace f { export var a = 1; }\n",
            "var { p } = { p: 1 };\n",
            "namespace p { export var b = 1; }\n",
            "declare var q: any;\n",
            "namespace q { export var c = 1; }\n",
            "namespace r { export var d = 1; }\n",
            "var r = 3;\n",
            "let s = 1;\n",
            "enum s { A }\n",
        ),
    )
    .expect("write merges");
    fs::write(
        tree.path("ambient.ts"),
        concat!("declare import a = b;\n", "var z = 1;\n",),
    )
    .expect("write ambient");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"es2015","declaration":true,"types":[],"outDir":"out"},"files":["patterns.ts","merges.ts","ambient.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        concat!(
            "ambient.ts(1,1): error TS1079: A 'declare' modifier cannot be used with an import declaration.\n",
            "ambient.ts(1,20): error TS2503: Cannot find namespace 'b'.\n",
            "merges.ts(1,5): error TS2300: Duplicate identifier 'x5'.\n",
            "merges.ts(2,11): error TS2300: Duplicate identifier 'x5'.\n",
            "merges.ts(5,7): error TS2300: Duplicate identifier 'p'.\n",
            "merges.ts(6,11): error TS2300: Duplicate identifier 'p'.\n",
            "merges.ts(7,13): error TS2300: Duplicate identifier 'q'.\n",
            "merges.ts(8,11): error TS2300: Duplicate identifier 'q'.\n",
            "merges.ts(9,11): error TS2300: Duplicate identifier 'r'.\n",
            "merges.ts(10,5): error TS2300: Duplicate identifier 'r'.\n",
            "merges.ts(11,5): error TS2567: Enum declarations can only merge with namespace or other enum declarations.\n",
            "merges.ts(12,6): error TS2567: Enum declarations can only merge with namespace or other enum declarations.\n",
        )
    );
    let read = |name: &str| fs::read_to_string(tree.path(name)).expect("read output");
    assert_eq!(
        read("out/patterns.d.ts"),
        concat!(
            "declare var [x]: [number, string];\n",
            "declare var [x1, y1]: [number, string];\n",
            "declare var [, , z1]: [number, number, number];\n",
            "declare var a: (string | number)[];\n",
            "declare var [x2]: (string | number)[];\n",
            "declare var { p, q: [r2] }: {\n",
            "    p: number;\n",
            "    q: [boolean];\n",
            "};\n",
            "declare var d: number;\n",
            "declare var g: number;\n",
            "declare let [...rest]: number[];\n",
            "declare const tuple: [number, string];\n",
            "declare const [t1, t2]: [number, string];\n",
        )
    );
    assert_eq!(
        read("out/merges.js"),
        concat!(
            "\"use strict\";\n",
            "var x5 = 1;\n",
            "(function (x5) {\n",
            "    x5.y = 2;\n",
            "})(x5 || (x5 = {}));\n",
            "function f() { }\n",
            "(function (f) {\n",
            "    f.a = 1;\n",
            "})(f || (f = {}));\n",
            "var { p } = { p: 1 };\n",
            "(function (p) {\n",
            "    p.b = 1;\n",
            "})(p || (p = {}));\n",
            "var q;\n",
            "(function (q) {\n",
            "    q.c = 1;\n",
            "})(q || (q = {}));\n",
            "var r;\n",
            "(function (r) {\n",
            "    r.d = 1;\n",
            "})(r || (r = {}));\n",
            "var r = 3;\n",
            "let s = 1;\n",
            "(function (s) {\n",
            "    s[s[\"A\"] = 0] = \"A\";\n",
            "})(s || (s = {}));\n",
        )
    );
    assert_eq!(
        read("out/ambient.js"),
        concat!("\"use strict\";\n", "var z = 1;\n",)
    );
}

#[test]
fn files_with_parse_errors_emit_their_recovered_trees_like_tsgo() {
    // tsgo prints the tree the parser recovered: missing nodes print as
    // nothing, a `;` between object members leaves a trailing comma, a
    // missing block follows isEmptyBlock, a missing module specifier is
    // `require()`. The same run checks constructor and setter return types,
    // instantiation expressions (JSDoc type arguments included) and a module
    // without a body. The expected bytes are tsgo's for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("recovered.ts"),
        concat!(
            "var f = () => ;\n",
            "var results = number[];\n",
            "var v = `foo ${a;\n",
            "var o = { a: 1; b: 2 };\n",
            "function fn() {\n",
            "    catch (x) { } // missing try\n",
            "    try { }; // missing finally\n",
            "}\n",
        ),
    )
    .expect("write recovered");
    fs::write(
        tree.path("members.ts"),
        concat!(
            "declare function g<T>(): T;\n",
            "const h = g<number>;\n",
            "const j = g<string?>;\n",
            "class D {\n",
            "    constructor(p: any): number {\n",
            "    }\n",
            "    set s(v: any): string {\n",
            "    }\n",
            "}\n",
            "class C {\n",
            "    global x\n",
            "}\n",
        ),
    )
    .expect("write members");
    fs::write(tree.path("imports.ts"), "import * from Zero from \"./0\"\n").expect("write imports");
    fs::write(tree.path("0.ts"), "export class C { }\n").expect("write module");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"target":"es2015","module":"commonjs","types":[],"outDir":"out"},"files":["recovered.ts","members.ts","imports.ts","0.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        concat!(
            "imports.ts(1,10): error TS1005: 'as' expected.\n",
            "imports.ts(1,15): error TS1005: 'from' expected.\n",
            "imports.ts(1,20): error TS1005: ';' expected.\n",
            "members.ts(11,5): error TS1068: Unexpected token. A constructor, method, accessor, or property was expected.\n",
            "members.ts(11,12): error TS1005: ';' expected.\n",
            "members.ts(12,1): error TS1128: Declaration or statement expected.\n",
            "recovered.ts(1,15): error TS1109: Expression expected.\n",
            "recovered.ts(2,22): error TS1011: An element access expression should take an argument.\n",
            "recovered.ts(3,17): error TS1005: '}' expected.\n",
            "recovered.ts(4,15): error TS1005: ',' expected.\n",
            "recovered.ts(6,5): error TS1005: 'try' expected.\n",
            "recovered.ts(7,12): error TS1472: 'catch' or 'finally' expected.\n",
        )
    );
    let read = |name: &str| fs::read_to_string(tree.path(name)).expect("read output");
    assert_eq!(
        read("out/recovered.js"),
        concat!(
            "\"use strict\";\n",
            "var f = () => ;\n",
            "var results = number[];\n",
            "var v = `foo ${a;\n",
            "var o = { a: 1, b: 2 };\n",
            "function fn() {\n",
            "    try {\n",
            "    }\n",
            "    catch (x) { } // missing try\n",
            "    try { }\n",
            "    finally { // missing finally\n",
            "     } // missing finally\n",
            "    ; // missing finally\n",
            "}\n",
        )
    );
    assert_eq!(
        read("out/members.js"),
        concat!(
            "\"use strict\";\n",
            "const h = g;\n",
            "const j = g;\n",
            "class D {\n",
            "    constructor(p) {\n",
            "    }\n",
            "    set s(v) {\n",
            "    }\n",
            "}\n",
            "class C {\n",
            "}\n",
            "x;\n",
        )
    );
    assert!(read("out/imports.js").ends_with(
        "Object.defineProperty(exports, \"__esModule\", { value: true });\nconst from = __importStar(require());\nfrom;\n\"./0\";\n"
    ));
}

#[test]
fn namespace_import_calls_point_at_the_import_like_tsgo() {
    // tsgo invocationErrorRecovery and checkTypeRelatedTo add TS7038 at a
    // namespace-style import whose module would be callable. The expected
    // bytes are tsgo's output for the same project.
    let tree = TempTree::new();
    fs::write(
        tree.path("foo.d.ts"),
        "declare function foo(): void;\ndeclare namespace foo {}\nexport = foo;\n",
    )
    .expect("write foo");
    fs::write(
        tree.path("index.ts"),
        "import * as foo from \"./foo\";\nfoo();\nfunction invoke(f: () => void) { f(); }\ninvoke(foo);\n",
    )
    .expect("write index");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{ "compilerOptions": { "module": "commonjs", "target": "es2015", "esModuleInterop": true, "noEmit": true, "types": [] }, "files": ["foo.d.ts", "index.ts"] }"#,
    )
    .expect("write config");
    let output = run(&tree, &["-p", ".", "--pretty", "true"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        concat!(
            "\u{1b}[96mindex.ts\u{1b}[0m:\u{1b}[93m2\u{1b}[0m:\u{1b}[93m1\u{1b}[0m - \u{1b}[91merror\u{1b}[0m\u{1b}[90m TS2349: \u{1b}[0mThis expression is not callable.\n",
            "  Type '{ default: () => void; }' has no call signatures.\n",
            "\n",
            "\u{1b}[7m2\u{1b}[0m foo();\n",
            "\u{1b}[7m \u{1b}[0m \u{1b}[91m~~~\u{1b}[0m\n",
            "\n",
            "  \u{1b}[96mindex.ts\u{1b}[0m:\u{1b}[93m1\u{1b}[0m:\u{1b}[93m1\u{1b}[0m - Type originates at this import. A namespace-style import cannot be called or constructed, and will cause a failure at runtime. Consider using a default import or import require here instead.\n",
            "    \u{1b}[7m1\u{1b}[0m import * as foo from \"./foo\";\n",
            "    \u{1b}[7m \u{1b}[0m \u{1b}[96m~~~~~~~~~~~~~~~~~~~~~~~~~~~~~\u{1b}[0m\n",
            "\n",
            "\u{1b}[96mindex.ts\u{1b}[0m:\u{1b}[93m4\u{1b}[0m:\u{1b}[93m8\u{1b}[0m - \u{1b}[91merror\u{1b}[0m\u{1b}[90m TS2345: \u{1b}[0mArgument of type '{ default: () => void; }' is not assignable to parameter of type '() => void'.\n",
            "  Type '{ default: () => void; }' provides no match for the signature '(): void'.\n",
            "\n",
            "\u{1b}[7m4\u{1b}[0m invoke(foo);\n",
            "\u{1b}[7m \u{1b}[0m \u{1b}[91m       ~~~\u{1b}[0m\n",
            "\n",
            "  \u{1b}[96mindex.ts\u{1b}[0m:\u{1b}[93m1\u{1b}[0m:\u{1b}[93m1\u{1b}[0m - Type originates at this import. A namespace-style import cannot be called or constructed, and will cause a failure at runtime. Consider using a default import or import require here instead.\n",
            "    \u{1b}[7m1\u{1b}[0m import * as foo from \"./foo\";\n",
            "    \u{1b}[7m \u{1b}[0m \u{1b}[96m~~~~~~~~~~~~~~~~~~~~~~~~~~~~~\u{1b}[0m\n",
            "\n",
            "\n",
            "Found 2 errors in the same file, starting at: index.ts\u{1b}[90m:2\u{1b}[0m\n",
            "\n",
        )
    );
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

    // tsgo reports the normalized absolute paths (tsc.go:165-180), and a
    // directory without a configuration is TS5081 naming the file.
    let root = fs::canonicalize(&tree.root).expect("canonical temp tree");
    let root = root.to_string_lossy().replace('\\', "/");
    let missing_file = run(&tree, &["-p", "missing.json"]);
    assert_eq!(missing_file.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&missing_file.stdout),
        format!("error TS5058: The specified path does not exist: '{root}/missing.json'.\n")
    );
    assert!(missing_file.stderr.is_empty());

    let missing_config = run(&tree, &["-p", "empty/../empty"]);
    assert_eq!(missing_config.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&missing_config.stdout),
        format!(
            "error TS5081: Cannot find a tsconfig.json file at the current directory: {root}/empty/tsconfig.json.\n"
        )
    );
    assert!(missing_config.stderr.is_empty());
}

/// Emit each `(name, text)` file of one program and read back the outputs.
fn emit_files(files: &[(&str, &str)], options: &str) -> Vec<String> {
    let tree = TempTree::new();
    for (name, text) in files {
        fs::write(tree.path(name), text).expect("write source");
    }
    let names = files
        .iter()
        .map(|(name, _)| format!("\"{name}\""))
        .collect::<Vec<_>>()
        .join(",");
    fs::write(
        tree.path("tsconfig.json"),
        format!(r#"{{"compilerOptions":{{{options},"outDir":"out"}},"files":[{names}]}}"#),
    )
    .expect("write config");
    run(&tree, &["--pretty", "false"]);
    files
        .iter()
        .map(|(name, _)| {
            let output = format!("out/{}.js", name.rsplit_once('.').expect("extension").0);
            fs::read_to_string(tree.path(&output)).expect("read output")
        })
        .collect()
}

#[test]
fn statementless_files_keep_the_comments_after_their_skipped_tokens_like_tsgo() {
    // tsgo emits a source file's remaining comments at its statement list's
    // end, the EOF token's full start after any skipped token
    // (emitDetachedCommentsAfterStatementList, printer.go:5404-5417), and
    // GetLeadingCommentRanges leaves a comment on that position's line to the
    // preceding token. The expected bytes are tsgo's.
    let outputs = emit_files(
        &[
            ("a.ts", "/*foo*/ \\"),
            ("b.ts", "/*foo*/ \\ /*bar*/\n"),
            ("c.ts", "/*foo*/ )\n"),
            ("d.ts", "/*a*/\n\n/*b*/ \\ /*c*/\n/*d*/\n"),
            ("e.ts", "/*a*/ /*c*/\n\n/*b*/\n"),
            ("f.ts", "//x\n)\n//y\n"),
        ],
        r#""types":[],"target":"es2015""#,
    );
    assert_eq!(
        outputs,
        [
            "\"use strict\";\n",
            "\"use strict\";\n",
            "\"use strict\";\n",
            "\"use strict\";\n/*a*/\n/*d*/\n",
            "\"use strict\";\n/*a*/ /*c*/\n/*b*/\n",
            "\"use strict\";\n//y\n",
        ]
    );
}

#[test]
fn unclosed_lists_leave_their_comments_to_the_statement_like_tsgo() {
    // tsgo's emitList writes the comments within an empty list through
    // emitTrailingComments and emitLeadingComments (printer.go:4765-4798),
    // and a trailing comma's through emitCommentsAfterToken (5371-5380);
    // emitTrailingComments leaves them to a container ending there
    // (5604-5611), which an unclosed list's statement does. The expected
    // bytes are tsgo's.
    let outputs = emit_files(
        &[
            (
                "a.ts",
                "class Type {\n    public examples = [ // typing here\n}\n",
            ),
            ("b.ts", "var x = [ // typing here\n"),
            ("c.ts", "var x = [ // typing here\n];\n"),
            ("d.ts", "var x = [1, // typing here\n"),
            ("e.ts", "f( // typing here\n"),
            ("f.ts", "f(1, [ /*x*/\n"),
            ("g.ts", "var o = { /*o*/\n"),
            (
                "h.ts",
                "var y = [ // t\n  // u\n];\nvar z = [ /*a*/ /*b*/ ];\n",
            ),
        ],
        r#""types":[],"target":"es2015","useDefineForClassFields":false"#,
    );
    assert_eq!(
        outputs,
        [
            concat!(
                "\"use strict\";\n",
                "class Type {\n",
                "    constructor() {\n",
                "        this.examples = []; // typing here\n",
                "    }\n",
                "}\n",
            ),
            "\"use strict\";\nvar x = []; // typing here\n",
            "\"use strict\";\nvar x = [ // typing here\n];\n",
            "\"use strict\";\nvar x = [1,]; // typing here\n",
            "\"use strict\";\nf(); // typing here\n",
            "\"use strict\";\nf(1, []); /*x*/\n",
            "\"use strict\";\nvar o = {}; /*o*/\n",
            "\"use strict\";\nvar y = [ // t\n// u\n];\nvar z = [ /*a*/ /*b*/];\n",
        ]
    );
}

#[test]
fn jsx_runtime_names_in_commonjs_scripts_keep_their_names_like_tsgo() {
    // tsgo's CommonJS transform leaves a script alone
    // (commonjsmodule.go:228-233), so the JSX runtime names stay as written.
    // The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::create_dir_all(tree.path("node_modules/react")).expect("create react");
    fs::write(tree.path("node_modules/react/index.d.ts"), "export {};\n").expect("write index");
    fs::write(
        tree.path("node_modules/react/jsx-runtime.d.ts"),
        concat!(
            "export namespace JSX { interface IntrinsicElements { [k: string]: any } interface Element {} }\n",
            "export function jsx(...a: any[]): any;\n",
            "export function jsxs(...a: any[]): any;\n",
            "export const Fragment: any;\n",
        ),
    )
    .expect("write jsx-runtime");
    fs::write(
        tree.path("node_modules/react/package.json"),
        r#"{"name":"react","version":"1.0.0","types":"index.d.ts"}"#,
    )
    .expect("write package.json");
    fs::write(
        tree.path("a.tsx"),
        concat!(
            "class C {\n",
            "    render() { return <div>{null /* p */}</div>; }\n",
            "}\n",
            "const x = <><span /></>;\n",
        ),
    )
    .expect("write a.tsx");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"target":"es2020","module":"commonjs","moduleDetection":"legacy","jsx":"react-jsx","outDir":"out"},"files":["a.tsx"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.js")).expect("read a.js"),
        concat!(
            "\"use strict\";\n",
            "class C {\n",
            "    render() { return _jsx(\"div\", { children: null /* p */ }); }\n",
            "}\n",
            "const x = _jsx(_Fragment, { children: _jsx(\"span\", {}) });\n",
        )
    );
}

#[test]
fn the_command_refuses_only_an_emit_that_writes_build_info() {
    // tsgo's command compiles an incremental program for `incremental` or
    // `composite` (execute/tsc.go:245), whose emit writes the build info this
    // command cannot write yet; `tsBuildInfoFile` alone selects none
    // (outputpaths.GetBuildInfoFileName), so tsgo emits the JavaScript alone.
    let tree = TempTree::new();
    fs::write(tree.path("a.ts"), "export const x = 1;\n").expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"target":"es2020","module":"esnext","tsBuildInfoFile":"out/a.tsbuildinfo","outDir":"out"},"files":["a.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.js")).expect("read a.js"),
        "export const x = 1;\n"
    );
    assert!(!tree.path("out/a.tsbuildinfo").exists());

    for option in ["incremental", "composite"] {
        let tree = TempTree::new();
        fs::write(tree.path("a.ts"), "export const x = 1;\n").expect("write a.ts");
        fs::write(
            tree.path("tsconfig.json"),
            format!(
                r#"{{"compilerOptions":{{"types":[],"target":"es2020","module":"esnext","{option}":true,"outDir":"out"}},"files":["a.ts"]}}"#
            ),
        )
        .expect("write config");
        let output = run(&tree, &["--pretty", "false"]);
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            format!("tsc-rs: compiler failure: unsupported emit compiler option: {option}\n")
        );
        assert!(!tree.path("out").exists());
    }
}

#[test]
fn source_maps_follow_tsgo() {
    // tsgo maps only braces among tokens (shouldEmitTokenSourceMaps,
    // printer.go:822-830), so `debugger`, a case colon and `new.target`'s
    // keyword map with their statements; a parameter property assignment
    // has no range of its own (runtimesyntax.go:784-807) and stays the
    // TypeScript transform's unless the class hoists initializers
    // (classfields.go:2365-2377); an erased parameter's name keeps its
    // trailing map (typeeraser.go:228-249); a class with static properties
    // keeps its trailing map; a namespace member name is the declaration
    // name's clone (printer/factory.go:561-586), and a substituted reference
    // is not (runtimesyntax.go:931-945); CommonJS renames an exported class
    // or function with GetDeclarationName and creates `exports.x = x`
    // without a source map, while `exports.x` of a reference or an
    // `import =` clones the name (commonjsmodule.go:502-588, 777-821,
    // 2064-2079); declaration emit updates constructors and methods
    // (declarations/transform.go:1074-1085, 1140-1159). The expected bytes
    // are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "function f(x: number = 1) {\n",
            "    debugger;\n",
            "    switch (x) {\n",
            "        case 1: return new.target;\n",
            "        default: return x;\n",
            "    }\n",
            "}\n",
            "class P {\n",
            "    static s = 1;\n",
            "    constructor(public p: string) { }\n",
            "}\n",
            "namespace N.M {\n",
            "    export class C { }\n",
            "    export var v = 1;\n",
            "    v = 2;\n",
            "}\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("b.ts"),
        concat!(
            "import m = require(\"./c\");\n",
            "export import n = require(\"./c\");\n",
            "export class A { }\n",
            "export function g() { }\n",
            "class B { }\n",
            "export { B };\n",
            "m.q; n.q;\n",
        ),
    )
    .expect("write b.ts");
    fs::write(tree.path("c.ts"), "export const q = 1;\n").expect("write c.ts");
    fs::write(
        tree.path("d.ts"),
        concat!(
            "export class D {\n",
            "    constructor(a: number) { }\n",
            "    m(): void { }\n",
            "}\n",
        ),
    )
    .expect("write d.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"target":"es2015","module":"commonjs","sourceMap":true,"declaration":true,"declarationMap":true,"outDir":"out"},"files":["a.ts","b.ts","c.ts","d.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    let read = |name: &str| fs::read_to_string(tree.path(name)).expect("read map");
    assert_eq!(
        read("out/a.js.map"),
        r#"{"version":3,"file":"a.js","sourceRoot":"","sources":["../a.ts"],"names":[],"mappings":";AAAA,SAAS,CAAC,CAAC,CAAC,GAAW,CAAC;IACpB,SAAS;IACT,QAAQ,CAAC,EAAE,CAAC;QACR,KAAK,CAAC,EAAE,OAAO,IAAI,MAAM,CAAC;QAC1B,SAAS,OAAO,CAAC,CAAC;IACtB,CAAC;AACL,CAAC;AACD,MAAM,CAAC;IAEH,YAAmB,CAAS;iBAAT,CAAC;IAAY,CAAC;CACpC;AAFU,GAAC,GAAG,CAAC,CAAC;AAGjB,IAAU,CAAC,CAIV;AAJD,WAAU,CAAC;IAAC,IAAA,CAAC,CAIZ;IAJW,WAAA,CAAC;QACT,MAAa,CAAC;SAAI;QAAL,EAAA,CAAC,IAAI,CAAA;QACP,GAAC,GAAG,CAAC,CAAC;QACjB,GAAC,GAAG,CAAC,CAAC;IACV,CAAC,EAJW,CAAC,GAAD,EAAA,CAAC,KAAD,EAAA,CAAC,QAIZ;AAAD,CAAC,EAJS,CAAC,KAAD,CAAC,QAIV"}"#
    );
    assert_eq!(
        read("out/a.d.ts.map"),
        r#"{"version":3,"file":"a.d.ts","sourceRoot":"","sources":["../a.ts"],"names":[],"mappings":"AAAA,iBAAS,CAAC,CAAC,CAAC,GAAE,MAAU,qBAMvB;AACD,cAAM,CAAC;IAEgB,CAAC,EAAE,MAAM;IAD5B,MAAM,CAAC,CAAC,SAAK;IACb,YAAmB,CAAC,EAAE,MAAM,EAAK;CACpC;AACD,kBAAU,CAAC,CAAC,CAAC,CAAC;IACV,MAAa,CAAC;KAAI;IACX,IAAI,CAAC,QAAI,CAAC;CAEpB"}"#
    );
    assert_eq!(
        read("out/b.js.map"),
        r#"{"version":3,"file":"b.js","sourceRoot":"","sources":["../b.ts"],"names":[],"mappings":";;;;AAAA,MAAO,CAAC,kBAAkB;AAC1B,QAAc,CAAC,kBAAkB;AACjC;CAAkB;;AAClB,eAAsB,CAAC;AACvB,MAAM,CAAC;CAAI;QACF,CAAC;AACV,CAAC,CAAC,CAAC,CAAC;AAAC,QAAA,CAAC,CAAC,CAAC,CAAC"}"#
    );
    assert_eq!(
        read("out/d.js.map"),
        r#"{"version":3,"file":"d.js","sourceRoot":"","sources":["../d.ts"],"names":[],"mappings":";;;AAAA;IACI,YAAY,CAAS,IAAI,CAAC;IAC1B,CAAC,KAAW,CAAC;CAChB"}"#
    );
    assert_eq!(
        read("out/d.d.ts.map"),
        r#"{"version":3,"file":"d.d.ts","sourceRoot":"","sources":["../d.ts"],"names":[],"mappings":"AAAA,qBAAa,CAAC;IACV,YAAY,CAAC,EAAE,MAAM,EAAK;IAC1B,CAAC,IAAI,IAAI,CAAI;CAChB"}"#
    );
}

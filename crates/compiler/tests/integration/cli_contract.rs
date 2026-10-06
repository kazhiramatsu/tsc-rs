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

#[test]
fn const_enum_values_are_inlined_after_the_module_transform_like_tsgo() {
    // tsgo inlines const enum values in a transform after the module
    // transform (compiler/emitter.go:174-177): the literal has no range
    // (inliners/constenum.go:32-89), so a line break before the access is
    // not kept and a comment before it is not written, while a separator
    // after it reads its own leading comments (printer.go:2901-2921). The
    // maps follow tsgo: a namespace import declares its name's clone, an
    // optional call's receiver and a concise body's parentheses map, a
    // lowered `catch {}` and parentheses the parenthesizer adds do not
    // (commonjsmodule.go:722-760, optionalchain.go:198-204,
    // optionalcatch.go:24-30, printer.go:2669-2685, 3222-3226). The expected
    // bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "const enum F { A = 1, B = 2, None = 0 }\n",
            "declare function g(): boolean;\n",
            "declare function h(a: number, b: number): number;\n",
            "const x = g()\n",
            "    // one\n",
            "    ? F.A\n",
            "    // two\n",
            "    : F.B;\n",
            "const y = g() ? F.A :\n",
            "    F.B;\n",
            "const z = h(F.A, /*b:*/ F.None);\n",
            "const w = F.A\n",
            "    | F.B;\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("b.ts"),
        concat!(
            "import * as ns from \"./c\";\n",
            "declare const r: { close?(): void; v?: number } | undefined;\n",
            "r?.close?.();\n",
            "if (!r?.v) { }\n",
            "try { r?.close?.(); } catch { }\n",
            "const o = () => ({ v: r } as object);\n",
            "ns.q;\n",
        ),
    )
    .expect("write b.ts");
    fs::write(tree.path("c.ts"), "export const q = 1;\n").expect("write c.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"target":"es2017","module":"commonjs","sourceMap":true,"outDir":"out"},"files":["a.ts","b.ts","c.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    let read = |name: &str| fs::read_to_string(tree.path(name)).expect("read output");
    assert_eq!(
        read("out/a.js"),
        concat!(
            "\"use strict\";\n",
            "const x = g()\n",
            "    // one\n",
            "    ? 1 /* F.A */ \n",
            "// two\n",
            ": 2 /* F.B */;\n",
            "const y = g() ? 1 /* F.A */ : 2 /* F.B */;\n",
            "const z = h(1 /* F.A */, 0 /* F.None */);\n",
            "const w = 1 /* F.A */ | 2 /* F.B */;\n",
            "//# sourceMappingURL=a.js.map",
        )
    );
    assert_eq!(
        read("out/a.js.map"),
        r#"{"version":3,"file":"a.js","sourceRoot":"","sources":["../a.ts"],"names":[],"mappings":";AAGA,MAAM,CAAC,GAAG,CAAC,EAAE;IACT,MAAM;IACN,CAAC;AACD,MAAM;AACN,CAAC,YAAI,CAAC;AACV,MAAM,CAAC,GAAG,CAAC,EAAE,CAAC,CAAC,aAAK,CAAC,YACd,CAAC;AACR,MAAM,CAAC,GAAG,CAAC,6BAAoB,CAAC;AAChC,MAAM,CAAC,GAAG,yBACD,CAAC"}"#
    );
    assert_eq!(
        read("out/b.js.map"),
        r#"{"version":3,"file":"b.js","sourceRoot":"","sources":["../b.ts"],"names":[],"mappings":";;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;AAAA,MAAY,EAAE,gCAAY;AAE1B,MAAA,CAAC,aAAD,CAAC,uBAAD,CAAC,CAAE,KAAK,+CAAR,CAAC,CAAW,CAAC;AACb,IAAI,EAAC,CAAC,aAAD,CAAC,uBAAD,CAAC,CAAE,CAAC,CAAA,EAAE,CAAC,CAAC,CAAC;AACd,IAAI,CAAC;IAAC,MAAA,CAAC,aAAD,CAAC,uBAAD,CAAC,CAAE,KAAK,+CAAR,CAAC,CAAW,CAAC;AAAC,CAAC;WAAO,CAAC,CAAC,CAAC;AAC/B,MAAM,CAAC,GAAG,GAAG,EAAE,CAAC,CAAC,EAAE,CAAC,EAAE,CAAC,EAAa,CAAA,CAAC;AACrC,EAAE,CAAC,CAAC,CAAC"}"#
    );
}

#[test]
fn const_enum_values_are_inlined_in_kept_rebuilt_and_moved_subtrees_like_tsgo() {
    // The inlining visit skips a parsed subtree the earlier transforms kept
    // unless it holds an access with a constant value, so accesses are
    // placed in a statement no transform changed, a function the type
    // eraser rebuilt, class field initializers moved into the constructor
    // or after the class, an element access, an imported const enum, and
    // receivers that need parentheses. The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "import { K } from \"./k\";\n",
            "const enum E { A = 1, B = 2 }\n",
            "const enum S { X = \"x\", N = -1 }\n",
            "declare let k: number;\n",
            "if (k === E.A) { k = E.B; }\n",
            "function f(p: number): number { return p === E.A ? E.B : p; }\n",
            "class C {\n",
            "    v = E[\"A\"] + K.Y;\n",
            "    static s = S.X;\n",
            "    m() { return S.N.toString() + E.B.toFixed(); }\n",
            "}\n",
            "export const t = [K.Y, new C().v, f(E.B)];\n",
        ),
    )
    .expect("write a.ts");
    fs::write(tree.path("k.ts"), "export const enum K { Y = 3 }\n").expect("write k.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"target":"es2017","module":"commonjs","sourceMap":true,"outDir":"out"},"files":["a.ts","k.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    let read = |name: &str| fs::read_to_string(tree.path(name)).expect("read output");
    assert_eq!(
        read("out/a.js"),
        concat!(
            "\"use strict\";\n",
            "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
            "exports.t = void 0;\n",
            "if (k === 1 /* E.A */) {\n",
            "    k = 2 /* E.B */;\n",
            "}\n",
            "function f(p) { return p === 1 /* E.A */ ? 2 /* E.B */ : p; }\n",
            "class C {\n",
            "    constructor() {\n",
            "        this.v = 1 /* E[\"A\"] */ + 3 /* K.Y */;\n",
            "    }\n",
            "    m() { return (-1 /* S.N */).toString() + 2 /* E.B */.toFixed(); }\n",
            "}\n",
            "C.s = \"x\" /* S.X */;\n",
            "exports.t = [3 /* K.Y */, new C().v, f(2 /* E.B */)];\n",
            "//# sourceMappingURL=a.js.map",
        )
    );
    assert_eq!(
        read("out/a.js.map"),
        r#"{"version":3,"file":"a.js","sourceRoot":"","sources":["../a.ts"],"names":[],"mappings":";;;AAIA,IAAI,CAAC,gBAAQ,EAAE,CAAC;IAAC,CAAC,cAAM,CAAC;AAAC,CAAC;AAC3B,SAAS,CAAC,CAAC,CAAS,IAAY,OAAO,CAAC,gBAAQ,CAAC,CAAC,aAAK,CAAC,CAAC,CAAC,CAAC,CAAC,CAAC;AAC7D,MAAM,CAAC;IAAP;QACI,MAAC,GAAG,4BAAY,CAAC;IAGrB,CAAC;IADG,CAAC,KAAK,OAAO,eAAI,QAAQ,EAAE,GAAG,YAAI,OAAO,EAAE,CAAC,CAAC,CAAC;CACjD;AAFU,GAAC,gBAAA,CAAO;AAGN,QAAA,CAAC,GAAG,cAAM,IAAI,CAAC,EAAE,CAAC,CAAC,EAAE,CAAC,aAAK,CAAC,CAAC"}"#
    );
}

#[test]
fn concise_body_blocks_and_name_clones_map_like_tsgo() {
    // A concise body that a lowering turns into a block has the body's range on
    // its statement list and writes no comments of its own (ConvertToFunctionBlock
    // and VisitFunctionBody, printer/emitcontext.go:930-962): the `}` maps after
    // the body, the comments after the body are written before it, and a single-
    // line block's return statement leaves its trailing comment to the arrow
    // function. tsgo's name clones keep their range: a parameter default's check
    // (emitcontext.go:892-917), a private field's receiver (classfields.go:
    // 1269-1281) and a function-valued export (commonjsmodule.go:1067-1077) map,
    // while `exports.x = x;` for `export { x }` has only a comment range
    // (commonjsmodule.go:578-588), a class fields default export maps nothing
    // (GetLocalName) and a `using` block keeps its statement range (using.go:
    // 204-206). The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare const r: { a?: { b: number } } | undefined;\n",
            "export const f = () => r?.a?.b /* t */;\n",
            "export function outer(o?: { p?: { q: Promise<number> } }) {\n",
            "  const get = () =>\n",
            "    o?.p?.q\n",
            "      .then((d) => d)\n",
            "\n",
            "  // after the body\n",
            "  return get;\n",
            "}\n",
            "export function h(/*c1*/ v /*c2*/ = r?.a?.b) { return v; }\n",
            "export class Q {\n",
            "    #n = 0;\n",
            "    run() { this.#n++; }\n",
            "}\n",
            "const local = () => 1;\n",
            "export { local };\n",
            "export async function u() {\n",
            "    await using d = { async [Symbol.asyncDispose]() {} };\n",
            "    return d;\n",
            "}\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("b.ts"),
        concat!(
            "export default class R {\n",
            "    static empty = new R();\n",
            "}\n",
        ),
    )
    .expect("write b.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"target":"es2018","module":"commonjs","lib":["esnext"],"noEmitHelpers":true,"sourceMap":true,"outDir":"out"},"files":["a.ts","b.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    let read = |name: &str| fs::read_to_string(tree.path(name)).expect("read output");
    assert_eq!(
        read("out/a.js"),
        concat!(
            "\"use strict\";\n",
            "var _Q_n;\n",
            "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
            "exports.local = exports.Q = exports.f = void 0;\n",
            "exports.outer = outer;\n",
            "exports.h = h;\n",
            "exports.u = u;\n",
            "const f = () => { var _a; return (_a = r === null || r === void 0 ? void 0 : r.a) === null || _a === void 0 ? void 0 : _a.b; } /* t */;\n",
            "exports.f = f;\n",
            "function outer(o) {\n",
            "    const get = () => {\n",
            "        var _a;\n",
            "        return (_a = o === null || o === void 0 ? void 0 : o.p) === null || _a === void 0 ? void 0 : _a.q.then((d) => d);\n",
            "        // after the body\n",
            "    };\n",
            "    // after the body\n",
            "    return get;\n",
            "}\n",
            "function h(/*c1*/ v /*c2*/) { var _a; if (v /*c2*/ === void 0) { v /*c2*/ = (_a = r === null || r === void 0 ? void 0 : r.a) === null || _a === void 0 ? void 0 : _a.b; } return v; }\n",
            "class Q {\n",
            "    constructor() {\n",
            "        _Q_n.set(this, 0);\n",
            "    }\n",
            "    run() { var _a; __classPrivateFieldSet(this, _Q_n, (_a = __classPrivateFieldGet(this, _Q_n, \"f\"), _a++, _a), \"f\"); }\n",
            "}\n",
            "exports.Q = Q;\n",
            "_Q_n = new WeakMap();\n",
            "const local = () => 1;\n",
            "exports.local = local;\n",
            "async function u() {\n",
            "    const env_1 = { stack: [], error: void 0, hasError: false };\n",
            "    try {\n",
            "        const d = __addDisposableResource(env_1, { async [Symbol.asyncDispose]() { } }, true);\n",
            "        return d;\n",
            "    }\n",
            "    catch (e_1) {\n",
            "        env_1.error = e_1;\n",
            "        env_1.hasError = true;\n",
            "    }\n",
            "    finally {\n",
            "        const result_1 = __disposeResources(env_1);\n",
            "        if (result_1)\n",
            "            await result_1;\n",
            "    }\n",
            "}\n",
            "//# sourceMappingURL=a.js.map",
        )
    );
    assert_eq!(
        read("out/a.js.map"),
        r#"{"version":3,"file":"a.js","sourceRoot":"","sources":["../a.ts"],"names":[],"mappings":";;;;;;;AACO,MAAM,CAAC,GAAG,GAAG,EAAE,WAAC,OAAA,MAAA,CAAC,aAAD,CAAC,uBAAD,CAAC,CAAE,CAAC,0CAAE,CAAC,CAAA,CAAQ,CAAC,AAAT,CAAC,OAAO,CAAC;AAA1B,QAAA,CAAC,GAAD,CAAC,CAAyB;AACvC,eAAsB,CAAkC;IACtD,MAAM,GAAG,GAAG,GAAG,EAAE;;QACf,OAAA,MAAA,CAAC,aAAD,CAAC,uBAAD,CAAC,CAAE,CAAC,0CAAE,CAAC,CACJ,IAAI,CAAC,CAAC,CAAC,EAAE,EAAE,CAAC,CAAC,CAAC,CAAA;QAEnB,iBAAiB;IACjB,CAAC,AAHkB,CAAA;IAEnB,iBAAiB;IACjB,OAAO,GAAG,CAAC;AACb,CAAC;AACD,WAAkB,MAAM,CAAC,CAAC,CAAC,MAAgB,gBAAlB,CAAC,CAAC,MAAM,aAAR,EAAA,EAAE,MAAM,SAAG,CAAC,aAAD,CAAC,uBAAD,CAAC,CAAE,CAAC,0CAAE,CAAC,IAAI,OAAO,CAAC,CAAC,CAAC,CAAC;AAC1D;IAAA;QACI,eAAK,CAAC,EAAC;IAEX,CAAC;IADG,GAAG,aAAK,uBAAA,IAAI,SAAJ,4BAAA,IAAI,YAAG,EAAP,IAAS,IAAA,OAAA,CAAC,CAAC,CAAC;CACvB;;;AACD,MAAM,KAAK,GAAG,GAAG,EAAE,CAAC,CAAC,CAAC;QACb,KAAK;AACP,KAAK;;;QACR,MAAY,CAAC,kCAAG,EAAE,KAAK,CAAC,CAAC,MAAM,CAAC,YAAY,CAAC,KAAI,CAAC,EAAE,OAAA,CAAC;QACrD,OAAO,CAAC,CAAC;;;;;;;;;;;AACb,CAAC"}"#
    );
    assert_eq!(
        read("out/b.js"),
        concat!(
            "\"use strict\";\n",
            "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
            "class R {\n",
            "}\n",
            "R.empty = new R();\n",
            "exports.default = R;\n",
            "//# sourceMappingURL=b.js.map",
        )
    );
    assert_eq!(
        read("out/b.js.map"),
        r#"{"version":3,"file":"b.js","sourceRoot":"","sources":["../b.ts"],"names":[],"mappings":";;AAAA,MAAqB,CAAC;CAErB;AADU,OAAK,GAAG,IAAI,CAAC,EAAE,CAAC"}"#
    );
}

#[test]
fn print_time_parentheses_and_new_callees_follow_tsgo() {
    // Parentheses that precedence needs are written while printing and map
    // nothing (printer.go:3222-3226), including those around an optional chain
    // whose type assertion was erased. A `new` callee is parenthesized only when
    // it is itself a call (printer.go:2592-2605), so `new (load() as any).C()`
    // prints `new load().C()` as tsgo does. A class fields default export after
    // a moved static initializer maps nothing. The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare const result: { $defs?: { Inner?: { id: number } } };\n",
            "declare function load(): { C: new () => object };\n",
            "export const id = (result.$defs?.Inner as any).id;\n",
            "export const made = new (load() as any).C();\n",
            "export const kept = new (load().C)();\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("b.ts"),
        concat!(
            "export default class R {\n",
            "    static empty = new R();\n",
            "}\n",
        ),
    )
    .expect("write b.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"target":"es2021","module":"esnext","sourceMap":true,"outDir":"out"},"files":["a.ts","b.ts"]}"#,
    )
    .expect("write config");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    let read = |name: &str| fs::read_to_string(tree.path(name)).expect("read output");
    assert_eq!(
        read("out/a.js"),
        concat!(
            "export const id = (result.$defs?.Inner).id;\n",
            "export const made = new load().C();\n",
            "export const kept = new (load().C)();\n",
            "//# sourceMappingURL=a.js.map",
        )
    );
    assert_eq!(
        read("out/a.js.map"),
        r#"{"version":3,"file":"a.js","sourceRoot":"","sources":["../a.ts"],"names":[],"mappings":"AAEA,MAAM,CAAC,MAAM,EAAE,GAAG,CAAC,MAAM,CAAC,KAAK,EAAE,KAAa,EAAC,EAAE,CAAC;AAClD,MAAM,CAAC,MAAM,IAAI,GAAG,IAAK,IAAI,EAAU,CAAC,CAAC,EAAE,CAAC;AAC5C,MAAM,CAAC,MAAM,IAAI,GAAG,IAAI,CAAC,IAAI,EAAE,CAAC,CAAC,CAAC,EAAE,CAAC"}"#
    );
    assert_eq!(
        read("out/b.js"),
        concat!(
            "class R {\n",
            "}\n",
            "R.empty = new R();\n",
            "export default R;\n",
            "//# sourceMappingURL=b.js.map",
        )
    );
    assert_eq!(
        read("out/b.js.map"),
        r#"{"version":3,"file":"b.js","sourceRoot":"","sources":["../b.ts"],"names":[],"mappings":"AAAA,MAAqB,CAAC;CAErB;AADU,OAAK,GAAG,IAAI,CAAC,EAAE,CAAC"}"#
    );
}

#[test]
fn declaration_type_nodes_are_named_reused_and_mapped_like_tsgo() {
    // The node builder as tsgo runs it for declarations. The expected bytes are
    // tsgo's.
    // - A name's path is the shortest of all candidate chains, the first by
    //   declaration order among equals (checker/symbolaccessibility.go:535-609):
    //   `Any` from its named import, not `Rpc.Any` from the namespace import
    //   written first. A link is named by its own export when the parent has it
    //   (checker/nodebuilderimpl.go:770-793): `.stringify`, not the `default`
    //   the module declares first.
    // - A node reused from another file has no position, its tokens included
    //   (the `?` of `waitUntil?`), and its lists have no trailing comma
    //   (checker/nodecopy.go:827-900; `{ a, b }`).
    // - A fake scope's parameter or type parameter hides a name only for its
    //   own meaning: `<Rpc extends Rpc.Any>` reuses the constraint as written,
    //   mapped part by part.
    // - A constraint is reused under the signature's mapper, and only the type
    //   parameters the mapper replaces are rebuilt (nodebuilderimpl.go:
    //   1669-1681, nodecopy.go:416-435): `Extract` and `Record` map in `t1`.
    // - The serialized type cache lasts one resolver request and hands out
    //   position-free copies (nodebuilderimpl.go:3229-3273, ast/deepclone.go):
    //   the second `{ match; check? }` of `setup` maps nothing, while `one` and
    //   `two` both map.
    // - A type parameter's renamed name (`K_1`) is a shared node that keeps no
    //   range (setTextRange, nodebuilderimpl.go:1434-1460).
    let tree = TempTree::new();
    fs::create_dir_all(tree.path("other/node_modules/pkg/esm"))
        .expect("create other/node_modules/pkg/esm");
    fs::create_dir_all(tree.path("src")).expect("create src");
    fs::write(
        tree.path("other/node_modules/pkg/index.d.ts"),
        concat!(
            "export function stringify(value: unknown): string\n",
            "export function configure(options: object): typeof stringify\n",
            "export const tools: { pick: (s: string, { a, b, }?: { a?: number; b?: number }) => string }\n",
        ),
    )
    .expect("write other/node_modules/pkg/index.d.ts");
    fs::write(
        tree.path("other/node_modules/pkg/esm/wrapper.d.ts"),
        concat!(
            "import { stringify } from '../index.js'\n",
            "\n",
            "export * from '../index.js'\n",
            "export default stringify\n",
        ),
    )
    .expect("write other/node_modules/pkg/esm/wrapper.d.ts");
    fs::write(
        tree.path("other/node_modules/pkg/package.json"),
        r#"{"name":"pkg","version":"1.0.0","exports":{"require":"./index.js","import":"./esm/wrapper.js"},"typings":"index.d.ts"}"#,
    )
    .expect("write other/node_modules/pkg/package.json");
    fs::write(
        tree.path("other/amb.d.ts"),
        concat!(
            "declare module 'compiled/pkg' {\n",
            "  import * as m from 'pkg'\n",
            "  export = m\n",
            "}\n",
        ),
    )
    .expect("write other/amb.d.ts");
    fs::write(
        tree.path("src/handler.ts"),
        concat!(
            "export function getHandler() {\n",
            "  return async (req: number, ctx: { waitUntil?: (prom: Promise<void>) => void; meta?: string }) => {};\n",
            "}\n",
        ),
    )
    .expect("write src/handler.ts");
    fs::write(
        tree.path("src/rpc.ts"),
        "export interface Any { readonly id: number }\n",
    )
    .expect("write src/rpc.ts");
    fs::write(
        tree.path("src/user.ts"),
        concat!(
            "import { configure, tools } from 'compiled/pkg';\n",
            "import { getHandler } from \"./handler\";\n",
            "import * as Rpc from \"./rpc\";\n",
            "import { Any } from \"./rpc\";\n",
            "declare function wrap<F>(f: F): F;\n",
            "export const handler = getHandler();\n",
            "export const s = configure({});\n",
            "export const copy = { ...tools };\n",
            "export const makeRequest = <Rpc extends Rpc.Any>(options: { readonly rpc: Rpc }) => options;\n",
            "export const named = (o: Rpc.Any) => o;\n",
            "export const d1 = <D extends string>(field: D) =>\n",
            "<R, Fn extends (_: Extract<R, Record<D, string>>) => R>(f: Fn) => f;\n",
            "export const t1 = d1(\"_tag\");\n",
            "export const build = <T>(item: T): T & { match: Any; check?: boolean } => item as any;\n",
            "export function setup() {\n",
            "  return { headers: [build({ a: 1 })], other: [build({ a: 1 })] };\n",
            "}\n",
            "export const one = build({ a: 1 });\n",
            "export const two = build({ a: 1 });\n",
            "export class Res<K, A> {\n",
            "  constructor(readonly k?: K, readonly a?: A) {}\n",
            "  static make = wrap(function <K, A>(lookup: (key: K) => A) { return new Res<K, A>(); });\n",
            "}\n",
        ),
    )
    .expect("write src/user.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"target":"es2022","module":"esnext","moduleResolution":"bundler","declaration":true,"declarationMap":true,"emitDeclarationOnly":true,"rootDir":".","outDir":"out"},"files":["other/amb.d.ts","src/handler.ts","src/rpc.ts","src/user.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    let read = |name: &str| fs::read_to_string(tree.path(name)).expect("read output");
    assert_eq!(
        read("out/src/user.d.ts"),
        concat!(
            "import * as Rpc from \"./rpc\";\n",
            "import { Any } from \"./rpc\";\n",
            "export declare const handler: (req: number, ctx: {\n",
            "    waitUntil?: (prom: Promise<void>) => void;\n",
            "    meta?: string;\n",
            "}) => Promise<void>;\n",
            "export declare const s: typeof import(\"compiled/pkg\").stringify;\n",
            "export declare const copy: {\n",
            "    pick: (s: string, { a, b }?: {\n",
            "        a?: number;\n",
            "        b?: number;\n",
            "    }) => string;\n",
            "};\n",
            "export declare const makeRequest: <Rpc extends Rpc.Any>(options: {\n",
            "    readonly rpc: Rpc;\n",
            "}) => {\n",
            "    readonly rpc: Rpc;\n",
            "};\n",
            "export declare const named: (o: Rpc.Any) => Any;\n",
            "export declare const d1: <D extends string>(field: D) => <R, Fn extends (_: Extract<R, Record<D, string>>) => R>(f: Fn) => Fn;\n",
            "export declare const t1: <R, Fn extends (_: Extract<R, Record<\"_tag\", string>>) => R>(f: Fn) => Fn;\n",
            "export declare const build: <T>(item: T) => T & {\n",
            "    match: Any;\n",
            "    check?: boolean;\n",
            "};\n",
            "export declare function setup(): {\n",
            "    headers: ({\n",
            "        a: number;\n",
            "    } & {\n",
            "        match: Any;\n",
            "        check?: boolean;\n",
            "    })[];\n",
            "    other: ({\n",
            "        a: number;\n",
            "    } & {\n",
            "        match: Any;\n",
            "        check?: boolean;\n",
            "    })[];\n",
            "};\n",
            "export declare const one: {\n",
            "    a: number;\n",
            "} & {\n",
            "    match: Any;\n",
            "    check?: boolean;\n",
            "};\n",
            "export declare const two: {\n",
            "    a: number;\n",
            "} & {\n",
            "    match: Any;\n",
            "    check?: boolean;\n",
            "};\n",
            "export declare class Res<K, A> {\n",
            "    readonly k?: K | undefined;\n",
            "    readonly a?: A | undefined;\n",
            "    constructor(k?: K | undefined, a?: A | undefined);\n",
            "    static make: <K_1, A_1>(lookup: (key: K_1) => A_1) => Res<K_1, A_1>;\n",
            "}\n",
            "//# sourceMappingURL=user.d.ts.map",
        )
    );
    assert_eq!(
        read("out/src/user.d.ts.map"),
        r#"{"version":3,"file":"user.d.ts","sourceRoot":"","sources":["../../src/user.ts"],"names":[],"mappings":"AAEA,OAAO,KAAK,GAAG,MAAM,OAAO,CAAC;AAC7B,OAAO,EAAE,GAAG,EAAE,MAAM,OAAO,CAAC;AAE5B,eAAO,MAAM,OAAO;;;mBAAe,CAAC;AACpC,eAAO,MAAM,CAAC,yCAAgB,CAAC;AAC/B,eAAO,MAAM,IAAI;;;;;CAAe,CAAC;AACjC,eAAO,MAAM,WAAW,GAAI,GAAG,SAAS,GAAG,CAAC,GAAG,WAAW;IAAE,QAAQ,CAAC,GAAG,EAAE,GAAG,CAAA;CAAE;kBAAL,GAAG;CAAc,CAAC;AAC5F,eAAO,MAAM,KAAK,MAAO,GAAG,CAAC,GAAG,QAAM,CAAC;AACvC,eAAO,MAAM,EAAE,GAAI,CAAC,SAAS,MAAM,SAAS,CAAC,MAC5C,CAAC,EAAE,EAAE,SAAS,CAAC,CAAC,EAAE,OAAO,CAAC,CAAC,EAAE,MAAM,CAAC,CAAC,EAAE,MAAM,CAAC,CAAC,KAAK,CAAC,KAAK,EAAE,OAAM,CAAC;AACpE,eAAO,MAAM,EAAE,GADd,CAAC,EAAE,EAAE,SAAS,CAAC,CAAC,EAAE,OAAO,IAAI,MAAM,SAAI,MAAM,CAAC,CAAC,MAAM,cAC1B,CAAC;AAC7B,eAAO,MAAM,KAAK,GAAI,CAAC,QAAQ,CAAC,KAAG,CAAC,GAAG;IAAE,KAAK,EAAE,GAAG,CAAC;IAAC,KAAK,CAAC,EAAE,OAAO,CAAA;CAAiB,CAAC;AACtF,wBAAgB,KAAK;IACV,OAAO;;;eAF8B,GAAG;gBAAU,OAAO;;IAE7B,KAAK;;;;;;EAC3C;AACD,eAAO,MAAM,GAAG;;;WAJgC,GAAG;YAAU,OAAO;CAIlC,CAAC;AACnC,eAAO,MAAM,GAAG;;;WALgC,GAAG;YAAU,OAAO;CAKlC,CAAC;AACnC,qBAAa,GAAG,CAAC,CAAC,EAAE,CAAC;IACP,QAAQ,CAAC,CAAC,CAAC,EAAE,CAAC;IAAE,QAAQ,CAAC,CAAC,CAAC,EAAE,CAAC;IAA1C,YAAqB,CAAC,CAAC,EAAE,CAAC,YAAA,EAAW,CAAC,CAAC,EAAE,CAAC,YAAA,EAAI;IAC9C,MAAM,CAAC,IAAI,qBAAgC,CAAC,GAAG,EAAE,GAAC,KAAK,GAAC,mBAA+B;CACxF"}"#
    );
}

#[test]
fn async_arrow_concise_body_block_maps_like_tsgo() {
    // The async transform turns a concise arrow body into a block whose return
    // statement, statement list and block take the body's range
    // (transformers/estransforms/async.go:876-893), so the generator's `}` maps
    // to the token after the body. The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare function poll(f: () => Promise<boolean>, n: number): void;\n",
            "declare const page: { evaluate(f: () => boolean): Promise<boolean> };\n",
            "poll(async () =>\n",
            "  page.evaluate(() => true)\n",
            ", 1);\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"target":"es2016","module":"esnext","noEmitHelpers":true,"sourceMap":true,"outDir":"out"},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    let read = |name: &str| fs::read_to_string(tree.path(name)).expect("read output");
    assert_eq!(
        read("out/a.js"),
        concat!(
            "\"use strict\";\n",
            "poll(() => __awaiter(void 0, void 0, void 0, function* () { return page.evaluate(() => true); }), 1);\n",
            "//# sourceMappingURL=a.js.map",
        )
    );
    assert_eq!(
        read("out/a.js.map"),
        r#"{"version":3,"file":"a.js","sourceRoot":"","sources":["../a.ts"],"names":[],"mappings":";AAEA,IAAI,CAAC,GAAS,EAAE,kDACd,OAAA,IAAI,CAAC,QAAQ,CAAC,GAAG,EAAE,CAAC,IAAI,CAAC,CAAA,CAC3B,CAAC,CAD0B,EACzB,CAAC,CAAC,CAAC"}"#
    );
}

#[test]
fn declaration_type_parameter_names_end_with_their_scope_like_tsgo() {
    // enterNewScope gives the names a scope took back when it ends: every
    // local a reused fake scope gained is removed and every replaced one is
    // put back (checker/nodebuilderscopes.go:142-152), and a mapped type names
    // its parameter inside its scope (checker/nodebuilderimpl.go:1582-1593).
    // tsc 6.0 undid only the first local of each kind and named a mapped
    // type's parameter before the scope, so a sibling conditional type's
    // `infer _E` became `_E_1`, the generic return type after a parameter
    // holding `<O, E, R>(…)` became `<E_1, R_1>`, the second member of a
    // returned type literal became `<R, O_1, E_1, …>` and a sibling mapped
    // type's `K` became `K_1`. A parameter that does shadow one in scope is
    // still renamed. The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "export interface Box<A, E> { a: A; e: E }\n",
            "export interface Fail<A, E> { b: A; f: E }\n",
            "export interface Kind<F, R, O, E, A> { f: F; r: R; o: O; e: E; a: A }\n",
            "export const builder = <T extends Box<any, any>>(self: T): [\n",
            "  T extends Box<infer _A, infer _E> ? _A : never,\n",
            "  T extends Fail<infer _A, infer _E> ? _E : never,\n",
            "  T extends Box<infer _A, infer _E> ? true : never\n",
            "] => null as any\n",
            "declare function dual<A, B>(n: number, f: unknown): A & B\n",
            "export const repeat = dual<{\n",
            "  <Output, Input>(builder: ($: <O, E, R>(_: [O, E, R, Input]) => [O, E, R]) => Output): <E, R>(self: [Input, E, R]) => [Output, E, R]\n",
            "}, {\n",
            "  <Input, E, R, Output>(self: [Input, E, R], builder: ($: <O, E, R>(_: [O, E, R, Input]) => [O, E, R]) => Output): [Output, E, R]\n",
            "}>(2, null)\n",
            "export const let_ = <F>(map: F): {\n",
            "  <N extends string, A extends object, B>(name: N, f: (a: A) => B): <R, O, E>(self: Kind<F, R, O, E, A>) => Kind<F, R, O, E, B>\n",
            "  <R, O, E, A extends object, N extends string, B>(self: Kind<F, R, O, E, A>, name: N, f: (a: A) => B): Kind<F, R, O, E, B>\n",
            "} => null as any\n",
            "declare function pair<T, U>(x: T, y: U): { a: { [K in keyof T]: T[K] }; b: { [K in keyof U]: [U[K]] } }\n",
            "export const g = <T, U>(x: T, y: U) => pair(x, y)\n",
            "export const m = <T>(x: T): { a: { [K in keyof T]: T[K] }; b: { [K in keyof T]: { [K in keyof T]: T[K] } } } => null as any\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"target":"es2022","module":"esnext","strict":true,"declaration":true,"declarationMap":true,"emitDeclarationOnly":true,"outDir":"out"},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    let read = |name: &str| fs::read_to_string(tree.path(name)).expect("read output");
    assert_eq!(
        read("out/a.d.ts"),
        concat!(
            "export interface Box<A, E> {\n",
            "    a: A;\n",
            "    e: E;\n",
            "}\n",
            "export interface Fail<A, E> {\n",
            "    b: A;\n",
            "    f: E;\n",
            "}\n",
            "export interface Kind<F, R, O, E, A> {\n",
            "    f: F;\n",
            "    r: R;\n",
            "    o: O;\n",
            "    e: E;\n",
            "    a: A;\n",
            "}\n",
            "export declare const builder: <T extends Box<any, any>>(self: T) => [T extends Box<infer _A, infer _E> ? _A : never, T extends Fail<infer _A, infer _E> ? _E : never, T extends Box<infer _A, infer _E> ? true : never];\n",
            "export declare const repeat: (<Output, Input>(builder: ($: <O, E, R>(_: [O, E, R, Input]) => [O, E, R]) => Output) => <E, R>(self: [Input, E, R]) => [Output, E, R]) & (<Input, E, R, Output>(self: [Input, E, R], builder: ($: <O, E_1, R_1>(_: [O, E_1, R_1, Input]) => [O, E_1, R_1]) => Output) => [Output, E, R]);\n",
            "export declare const let_: <F>(map: F) => {\n",
            "    <N extends string, A extends object, B>(name: N, f: (a: A) => B): <R, O, E>(self: Kind<F, R, O, E, A>) => Kind<F, R, O, E, B>;\n",
            "    <R, O, E, A extends object, N extends string, B>(self: Kind<F, R, O, E, A>, name: N, f: (a: A) => B): Kind<F, R, O, E, B>;\n",
            "};\n",
            "export declare const g: <T, U>(x: T, y: U) => {\n",
            "    a: { [K in keyof T]: T[K]; };\n",
            "    b: { [K in keyof U]: [U[K]]; };\n",
            "};\n",
            "export declare const m: <T>(x: T) => {\n",
            "    a: { [K in keyof T]: T[K]; };\n",
            "    b: { [K in keyof T]: { [K_1 in keyof T]: T[K_1]; }; };\n",
            "};\n",
            "//# sourceMappingURL=a.d.ts.map",
        )
    );
    assert_eq!(
        read("out/a.d.ts.map"),
        r#"{"version":3,"file":"a.d.ts","sourceRoot":"","sources":["../a.ts"],"names":[],"mappings":"AAAA,MAAM,WAAW,GAAG,CAAC,CAAC,EAAE,CAAC;IAAI,CAAC,EAAE,CAAC,CAAC;IAAC,CAAC,EAAE,CAAC,CAAA;CAAE;AACzC,MAAM,WAAW,IAAI,CAAC,CAAC,EAAE,CAAC;IAAI,CAAC,EAAE,CAAC,CAAC;IAAC,CAAC,EAAE,CAAC,CAAA;CAAE;AAC1C,MAAM,WAAW,IAAI,CAAC,CAAC,EAAE,CAAC,EAAE,CAAC,EAAE,CAAC,EAAE,CAAC;IAAI,CAAC,EAAE,CAAC,CAAC;IAAC,CAAC,EAAE,CAAC,CAAC;IAAC,CAAC,EAAE,CAAC,CAAC;IAAC,CAAC,EAAE,CAAC,CAAC;IAAC,CAAC,EAAE,CAAC,CAAA;CAAE;AACrE,eAAO,MAAM,OAAO,GAAI,CAAC,SAAS,GAAG,CAAC,GAAG,EAAE,GAAG,CAAC,QAAQ,CAAC,KAAG,CACzD,CAAC,SAAS,GAAG,CAAC,MAAM,EAAE,EAAE,MAAM,EAAE,CAAC,GAAG,EAAE,GAAG,KAAK,EAC9C,CAAC,SAAS,IAAI,CAAC,MAAM,EAAE,EAAE,MAAM,EAAE,CAAC,GAAG,EAAE,GAAG,KAAK,EAC/C,CAAC,SAAS,GAAG,CAAC,MAAM,EAAE,EAAE,MAAM,EAAE,CAAC,GAAG,IAAI,GAAG,KAAK,CAClC,CAAA;AAEhB,eAAO,MAAM,MAAM,IAChB,MAAM,EAAE,KAAK,WAAW,CAAC,CAAC,EAAE,CAAC,CAAC,EAAE,CAAC,EAAE,CAAC,EAAE,CAAC,EAAE,CAAC,CAAC,EAAE,CAAC,EAAE,CAAC,EAAE,KAAK,CAAC,KAAK,CAAC,CAAC,EAAE,CAAC,EAAE,CAAC,CAAC,KAAK,MAAM,KAAG,CAAC,CAAC,EAAE,CAAC,EAAE,IAAI,EAAE,CAAC,KAAK,EAAE,CAAC,EAAE,CAAC,CAAC,KAAK,CAAC,MAAM,EAAE,CAAC,EAAE,CAAC,CAAC,MAElI,KAAK,EAAE,CAAC,EAAE,CAAC,EAAE,MAAM,QAAQ,CAAC,KAAK,EAAE,CAAC,EAAE,CAAC,CAAC,WAAW,CAAC,CAAC,EAAE,CAAC,CAAC,EAAE,GAAC,EAAE,GAAC,EAAE,CAAC,EAAE,CAAC,CAAC,EAAE,GAAC,EAAE,GAAC,EAAE,KAAK,CAAC,KAAK,CAAC,CAAC,EAAE,GAAC,EAAE,GAAC,CAAC,KAAK,MAAM,KAAG,CAAC,MAAM,EAAE,CAAC,EAAE,CAAC,CAAC,CACtH,CAAA;AACX,eAAO,MAAM,IAAI,GAAI,CAAC,OAAO,CAAC,KAAG;IAC/B,CAAC,CAAC,SAAS,MAAM,EAAE,CAAC,SAAS,MAAM,EAAE,CAAC,EAAE,IAAI,EAAE,CAAC,EAAE,CAAC,EAAE,CAAC,CAAC,EAAE,CAAC,KAAK,CAAC,GAAG,CAAC,CAAC,EAAE,CAAC,EAAE,CAAC,EAAE,IAAI,EAAE,IAAI,CAAC,CAAC,EAAE,CAAC,EAAE,CAAC,EAAE,CAAC,EAAE,CAAC,CAAC,KAAK,IAAI,CAAC,CAAC,EAAE,CAAC,EAAE,CAAC,EAAE,CAAC,EAAE,CAAC,CAAC,CAAA;IAC7H,CAAC,CAAC,EAAE,CAAC,EAAE,CAAC,EAAE,CAAC,SAAS,MAAM,EAAE,CAAC,SAAS,MAAM,EAAE,CAAC,EAAE,IAAI,EAAE,IAAI,CAAC,CAAC,EAAE,CAAC,EAAE,CAAC,EAAE,CAAC,EAAE,CAAC,CAAC,EAAE,IAAI,EAAE,CAAC,EAAE,CAAC,EAAE,CAAC,CAAC,EAAE,CAAC,KAAK,CAAC,GAAG,IAAI,CAAC,CAAC,EAAE,CAAC,EAAE,CAAC,EAAE,CAAC,EAAE,CAAC,CAAC,CAAA;CAC3G,CAAA;AAEhB,eAAO,MAAM,CAAC,GAAI,CAAC,EAAE,CAAC,KAAK,CAAC,KAAK,CAAC;UADe,CAAC;UAA4B,CAAC;CAC9B,CAAA;AACjD,eAAO,MAAM,CAAC,GAAI,CAAC,KAAK,CAAC,KAAG;IAAE,CAAC,EAAE,GAAG,CAAC,IAAI,MAAM,CAAC,GAAG,CAAC,CAAC,CAAC,CAAC,GAAE,CAAC;IAAC,CAAC,EAAE,GAAG,CAAC,IAAI,MAAM,CAAC,GAAG,GAAG,GAAC,IAAI,MAAM,CAAC,GAAG,CAAC,CAAC,GAAC,CAAC,GAAE,GAAE,CAAA;CAAiB,CAAA"}"#
    );
}

#[test]
fn declaration_alias_chains_try_the_earlier_declared_parent_like_tsgo() {
    // getSymbolChain orders the parents of a chain's root with sortByBestName
    // (checker/nodebuilderimpl.go:1153-1170): two module parents by their
    // specifiers, any other pair by compareSymbols. A module that exports the
    // target and is declared before the alias's container is therefore tried
    // first: where the scope shadows the alias, the target is written through
    // the module (`typeof import("./f1")`, `typeof import("./f2").g`) rather
    // than through the container (`typeof M.d`, `typeof M.e`, as tsc 6.0
    // did). An alias no imported module exports keeps its container
    // (`typeof M.k`), and an alias that is not shadowed is used by its name.
    // The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("f1.ts"),
        concat!("namespace f { export class c { } }\n", "export = f;\n",),
    )
    .expect("write f1.ts");
    fs::write(
        tree.path("f2.ts"),
        "export namespace g { export class c { } }\n",
    )
    .expect("write f2.ts");
    fs::write(
        tree.path("f0.ts"),
        concat!(
            "import im = require('./f1');\n",
            "import * as two from './f2';\n",
            "namespace Loc { export namespace Deep { export class K { } } }\n",
            "export namespace M {\n",
            "    export import d = im;\n",
            "    export import e = two.g;\n",
            "    export import k = Loc.Deep;\n",
            "}\n",
            "export namespace M.P {\n",
            "    export var viaD = M.d;\n",
            "    export var viaE = M.e;\n",
            "    export var viaK = M.k;\n",
            "}\n",
            "export namespace M.R {\n",
            "    export var d = 1;\n",
            "    export var e = 2;\n",
            "    export var k = 3;\n",
            "    export var two = 4;\n",
            "    export var Loc = 5;\n",
            "    export var viaD = M.d;\n",
            "    export var viaE = M.e;\n",
            "    export var viaK = M.k;\n",
            "}\n",
        ),
    )
    .expect("write f0.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"target":"es2015","module":"commonjs","declaration":true,"emitDeclarationOnly":true,"outDir":"out"},"files":["f1.ts","f2.ts","f0.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/f0.d.ts")).expect("read out/f0.d.ts"),
        concat!(
            "import im = require('./f1');\n",
            "import * as two from './f2';\n",
            "declare namespace Loc {\n",
            "    namespace Deep {\n",
            "        class K {\n",
            "        }\n",
            "    }\n",
            "}\n",
            "export declare namespace M {\n",
            "    export import d = im;\n",
            "    export import e = two.g;\n",
            "    export import k = Loc.Deep;\n",
            "}\n",
            "export declare namespace M.P {\n",
            "    var viaD: typeof d;\n",
            "    var viaE: typeof e;\n",
            "    var viaK: typeof k;\n",
            "}\n",
            "export declare namespace M.R {\n",
            "    var d: number;\n",
            "    var e: number;\n",
            "    var k: number;\n",
            "    var two: number;\n",
            "    var Loc: number;\n",
            "    var viaD: typeof import(\"./f1\");\n",
            "    var viaE: typeof import(\"./f2\").g;\n",
            "    var viaK: typeof M.k;\n",
            "}\n",
            "export {};\n",
        )
    );
}

#[test]
fn declaration_cached_type_copy_drops_the_elided_comment_like_tsgo() {
    // A hit of the serialized type cache hands out DeepCloneNode's copy, and
    // the clone hook copies an emit node without its synthetic comments
    // (emitNode.copyFrom, printer/emitcontext.go:562-574): the second use of
    // the cached literal writes its elided placeholders as plain `any`. tsc
    // 6.0 merged the comments into the clone. The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "export var circularReference = class C {\n",
            "    static getTags(c: C): C { return c }\n",
            "    tags(c: C): C { return c }\n",
            "}\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"target":"es2015","module":"commonjs","declaration":true,"emitDeclarationOnly":true,"outDir":"out"},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.d.ts")).expect("read out/a.d.ts"),
        concat!(
            "export declare var circularReference: {\n",
            "    new (): {\n",
            "        tags(c: /*elided*/ any): /*elided*/ any;\n",
            "    };\n",
            "    getTags(c: {\n",
            "        tags(c: /*elided*/ any): /*elided*/ any;\n",
            "    }): {\n",
            "        tags(c: any): any;\n",
            "    };\n",
            "};\n",
        )
    );
}

#[test]
fn expando_member_types_name_exported_functions_like_tsgo() {
    // The types of an expando function's members are written inside its
    // namespace, where the file's own declarations are in scope: an exported
    // function is `typeof Vec2`, like a local function or a class. tsc 6.0
    // qualified an exported function with its module there
    // (`typeof import("./a").Vec2`). The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "export function Vec2(len: number) {}\n",
            "function local(len: number) {}\n",
            "export class K {}\n",
            "export function P2(x: number) {}\n",
            "P2.direct = Vec2;\n",
            "P2.obj = { p: Vec2, q: local, k: K };\n",
            "P2.arrow = (v: typeof Vec2) => v;\n",
            "export const Q = (x: number) => x;\n",
            "Q.obj = { p: Vec2 };\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"target":"es2015","module":"commonjs","strict":true,"declaration":true,"emitDeclarationOnly":true,"outDir":"out"},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.d.ts")).expect("read out/a.d.ts"),
        concat!(
            "export declare function Vec2(len: number): void;\n",
            "declare function local(len: number): void;\n",
            "export declare class K {\n",
            "}\n",
            "export declare function P2(x: number): void;\n",
            "export declare namespace P2 {\n",
            "    export { Vec2 as direct };\n",
            "    export var obj: {\n",
            "        p: typeof Vec2;\n",
            "        q: typeof local;\n",
            "        k: typeof K;\n",
            "    };\n",
            "    export var arrow: (v: typeof Vec2) => typeof Vec2;\n",
            "}\n",
            "export declare function Q(x: number): number;\n",
            "export declare namespace Q {\n",
            "    var obj: {\n",
            "        p: typeof Vec2;\n",
            "    };\n",
            "}\n",
            "export {};\n",
        )
    );
}

#[test]
fn jsx_children_start_on_their_own_marks_like_tsgo() {
    // With more than one child, each JSX child is marked to start on a new
    // line (transformers/jsxtransforms/jsx.go:709-713). The CommonJS
    // transform replaces an imported name with a new access that takes only
    // the name's ranges, so `{tree}` is written on the call's line, and an
    // exported local with `exports.` and a clone of the name, which keeps
    // the mark: `exports.⏎local`
    // (transformers/moduletransforms/commonjsmodule.go:2064-2124). An element
    // child and a local name keep their own lines. tsc substituted the names
    // while printing, after the list had read the marks. The expected bytes
    // are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("renderer.ts"),
        "export function dom(...args: any[]): any { return args; }\n",
    )
    .expect("write renderer.ts");
    fs::write(tree.path("component.ts"), "export const tree: any = 1;\n")
        .expect("write component.ts");
    fs::write(
        tree.path("a.tsx"),
        concat!(
            "import { dom } from \"./renderer\";\n",
            "import { tree } from \"./component\";\n",
            "declare const Box: any;\n",
            "export let local: any = tree;\n",
            "const both = <Box x={1}>{tree}{tree}</Box>;\n",
            "const first = <Box x={1}>{tree}<Box /></Box>;\n",
            "const last = <Box x={1}><Box />{tree}</Box>;\n",
            "const exported = <Box x={1}>{local}{local}</Box>;\n",
            "const plain = <Box x={1}>{Box}{Box}</Box>;\n",
        ),
    )
    .expect("write a.tsx");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"target":"es2015","module":"commonjs","jsx":"react","jsxFactory":"dom","sourceMap":true,"outDir":"out"},"files":["a.tsx"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    let read = |name: &str| fs::read_to_string(tree.path(name)).expect("read output");
    assert_eq!(
        read("out/a.js"),
        concat!(
            "\"use strict\";\n",
            "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
            "exports.local = void 0;\n",
            "const renderer_1 = require(\"./renderer\");\n",
            "const component_1 = require(\"./component\");\n",
            "exports.local = component_1.tree;\n",
            "const both = (0, renderer_1.dom)(Box, { x: 1 }, component_1.tree, component_1.tree);\n",
            "const first = (0, renderer_1.dom)(Box, { x: 1 }, component_1.tree,\n",
            "    (0, renderer_1.dom)(Box, null));\n",
            "const last = (0, renderer_1.dom)(Box, { x: 1 },\n",
            "    (0, renderer_1.dom)(Box, null), component_1.tree);\n",
            "const exported = (0, renderer_1.dom)(Box, { x: 1 }, exports.\n",
            "    local, exports.\n",
            "    local);\n",
            "const plain = (0, renderer_1.dom)(Box, { x: 1 },\n",
            "    Box,\n",
            "    Box);\n",
            "//# sourceMappingURL=a.js.map",
        )
    );
    assert_eq!(
        read("out/a.js.map"),
        r#"{"version":3,"file":"a.js","sourceRoot":"","sources":["../a.tsx"],"names":[],"mappings":";;;AAAA,yCAAiC;AACjC,2CAAmC;AAExB,QAAA,KAAK,GAAQ,gBAAI,CAAC;AAC7B,MAAM,IAAI,GAAG,oBAAC,GAAG,IAAC,CAAC,EAAE,CAAC,IAAG,gBAAI,EAAE,gBAAI,CAAO,CAAC;AAC3C,MAAM,KAAK,GAAG,oBAAC,GAAG,IAAC,CAAC,EAAE,CAAC,IAAG,gBAAI;IAAC,oBAAC,GAAG,OAAG,CAAM,CAAC;AAC7C,MAAM,IAAI,GAAG,oBAAC,GAAG,IAAC,CAAC,EAAE,CAAC;IAAE,oBAAC,GAAG,OAAG,EAAC,gBAAI,CAAO,CAAC;AAC5C,MAAM,QAAQ,GAAG,oBAAC,GAAG,IAAC,CAAC,EAAE,CAAC,IAAG;IAAA,KAAK,EAAE;IAAA,KAAK,CAAO,CAAC;AACjD,MAAM,KAAK,GAAG,oBAAC,GAAG,IAAC,CAAC,EAAE,CAAC;IAAG,GAAG;IAAE,GAAG,CAAO,CAAC"}"#
    );
}

#[test]
fn object_type_members_follow_their_base_types_like_tsgo() {
    // resolveObjectTypeMembers publishes a type's members only after its base
    // types are resolved (checker/checker.go:19446-19493), so a base type
    // argument that needs the members of the type being resolved instantiates
    // it again until the instantiation depth ends it with TS5115; tsc 6.0
    // published the own members first and reported nothing. The type
    // arguments of a reference are checked against their constraints in place
    // (checker/checker.go:3046-3062), so the error is at the interface whose
    // heritage is being checked, not at the reference tsc 6.0 revisited as a
    // lazy diagnostic. The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "type Diff<T, U> = T extends U ? never : T;\n",
            "type Node = Tag | Selector | Root | Pseudo;\n",
            "interface Base<Value extends string | undefined = string> {\n",
            "    type: string;\n",
            "    value: Value;\n",
            "}\n",
            "interface Container<Value extends string | undefined = string, Child extends Node = Node> extends Base<Value> {\n",
            "    nodes: Array<Child>;\n",
            "}\n",
            "interface Root extends Container<undefined, Selector> { type: \"root\" }\n",
            "interface _Selector<S> extends Container<string, Diff<Node, S>> { type: \"selector\" }\n",
            "type Selector = _Selector<Selector>;\n",
            "interface Pseudo extends Container<string, Selector> { type: \"pseudo\" }\n",
            "interface Tag extends Base { type: \"tag\" }\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","strict":true,"noEmit":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        "a.ts(10,11): error TS5115: Instantiations of the following types appear infinitely circular: 'Container', 'Diff'.\n"
    );
}

#[test]
fn intersections_of_mappings_of_one_object_stay_unreduced_like_tsgo() {
    // getReducedType does not look for a property that reduces an
    // intersection to never when every constituent is a mapped type over the
    // same object type (isMappingOfSameObjectType, checker/checker.go:
    // 22188-22214), so `M1<O> & M2<O>` keeps its `kind: "a" & "b"` while
    // `M1<O> & M2<P>` is never. tsc 6.0 reduced both. The expected bytes are
    // tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "type O = { kind: \"a\"; x: number };\n",
            "type P = { kind: \"a\"; x: number };\n",
            "type M1<T> = { [K in keyof T]: T[K] };\n",
            "type M2<T> = { [K in keyof T]: K extends \"kind\" ? \"b\" : T[K] };\n",
            "declare let same: M1<O> & M2<O>;\n",
            "declare let other: M1<O> & M2<P>;\n",
            "declare let plain: O & { kind: \"b\" };\n",
            "const n1: never = same;\n",
            "const n2: never = other;\n",
            "const n3: never = plain;\n",
            "export const k1 = same.kind;\n",
            "export const k2 = other.kind;\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","strict":true,"noEmit":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "a.ts(8,7): error TS2322: Type 'M1<O> & M2<O>' is not assignable to type 'never'.\n",
            "a.ts(12,25): error TS2339: Property 'kind' does not exist on type 'never'.\n",
            "  The intersection 'M1<O> & M2<P>' was reduced to 'never' because property 'kind' has conflicting types in some constituents.\n",
        )
    );
}

#[test]
fn nested_comparisons_end_in_maybe_at_a_hundred_levels_like_tsgo() {
    // recursiveTypeRelatedTo answers Maybe once a hundred comparisons are
    // nested (checker/relater.go:3135-3140); tsc 6.0 failed the whole
    // comparison there with TS2321, which tsgo never reports. The constraint
    // check of `TUnionResult`'s type argument (typebox's `UnionToTuple`)
    // nests that deep and passes. The expected result is tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "export type UnionToIntersect<U> = (U extends unknown ? (arg: U) => 0 : never) extends (arg: infer I) => 0 ? I : never;\n",
            "export type UnionLast<U> = UnionToIntersect<U extends unknown ? (x: U) => 0 : never> extends (x: infer L) => 0 ? L : never;\n",
            "export type UnionToTuple<U, L = UnionLast<U>> = [U] extends [never] ? [] : [...UnionToTuple<Exclude<U, L>>, L];\n",
            "export type Assert<T, E> = T extends E ? T : never;\n",
            "interface TSchema { x: string }\n",
            "interface TLiteral<T> extends TSchema { const: T }\n",
            "export type TUnionResult<T extends TSchema[]> = T extends [] ? never : T extends [infer S] ? S : T;\n",
            "export type R<T extends string> = TUnionResult<Assert<UnionToTuple<{ [K in T]: TLiteral<K>; }[T]>, TSchema[]>>;\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","strict":true,"noEmit":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
}

#[test]
fn circular_variances_are_measured_from_the_smallest_symbol_like_tsgo() {
    // getVariancesWorker keeps the generic types being measured on a stack and
    // restarts from the one with the smallest symbol when a measurement asks
    // for a type already on it (checker/relater.go:1334-1434). tsc 6.0 measured
    // the rest of a cycle from inside whichever type was compared first: here
    // the subtype reduction of `resolveDynamicModel`'s return type entered at
    // `ObjectDirective`, `DirectiveBinding` was measured without its `dir`
    // property, and the call was accepted. The expected bytes are tsgo's
    // (the reduction is from Vue's `vModel.ts`).
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "type Data = Record<string, unknown>\n",
            "interface VNode<A = any, B = any> { props: Record<string, any> | null; type: string | object; a?: A; b?: B }\n",
            "type DirectiveModifiers<K extends string = string> = Partial<Record<K, boolean>>\n",
            "\n",
            "export interface DirectiveBinding<Value = any, Modifiers extends string = string, Arg = any> {\n",
            "  instance: Record<string, any> | null\n",
            "  value: Value\n",
            "  oldValue: Value | null\n",
            "  arg?: Arg\n",
            "  modifiers: DirectiveModifiers<Modifiers>\n",
            "  dir: ObjectDirective<any, Value, Modifiers, Arg>\n",
            "}\n",
            "\n",
            "export type DirectiveHook<HostElement = any, Prev = VNode<any, HostElement> | null, Value = any, Modifiers extends string = string, Arg = any> = (\n",
            "  el: HostElement,\n",
            "  binding: DirectiveBinding<Value, Modifiers, Arg>,\n",
            "  vnode: VNode<any, HostElement>,\n",
            "  prevVNode: Prev,\n",
            ") => void\n",
            "\n",
            "export type SSRDirectiveHook<Value = any, Modifiers extends string = string, Arg = any> = (\n",
            "  binding: DirectiveBinding<Value, Modifiers, Arg>,\n",
            "  vnode: VNode,\n",
            ") => Data | undefined\n",
            "\n",
            "export interface ObjectDirective<HostElement = any, Value = any, Modifiers extends string = string, Arg = any> {\n",
            "  __mod?: Modifiers\n",
            "  created?: DirectiveHook<HostElement, null, Value, Modifiers, Arg>\n",
            "  beforeUpdate?: DirectiveHook<HostElement, VNode<any, HostElement>, Value, Modifiers, Arg>\n",
            "  getSSRProps?: SSRDirectiveHook<Value, Modifiers, Arg>\n",
            "  deep?: boolean\n",
            "}\n",
            "\n",
            "type ModelDirective<T, Modifiers extends string = string> = ObjectDirective<T & { k: string }, any, Modifiers>\n",
            "\n",
            "interface In { i: string }\n",
            "interface Sel { s: string }\n",
            "interface Ta { t: string }\n",
            "\n",
            "export const vModelText: ModelDirective<In | Ta, 'trim' | 'number' | 'lazy'> = {}\n",
            "export const vModelCheckbox: ModelDirective<In> = {}\n",
            "export const vModelRadio: ModelDirective<In> = {}\n",
            "export const vModelSelect: ModelDirective<Sel, 'number'> = {}\n",
            "\n",
            "export const vModelDynamic: ObjectDirective<In | Sel | Ta> = {}\n",
            "\n",
            "function resolveDynamicModel(tagName: string, type: string | undefined) {\n",
            "  switch (tagName) {\n",
            "    case 'SELECT':\n",
            "      return vModelSelect\n",
            "    case 'TEXTAREA':\n",
            "      return vModelText\n",
            "    default:\n",
            "      switch (type) {\n",
            "        case 'checkbox':\n",
            "          return vModelCheckbox\n",
            "        case 'radio':\n",
            "          return vModelRadio\n",
            "        default:\n",
            "          return vModelText\n",
            "      }\n",
            "  }\n",
            "}\n",
            "\n",
            "export function initVModelForSSR(): void {\n",
            "  vModelDynamic.getSSRProps = (binding, vnode) => {\n",
            "    if (typeof vnode.type !== 'string') {\n",
            "      return\n",
            "    }\n",
            "    const modelToUse = resolveDynamicModel(vnode.type.toUpperCase(), vnode.props && vnode.props.type)\n",
            "    if (modelToUse.getSSRProps) {\n",
            "      return modelToUse.getSSRProps(binding, vnode)\n",
            "    }\n",
            "  }\n",
            "}\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","strict":true,"noEmit":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "a.ts(72,37): error TS2345: Argument of type 'DirectiveBinding<any, string, any>' is not assignable to parameter of type 'DirectiveBinding<any, \"number\", any>'.\n",
            "  Types of property 'dir' are incompatible.\n",
            "    Type 'ObjectDirective<any, any, string, any>' is not assignable to type 'ObjectDirective<any, any, \"number\", any>'.\n",
            "      Type 'string' is not assignable to type '\"number\"'.\n",
        )
    );
}

#[test]
fn alias_type_arguments_are_instantiated_like_tsgo() {
    // instantiateType also instantiates a type whose alias type arguments could
    // contain type variables (checker/checker.go:22494-22500), so a union or
    // intersection alias that never refers to its parameter still gets the
    // arguments of each reference: `Un<number>`, and `number` for `Brand<U>`
    // (the intersection is rebuilt without its `& {}`). tsc 6.0 returned the
    // declared type, `Un<T>` and `Brand<T>`. The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "type Brand<T> = number & {};\n",
            "declare function f<U>(x: U): Brand<U>;\n",
            "export const r = f(\"a\");\n",
            "export function g<V>(x: V) { return f(x); }\n",
            "const s: string = r;\n",
            "type Box<T> = { v: number };\n",
            "declare function h<U>(x: U): Box<U>;\n",
            "export const b = h(1);\n",
            "const t: string = b;\n",
            "type Un<T> = string | number;\n",
            "declare function u<U>(x: U): Un<U>;\n",
            "export const c = u(1);\n",
            "const w: boolean = c;\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","strict":true,"declaration":true,"outDir":"out"},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "a.ts(5,7): error TS2322: Type 'number' is not assignable to type 'string'.\n",
            "a.ts(9,7): error TS2322: Type 'Box<number>' is not assignable to type 'string'.\n",
            "a.ts(13,7): error TS2322: Type 'Un<number>' is not assignable to type 'boolean'.\n",
            "  Type 'string' is not assignable to type 'boolean'.\n",
        )
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/a.d.ts")).expect("read out/a.d.ts"),
        concat!(
            "export declare const r: number;\n",
            "export declare function g<V>(x: V): number;\n",
            "type Box<T> = {\n",
            "    v: number;\n",
            "};\n",
            "export declare const b: Box<number>;\n",
            "type Un<T> = string | number;\n",
            "export declare const c: Un<number>;\n",
            "export {};\n",
        )
    );
}

#[test]
fn intersection_properties_of_namespace_exports_are_optional_like_tsgo() {
    // createUnionOrIntersectionProperty starts an intersection's property as
    // optional and narrows that only by the properties, methods and accessors
    // among the constituents' properties (checker/checker.go:21796-21818), so
    // a name two namespaces export stays optional in their intersection;
    // interface members do not. tsc 6.0 left the flag unset. The expected
    // bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "namespace A {\n",
            "    export const x = 1;\n",
            "    export const p = 1;\n",
            "}\n",
            "namespace B {\n",
            "    export const x = 1;\n",
            "    export const q = 1;\n",
            "}\n",
            "declare const both: typeof A & typeof B;\n",
            "const all: { x: number; p: number; q: number } = both;\n",
            "interface C { x: number; p: number }\n",
            "interface D { x: number; q: number }\n",
            "declare const members: C & D;\n",
            "const same: { x: number; p: number; q: number } = members;\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","strict":true,"noEmit":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "a.ts(10,7): error TS2322: Type 'typeof A & typeof B' is not assignable to type '{ x: number; p: number; q: number; }'.\n",
            "  Property 'x' is optional in type 'typeof A & typeof B' but required in type '{ x: number; p: number; q: number; }'.\n",
        )
    );
}

#[test]
fn a_comparison_a_hundred_levels_deep_reports_its_own_failure_like_tsgo() {
    // The same `UnionToTuple` as a function's return value: the comparison
    // nests until recursiveTypeRelatedTo answers Maybe at a hundred levels
    // (checker/relater.go:3135-3140) and then fails on the union member that
    // is not assignable, with the chain of every level. tsc 6.0 printed
    // TS2321 for the outermost pair instead. tsgo prints 199 lines; the
    // first and the last are checked.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "export type UnionToIntersect<U> = (U extends unknown ? (arg: U) => 0 : never) extends (arg: infer I) => 0 ? I : never;\n",
            "export type UnionLast<U> = UnionToIntersect<U extends unknown ? (x: U) => 0 : never> extends (x: infer L) => 0 ? L : never;\n",
            "export type UnionToTuple<U, L = UnionLast<U>> = [U] extends [never] ? [] : [...UnionToTuple<Exclude<U, L>>, L];\n",
            "interface TSchema { x: string }\n",
            "interface TLiteral<T> extends TSchema { const: T }\n",
            "function h<T extends string>(a: UnionToTuple<{ [K in T]: TLiteral<K>; }[T]>): TSchema[] { return a; }\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","strict":true,"noEmit":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 diagnostics");
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 199, "{stdout}");
    assert_eq!(
        lines[0],
        "a.ts(6,91): error TS2322: Type '[] | [...UnionToTuple<Exclude<{ [K in T]: TLiteral<K>; }[T], UnionLast<{ [K in T]: TLiteral<K>; }[T]>>, UnionLast<Exclude<{ [K in T]: TLiteral<K>; }[T], UnionLast<...>>>>, UnionLast<...>]' is not assignable to type 'TSchema[]'."
    );
    assert_eq!(
        lines[198].trim_start(),
        "Type 'unknown' is not assignable to type 'TSchema'."
    );
    assert_eq!(lines[198].len() - lines[198].trim_start().len(), 396);
    assert!(!stdout.contains("TS2321"), "{stdout}");
}

#[test]
fn array_types_written_as_type_nodes_share_a_recursion_identity_like_tsgo() {
    // A type reference made from a type node carries ObjectFlagsFromTypeNode and
    // its recursion identity is its node (checker/relater.go:766-870; checker.go
    // 23664, 24600-24602), so the arrays `Inner[]`, `Mid[]` and `Leaf[]` of two
    // structurally parallel aliases are not taken for one expanding type and the
    // comparison reaches the differing `id`. tsc 6.0 identified every array by
    // the global `Array` and stopped at the third level with Maybe: no error.
    // The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "namespace A {\n",
            "    export type Outer = { inners: Inner[] }\n",
            "    export type Inner = { mids: Mid[] }\n",
            "    export type Mid = { leaves: Leaf[] }\n",
            "    export type Leaf = { id: string }\n",
            "}\n",
            "namespace B {\n",
            "    export type Outer = { inners: Inner[] }\n",
            "    export type Inner = { mids: Mid[] }\n",
            "    export type Mid = { leaves: Leaf[] }\n",
            "    export type Leaf = { id: number }\n",
            "}\n",
            "function test(a: A.Outer, b: B.Outer) {\n",
            "    a = b\n",
            "}\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","noEmit":true,"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "a.ts(14,5): error TS2322: Type 'B.Outer' is not assignable to type 'A.Outer'.\n",
            "  Types of property 'inners' are incompatible.\n",
            "    Type 'B.Inner[]' is not assignable to type 'A.Inner[]'.\n",
            "      Type 'B.Inner' is not assignable to type 'A.Inner'.\n",
            "        Types of property 'mids' are incompatible.\n",
            "          Type 'B.Mid[]' is not assignable to type 'A.Mid[]'.\n",
            "            Type 'B.Mid' is not assignable to type 'A.Mid'.\n",
            "              Types of property 'leaves' are incompatible.\n",
            "                Type 'B.Leaf[]' is not assignable to type 'A.Leaf[]'.\n",
            "                  Type 'B.Leaf' is not assignable to type 'A.Leaf'.\n",
            "                    Types of property 'id' are incompatible.\n",
            "                      Type 'number' is not assignable to type 'string'.\n",
        )
    );
}

#[test]
fn a_circular_mapped_property_is_an_error_before_it_is_reported_like_tsgo() {
    // getTypeOfMappedSymbol stores the error type of a property whose type
    // circularly references itself and then writes TS2615 (checker/checker.go:
    // 21345-21349). Printing the mapped type for the message asks for the
    // property again: tsc 6.0 stored the type after the message and the print
    // resolved the property once more, down to the instantiation depth (an
    // extra TS5114). The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "type N<T, K extends string> = T | { [P in K]: N<T, K> }[K];\n",
            "\n",
            "type M = N<number, \"M\">;\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","noEmit":true,"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        "a.ts(3,10): error TS2615: Type of property 'M' circularly references itself in mapped type '{ [P in \"M\"]: any; }'.\n"
    );
}

#[test]
fn a_computed_name_that_names_its_own_binding_is_checked_once_like_tsgo() {
    // checkComputedPropertyName caches on the computed name node and marks it
    // while its expression is checked (checker/checker.go:27272-27290), so the
    // binding element `{[b]: b}`, whose name expression is the variable it
    // declares, reports the use before the declaration once and no implicit
    // any. The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "for (let {[a]: a} of [{ }]) continue;\n",
            "\n",
            "for (let {[c]: c} = { }; false; ) continue;\n",
            "\n",
            "let {[b]: b} = { };\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2015","module":"esnext","noEmit":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "a.ts(1,12): error TS2448: Block-scoped variable 'a' used before its declaration.\n",
            "a.ts(1,12): error TS2538: Type '{}' cannot be used as an index type.\n",
            "a.ts(3,12): error TS2448: Block-scoped variable 'c' used before its declaration.\n",
            "a.ts(3,12): error TS2538: Type '{}' cannot be used as an index type.\n",
            "a.ts(5,7): error TS2448: Block-scoped variable 'b' used before its declaration.\n",
            "a.ts(5,7): error TS2538: Type '{}' cannot be used as an index type.\n",
        )
    );
}

#[test]
fn the_keys_of_a_remapping_mapped_type_are_not_deferred_like_tsgo() {
    // The base constraint of `keyof M` for a generic mapped type `M` with a
    // `as` clause is the constraint of its keys (checker/checker.go:27901-27910,
    // 27988-27995), getSimplifiedType has no index type case (28367-28375), and
    // checkIndexedAccessIndexType takes the keys of a remapping mapped type
    // undeferred (8399-8405). `obj[key]` with `key: keyof Mapped6<K>` is
    // `Mapped6<K>[keyof Mapped6<K>]` and is not a `_${string}`. The expected
    // bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "type Mapped5<K extends string> = {\n",
            "  [P in K as P extends `_${string}` ? P : never]: P;\n",
            "};\n",
            "\n",
            "function f5<K extends string>(obj: Mapped5<K>, key: keyof Mapped5<K>) {\n",
            "  let s: `_${string}` = obj[key];\n",
            "}\n",
            "\n",
            "type Mapped6<K extends string> = {\n",
            "  [P in K as `_${P}`]: P;\n",
            "};\n",
            "\n",
            "function f6<K extends string>(obj: Mapped6<K>, key: keyof Mapped6<K>) {\n",
            "  let s: `_${string}` = obj[key]; // Error\n",
            "}\n",
            "\n",
            "type Foo<T extends string> = {\n",
            "    [RemappedT in T as `get${RemappedT}`]: RemappedT;\n",
            "};\n",
            "\n",
            "const get = <T extends string>(t: T, foo: Foo<T>): T => foo[`get${t}`];\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2017","module":"esnext","noEmit":true,"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "a.ts(14,7): error TS2322: Type 'Mapped6<K>[keyof Mapped6<K>]' is not assignable to type '`_${string}`'.\n",
            "  Type 'Mapped6<K>[`_${string}`]' is not assignable to type '`_${string}`'.\n",
            "a.ts(21,57): error TS2322: Type 'Foo<T>[`get${T}`]' is not assignable to type 'T'.\n",
            "  'T' could be instantiated with an arbitrary type which could be unrelated to 'Foo<T>[`get${T}`]'.\n",
        )
    );
}

#[test]
fn a_recursive_alias_indexed_by_number_keeps_its_indexed_access_like_tsgo() {
    // Three rules meet in `Recur<T>[number]`, whose alias contains an array of
    // itself. getTypeArguments stores the instantiated arguments of a deferred
    // reference with `??=` after its resolution frame is popped, so the result
    // of the outermost instantiation replaces the error type the innermost one
    // stored at the depth limit (checker/checker.go:22319-22323).
    // getSimplifiedIndexedAccessType drops the type itself from a simplified
    // union (28380-28393). In a writing position the access is
    // `Recur<T>[number] & (…)[number]`, which a string is not assignable to.
    // tsc 6.0 kept the innermost error type: every assignment here was to
    // `any`. The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "type Recur<T> =\n",
            "    (T extends  (unknown[]) ? {} : { [K in keyof T]?: Recur<T[K]>}) |\n",
            "    [...Recur<T>[number][]];\n",
            "\n",
            "declare function g1<T>(): Recur<T>[number];\n",
            "function f1<T>() { const n: { zz: 1 } = g1<T>(); }\n",
            "type C<T> = (T extends (unknown[]) ? {} : { [K in keyof T]?: Recur<T[K]>});\n",
            "function f2<T>(s: string) { const a: C<T>[number] = s; }\n",
            "function f3<T>(s: string, c: C<T>) { c[0] = s; }\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","noEmit":true,"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "a.ts(3,5): error TS4109: Type arguments for 'Array' circularly reference themselves.\n",
            "a.ts(3,9): error TS2536: Type 'number' cannot be used to index type 'Recur<T>'.\n",
            "a.ts(5,27): error TS2536: Type 'number' cannot be used to index type 'Recur<T>'.\n",
            "a.ts(5,27): error TS2589: Type instantiation is excessively deep and possibly infinite.\n",
            "a.ts(6,26): error TS2322: Type '(T extends unknown[] ? {} : { [K in keyof T]?: Recur<T[K]> | undefined; })[number]' is not assignable to type '{ zz: 1; }'.\n",
            "  Type 'unknown' is not assignable to type '{ zz: 1; }'.\n",
            "a.ts(6,41): error TS2589: Type instantiation is excessively deep and possibly infinite.\n",
            "a.ts(8,35): error TS2322: Type 'string' is not assignable to type 'C<T>[number]'.\n",
            "a.ts(8,38): error TS2536: Type 'number' cannot be used to index type 'C<T>'.\n",
            "a.ts(9,38): error TS7053: Element implicitly has an 'any' type because expression of type '0' can't be used to index type '{ [K in keyof T]?: Recur<T[K]> | undefined; } | {}'.\n",
            "  Property '0' does not exist on type '{ [K in keyof T]?: Recur<T[K]> | undefined; } | {}'.\n",
        )
    );
}

#[test]
fn a_distributive_conditional_type_depends_on_its_distribution_like_tsgo() {
    // isTypeParameterPossiblyReferenced finds a reference by the symbol of the
    // type reference (checker/checker.go, getSymbolFromTypeReference(node) ==
    // tp.symbol). The check type of a distributive conditional type is the
    // distributed form of its type parameter, so a comparison of types found
    // no reference, isDistributionDependent was false, and a relation to
    // `T extends unknown[] ? {} : {…}` instantiated its check type again: the
    // return statement got TS2589 in place of TS2322. The expected bytes are
    // tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "type Recur<T> =\n",
            "    (T extends  (unknown[]) ? {} : { [K in keyof T]?: Recur<T[K]>}) |\n",
            "    [...Recur<T>[number][]];\n",
            "\n",
            "function join<T>(l: Recur<T>[]): Recur<T> {\n",
            "    return ['marker', ...l];\n",
            "}\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","noEmit":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "a.ts(3,5): error TS4109: Type arguments for 'Array' circularly reference themselves.\n",
            "a.ts(3,9): error TS2536: Type 'number' cannot be used to index type 'Recur<T>'.\n",
            "a.ts(6,5): error TS2322: Type '[string, ...Recur<T>[]]' is not assignable to type 'Recur<T>'.\n",
            "  Type '[string, ...Recur<T>[]]' is not assignable to type 'Recur<T>[number][]'.\n",
            "    Type 'string | Recur<T>' is not assignable to type 'Recur<T>[number] & (T extends unknown[] ? {} : { [K in keyof T]?: Recur<T[K]> | undefined; })[number]'.\n",
            "      Type 'string' is not assignable to type 'Recur<T>[number] & (T extends unknown[] ? {} : { [K in keyof T]?: Recur<T[K]> | undefined; })[number]'.\n",
            "        Type 'string' is not assignable to type 'Recur<T>[number] & (T extends unknown[] ? {} : { [K in keyof T]?: Recur<T[K]> | undefined; })[number]'.\n",
            "          Type 'string' is not assignable to type '(T extends unknown[] ? {} : { [K in keyof T]?: Recur<T[K]> | undefined; })[number]'.\n",
            "a.ts(6,12): error TS2589: Type instantiation is excessively deep and possibly infinite.\n",
        )
    );
}

#[test]
fn closely_matched_union_members_are_inferred_deepest_first_like_tsgo() {
    // inferFromMatchingTypes collects the closely matched pairs and infers over
    // the matched targets in order of decreasing depth of generic
    // instantiation (checker/inference.go:370-409), so `Value[][]` is related to
    // `T[][]` before `T[]` and T is `Value`. tsc 6.0 inferred in the order the
    // union lists its members, took `Value[]` for T and rejected the call
    // (TS2345). The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare function flat<T>(args: T[] | T[][]): T;\n",
            "type Value = 1 | 2;\n",
            "declare const n: Value[] | Value[][];\n",
            "const a: string = flat(n);\n",
            "\n",
            "type Box<T> = { value: T };\n",
            "declare function flat0<T>(args: Box<T> | Box<Box<T>>): T;\n",
            "declare const arg0: Box<string> | Box<Box<string>>;\n",
            "const b: number = flat0(arg0);\n",
            "\n",
            "interface Column<T> {\n",
            "  dataIndex?: (T | (string & {}))[]\n",
            "}\n",
            "declare function table<T>(rows: readonly T[], columns: Column<T>[]): T\n",
            "declare const rows: { id: number }[]\n",
            "const c: string = table(rows, [{ dataIndex: ['id'] }])\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","noEmit":true,"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "a.ts(4,7): error TS2322: Type 'number' is not assignable to type 'string'.\n",
            "  Type 'number' is not assignable to type 'string'.\n",
            "a.ts(9,7): error TS2322: Type 'string' is not assignable to type 'number'.\n",
            "a.ts(16,7): error TS2322: Type '{ id: number; }' is not assignable to type 'string'.\n",
        )
    );
}

#[test]
fn a_recursive_call_resolution_skips_constraint_checks_like_tsgo() {
    // A call that is resolved again while its overloads are being chosen, and
    // has a single candidate, infers without comparing the inferences with
    // their constraints (Checker.callResolutionStack and
    // InferenceFlagsNoConstraintChecks, checker/checker.go:9108-9116,
    // 9243-9246; inference.go:1381). `Item`'s base expression asks for
    // `typeof BaseItem`, whose construct signatures need `typeof Item` and so
    // the same call: tsc 6.0 compared `typeof BaseItem`, still without
    // signatures, with the constraint and reported TS2345. The expected result
    // is tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare class Document<Parent> {}\n",
            "\n",
            "declare class BaseItem extends Document<typeof Item> {}\n",
            "\n",
            "declare function ClientDocumentMixin<\n",
            "  BaseClass extends new (...args: any[]) => any,\n",
            ">(Base: BaseClass): any;\n",
            "\n",
            "declare class Item extends ClientDocumentMixin(BaseItem) {}\n",
            "\n",
            "export {};\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2015","module":"esnext","noEmit":true,"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
}

#[test]
fn an_unknown_extends_type_instantiates_nothing_like_tsgo() {
    // getConditionalType reaches the permissive and restrictive instantiations
    // of the check type through `&&` and `||` (checker/checker.go:24838-24893):
    // with an `unknown` extends type none is made. Computing them first
    // instantiated every check type of `T extends unknown ? … : never`; in a
    // constraint walk whose check types nest a level deeper at each step that
    // reached the depth limit (TS2589). The expected result is tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "export type Prepend<Elm, T extends unknown[]> =\n",
            " T extends unknown ?\n",
            " ((arg: Elm, ...rest: T) => void) extends ((...args: infer T2) => void) ? T2 :\n",
            " never :\n",
            " never;\n",
            "export type ExactExtract<T, U> = (T extends U ? U extends T ? T : never : never) & string;\n",
            "type Conv<T, U = T> = {\n",
            "    0: [T];\n",
            "    1: Prepend<T, Conv<ExactExtract<U, T>, {a: number}>>;\n",
            "}[U extends T ? 0 : 1];\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","noEmit":true,"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
}

#[test]
fn a_type_that_is_not_iterable_is_reported_after_the_file_like_tsgo() {
    // reportTypeNotIterableError is queued with addDeferredDiagnostic and
    // produced at the end of the file check (checker/checker.go:6463-6467,
    // 6511-6522), when printing the type cannot re-enter a resolution in
    // progress. tsc 6.0 reported inside the inference of `foo`'s return type,
    // which printed the function as `() => any` and failed that resolution.
    // The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "function* foo() {\n",
            "  yield*foo\n",
            "}\n",
            "declare const u: number | string[];\n",
            "for (const x of u) {}\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es6","module":"esnext","noEmit":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "a.ts(2,9): error TS2488: Type '() => Generator<any, void, unknown>' must have a '[Symbol.iterator]()' method that returns an iterator.\n",
            "a.ts(5,17): error TS2488: Type 'number | string[]' must have a '[Symbol.iterator]()' method that returns an iterator.\n",
        )
    );
}

#[test]
fn a_contextual_function_return_type_is_computed_in_full_like_tsgo() {
    // contextuallyCheckFunctionExpressionOrObjectLiteralMethod removes
    // CheckModeSkipContextSensitive before getReturnTypeFromBody
    // (checker/checker.go:10377-10384): the return type is cached for good and
    // must not hold the wildcard function. tsc 6.0 passed the mode through, so
    // a generic callback that returns a context-sensitive function was
    // accepted whatever that function is (TypeScript issue 61979). The
    // expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare function fn<P>(config: {\n",
            "  callback: (params: P) => (context: number, params: P) => number;\n",
            "  unrelated?: (arg: string) => void;\n",
            "}): (params: P) => number;\n",
            "\n",
            "export const result1 = fn({\n",
            "  callback: <T,>(params: T) => {\n",
            "    return (a: boolean, b) => (a ? 1 : 0);\n",
            "  },\n",
            "  unrelated: (_) => {},\n",
            "});\n",
            "\n",
            "export const result2 = fn({\n",
            "  callback: <T,>(params: T) => {\n",
            "    return (a, b) => true;\n",
            "  },\n",
            "  unrelated: (_) => {},\n",
            "});\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","noEmit":true,"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "a.ts(7,3): error TS2322: Type '<T>(params: T) => (a: boolean, b: unknown) => 0 | 1' is not assignable to type '(params: T) => (context: number, params: T) => number'.\n",
            "  Type '(a: boolean, b: unknown) => 0 | 1' is not assignable to type '(context: number, params: T) => number'.\n",
            "    Types of parameters 'a' and 'context' are incompatible.\n",
            "      Type 'number' is not assignable to type 'boolean'.\n",
            "a.ts(14,3): error TS2322: Type '<T>(params: T) => (a: number, b: unknown) => boolean' is not assignable to type '(params: T) => (context: number, params: T) => number'.\n",
            "  Type '(a: number, b: unknown) => boolean' is not assignable to type '(context: number, params: T) => number'.\n",
            "    Type 'boolean' is not assignable to type 'number'.\n",
        )
    );
}

#[test]
fn an_error_typed_property_prints_its_pseudo_type_like_tsgo() {
    // With an enclosing declaration serializeTypeForDeclaration compares the
    // pseudo type of a property's value declaration with its type, and an
    // error type counts as equal (checker/pseudotypenodebuilder.go:363-366).
    // A property written twice keeps the first declaration and the last type:
    // with an unresolved last value the first value's pseudo type is printed.
    // An expression the pseudochecker gives up on (`[1]`) prints `any`, a type
    // assertion its type node. tsc 6.0 printed `any` for all of them. The
    // expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare const P: number;\n",
            "declare const T: any;\n",
            "declare const x: number;\n",
            "P < T > { a: 1 as Missing };\n",
            "P < T > { a: \"s\", a: v };\n",
            "P < T > { a: true, a: v };\n",
            "P < T > { a: null, a: v };\n",
            "P < T > { a: undefined, a: v };\n",
            "P < T > { a: -1, a: v };\n",
            "P < T > { a: 1n, a: v };\n",
            "P < T > { a: `t`, a: v };\n",
            "P < T > { a: () => 1, a: v };\n",
            "P < T > { a: [1], a: v };\n",
            "P < T > { a: { b: 1 }, a: v };\n",
            "P < T > { a: [1] as const, a: v };\n",
            "P < T > { a: x, a: v };\n",
            "P < T > { a: 1 as const, a: v };\n",
            "P < T > { a: \"s\" as string | number, a: v };\n",
            "P < T > { a: (y: number) => y, a: v };\n",
            "P < T > { a: x + 1, a: v };\n",
            "P < T > { a() { return 1; }, a: v };\n",
            "const k = { a: 1, a: v } as const;\n",
            "P < T > k;\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","noEmit":true,"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "a.ts(4,1): error TS2365: Operator '>' cannot be applied to types 'boolean' and '{ a: Missing; }'.\n",
            "a.ts(4,19): error TS2304: Cannot find name 'Missing'.\n",
            "a.ts(5,1): error TS2365: Operator '>' cannot be applied to types 'boolean' and '{ a: string; }'.\n",
            "a.ts(5,19): error TS1117: An object literal cannot have multiple properties with the same name.\n",
            "a.ts(5,22): error TS2304: Cannot find name 'v'.\n",
            "a.ts(6,1): error TS2365: Operator '>' cannot be applied to types 'boolean' and '{ a: boolean; }'.\n",
            "a.ts(6,20): error TS1117: An object literal cannot have multiple properties with the same name.\n",
            "a.ts(6,23): error TS2304: Cannot find name 'v'.\n",
            "a.ts(7,1): error TS2365: Operator '>' cannot be applied to types 'boolean' and '{ a: null; }'.\n",
            "a.ts(7,20): error TS1117: An object literal cannot have multiple properties with the same name.\n",
            "a.ts(7,23): error TS2304: Cannot find name 'v'.\n",
            "a.ts(8,1): error TS2365: Operator '>' cannot be applied to types 'boolean' and '{ a: undefined; }'.\n",
            "a.ts(8,25): error TS1117: An object literal cannot have multiple properties with the same name.\n",
            "a.ts(8,28): error TS2304: Cannot find name 'v'.\n",
            "a.ts(9,1): error TS2365: Operator '>' cannot be applied to types 'boolean' and '{ a: number; }'.\n",
            "a.ts(9,18): error TS1117: An object literal cannot have multiple properties with the same name.\n",
            "a.ts(9,21): error TS2304: Cannot find name 'v'.\n",
            "a.ts(10,1): error TS2365: Operator '>' cannot be applied to types 'boolean' and '{ a: bigint; }'.\n",
            "a.ts(10,18): error TS1117: An object literal cannot have multiple properties with the same name.\n",
            "a.ts(10,21): error TS2304: Cannot find name 'v'.\n",
            "a.ts(11,1): error TS2365: Operator '>' cannot be applied to types 'boolean' and '{ a: string; }'.\n",
            "a.ts(11,19): error TS1117: An object literal cannot have multiple properties with the same name.\n",
            "a.ts(11,22): error TS2304: Cannot find name 'v'.\n",
            "a.ts(12,1): error TS2365: Operator '>' cannot be applied to types 'boolean' and '{ a: () => number; }'.\n",
            "a.ts(12,23): error TS1117: An object literal cannot have multiple properties with the same name.\n",
            "a.ts(12,26): error TS2304: Cannot find name 'v'.\n",
            "a.ts(13,1): error TS2365: Operator '>' cannot be applied to types 'boolean' and '{ a: any; }'.\n",
            "a.ts(13,19): error TS1117: An object literal cannot have multiple properties with the same name.\n",
            "a.ts(13,22): error TS2304: Cannot find name 'v'.\n",
            "a.ts(14,1): error TS2365: Operator '>' cannot be applied to types 'boolean' and '{ a: { b: number; }; }'.\n",
            "a.ts(14,24): error TS1117: An object literal cannot have multiple properties with the same name.\n",
            "a.ts(14,27): error TS2304: Cannot find name 'v'.\n",
            "a.ts(15,1): error TS2365: Operator '>' cannot be applied to types 'boolean' and '{ a: readonly [1]; }'.\n",
            "a.ts(15,28): error TS1117: An object literal cannot have multiple properties with the same name.\n",
            "a.ts(15,31): error TS2304: Cannot find name 'v'.\n",
            "a.ts(16,1): error TS2365: Operator '>' cannot be applied to types 'boolean' and '{ a: number; }'.\n",
            "a.ts(16,17): error TS1117: An object literal cannot have multiple properties with the same name.\n",
            "a.ts(16,20): error TS2304: Cannot find name 'v'.\n",
            "a.ts(17,1): error TS2365: Operator '>' cannot be applied to types 'boolean' and '{ a: 1; }'.\n",
            "a.ts(17,26): error TS1117: An object literal cannot have multiple properties with the same name.\n",
            "a.ts(17,29): error TS2304: Cannot find name 'v'.\n",
            "a.ts(18,1): error TS2365: Operator '>' cannot be applied to types 'boolean' and '{ a: string | number; }'.\n",
            "a.ts(18,38): error TS1117: An object literal cannot have multiple properties with the same name.\n",
            "a.ts(18,41): error TS2304: Cannot find name 'v'.\n",
            "a.ts(19,1): error TS2365: Operator '>' cannot be applied to types 'boolean' and '{ a: (y: number) => number; }'.\n",
            "a.ts(19,32): error TS1117: An object literal cannot have multiple properties with the same name.\n",
            "a.ts(19,35): error TS2304: Cannot find name 'v'.\n",
            "a.ts(20,1): error TS2365: Operator '>' cannot be applied to types 'boolean' and '{ a: number; }'.\n",
            "a.ts(20,21): error TS1117: An object literal cannot have multiple properties with the same name.\n",
            "a.ts(20,24): error TS2304: Cannot find name 'v'.\n",
            "a.ts(21,1): error TS2365: Operator '>' cannot be applied to types 'boolean' and '{ a: any; }'.\n",
            "a.ts(21,11): error TS2300: Duplicate identifier 'a'.\n",
            "a.ts(21,30): error TS1119: An object literal cannot have property and accessor with the same name.\n",
            "a.ts(21,30): error TS2300: Duplicate identifier 'a'.\n",
            "a.ts(21,33): error TS2304: Cannot find name 'v'.\n",
            "a.ts(22,19): error TS1117: An object literal cannot have multiple properties with the same name.\n",
            "a.ts(22,22): error TS2304: Cannot find name 'v'.\n",
            "a.ts(23,1): error TS2365: Operator '>' cannot be applied to types 'boolean' and '{ readonly a: 1; }'.\n",
        )
    );
}

#[test]
fn a_jsx_element_without_attributes_discriminates_its_props_like_tsgo() {
    // discriminateContextualTypeByJSXAttributes takes the optional discriminant
    // properties the element does not write whenever the attributes node has a
    // symbol (checker/jsx.go:266-292). An element with no attribute at all was
    // left undiscriminated, and the parameter of its child function was an
    // implicit any (TS7006). The expected result is tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("jsx.d.ts"),
        concat!(
            "declare namespace JSX {\n",
            "    interface Element {}\n",
            "    interface IntrinsicElements { div: {} }\n",
            "    interface IntrinsicAttributes { key?: string }\n",
            "    interface ElementChildrenAttribute { children: {} }\n",
            "}\n",
        ),
    )
    .expect("write jsx.d.ts");
    fs::write(
        tree.path("a.tsx"),
        concat!(
            "type Props =\n",
            "  | { renderNumber?: false; children: (arg: string) => void }\n",
            "  | { renderNumber: true; children: (arg: number) => void };\n",
            "\n",
            "declare function Foo(props: Props): JSX.Element;\n",
            "\n",
            "export const a = <Foo>{(value) => { const s: number = value; }}</Foo>;\n",
            "export const b = <Foo renderNumber>{(value) => { const n: string = value; }}</Foo>;\n",
            "export const c = <Foo children={(value) => { const s: number = value; }} />;\n",
        ),
    )
    .expect("write a.tsx");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","noEmit":true,"strict":true,"jsx":"preserve"},"files":["jsx.d.ts","a.tsx"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "a.tsx(7,43): error TS2322: Type 'string' is not assignable to type 'number'.\n",
            "a.tsx(8,56): error TS2322: Type 'number' is not assignable to type 'string'.\n",
            "a.tsx(9,52): error TS2322: Type 'string' is not assignable to type 'number'.\n",
        )
    );
}

#[test]
fn the_children_of_a_jsx_element_body_are_an_excess_property_like_tsgo() {
    // The `children` property synthesized from an element's body has a
    // fabricated declaration whose parent is the attributes node (checker/
    // jsx.go:845-848), which makes it subject to the excess property check.
    // The port left it without a declaration: body children of a component
    // that takes none were accepted next to another attribute. The expected
    // bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("jsx.d.ts"),
        concat!(
            "declare namespace JSX {\n",
            "    interface Element {}\n",
            "    interface IntrinsicElements { div: {} }\n",
            "    interface IntrinsicAttributes { key?: string }\n",
            "    interface ElementChildrenAttribute { children: {} }\n",
            "}\n",
        ),
    )
    .expect("write jsx.d.ts");
    fs::write(
        tree.path("a.tsx"),
        concat!(
            "const Tag = (x: {}) => <div></div>;\n",
            "\n",
            "const k1 = <Tag />;\n",
            "const k2 = <Tag></Tag>;\n",
            "const k3 = <Tag children={<div></div>} />;\n",
            "const k4 = <Tag key=\"1\"><div></div></Tag>;\n",
            "const k5 = <Tag key=\"1\"><div></div><div></div></Tag>;\n",
            "const k6 = <Tag><div></div></Tag>;\n",
        ),
    )
    .expect("write a.tsx");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","noEmit":true,"jsx":"preserve"},"files":["jsx.d.ts","a.tsx"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "a.tsx(5,17): error TS2322: Type '{ children: Element; }' is not assignable to type 'IntrinsicAttributes'.\n",
            "  Property 'children' does not exist on type 'IntrinsicAttributes'.\n",
            "a.tsx(6,13): error TS2322: Type '{ key: string; children: Element; }' is not assignable to type 'IntrinsicAttributes'.\n",
            "  Property 'children' does not exist on type 'IntrinsicAttributes'.\n",
            "a.tsx(7,13): error TS2322: Type '{ key: string; children: Element[]; }' is not assignable to type 'IntrinsicAttributes'.\n",
            "  Property 'children' does not exist on type 'IntrinsicAttributes'.\n",
            "a.tsx(8,13): error TS2559: Type '{ children: Element; }' has no properties in common with type 'IntrinsicAttributes'.\n",
        )
    );
}

#[test]
fn a_member_named_by_a_bigint_literal_is_not_a_property_like_tsgo() {
    // The binder declares an object literal member named by a bigint literal
    // as `__missing`, and getNamedMembers keeps reserved names out of a type's
    // properties. The literal `{ 3n: … }` is `{}`: it lacks the property `"3n"`
    // (TS2741) and has no excess one (TS2353 was reported, and the type printed
    // as `{ __missing: string; }`). The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "interface H {\n",
            "    \"3n\": string;\n",
            "}\n",
            "const h : H = { 3n: \"propertyNameErrorAndMissingProperty3\" };\n",
            "const h3 = { 3n: \"x\", a: 1 };\n",
            "const n: number = h3;\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"esnext","module":"esnext","noEmit":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "a.ts(4,7): error TS2741: Property '\"3n\"' is missing in type '{}' but required in type 'H'.\n",
            "a.ts(4,17): error TS1539: A 'bigint' literal cannot be used as a property name.\n",
            "a.ts(5,14): error TS1539: A 'bigint' literal cannot be used as a property name.\n",
            "a.ts(6,7): error TS2322: Type '{ a: number; }' is not assignable to type 'number'.\n",
        )
    );
}

#[test]
fn a_defaulted_expando_initializer_is_an_ordinary_expression_like_tsgo() {
    // tsgo has no `X = X || {}` form: checkBinaryLikeExpression checks both
    // operands (checker/checker.go:12538-12544) and the assignment is always
    // compared. tsc 6.0 took the right operand's type and skipped the
    // comparison; the port kept the first half and reported the expando
    // members of `Common` as missing from `{}` (TS2741). The expected bytes
    // are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.js"),
        concat!(
            "var Common = {};\n",
            "self['Common'] = self['Common'] || {};\n",
            "/**\n",
            " * @param {string} string\n",
            " * @return {string}\n",
            " */\n",
            "Common.localize = function (string) {\n",
            "    return string;\n",
            "};\n",
            "/** @type {number} */\n",
            "const n1 = self['Common'];\n",
            "/** @type {number} */\n",
            "const n2 = self['Common'] || {};\n",
            "/** @type {number} */\n",
            "const n3 = Common;\n",
            "self['Common'] = {};\n",
            "self.Common = {};\n",
            "Common = {};\n",
        ),
    )
    .expect("write a.js");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020","dom"],"target":"es2015","module":"esnext","noEmit":true,"checkJs":true,"allowJs":true},"files":["a.js"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "a.js(11,7): error TS2322: Type '{ localize: (string: string) => string; }' is not assignable to type 'number'.\n",
            "a.js(13,7): error TS2322: Type '{ localize: (string: string) => string; }' is not assignable to type 'number'.\n",
            "a.js(15,7): error TS2322: Type '{ localize: (string: string) => string; }' is not assignable to type 'number'.\n",
            "a.js(16,1): error TS2741: Property 'localize' is missing in type '{}' but required in type '{ localize: (string: string) => string; }'.\n",
            "a.js(17,1): error TS2741: Property 'localize' is missing in type '{}' but required in type '{ localize: (string: string) => string; }'.\n",
            "a.js(18,1): error TS2741: Property 'localize' is missing in type '{}' but required in type '{ localize: (string: string) => string; }'.\n",
        )
    );
}

#[test]
fn the_root_of_a_javascript_property_assignment_is_an_ordinary_name_like_tsgo() {
    // tsgo's binder declares no container for an assignment to a property of
    // an undeclared name, so its checker resolves the root as any other name:
    // TS2304 at each use, also for a prototype assignment, and TS2708 where the
    // name is a namespace without values. tsc 6.0's binder declared the
    // container. The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("lf.d.ts"),
        concat!(
            "declare namespace lf {\n",
            "  export interface Transaction {\n",
            "    commit(): Promise<void>\n",
            "  }\n",
            "}\n",
        ),
    )
    .expect("write lf.d.ts");
    fs::write(
        tree.path("a.js"),
        concat!(
            "lf.Transaction = function() {};\n",
            "/**\n",
            " * @param {number} scope\n",
            " */\n",
            "lf.Transaction.prototype.begin = function(scope) {};\n",
            "C.prototype = {}\n",
            "C.prototype.bar.foo = {};\n",
            "D.x = 1;\n",
            "E.prototype.m = function() {};\n",
        ),
    )
    .expect("write a.js");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2015","module":"esnext","noEmit":true,"checkJs":true,"allowJs":true},"files":["lf.d.ts","a.js"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "a.js(1,1): error TS2708: Cannot use namespace 'lf' as a value.\n",
            "a.js(5,1): error TS2708: Cannot use namespace 'lf' as a value.\n",
            "a.js(6,1): error TS2304: Cannot find name 'C'.\n",
            "a.js(7,1): error TS2304: Cannot find name 'C'.\n",
            "a.js(8,1): error TS2304: Cannot find name 'D'.\n",
            "a.js(9,1): error TS2304: Cannot find name 'E'.\n",
        )
    );
}

#[test]
fn an_export_of_a_declaration_file_marks_nothing_visible_like_tsgo() {
    // The declarations an `export =` or an export specifier names become
    // visible when the declarations of that file are transformed
    // (PrecalculateDeclarationEmitVisibility, checker/emitresolver.go:236-306;
    // transformers/declarations/transform.go:304). A declaration file is never
    // transformed, so `namespace foo` of `export = foo` stays invisible and a
    // name an augmentation resolves to its member is a private name (TS4060):
    // `a.d.ts` is not written. tsc 6.0 marked the namespace when it checked
    // the export; the port did too, on the checker that checked that file, so
    // the result depended on how the files were shared out (the default
    // library set of this test put both files on one checker). The expected
    // bytes are tsgo's.
    let tree = TempTree::new();
    fs::create_dir_all(tree.path("node_modules/foo")).expect("create node_modules/foo");
    fs::write(
        tree.path("node_modules/foo/index.d.ts"),
        concat!(
            "export = foo;\n",
            "declare namespace foo {\n",
            "    export type T = number;\n",
            "}\n",
        ),
    )
    .expect("write node_modules/foo/index.d.ts");
    fs::write(
        tree.path("a.ts"),
        concat!(
            "import * as foo from \"foo\";\n",
            "declare module \"foo\" {\n",
            "    export function f(): T;\n",
            "}\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("b.ts"),
        concat!(
            "import * as foo from \"foo\";\n",
            "declare module \"foo\" {\n",
            "    export function g(): foo.T;\n",
            "}\n",
        ),
    )
    .expect("write b.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"target":"es2015","module":"commonjs","declaration":true,"outDir":"out"},"files":["a.ts","b.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        "a.ts(3,26): error TS4060: Return type of exported function has or is using private name 'T'.\n"
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/b.d.ts")).expect("read out/b.d.ts"),
        concat!(
            "import * as foo from \"foo\";\n",
            "declare module \"foo\" {\n",
            "    function g(): foo.T;\n",
            "}\n",
        )
    );
    assert!(!tree.path("out/a.d.ts").exists());
}

#[test]
fn ambient_modules_merge_after_the_global_augmentations_like_tsgo() {
    // initializeChecker merges the locals of the script files, then the
    // global-scope augmentations, looks the global types up, and then merges
    // the ambient module declarations (checker/checker.go initializeChecker).
    // Merging the two `mymod` declarations resolves the alias `foo`, which
    // loads the module `foo` and reads `string[]`: the import was reported as
    // unresolved (TS2307) in place of the conflict (TS2451), and with lib es5
    // alone the augmented `Array<T>`, a global with one declaration, had two
    // declared types (TS2339 for `customMethod`). The expected bytes are
    // tsgo's.
    let tree = TempTree::new();
    fs::create_dir_all(tree.path("node_modules/foo")).expect("create node_modules/foo");
    fs::write(
        tree.path("node_modules/foo/index.d.ts"),
        concat!(
            "declare function foo(): void;\n",
            "declare namespace foo { export const items: string[]; }\n",
            "export = foo;\n",
        ),
    )
    .expect("write node_modules/foo/index.d.ts");
    fs::write(
        tree.path("a.d.ts"),
        "declare module 'mymod' { import * as foo from 'foo'; export { foo }; }\n",
    )
    .expect("write a.d.ts");
    fs::write(
        tree.path("b.d.ts"),
        "declare module 'mymod' { export const foo: number; }\n",
    )
    .expect("write b.d.ts");
    fs::write(
        tree.path("augment.ts"),
        concat!(
            "declare global {\n",
            "    interface Array<T> {\n",
            "        customMethod(): T;\n",
            "    }\n",
            "}\n",
            "export {};\n",
        ),
    )
    .expect("write augment.ts");
    fs::write(
        tree.path("index.ts"),
        concat!(
            "import * as foo from 'foo';\n",
            "const items = foo.items;\n",
            "const result: string = items.customMethod();\n",
            "\n",
            "const fresh: string[] = [];\n",
            "const result2: number = fresh.customMethod();\n",
        ),
    )
    .expect("write index.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es5"],"target":"es2015","module":"esnext","noEmit":true},"files":["a.d.ts","b.d.ts","augment.ts","index.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "a.d.ts(1,63): error TS2451: Cannot redeclare block-scoped variable 'foo'.\n",
            "b.d.ts(1,39): error TS2451: Cannot redeclare block-scoped variable 'foo'.\n",
            "index.ts(6,7): error TS2322: Type 'string' is not assignable to type 'number'.\n",
        )
    );
}

#[test]
fn a_nested_call_resolution_keeps_the_errors_of_the_outer_one_like_tsgo() {
    // The call in the body of an arrow function argument is resolved again
    // while it is being resolved (the argument is checked for the outer call's
    // inference). The nested resolution skips the constraint checks and
    // succeeds; resolveCall has no early return for the signature it cached
    // (checker/checker.go:9117-9133), so the outer resolution still fails and
    // reports, and getResolvedSignature swaps to the cached signature only
    // afterwards (8606-8614). tsc 6.0 returned the cached signature before
    // reporting, which with the unchecked nested resolution lost the error
    // (zod's `// @ts-expect-error` lines over `z.templateLiteral([…])` became
    // unused directives). The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare function expect<T>(actual: T): void;\n",
            "declare function templateLiteral<const Parts extends string[]>(parts: Parts): Parts;\n",
            "declare function obj(): { o: 1 };\n",
            "expect(() => templateLiteral([obj()]));\n",
            "expect(() => templateLiteral([{}]));\n",
            "templateLiteral([obj()]);\n",
            "declare function lit<const P extends readonly (string | number)[]>(parts: P): P;\n",
            "expect(() => lit([obj(), \"a\"]));\n",
            "const direct = () => lit([obj()]);\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","noEmit":true,"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "a.ts(4,31): error TS2322: Type '{ o: 1; }' is not assignable to type 'string'.\n",
            "a.ts(5,31): error TS2322: Type '{}' is not assignable to type 'string'.\n",
            "a.ts(6,18): error TS2322: Type '{ o: 1; }' is not assignable to type 'string'.\n",
            "a.ts(8,19): error TS2322: Type '{ o: 1; }' is not assignable to type 'string | number'.\n",
            "a.ts(9,27): error TS2322: Type '{ o: 1; }' is not assignable to type 'string | number'.\n",
        )
    );
}

#[test]
fn an_import_alias_through_a_type_only_namespace_is_not_type_only_like_tsgo() {
    // `import A = a.A` names a class through the type-only namespace import `a`:
    // the import alias is reported (TS1380), and that is all. tsgo records a
    // type-only declaration on an alias only when the alias is written type-only
    // or when its target is a pure alias that has one (markSymbolOfAlias…,
    // resolveIndirectionAlias, checker/checker.go:15325-15341, 16612-16620), and
    // `a.A` resolves to the class itself. So `A` is usable as a value, `b.A` in
    // another file is an ordinary alias, and only `b.a.A`, which names the
    // type-only `a` again, is reported. tsc 6.0 marked the import alias from the
    // namespace on the left of its name and reported every use of `A` and of
    // the aliases that reach it. The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(tree.path("a.ts"), "export class A {}\n").expect("write a.ts");
    fs::write(
        tree.path("b.ts"),
        concat!(
            "import type * as a from './a';\n",
            "import A = a.A;\n",
            "export { a, A };\n",
            "new A();\n",
        ),
    )
    .expect("write b.ts");
    fs::write(
        tree.path("c.ts"),
        concat!(
            "import * as b from './b';\n",
            "import A = b.a.A;\n",
            "import AA = b.A;\n",
            "new A();\n",
            "new AA();\n",
        ),
    )
    .expect("write c.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","noEmit":true,"strict":true},"files":["a.ts","b.ts","c.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "b.ts(2,12): error TS1380: An import alias cannot reference a declaration that was imported using 'import type'.\n",
            "c.ts(2,12): error TS1380: An import alias cannot reference a declaration that was imported using 'import type'.\n",
        )
    );
}

#[test]
fn an_alias_merged_with_a_namespace_ends_the_type_only_chain_like_tsgo() {
    // `A` in b.ts is an import merged with a namespace, so the symbol has a
    // value meaning of its own. getTypeOnlyAliasDeclaration walks the alias
    // chain only until a symbol with the wanted meaning
    // (checker/checker.go:2182-2194): the walk from c.ts ends at b.ts's `A` and
    // never reaches `export type { A }` in a.ts. tsc 6.0 took the type-only
    // declaration from the final target of the chain and reported each use of
    // `A` in c.ts (TS1362). The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!("function A() {}\n", "export type { A };\n",),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("b.ts"),
        concat!(
            "import { A } from \"./a\";\n",
            "namespace A {\n",
            "  export const displayName = \"A\";\n",
            "}\n",
            "export { A };\n",
        ),
    )
    .expect("write b.ts");
    fs::write(
        tree.path("c.ts"),
        concat!(
            "import { A } from \"./b\";\n",
            "A;\n",
            "A.displayName;\n",
            "A();\n",
        ),
    )
    .expect("write c.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","noEmit":true,"strict":true},"files":["a.ts","b.ts","c.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "b.ts(1,10): error TS2440: Import declaration conflicts with local declaration of 'A'.\n",
            "c.ts(4,1): error TS2349: This expression is not callable.\n",
            "  Type 'typeof A' has no call signatures.\n",
        )
    );
}

#[test]
fn a_renamed_re_export_of_a_name_from_export_type_star_is_type_only_like_tsgo() {
    // `export { A as A1 } from "./b"` takes `A` from a module that has it only
    // through `export type *`. getExportOfModule marks the export specifier with
    // that export declaration (checker/checker.go:15031-15040), and the import
    // of `A1` in d.ts copies the record when it resolves through the specifier
    // (resolveIndirectionAlias). tsc 6.0 looked the importing name `A1` up in
    // the exports of the `export type *` module, found nothing and let `new
    // A1()` pass. The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(tree.path("a.ts"), "export class A {}\n").expect("write a.ts");
    fs::write(tree.path("b.ts"), "export type * from './a';\n").expect("write b.ts");
    fs::write(tree.path("c.ts"), "export { A as A1 } from './b';\n").expect("write c.ts");
    fs::write(
        tree.path("d.ts"),
        concat!("import { A1 } from './c';\n", "new A1();\n", "let x: A1;\n",),
    )
    .expect("write d.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","noEmit":true,"strict":true},"files":["a.ts","b.ts","c.ts","d.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        "d.ts(2,5): error TS1362: 'A1' cannot be used as a value because it was exported using 'export type'.\n"
    );
}

#[test]
fn an_import_alias_of_a_type_only_import_is_reported_like_tsgo() {
    // `import AA = A` names the type-only `import type A = require(…)` of a
    // class. checkAndReportErrorForResolvingImportAliasToTypeOnlySymbol asks
    // each part of the entity name, from the whole name to its leftmost
    // identifier, whether it names a type-only alias under any meaning
    // (checker/checker.go:14722-14754), so the import alias is reported
    // (TS1380) next to the failed namespace lookup (TS2702). tsc 6.0 asked
    // whether the import alias had been marked from its resolved target; the
    // failed lookup left no target and only TS2702 was reported. The expected
    // bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(tree.path("a.ts"), concat!("class A {}\n", "export = A;\n",)).expect("write a.ts");
    fs::write(
        tree.path("b.ts"),
        concat!(
            "import type A = require('./a');\n",
            "import AA = A;\n",
            "let x: AA;\n",
        ),
    )
    .expect("write b.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"commonjs","noEmit":true,"strict":true},"files":["a.ts","b.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "b.ts(2,13): error TS1380: An import alias cannot reference a declaration that was imported using 'import type'.\n",
            "b.ts(2,13): error TS2702: 'A' only refers to a type, but is being used as a namespace here.\n",
        )
    );
}

#[test]
fn a_type_only_import_alias_of_a_declaration_is_not_recorded_like_tsgo() {
    // `import type T = N.C` is a grammar error (TS1392). Its name passes through
    // no alias, and tsgo records the type-only declaration of an import alias
    // while it resolves an alias in the name (resolveEntityName,
    // checker/checker.go:16135-16138), so nothing is recorded and `new T()` is
    // not reported. `import type U = M.C`, whose name starts at the import `M`,
    // is recorded and its use is reported (TS1361). tsc 6.0 marked every import
    // alias written `import type` and reported both uses. The expected bytes
    // are tsgo's.
    let tree = TempTree::new();
    fs::write(tree.path("a.ts"), "export class C {}\n").expect("write a.ts");
    fs::write(
        tree.path("b.ts"),
        concat!(
            "import * as M from './a';\n",
            "namespace N {\n",
            "  export class C {}\n",
            "}\n",
            "import type T = N.C;\n",
            "import type U = M.C;\n",
            "new T();\n",
            "new U();\n",
        ),
    )
    .expect("write b.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","noEmit":true,"strict":true},"files":["a.ts","b.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "b.ts(5,1): error TS1392: An import alias cannot use 'import type'\n",
            "b.ts(6,1): error TS1392: An import alias cannot use 'import type'\n",
            "b.ts(8,5): error TS1361: 'U' cannot be used as a value because it was imported using 'import type'.\n",
        )
    );
}

#[test]
fn a_missing_jsx_runtime_is_reported_at_the_first_tag_of_the_file_like_tsgo() {
    // The module of the automatic JSX runtime cannot be found. tsgo reports it
    // at the first JSX tag of the file in source order, whichever tag asked
    // for the runtime first (getJsxNamespaceContainerForImplicitImport,
    // checker/jsx.go:1449-1484). Here the first tag is in the body of an arrow
    // function, which is checked after the statement below it. tsc 6.0
    // reported at the tag that asked first, `<b />`. The expected bytes are
    // tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.tsx"),
        concat!(
            "declare namespace JSX {\n",
            "    interface IntrinsicElements {\n",
            "        [name: string]: any;\n",
            "    }\n",
            "}\n",
            "const f = () => <a />;\n",
            "const x = <b />;\n",
        ),
    )
    .expect("write a.tsx");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","noEmit":true,"strict":true,"jsx":"react-jsx","moduleResolution":"bundler"},"files":["a.tsx"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "a.tsx(6,17): error TS2875: This JSX tag requires the module path 'react/jsx-runtime' to exist, but none could be found. Make sure you have types for the appropriate package installed.\n",
            "a.tsx(6,17): error TS7026: JSX element implicitly has type 'any' because no interface 'JSX.IntrinsicElements' exists.\n",
            "a.tsx(7,11): error TS7026: JSX element implicitly has type 'any' because no interface 'JSX.IntrinsicElements' exists.\n",
        )
    );
}

#[test]
fn a_missing_jsx_runtime_is_reported_at_the_opening_of_a_first_fragment_like_tsgo() {
    // When the first JSX tag of the file is a fragment, the missing runtime
    // module is reported at its opening `<>`
    // (getJsxNamespaceContainerForImplicitImport, checker/jsx.go:1463-1466).
    // The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.tsx"),
        concat!(
            "declare namespace JSX {\n",
            "    interface IntrinsicElements {\n",
            "        [name: string]: any;\n",
            "    }\n",
            "}\n",
            "const f = () => <><a /></>;\n",
            "const x = <b />;\n",
        ),
    )
    .expect("write a.tsx");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","noEmit":true,"strict":true,"jsx":"react-jsx","moduleResolution":"bundler"},"files":["a.tsx"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "a.tsx(6,17): error TS2875: This JSX tag requires the module path 'react/jsx-runtime' to exist, but none could be found. Make sure you have types for the appropriate package installed.\n",
            "a.tsx(6,19): error TS7026: JSX element implicitly has type 'any' because no interface 'JSX.IntrinsicElements' exists.\n",
            "a.tsx(7,11): error TS7026: JSX element implicitly has type 'any' because no interface 'JSX.IntrinsicElements' exists.\n",
        )
    );
}

#[test]
fn same_named_aliases_are_ordered_by_their_symbols_like_tsgo() {
    // compareTypeNames orders two alias symbols of the same name by
    // compareSymbols before the alias arguments or the structure are looked at
    // (checker/utilities.go:632-649): `N1.A<number>` precedes `N2.A<string>`
    // because `N1.A` is declared first. The port compared the type arguments
    // of the two aliases as if they were one. The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "type Inner<T> = { v: T };\n",
            "namespace N1 { export type A<T> = Inner<T>; }\n",
            "namespace N2 { export type A<T> = Inner<T>; }\n",
            "declare const a1: N2.A<string> | N1.A<number>;\n",
            "export const sameNamedAliases = [a1];\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"esnext","module":"esnext","outDir":"out","declaration":true,"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.d.ts")).expect("read out/a.d.ts"),
        concat!(
            "type Inner<T> = {\n",
            "    v: T;\n",
            "};\n",
            "declare namespace N1 {\n",
            "    type A<T> = Inner<T>;\n",
            "}\n",
            "declare namespace N2 {\n",
            "    type A<T> = Inner<T>;\n",
            "}\n",
            "export declare const sameNamedAliases: (N1.A<number> | N2.A<string>)[];\n",
            "export {};\n",
        )
    );
}

#[test]
fn instantiation_expression_types_are_ordered_by_declaration_and_expression_like_tsgo() {
    // Two instantiation expression types are ordered by the first declaration
    // of their symbol, which is the declaration of the function they
    // instantiate (checker/checker.go:10893-10895), and then by the expression
    // node (checker/utilities.go:442-462): `f<string>` precedes `g<string>`
    // although `g<string>` is written first, and the instantiations of one
    // function keep the order they are written in. The expected bytes are
    // tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare function f<T>(x: T): T;\n",
            "declare function g<T>(x: T): T[];\n",
            "export const byDeclaration = [g<string>, f<string>];\n",
            "export const byExpression = [f<string>, f<number>, f<boolean>];\n",
            "export const byExpression2 = [f<boolean>, f<number>, f<string>];\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"esnext","module":"esnext","outDir":"out","declaration":true,"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.d.ts")).expect("read out/a.d.ts"),
        concat!(
            "export declare const byDeclaration: (((x: string) => string) | ((x: string) => string[]))[];\n",
            "export declare const byExpression: (((x: string) => string) | ((x: number) => number) | ((x: boolean) => boolean))[];\n",
            "export declare const byExpression2: (((x: boolean) => boolean) | ((x: number) => number) | ((x: string) => string))[];\n",
        )
    );
}

#[test]
fn tuple_types_are_ordered_by_the_labels_of_every_element_like_tsgo() {
    // compareTupleTypes compares the label of every element, an unlabeled
    // element before a labeled one and two labels by their text
    // (checker/utilities.go:668-700). The port read the labels of the first
    // tuple only: a first tuple without labels compared equal to a labeled
    // one, and the order fell through to the type identifiers. The expected
    // bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare const t1: [a: string] | [string];\n",
            "declare const t2: [b: string] | [a: string];\n",
            "declare const t3: [x: string, ...b: number[]] | [x: string, ...a: number[]];\n",
            "declare const t4: [x?: string, b?: number] | [x?: string, a?: number];\n",
            "export const tuples1 = [t1];\n",
            "export const tuples2 = [t2];\n",
            "export const tuples3 = [t3];\n",
            "export const tuples4 = [t4];\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"esnext","module":"esnext","outDir":"out","declaration":true,"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.d.ts")).expect("read out/a.d.ts"),
        concat!(
            "export declare const tuples1: ([string] | [a: string])[];\n",
            "export declare const tuples2: ([a: string] | [b: string])[];\n",
            "export declare const tuples3: ([x: string, ...a: number[]] | [x: string, ...b: number[]])[];\n",
            "export declare const tuples4: ([x?: string | undefined, a?: number | undefined] | [x?: string | undefined, b?: number | undefined])[];\n",
        )
    );
}

#[test]
fn mapped_types_are_ordered_by_their_instantiation_like_tsgo() {
    // An instantiated mapped type is ordered by the mapper of its
    // instantiation: for a composite mapper tsgo compares the second mapper,
    // the one that carries the type arguments, and never the fresh type
    // parameter of the first (checker/utilities.go:509-520). The members come
    // out in the order of their type arguments (`string`, `number`,
    // `boolean`), not in the order the instantiations were made. The expected
    // bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare function mapped<T>(): { [K in \"value\"]: T };\n",
            "export const mappedValues = [mapped<number>(), mapped<string>(), mapped<boolean>()];\n",
            "declare function mapped2<T, U>(): { [K in \"value\"]: [T, U] };\n",
            "export const mappedValues2 = [mapped2<number, string>(), mapped2<string, number>(), mapped2<boolean, boolean>()];\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"esnext","module":"esnext","outDir":"out","declaration":true,"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.d.ts")).expect("read out/a.d.ts"),
        concat!(
            "export declare const mappedValues: ({\n",
            "    value: string;\n",
            "} | {\n",
            "    value: number;\n",
            "} | {\n",
            "    value: boolean;\n",
            "})[];\n",
            "export declare const mappedValues2: ({\n",
            "    value: [string, number];\n",
            "} | {\n",
            "    value: [number, string];\n",
            "} | {\n",
            "    value: [boolean, boolean];\n",
            "})[];\n",
        )
    );
}

#[test]
fn reverse_mapped_types_are_ordered_by_source_mapped_type_and_constraint_like_tsgo() {
    // Two reverse mapped types are ordered by their source type, then by the
    // mapped type and then by the constraint type
    // (checker/utilities.go:497-508). The port fell through to the type
    // identifiers, so the order followed the order of inference. The expected
    // bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare function unbox<T>(value: { [K in keyof T]: { value: T[K] } }): T;\n",
            "declare function identity<T>(value: { [K in keyof T]: T[K] }): T;\n",
            "declare const strings: { value: { value: string } };\n",
            "declare const numbers: { value: { value: number } };\n",
            "export const reverse = [unbox(numbers), unbox(strings), identity(numbers), identity(strings)];\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"esnext","module":"esnext","outDir":"out","declaration":true,"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.d.ts")).expect("read out/a.d.ts"),
        concat!(
            "export declare const reverse: ({\n",
            "    value: string;\n",
            "} | {\n",
            "    value: {\n",
            "        value: string;\n",
            "    };\n",
            "} | {\n",
            "    value: number;\n",
            "} | {\n",
            "    value: {\n",
            "        value: number;\n",
            "    };\n",
            "})[];\n",
        )
    );
}

#[test]
fn anonymous_instantiations_are_ordered_by_their_type_arguments_like_tsgo() {
    // Two instantiations of one anonymous type are ordered by their mappers:
    // a simple mapper by its target and an array mapper by its targets in
    // order (compareTypeMappers, checker/utilities.go:716-755), so
    // `{ p: string }` precedes `{ p: number }` whichever is made first. An
    // object type that also depends on a type parameter of an enclosing
    // function is ordered the same way once both are known. The expected
    // bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare function anon<T>(): { p: T };\n",
            "declare function anon2<T, U>(): { p: T, q: U };\n",
            "export const anonymous = [anon<number>(), anon<string>(), anon2<number, string>(), anon2<string, number>()];\n",
            "function outer<A>() {\n",
            "    function inner<B>() {\n",
            "        return null! as { a: A, b: B };\n",
            "    }\n",
            "    return [inner<number>(), inner<string>()];\n",
            "}\n",
            "export const nested = [...outer<string>(), ...outer<number>()];\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"esnext","module":"esnext","outDir":"out","declaration":true,"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.d.ts")).expect("read out/a.d.ts"),
        concat!(
            "export declare const anonymous: ({\n",
            "    p: string;\n",
            "} | {\n",
            "    p: number;\n",
            "})[];\n",
            "export declare const nested: ({\n",
            "    a: string;\n",
            "    b: string;\n",
            "} | {\n",
            "    a: string;\n",
            "    b: number;\n",
            "} | {\n",
            "    a: number;\n",
            "    b: string;\n",
            "} | {\n",
            "    a: number;\n",
            "    b: number;\n",
            "})[];\n",
        )
    );
}

#[test]
fn a_js_function_typed_by_a_generic_type_tag_declares_its_type_parameters_like_tsgo() {
    // A JavaScript function whose signature comes from a `@type` tag has no
    // type parameter list of its own. ensureTypeParams asks the resolver for
    // the type parameters of the signature
    // (transformers/declarations/transform.go:2376-2384;
    // CreateTypeParametersOfSignatureDeclaration, checker/emitresolver.go:962),
    // and typeParametersToTypeParameterDeclarations
    // (checker/nodebuilderimpl.go:1696) answers for a function symbol with the
    // type parameters of its value declaration. A method symbol gets none, so
    // tsgo writes `method(m: T): T;` without declaring `T`; that output is
    // pinned as it is. The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.js"),
        concat!(
            "/**\n",
            " * @typedef {<T>(m : T) => T} IFn\n",
            " */\n",
            "\n",
            "/**@type {IFn}*/\n",
            "export function inJs(l) {\n",
            "    return l;\n",
            "}\n",
            "\n",
            "/** @type {<T extends string, U = T[]>(a: T, b: U) => [T, U]} */\n",
            "export function two(a, b) {\n",
            "    return [a, b];\n",
            "}\n",
            "\n",
            "/** @type {(a: number) => number} */\n",
            "export function plain(a) {\n",
            "    return a;\n",
            "}\n",
            "\n",
            "export class C {\n",
            "    /** @type {<T>(m: T) => T} */\n",
            "    method(m) {\n",
            "        return m;\n",
            "    }\n",
            "}\n",
            "\n",
            "/** @type {<T>(m: T) => T} */\n",
            "export const arrow = (m) => m;\n",
            "\n",
            "/** @type {<const T extends readonly unknown[]>(...args: T) => T} */\n",
            "export function rest(...args) {\n",
            "    return args;\n",
            "}\n",
        ),
    )
    .expect("write a.js");
    fs::write(
        tree.path("b.js"),
        concat!(
            "/** @type {<T>(m: T) => T} */\n",
            "export default function (m) {\n",
            "    return m;\n",
            "}\n",
        ),
    )
    .expect("write b.js");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"esnext","module":"esnext","outDir":"out","declaration":true,"strict":true,"allowJs":true,"checkJs":true},"files":["a.js","b.js"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.d.ts")).expect("read out/a.d.ts"),
        concat!(
            "/**\n",
            " * @typedef {<T>(m : T) => T} IFn\n",
            " */\n",
            "export type IFn = <T>(m: T) => T;\n",
            "/**@type {IFn}*/\n",
            "export declare function inJs<T>(l: T): T;\n",
            "/** @type {<T extends string, U = T[]>(a: T, b: U) => [T, U]} */\n",
            "export declare function two<T extends string, U = T[]>(a: T, b: U): [T, U];\n",
            "/** @type {(a: number) => number} */\n",
            "export declare function plain(a: number): number;\n",
            "export declare class C {\n",
            "    /** @type {<T>(m: T) => T} */\n",
            "    method(m: T): T;\n",
            "}\n",
            "/** @type {<T>(m: T) => T} */\n",
            "export declare const arrow: <T>(m: T) => T;\n",
            "/** @type {<const T extends readonly unknown[]>(...args: T) => T} */\n",
            "export declare function rest<const T extends readonly unknown[]>(...args: T): T;\n",
        )
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/b.d.ts")).expect("read out/b.d.ts"),
        concat!(
            "/** @type {<T>(m: T) => T} */\n",
            "export default function <T>(m: T): T;\n",
        )
    );
}

#[test]
fn a_comment_after_a_property_without_a_value_is_written_once_like_tsgo() {
    // `x()?: 1 // error` parses as a method without a body and a property `1`
    // whose value is missing. emitPropertyAssignment asks for the trailing
    // comments at the start of the value (printer/printer.go:4554-4558), and
    // emitTrailingComments leaves a position where the enclosing container
    // ends to that container (5604-5611). The comment is written once, after
    // the empty value; the port wrote it before the value as well. The
    // expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!("var b = {\n", "    x()?: 1 // error\n", "}\n",),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2015","module":"esnext","outDir":"out"},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "a.ts(2,8): error TS1005: '{' expected.\n",
            "a.ts(2,9): error TS1136: Property assignment expected.\n",
            "a.ts(3,1): error TS1005: ':' expected.\n",
        )
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/a.js")).expect("read out/a.js"),
        concat!(
            "\"use strict\";\n",
            "var b = {\n",
            "    x() { }, 1:  // error\n",
            "};\n",
        )
    );
}

#[test]
fn a_private_static_call_clones_the_class_alias_for_each_use_like_tsgo() {
    // The class fields transform substitutes the class alias for the class
    // name when it visits the identifier, with a clone for every use
    // (classFieldsTransformer.visitIdentifier,
    // estransforms/classfields.go:462-473), and
    // createPrivateIdentifierAccessHelper gives the receiver the comment range
    // (-1, end) (1027-1028). The `this` argument of `.call` is therefore a
    // separate node that keeps the range of `A1`, and the comment after a call
    // without arguments is written before and after it. The expected bytes
    // are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "class A1 {\n",
            "    static #method(param: string): string {\n",
            "        return \"\";\n",
            "    }\n",
            "    constructor() {\n",
            "        A1.#method(\"\")\n",
            "        A1.#method(1) // Error\n",
            "        A1.#method()  // Error\n",
            "\n",
            "    }\n",
            "}\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2015","module":"esnext","outDir":"out","strict":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "a.ts(7,20): error TS2345: Argument of type 'number' is not assignable to parameter of type 'string'.\n",
            "a.ts(8,12): error TS2554: Expected 1 arguments, but got 0.\n",
        )
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/a.js")).expect("read out/a.js"),
        concat!(
            "\"use strict\";\n",
            "var __classPrivateFieldGet = (this && this.__classPrivateFieldGet) || function (receiver, state, kind, f) {\n",
            "    if (kind === \"a\" && !f) throw new TypeError(\"Private accessor was defined without a getter\");\n",
            "    if (typeof state === \"function\" ? receiver !== state || !f : !state.has(receiver)) throw new TypeError(\"Cannot read private member from an object whose class did not declare it\");\n",
            "    return kind === \"m\" ? f : kind === \"a\" ? f.call(receiver) : f ? f.value : state.get(receiver);\n",
            "};\n",
            "var _a, _A1_method;\n",
            "class A1 {\n",
            "    constructor() {\n",
            "        __classPrivateFieldGet(_a, _a, \"m\", _A1_method).call(_a, \"\");\n",
            "        __classPrivateFieldGet(_a, _a, \"m\", _A1_method).call(_a, 1); // Error\n",
            "        __classPrivateFieldGet(_a, _a, \"m\", _A1_method).call(// Error\n",
            "        _a); // Error\n",
            "    }\n",
            "}\n",
            "_a = A1, _A1_method = function _A1_method(param) {\n",
            "    return \"\";\n",
            "};\n",
        )
    );
}

#[test]
fn class_expression_temps_in_a_loop_are_block_scoped_like_tsgo() {
    // requiresBlockScopedVar is `inIterationStatement` and a class expression
    // container (classFieldsTransformer, estransforms/classfields.go:195-201):
    // the temps of the computed names of a class expression in a loop body
    // are declared with `let` in the body. The temp of the class itself is
    // block-scoped only when the class has an instance property with a
    // computed name (classExpressionNeedsBlockScopedTemp), and a class
    // declaration hoists its temps with `var`. tsc 6.0 asked the checker for
    // a block-scoped binding captured in the loop. The expected bytes are
    // tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "const array: any[] = [];\n",
            "for (let i = 0; i < 10; ++i) {\n",
            "    array.push(class C { [i] = () => C; static [i] = 100; });\n",
            "}\n",
            "for (let i = 0; i < 10; ++i) {\n",
            "    class D { [i] = 1; static [i] = 2; }\n",
            "    array.push(D);\n",
            "}\n",
            "for (let i = 0; i < 10; ++i) {\n",
            "    array.push(class E { static [i] = 100; });\n",
            "}\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2015","module":"esnext","outDir":"out","noCheck":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.js")).expect("read out/a.js"),
        concat!(
            "\"use strict\";\n",
            "var _a, _b, _c;\n",
            "const array = [];\n",
            "for (let i = 0; i < 10; ++i) {\n",
            "    let _d, _e, _f;\n",
            "    array.push((_f = class C {\n",
            "            constructor() {\n",
            "                this[_d] = () => _f;\n",
            "            }\n",
            "        },\n",
            "        _d = i,\n",
            "        _e = i,\n",
            "        _f[_e] = 100,\n",
            "        _f));\n",
            "}\n",
            "for (let i = 0; i < 10; ++i) {\n",
            "    class D {\n",
            "        constructor() {\n",
            "            this[_a] = 1;\n",
            "        }\n",
            "    }\n",
            "    _a = i, _b = i;\n",
            "    D[_b] = 2;\n",
            "    array.push(D);\n",
            "}\n",
            "for (let i = 0; i < 10; ++i) {\n",
            "    let _g;\n",
            "    array.push((_c = class E {\n",
            "        },\n",
            "        _g = i,\n",
            "        _c[_g] = 100,\n",
            "        _c));\n",
            "}\n",
        )
    );
}

#[test]
fn class_expression_temps_in_a_loop_are_block_scoped_at_es2022_like_tsgo() {
    // The same rule in the transform that keeps class fields native: with
    // `useDefineForClassFields: false` at ES2022 the property assignments move
    // into the constructor and the computed names need temps, declared with
    // `let` in the loop body for a class expression
    // (estransforms/classfields.go:195-201). The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "const array: any[] = [];\n",
            "for (let i = 0; i < 10; ++i) {\n",
            "    array.push(class C { [i] = () => C; static [i] = 100; });\n",
            "}\n",
            "for (let i = 0; i < 10; ++i) {\n",
            "    class D { [i] = 1; static [i] = 2; }\n",
            "    array.push(D);\n",
            "}\n",
            "for (let i = 0; i < 10; ++i) {\n",
            "    array.push(class E { static [i] = 100; });\n",
            "}\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2022"],"target":"es2022","module":"esnext","outDir":"out","useDefineForClassFields":false,"noCheck":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.js")).expect("read out/a.js"),
        concat!(
            "\"use strict\";\n",
            "var _a, _b;\n",
            "const array = [];\n",
            "for (let i = 0; i < 10; ++i) {\n",
            "    let _c, _d;\n",
            "    array.push(class C {\n",
            "        constructor() {\n",
            "            this[_c] = () => C;\n",
            "        }\n",
            "        static { _c = i, _d = i; }\n",
            "        static { this[_d] = 100; }\n",
            "    });\n",
            "}\n",
            "for (let i = 0; i < 10; ++i) {\n",
            "    class D {\n",
            "        constructor() {\n",
            "            this[_a] = 1;\n",
            "        }\n",
            "        static { _a = i, _b = i; }\n",
            "        static { this[_b] = 2; }\n",
            "    }\n",
            "    array.push(D);\n",
            "}\n",
            "for (let i = 0; i < 10; ++i) {\n",
            "    let _e;\n",
            "    array.push(class E {\n",
            "        static { _e = i; }\n",
            "        static { this[_e] = 100; }\n",
            "    });\n",
            "}\n",
        )
    );
}

#[test]
fn functions_in_a_loop_reset_the_iteration_state_of_class_fields_like_tsgo() {
    // `inIterationStatement` is cleared for a function expression or
    // declaration and for a method of an object literal, and kept for an arrow
    // function and for the members of a class
    // (classFieldsTransformer.visit, estransforms/classfields.go:329-336;
    // visitClassElement, 416-437). A class expression returned by an arrow
    // function or by a class method inside a loop declares its temps with
    // `let` in that function; one inside a function expression uses `var`.
    // The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "const array: any[] = [];\n",
            "for (let i = 0; i < 10; ++i) {\n",
            "    array.push(() => class E { [i] = 1; static [i] = 2; });\n",
            "    array.push(function () { return class F { [i] = 1; static [i] = 2; }; });\n",
            "    array.push({ m() { return class O { [i] = 1; static [i] = 2; }; } });\n",
            "    array.push(class G { m() { return class H { [i] = 1; static [i] = 2; }; } });\n",
            "}\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2015","module":"esnext","outDir":"out","noCheck":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.js")).expect("read out/a.js"),
        concat!(
            "\"use strict\";\n",
            "const array = [];\n",
            "for (let i = 0; i < 10; ++i) {\n",
            "    array.push(() => { let _a, _b, _c; return _c = class E {\n",
            "            constructor() {\n",
            "                this[_a] = 1;\n",
            "            }\n",
            "        },\n",
            "        _a = i,\n",
            "        _b = i,\n",
            "        _c[_b] = 2,\n",
            "        _c; });\n",
            "    array.push(function () { var _a, _b, _c; return _c = class F {\n",
            "            constructor() {\n",
            "                this[_a] = 1;\n",
            "            }\n",
            "        },\n",
            "        _a = i,\n",
            "        _b = i,\n",
            "        _c[_b] = 2,\n",
            "        _c; });\n",
            "    array.push({ m() { var _a, _b, _c; return _c = class O {\n",
            "                constructor() {\n",
            "                    this[_a] = 1;\n",
            "                }\n",
            "            },\n",
            "            _a = i,\n",
            "            _b = i,\n",
            "            _c[_b] = 2,\n",
            "            _c; } });\n",
            "    array.push(class G {\n",
            "        m() { let _a, _b, _c; return _c = class H {\n",
            "                constructor() {\n",
            "                    this[_a] = 1;\n",
            "                }\n",
            "            },\n",
            "            _a = i,\n",
            "            _b = i,\n",
            "            _c[_b] = 2,\n",
            "            _c; }\n",
            "    });\n",
            "}\n",
        )
    );
}

#[test]
fn iteration_statement_headers_count_as_the_loop_for_class_fields_like_tsgo() {
    // Every child of a `for`-`in`, `for`-`of`, `while` or `do` statement is
    // visited with `inIterationStatement` set, and of a `for` statement only
    // the body (estransforms/classfields.go:329-330, visitForStatement
    // 1243-1253). The `let` of a temp requested in a header joins the
    // enclosing function or source file. The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare let k: any;\n",
            "function f() {\n",
            "    for (let i = (class I { [k] = 1; }, 0); i < (class J { [k] = 1; }, 10); i += (class K { [k] = 1; }, 1)) {\n",
            "    }\n",
            "    while (class L { [k] = 1; }) { break; }\n",
            "    do { } while (class M { [k] = 1; });\n",
            "    for (const x in class N { [k] = 1; }) { }\n",
            "}\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2015","module":"esnext","outDir":"out","noCheck":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.js")).expect("read out/a.js"),
        concat!(
            "\"use strict\";\n",
            "function f() {\n",
            "    var _a, _b, _c, _d, _e, _f;\n",
            "    let _g, _h, _j, _k, _l, _m;\n",
            "    for (let i = (_b = class I {\n",
            "            constructor() {\n",
            "                this[_a] = 1;\n",
            "            }\n",
            "        },\n",
            "        _a = k,\n",
            "        _b, 0); i < (_d = class J {\n",
            "            constructor() {\n",
            "                this[_c] = 1;\n",
            "            }\n",
            "        },\n",
            "        _c = k,\n",
            "        _d, 10); i += (_f = class K {\n",
            "            constructor() {\n",
            "                this[_e] = 1;\n",
            "            }\n",
            "        },\n",
            "        _e = k,\n",
            "        _f, 1)) {\n",
            "    }\n",
            "    while (_h = class L {\n",
            "            constructor() {\n",
            "                this[_g] = 1;\n",
            "            }\n",
            "        },\n",
            "        _g = k,\n",
            "        _h) {\n",
            "        break;\n",
            "    }\n",
            "    do { } while (_k = class M {\n",
            "            constructor() {\n",
            "                this[_j] = 1;\n",
            "            }\n",
            "        },\n",
            "        _j = k,\n",
            "        _k);\n",
            "    for (const x in _m = class N {\n",
            "            constructor() {\n",
            "                this[_l] = 1;\n",
            "            }\n",
            "        },\n",
            "        _l = k,\n",
            "        _m) { }\n",
            "}\n",
        )
    );
}

#[test]
fn temps_of_computed_member_names_are_declared_in_member_order_like_tsgo() {
    // The temp that caches the computed name of an auto-accessor is created
    // when the class element visitor reaches the accessor (transformAutoAccessor,
    // estransforms/classfields.go:839-857), after the temps of the members
    // before it, and it is not reserved in nested scopes. The temps of a
    // method's computed name belong to the scope around the class. The port
    // allocated the accessor temp before every other member. The expected
    // bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "class C1 {\n",
            "    [class A1 { static x = 1; } as any]() { }\n",
            "    get [class A2 { static x = 1; } as any]() { return 1; }\n",
            "    static [class A4 { static x = 1; } as any]() { }\n",
            "    [class A5 { static x = 1; } as any] = 1;\n",
            "    static [class A6 { static x = 1; } as any] = 1;\n",
            "    accessor [class A7 { static x = 1; } as any] = 1;\n",
            "    m() { return function () { var x = class A8 { static x = 1; }; return x; }; }\n",
            "}\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2015","module":"esnext","outDir":"out","noCheck":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.js")).expect("read out/a.js"),
        concat!(
            "\"use strict\";\n",
            "var __classPrivateFieldGet = (this && this.__classPrivateFieldGet) || function (receiver, state, kind, f) {\n",
            "    if (kind === \"a\" && !f) throw new TypeError(\"Private accessor was defined without a getter\");\n",
            "    if (typeof state === \"function\" ? receiver !== state || !f : !state.has(receiver)) throw new TypeError(\"Cannot read private member from an object whose class did not declare it\");\n",
            "    return kind === \"m\" ? f : kind === \"a\" ? f.call(receiver) : f ? f.value : state.get(receiver);\n",
            "};\n",
            "var __classPrivateFieldSet = (this && this.__classPrivateFieldSet) || function (receiver, state, value, kind, f) {\n",
            "    if (kind === \"m\") throw new TypeError(\"Private method is not writable\");\n",
            "    if (kind === \"a\" && !f) throw new TypeError(\"Private accessor was defined without a setter\");\n",
            "    if (typeof state === \"function\" ? receiver !== state || !f : !state.has(receiver)) throw new TypeError(\"Cannot write private member to an object whose class did not declare it\");\n",
            "    return (kind === \"a\" ? f.call(receiver, value) : f ? f.value = value : state.set(receiver, value)), value;\n",
            "};\n",
            "var _C1__a_accessor_storage, _a, _b, _c, _d, _e, _f, _g, _h, _j;\n",
            "class C1 {\n",
            "    constructor() {\n",
            "        this[_e] = 1;\n",
            "        _C1__a_accessor_storage.set(this, 1);\n",
            "    }\n",
            "    [(_C1__a_accessor_storage = new WeakMap(), _a = class A1 {\n",
            "        },\n",
            "        _a.x = 1,\n",
            "        _a)]() { }\n",
            "    get [(_b = class A2 {\n",
            "        },\n",
            "        _b.x = 1,\n",
            "        _b)]() { return 1; }\n",
            "    static [(_c = class A4 {\n",
            "        },\n",
            "        _c.x = 1,\n",
            "        _c)]() { }\n",
            "    get [(_e = (_d = class A5 {\n",
            "        },\n",
            "        _d.x = 1,\n",
            "        _d), _g = (_f = class A6 {\n",
            "        },\n",
            "        _f.x = 1,\n",
            "        _f), _h = (_j = class A7 {\n",
            "        },\n",
            "        _j.x = 1,\n",
            "        _j))]() { return __classPrivateFieldGet(this, _C1__a_accessor_storage, \"f\"); }\n",
            "    set [_h](value) { __classPrivateFieldSet(this, _C1__a_accessor_storage, value, \"f\"); }\n",
            "    m() { return function () { var _h; var x = (_h = class A8 {\n",
            "        },\n",
            "        _h.x = 1,\n",
            "        _h); return x; }; }\n",
            "}\n",
            "C1[_g] = 1;\n",
        )
    );
}

#[test]
fn a_lowered_private_method_declares_the_temps_of_its_body_like_tsgo() {
    // The function a private method becomes has a variable environment of its
    // own for its body; its parameters are visited after the body and outside
    // that environment, so the temp of a parameter initializer is declared
    // around the class (visitMethodOrAccessorDeclaration,
    // estransforms/classfields.go:695-702). The port declared the temps of
    // the body around the class. The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare let k: any;\n",
            "class C {\n",
            "    #m(a = class P1 { static x = 1; }) { return class X1 { static x = 1; }; }\n",
            "    get #g() { return class X2 { static x = 1; }; }\n",
            "    set #s(v: any) { k = class X3 { static x = 1; }; }\n",
            "    static #sm() { return class X4 { static x = 1; }; }\n",
            "    use() { this.#m(); this.#g; this.#s = 1; C.#sm(); }\n",
            "}\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2015","module":"esnext","outDir":"out","noCheck":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.js")).expect("read out/a.js"),
        concat!(
            "\"use strict\";\n",
            "var __classPrivateFieldGet = (this && this.__classPrivateFieldGet) || function (receiver, state, kind, f) {\n",
            "    if (kind === \"a\" && !f) throw new TypeError(\"Private accessor was defined without a getter\");\n",
            "    if (typeof state === \"function\" ? receiver !== state || !f : !state.has(receiver)) throw new TypeError(\"Cannot read private member from an object whose class did not declare it\");\n",
            "    return kind === \"m\" ? f : kind === \"a\" ? f.call(receiver) : f ? f.value : state.get(receiver);\n",
            "};\n",
            "var __classPrivateFieldSet = (this && this.__classPrivateFieldSet) || function (receiver, state, value, kind, f) {\n",
            "    if (kind === \"m\") throw new TypeError(\"Private method is not writable\");\n",
            "    if (kind === \"a\" && !f) throw new TypeError(\"Private accessor was defined without a setter\");\n",
            "    if (typeof state === \"function\" ? receiver !== state || !f : !state.has(receiver)) throw new TypeError(\"Cannot write private member to an object whose class did not declare it\");\n",
            "    return (kind === \"a\" ? f.call(receiver, value) : f ? f.value = value : state.set(receiver, value)), value;\n",
            "};\n",
            "var _C_instances, _a, _C_m, _C_g_get, _C_s_set, _C_sm, _b;\n",
            "class C {\n",
            "    constructor() {\n",
            "        _C_instances.add(this);\n",
            "    }\n",
            "    use() { __classPrivateFieldGet(this, _C_instances, \"m\", _C_m).call(this); __classPrivateFieldGet(this, _C_instances, \"a\", _C_g_get); __classPrivateFieldSet(this, _C_instances, 1, \"a\", _C_s_set); __classPrivateFieldGet(_a, _a, \"m\", _C_sm).call(_a); }\n",
            "}\n",
            "_a = C, _C_instances = new WeakSet(), _C_m = function _C_m(a = (_b = class P1 {\n",
            "    },\n",
            "    _b.x = 1,\n",
            "    _b)) { var _c; return _c = class X1 {\n",
            "    },\n",
            "    _c.x = 1,\n",
            "    _c; }, _C_g_get = function _C_g_get() { var _c; return _c = class X2 {\n",
            "    },\n",
            "    _c.x = 1,\n",
            "    _c; }, _C_s_set = function _C_s_set(v) { var _c; k = (_c = class X3 {\n",
            "    },\n",
            "    _c.x = 1,\n",
            "    _c); }, _C_sm = function _C_sm() { var _c; return _c = class X4 {\n",
            "    },\n",
            "    _c.x = 1,\n",
            "    _c; };\n",
        )
    );
}

#[test]
fn a_constructor_that_receives_initializers_keeps_its_parameters_like_tsgo() {
    // transformConstructor visits the parameters of a constructor that
    // receives the initializers of its class with the plain visitor, before
    // the variable environment of the body starts
    // (estransforms/classfields.go:2382-2387): a parameter initializer stays in
    // the parameter list and its temps are declared around the class. The
    // initializers and the statements of the body share one environment
    // (transformConstructorBody, 2520-2600), so their temps share one `var`
    // statement, the initializers' first. tsc 6.0 moved the parameter
    // initializer into the body and wrote two statements. The expected bytes
    // are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare let k: any;\n",
            "class P {\n",
            "    p = class Q3 { static x = 1; };\n",
            "    constructor(a = class Q6 { static x = 1; }) { k = class Q7 { static x = 1; }; }\n",
            "}\n",
            "class R {\n",
            "    constructor(a = class Q8 { static x = 1; }) { k = class Q9 { static x = 1; }; }\n",
            "}\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2015","module":"esnext","outDir":"out","noCheck":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.js")).expect("read out/a.js"),
        concat!(
            "\"use strict\";\n",
            "var _a;\n",
            "class P {\n",
            "    constructor(a = (_a = class Q6 {\n",
            "        },\n",
            "        _a.x = 1,\n",
            "        _a)) {\n",
            "        var _b, _c;\n",
            "        this.p = (_b = class Q3 {\n",
            "            },\n",
            "            _b.x = 1,\n",
            "            _b);\n",
            "        k = (_c = class Q7 {\n",
            "            },\n",
            "            _c.x = 1,\n",
            "            _c);\n",
            "    }\n",
            "}\n",
            "class R {\n",
            "    constructor(a) { var _b, _c; if (a === void 0) { a = (_b = class Q8 {\n",
            "        },\n",
            "        _b.x = 1,\n",
            "        _b); } k = (_c = class Q9 {\n",
            "        },\n",
            "        _c.x = 1,\n",
            "        _c); }\n",
            "}\n",
        )
    );
}

#[test]
fn a_retained_static_block_declares_its_temps_around_the_class_like_tsgo() {
    // A class static block that stays in the output is visited child by
    // child without a variable environment of its own
    // (visitClassStaticBlockDeclaration, estransforms/classfields.go:2181-2184):
    // a temp requested inside is declared around the class. The constructor
    // that receives the property assignments keeps its parameter initializer,
    // as in the lowering to ES2015. The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare let k: any, j: any;\n",
            "class P {\n",
            "    static { k = class Q { [k] = 1; static [j] = 2; }; }\n",
            "    p = class Q3 { [k] = 1; static [j] = 2; };\n",
            "    constructor(a = class Q6 { [k] = 1; static [j] = 2; }) { k = class Q7 { [k] = 1; static [j] = 2; }; }\n",
            "}\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2022"],"target":"es2022","module":"esnext","outDir":"out","useDefineForClassFields":false,"noCheck":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.js")).expect("read out/a.js"),
        concat!(
            "\"use strict\";\n",
            "var _a, _b, _c, _d;\n",
            "class P {\n",
            "    static { k = class Q {\n",
            "        constructor() {\n",
            "            this[_a] = 1;\n",
            "        }\n",
            "        static { _a = k, _b = j; }\n",
            "        static { this[_b] = 2; }\n",
            "    }; }\n",
            "    constructor(a = class Q6 {\n",
            "        constructor() {\n",
            "            this[_c] = 1;\n",
            "        }\n",
            "        static { _c = k, _d = j; }\n",
            "        static { this[_d] = 2; }\n",
            "    }) {\n",
            "        var _e, _f, _g, _h;\n",
            "        this.p = class Q3 {\n",
            "            constructor() {\n",
            "                this[_e] = 1;\n",
            "            }\n",
            "            static { _e = k, _f = j; }\n",
            "            static { this[_f] = 2; }\n",
            "        };\n",
            "        k = class Q7 {\n",
            "            constructor() {\n",
            "                this[_g] = 1;\n",
            "            }\n",
            "            static { _g = k, _h = j; }\n",
            "            static { this[_h] = 2; }\n",
            "        };\n",
            "    }\n",
            "}\n",
        )
    );
}

#[test]
fn a_lowered_class_expression_is_not_parenthesized_by_a_statement_like_tsgo() {
    // A class expression with static members becomes a comma sequence. The
    // printer writes the expression of `if`, `while`, `switch`, `case` and
    // `throw` with the lowest precedence (printer/printer.go emitIfStatement
    // 3465, emitWhileStatement 3509, emitSwitchStatement 3628-3638,
    // emitThrowStatement 3656, emitCaseClause 4471), so the sequence is not
    // parenthesized. tsc 6.0 parenthesized the expression of `switch` and
    // `case` in the factory, and the class fields transform of the port
    // parenthesized the others. The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "function f() {\n",
            "    if (class A { static x = 1; }) { }\n",
            "    while (class B { static x = 1; }) { break; }\n",
            "    switch (class C { static x = 1; }) { case class D { static x = 1; }: break; }\n",
            "    throw class H { static x = 1; };\n",
            "}\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2015","module":"esnext","outDir":"out","noCheck":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.js")).expect("read out/a.js"),
        concat!(
            "\"use strict\";\n",
            "function f() {\n",
            "    var _a, _b, _c, _d, _e;\n",
            "    if (_a = class A {\n",
            "        },\n",
            "        _a.x = 1,\n",
            "        _a) { }\n",
            "    while (_b = class B {\n",
            "        },\n",
            "        _b.x = 1,\n",
            "        _b) {\n",
            "        break;\n",
            "    }\n",
            "    switch (_c = class C {\n",
            "        },\n",
            "        _c.x = 1,\n",
            "        _c) {\n",
            "        case _d = class D {\n",
            "            },\n",
            "            _d.x = 1,\n",
            "            _d: break;\n",
            "    }\n",
            "    throw _e = class H {\n",
            "        },\n",
            "        _e.x = 1,\n",
            "        _e;\n",
            "}\n",
        )
    );
}

#[test]
fn an_async_generator_method_names_its_generator_after_its_name_node_like_tsgo() {
    // transformAsyncGeneratorFunctionBody names the inner generator
    // `getGeneratedNameForNode(node.name)` whatever the name is
    // (estransforms/forawait.go:803-806): an identifier gives `name_1`, a
    // computed, string or numeric name a temp. The port named the generator
    // only for an identifier. The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare let k: any;\n",
            "class C {\n",
            "    async *[Symbol.asyncIterator]() { yield 1; }\n",
            "    async *[\"lit\"]() { yield 1; }\n",
            "    async *[k]() { yield 1; }\n",
            "    async *named() { yield 1; }\n",
            "    async *1() { yield 1; }\n",
            "}\n",
            "const o = {\n",
            "    async *[Symbol.asyncIterator]() { yield 1; },\n",
            "    async *2() { yield 1; },\n",
            "};\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2017","module":"esnext","outDir":"out","noCheck":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.js")).expect("read out/a.js"),
        concat!(
            "\"use strict\";\n",
            "var __await = (this && this.__await) || function (v) { return this instanceof __await ? (this.v = v, this) : new __await(v); }\n",
            "var __asyncGenerator = (this && this.__asyncGenerator) || function (thisArg, _arguments, generator) {\n",
            "    if (!Symbol.asyncIterator) throw new TypeError(\"Symbol.asyncIterator is not defined.\");\n",
            "    var g = generator.apply(thisArg, _arguments || []), i, q = [];\n",
            "    return i = Object.create((typeof AsyncIterator === \"function\" ? AsyncIterator : Object).prototype), verb(\"next\"), verb(\"throw\"), verb(\"return\", awaitReturn), i[Symbol.asyncIterator] = function () { return this; }, i;\n",
            "    function awaitReturn(f) { return function (v) { return Promise.resolve(v).then(f, reject); }; }\n",
            "    function verb(n, f) { if (g[n]) { i[n] = function (v) { return new Promise(function (a, b) { q.push([n, v, a, b]) > 1 || resume(n, v); }); }; if (f) i[n] = f(i[n]); } }\n",
            "    function resume(n, v) { try { step(g[n](v)); } catch (e) { settle(q[0][3], e); } }\n",
            "    function step(r) { r.value instanceof __await ? Promise.resolve(r.value.v).then(fulfill, reject) : settle(q[0][2], r); }\n",
            "    function fulfill(value) { resume(\"next\", value); }\n",
            "    function reject(value) { resume(\"throw\", value); }\n",
            "    function settle(f, v) { if (f(v), q.shift(), q.length) resume(q[0][0], q[0][1]); }\n",
            "};\n",
            "class C {\n",
            "    [Symbol.asyncIterator]() { return __asyncGenerator(this, arguments, function* _a() { yield yield __await(1); }); }\n",
            "    [\"lit\"]() { return __asyncGenerator(this, arguments, function* _a() { yield yield __await(1); }); }\n",
            "    [k]() { return __asyncGenerator(this, arguments, function* _a() { yield yield __await(1); }); }\n",
            "    named() { return __asyncGenerator(this, arguments, function* named_1() { yield yield __await(1); }); }\n",
            "    1() { return __asyncGenerator(this, arguments, function* _a() { yield yield __await(1); }); }\n",
            "}\n",
            "const o = {\n",
            "    [Symbol.asyncIterator]() { return __asyncGenerator(this, arguments, function* _a() { yield yield __await(1); }); },\n",
            "    2() { return __asyncGenerator(this, arguments, function* _a() { yield yield __await(1); }); },\n",
            "};\n",
        )
    );
}

#[test]
fn an_async_generator_keeps_a_rest_parameter_on_the_outer_function_like_tsgo() {
    // isSimpleParameterList ignores the rest token
    // (estransforms/async.go:952-960): `...rest` stays on the outer function
    // and the generator closes over it. The port treated a rest parameter as
    // not simple and moved the parameters to the generator. The expected
    // bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "class C {\n",
            "    async *m(...rest: any[]) { yield rest; }\n",
            "    async *n(a: any, ...rest: any[]) { yield [a, rest]; }\n",
            "}\n",
            "async function* f(...rest: any[]) { yield rest; }\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2017","module":"esnext","outDir":"out","noCheck":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.js")).expect("read out/a.js"),
        concat!(
            "\"use strict\";\n",
            "var __await = (this && this.__await) || function (v) { return this instanceof __await ? (this.v = v, this) : new __await(v); }\n",
            "var __asyncGenerator = (this && this.__asyncGenerator) || function (thisArg, _arguments, generator) {\n",
            "    if (!Symbol.asyncIterator) throw new TypeError(\"Symbol.asyncIterator is not defined.\");\n",
            "    var g = generator.apply(thisArg, _arguments || []), i, q = [];\n",
            "    return i = Object.create((typeof AsyncIterator === \"function\" ? AsyncIterator : Object).prototype), verb(\"next\"), verb(\"throw\"), verb(\"return\", awaitReturn), i[Symbol.asyncIterator] = function () { return this; }, i;\n",
            "    function awaitReturn(f) { return function (v) { return Promise.resolve(v).then(f, reject); }; }\n",
            "    function verb(n, f) { if (g[n]) { i[n] = function (v) { return new Promise(function (a, b) { q.push([n, v, a, b]) > 1 || resume(n, v); }); }; if (f) i[n] = f(i[n]); } }\n",
            "    function resume(n, v) { try { step(g[n](v)); } catch (e) { settle(q[0][3], e); } }\n",
            "    function step(r) { r.value instanceof __await ? Promise.resolve(r.value.v).then(fulfill, reject) : settle(q[0][2], r); }\n",
            "    function fulfill(value) { resume(\"next\", value); }\n",
            "    function reject(value) { resume(\"throw\", value); }\n",
            "    function settle(f, v) { if (f(v), q.shift(), q.length) resume(q[0][0], q[0][1]); }\n",
            "};\n",
            "class C {\n",
            "    m(...rest) { return __asyncGenerator(this, arguments, function* m_1() { yield yield __await(rest); }); }\n",
            "    n(a, ...rest) { return __asyncGenerator(this, arguments, function* n_1() { yield yield __await([a, rest]); }); }\n",
            "}\n",
            "function f(...rest) { return __asyncGenerator(this, arguments, function* f_1() { yield yield __await(rest); }); }\n",
        )
    );
}

#[test]
fn two_copies_of_a_package_are_one_file_by_default_like_tsgo() {
    // Two installed copies of one package at one version are loaded once: the
    // second resolves to the file of the first (compiler/filesparser.go:361-368)
    // and its own text is never read. `b` therefore has the type the first
    // copy declares and `a(b)` checks, although the second copy declares a
    // different type. The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::create_dir_all(tree.path("node_modules/a")).expect("create node_modules/a");
    fs::write(
        tree.path("node_modules/a/index.d.ts"),
        concat!(
            "import value from \"x\";\n",
            "export function a(x: typeof value): void;\n",
        ),
    )
    .expect("write node_modules/a/index.d.ts");
    fs::create_dir_all(tree.path("node_modules/a/node_modules/x"))
        .expect("create node_modules/a/node_modules/x");
    fs::write(
        tree.path("node_modules/a/node_modules/x/index.d.ts"),
        concat!(
            "declare const value: { kind: \"a\" };\n",
            "export default value;\n",
        ),
    )
    .expect("write node_modules/a/node_modules/x/index.d.ts");
    fs::write(
        tree.path("node_modules/a/node_modules/x/package.json"),
        "{ \"name\": \"x\", \"version\": \"1.2.3\" }\n",
    )
    .expect("write node_modules/a/node_modules/x/package.json");
    fs::create_dir_all(tree.path("node_modules/b")).expect("create node_modules/b");
    fs::write(
        tree.path("node_modules/b/index.d.ts"),
        concat!(
            "import value from \"x\";\n",
            "export const b: typeof value;\n",
        ),
    )
    .expect("write node_modules/b/index.d.ts");
    fs::create_dir_all(tree.path("node_modules/b/node_modules/x"))
        .expect("create node_modules/b/node_modules/x");
    fs::write(
        tree.path("node_modules/b/node_modules/x/index.d.ts"),
        concat!(
            "declare const value: { kind: \"b\" };\n",
            "export default value;\n",
        ),
    )
    .expect("write node_modules/b/node_modules/x/index.d.ts");
    fs::write(
        tree.path("node_modules/b/node_modules/x/package.json"),
        "{ \"name\": \"x\", \"version\": \"1.2.3\" }\n",
    )
    .expect("write node_modules/b/node_modules/x/package.json");
    fs::create_dir_all(tree.path("src")).expect("create src");
    fs::write(
        tree.path("src/a.ts"),
        concat!(
            "import { a } from \"a\";\n",
            "import { b } from \"b\";\n",
            "a(b);\n",
        ),
    )
    .expect("write src/a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","noEmit":true,"moduleResolution":"bundler","strict":true},"files":["src/a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
}

#[test]
fn deduplicate_packages_false_keeps_each_copy_of_a_package_like_tsgo() {
    // `deduplicatePackages: false`, an option TypeScript 7 adds, turns the
    // package redirect off (compiler/filesparser.go:361-368): each copy is its
    // own file, `b` has the type its own copy declares, and `a(b)` is an
    // error. The port rejected the option as unknown. The expected bytes are
    // tsgo's.
    let tree = TempTree::new();
    fs::create_dir_all(tree.path("node_modules/a")).expect("create node_modules/a");
    fs::write(
        tree.path("node_modules/a/index.d.ts"),
        concat!(
            "import value from \"x\";\n",
            "export function a(x: typeof value): void;\n",
        ),
    )
    .expect("write node_modules/a/index.d.ts");
    fs::create_dir_all(tree.path("node_modules/a/node_modules/x"))
        .expect("create node_modules/a/node_modules/x");
    fs::write(
        tree.path("node_modules/a/node_modules/x/index.d.ts"),
        concat!(
            "declare const value: { kind: \"a\" };\n",
            "export default value;\n",
        ),
    )
    .expect("write node_modules/a/node_modules/x/index.d.ts");
    fs::write(
        tree.path("node_modules/a/node_modules/x/package.json"),
        "{ \"name\": \"x\", \"version\": \"1.2.3\" }\n",
    )
    .expect("write node_modules/a/node_modules/x/package.json");
    fs::create_dir_all(tree.path("node_modules/b")).expect("create node_modules/b");
    fs::write(
        tree.path("node_modules/b/index.d.ts"),
        concat!(
            "import value from \"x\";\n",
            "export const b: typeof value;\n",
        ),
    )
    .expect("write node_modules/b/index.d.ts");
    fs::create_dir_all(tree.path("node_modules/b/node_modules/x"))
        .expect("create node_modules/b/node_modules/x");
    fs::write(
        tree.path("node_modules/b/node_modules/x/index.d.ts"),
        concat!(
            "declare const value: { kind: \"b\" };\n",
            "export default value;\n",
        ),
    )
    .expect("write node_modules/b/node_modules/x/index.d.ts");
    fs::write(
        tree.path("node_modules/b/node_modules/x/package.json"),
        "{ \"name\": \"x\", \"version\": \"1.2.3\" }\n",
    )
    .expect("write node_modules/b/node_modules/x/package.json");
    fs::create_dir_all(tree.path("src")).expect("create src");
    fs::write(
        tree.path("src/a.ts"),
        concat!(
            "import { a } from \"a\";\n",
            "import { b } from \"b\";\n",
            "a(b);\n",
        ),
    )
    .expect("write src/a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","noEmit":true,"moduleResolution":"bundler","strict":true,"deduplicatePackages":false},"files":["src/a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 diagnostics"),
        concat!(
            "src/a.ts(3,3): error TS2345: Argument of type '{ kind: \"b\"; }' is not assignable to parameter of type '{ kind: \"a\"; }'.\n",
            "  Types of property 'kind' are incompatible.\n",
            "    Type '\"b\"' is not assignable to type '\"a\"'.\n",
        )
    );
}

/// The source `export const <name> = <expression>;` and the JavaScript tsgo
/// writes for it at ES2020 with ESNext modules: the same line.
fn deep_source(name: &str, expression: &str) -> String {
    format!("export const {name} = {expression};\n")
}

#[test]
fn a_string_concatenation_of_five_thousand_operands_compiles_like_tsgo() {
    // The parser, binder, checker and emitter recurse on the native stack in
    // proportion to the nesting depth of the source, and the command runs
    // them on threads with the compiler's stack reservation
    // (tsc_program::WORKER_STACK_BYTES); tsgo's Go stacks grow to 1 GB. The
    // emit used to refuse a source deeper than 256 nodes
    // (`emit transform AST depth above 256`), which failed the whole compile
    // for inputs tsgo compiles, such as this 4,999-operator chain
    // (binderBinaryExpressionStress has 4,954). The expected bytes are
    // tsgo's: the expression on one line, and the declaration.
    let expression = vec!["\"a\""; 5000].join(" + ");
    let source = deep_source("s", &expression);
    let tree = TempTree::new();
    fs::write(tree.path("a.ts"), &source).expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","outDir":"out","declaration":true,"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.js")).expect("read out/a.js"),
        source
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/a.d.ts")).expect("read out/a.d.ts"),
        "export declare const s: string;\n"
    );
}

#[test]
fn two_thousand_nested_calls_type_check_like_tsgo() {
    // `f(f(f(…)))` two thousand deep overflowed the main thread's 8 MiB stack
    // in the checker, which aborted the process; the command's work now runs
    // on a thread with the compiler's stack reservation. tsgo reports
    // nothing. The expected bytes are tsgo's.
    let expression = format!("{}1{}", "f(".repeat(2000), ")".repeat(2000));
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        format!(
            "declare function f(x: any): any;\n{}",
            deep_source("v", &expression)
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","noEmit":true,"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
}

#[test]
fn a_method_chain_of_a_thousand_calls_compiles_like_tsgo() {
    // A chain of property accesses and calls is the deepest common shape of
    // generated code (about 16 KiB of stack per level in the emit). The
    // expected bytes are tsgo's: the chain on one line, and `any` declared.
    let expression = format!("f{}", ".g()".repeat(1000));
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        format!("declare const f: any;\n{}", deep_source("c", &expression)),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","outDir":"out","declaration":true,"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.js")).expect("read out/a.js"),
        deep_source("c", &expression)
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/a.d.ts")).expect("read out/a.d.ts"),
        "export declare const c: any;\n"
    );
}

#[test]
fn two_thousand_nested_array_literals_compile_like_tsgo() {
    // The declaration of the nested arrays is as deep as the literal: the
    // node builder recurses once per level too. The expected bytes are
    // tsgo's.
    let expression = format!("{}1{}", "[".repeat(2000), "]".repeat(2000));
    let tree = TempTree::new();
    fs::write(tree.path("a.ts"), deep_source("o", &expression)).expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","outDir":"out","declaration":true,"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.js")).expect("read out/a.js"),
        deep_source("o", &expression)
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/a.d.ts")).expect("read out/a.d.ts"),
        format!("export declare const o: number{};\n", "[]".repeat(2000))
    );
}

#[test]
fn a_thousand_deep_object_literal_compiles_like_tsgo() {
    // Nested object literals are the deepest shape measured in the emit, and
    // their declaration is written one level per line. The expected bytes
    // are tsgo's.
    let depth = 1000;
    let expression = format!("{}1{}", "{ a: ".repeat(depth), " }".repeat(depth));
    let mut declaration = String::from("export declare const o: {\n");
    for level in 1..depth {
        declaration.push_str(&"    ".repeat(level));
        declaration.push_str("a: {\n");
    }
    declaration.push_str(&"    ".repeat(depth));
    declaration.push_str("a: number;\n");
    for level in (1..depth).rev() {
        declaration.push_str(&"    ".repeat(level));
        declaration.push_str("};\n");
    }
    declaration.push_str("};\n");
    let tree = TempTree::new();
    fs::write(tree.path("a.ts"), deep_source("o", &expression)).expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es2020"],"target":"es2020","module":"esnext","outDir":"out","declaration":true,"strict":true},"files":["a.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(tree.path("out/a.js")).expect("read out/a.js"),
        deep_source("o", &expression)
    );
    assert_eq!(
        fs::read_to_string(tree.path("out/a.d.ts")).expect("read out/a.d.ts"),
        declaration
    );
}

/// The `tsconfig.json` of the real-project tests: one checker (the sharded
/// resolution order differs), no libraries beyond `lib`, no emit.
fn no_emit_config(lib: &str, module: &str, files: &str) -> String {
    format!(
        r#"{{"compilerOptions":{{"types":[],"lib":[{lib}],"module":"{module}","noEmit":true,"strict":true}},"files":[{files}]}}"#
    )
}

#[test]
fn an_augmentation_of_a_module_that_is_not_imported_does_not_load_it_like_tsgo() {
    // tsgo resolves the name of a module augmentation but adds a file to the
    // program only for an import (compiler/fileloader.go:915-923): `./b`
    // exists and is not in the program, so the augmentation reports TS2664
    // and the error inside `b.ts` is never seen. The command used to fail
    // the program construction ("resolved non-JavaScript target ... has no
    // independent program membership"). The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "export {};\n",
            "declare module \"./b\" {\n",
            "    export const extra: number;\n",
            "}\n",
        ),
    )
    .expect("write a.ts");
    fs::write(tree.path("b.ts"), "export const original: string = 1;\n").expect("write b.ts");
    fs::write(
        tree.path("tsconfig.json"),
        no_emit_config("\"es2020\"", "esnext", "\"a.ts\""),
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "a.ts(2,16): error TS2664: Invalid module name in augmentation, module './b' cannot be found.\n"
    );
}

#[test]
fn an_ambient_module_imported_by_a_base_of_a_global_type_is_not_found_like_tsgo() {
    // tsgo's initializeChecker resolves the global types (`Function`) before
    // it merges the ambient modules of the files: `getDeclaredTypeOfSymbol`
    // of `Function` walks its base `N.I`, which resolves the import of the
    // ambient module `foo` while that module is not yet in the globals, so
    // TS2307 is reported for an import whose module is declared. The
    // expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("index.d.ts"),
        "declare module \"foo\" { namespace N { interface I { p(): any; } } export default N; }\n",
    )
    .expect("write index.d.ts");
    fs::write(
        tree.path("test.ts"),
        concat!(
            "import N from \"foo\";\n",
            "declare global { interface Function extends N.I {} }\n",
        ),
    )
    .expect("write test.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es6","dom"],"module":"node16","target":"es6","noEmit":true,"strict":true},"files":["index.d.ts","test.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "test.ts(1,15): error TS2307: Cannot find module 'foo' or its corresponding type declarations.\n"
    );
}

#[test]
fn the_bundled_library_files_are_named_and_ordered_like_tsgo() {
    // A diagnostic in a library file names the file `bundled:///libs/<lib>`
    // (tsgo's embedded library path, internal/bundled), which is not made
    // relative to the current directory, and the diagnostics sort by that
    // file name after the files of the project (`/...` sorts before
    // `bundled:`). The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare const Infinity: number;\n",
            "declare const NaN: number;\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        no_emit_config("\"es2020\"", "esnext", "\"a.ts\""),
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        concat!(
            "a.ts(1,15): error TS2451: Cannot redeclare block-scoped variable 'Infinity'.\n",
            "a.ts(2,15): error TS2451: Cannot redeclare block-scoped variable 'NaN'.\n",
            "bundled:///libs/lib.es5.d.ts(24,13): error TS2451: Cannot redeclare block-scoped variable 'NaN'.\n",
            "bundled:///libs/lib.es5.d.ts(25,13): error TS2451: Cannot redeclare block-scoped variable 'Infinity'.\n",
        )
    );
}

#[test]
fn missing_properties_name_two_alike_types_with_their_qualifiers_like_tsgo() {
    // Every missing-property message names its types through
    // getTypeNamesForErrorDisplay (relater.go:1270-1292): two types that
    // print alike are written fully qualified, for one missing property and
    // for several (TS2739), at the top of a chain and inside one. The port
    // qualified the one-property message only. The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "namespace N1 { export interface A { x: number; y: number; } }\n",
            "namespace N2 { export interface A { z: number; } }\n",
            "declare const a2: N2.A;\n",
            "const a1: N1.A = a2;\n",
            "declare function f(a: N1.A): void;\n",
            "f(a2);\n",
            "declare const b2: { inner: N2.A };\n",
            "const b1: { inner: N1.A } = b2;\n",
            "export {};\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        no_emit_config("\"es2020\"", "esnext", "\"a.ts\""),
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        concat!(
            "a.ts(4,7): error TS2739: Type 'N2.A' is missing the following properties from type 'N1.A': x, y\n",
            "a.ts(6,3): error TS2739: Type 'N2.A' is missing the following properties from type 'N1.A': x, y\n",
            "a.ts(8,7): error TS2322: Type '{ inner: N2.A; }' is not assignable to type '{ inner: N1.A; }'.\n",
            "  Types of property 'inner' are incompatible.\n",
            "    Type 'N2.A' is missing the following properties from type 'N1.A': x, y\n",
        )
    );
}

#[test]
fn a_this_property_tested_in_its_condition_body_is_not_always_defined_like_tsgo() {
    // isSymbolUsedInConditionBody compares the symbol of `this.q` in the
    // condition with the symbol of each `this.q` in the body: a use of the
    // same property in the body means the test is deliberate, so TS2774 is
    // not reported for `if (this.q) { this.q(); }` in an object literal typed
    // through ThisType. The expected bytes are tsgo's: no diagnostics.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare const o: { a(): void } & { b: number };\n",
            "if (o.a) { o.a(); }\n",
            "declare const p: { a(): void };\n",
            "if (p.a) { p.a(); }\n",
            "class C { m() {} n() { if (this.m) { this.m(); } } }\n",
            "declare function withThis<T>(o: T & ThisType<T & { z: number }>): void;\n",
            "withThis({ q() {}, r() { if (this.q) { this.q(); } } });\n",
            "declare function withThis2<T>(o: T & ThisType<T>): void;\n",
            "withThis2({ q() {}, r() { if (this.q) { this.q(); } } });\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        no_emit_config("\"es2020\"", "esnext", "\"a.ts\""),
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
}

#[test]
fn an_inherited_static_member_replaces_an_alias_member_like_tsgo() {
    // addInheritedMembers (checker.go:19935-19947) lets a base member replace
    // an own member that is not a value: the alias `export import Strategy =
    // Derived` of the namespace the class merges with yields to the base
    // class's static `Strategy`, so `Derived.Strategy` has the base's type.
    // tsc 6.0 replaced only a JavaScript expando assignment. The expected
    // bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare class Base {\n",
            "    static Strategy: { kind: \"base\" };\n",
            "    constructor(name: string);\n",
            "}\n",
            "declare class Derived extends Base {\n",
            "    constructor(name: string, extra: number);\n",
            "}\n",
            "declare namespace Derived {\n",
            "    export import Strategy = Derived;\n",
            "    interface Options { verify: boolean; }\n",
            "}\n",
            "const value: { kind: \"derived\" } = Derived.Strategy;\n",
            "export {};\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        no_emit_config("\"es2020\"", "esnext", "\"a.ts\""),
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        concat!(
            "a.ts(12,7): error TS2322: Type '{ kind: \"base\"; }' is not assignable to type '{ kind: \"derived\"; }'.\n",
            "  Types of property 'kind' are incompatible.\n",
            "    Type '\"base\"' is not assignable to type '\"derived\"'.\n",
        )
    );
}

#[test]
fn the_export_target_of_a_file_is_named_by_the_file_like_tsgo() {
    // getSpecifierForModuleSymbol (nodebuilderimpl.go:1249-1336) names the
    // module of `typeof import(...)` by its file: the namespace `P` that
    // `index.d.ts` exports with `export =` has that file
    // (getFileSymbolIfFileSymbolExportEqualsContainer) although its own
    // declarations are the namespace and the augmentation `declare module
    // "pkg"`, whose quoted name the port used to print. With no declaration in
    // scope the file name is written in full. The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::create_dir_all(tree.path("node_modules/pkg")).expect("create node_modules/pkg");
    fs::write(
        tree.path("node_modules/pkg/package.json"),
        "{ \"name\": \"pkg\", \"version\": \"1.0.0\", \"types\": \"index.d.ts\" }\n",
    )
    .expect("write package.json");
    fs::write(
        tree.path("node_modules/pkg/index.d.ts"),
        concat!(
            "export = P;\n",
            "declare namespace P {\n",
            "    const version: string;\n",
            "}\n",
        ),
    )
    .expect("write index.d.ts");
    fs::write(
        tree.path("a.ts"),
        concat!(
            "import * as P from \"pkg\";\n",
            "declare module \"pkg\" {\n",
            "    const extra: number;\n",
            "}\n",
            "export const m = P;\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("b.ts"),
        concat!("import { m } from \"./a\";\n", "const s: string = m;\n"),
    )
    .expect("write b.ts");
    fs::write(
        tree.path("tsconfig.json"),
        no_emit_config("\"es2020\"", "node16", "\"a.ts\",\"b.ts\""),
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    let root = compiler_current_directory(&tree);
    let root = root.to_string_lossy().replace('\\', "/");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!(
            "b.ts(2,7): error TS2322: Type 'typeof import(\"{root}/node_modules/pkg/index.d.ts\")' is not assignable to type 'string'.\n"
        )
    );
}

#[test]
fn the_length_estimate_of_methods_elides_members_like_tsgo() {
    // The node builder's length estimate decides where a long type is
    // truncated: a signature adds 3 (signatureToSignatureDeclarationHelper)
    // and each parameter its name plus 3 (symbolToParameterDeclaration), so
    // the estimate of an object type with eight long methods passes the
    // budget inside the member list and the members are elided (`... 5 more
    // ...`); the port's smaller estimate let the whole string be cut at its
    // end instead. The expected bytes are tsgo's.
    let members = (0..8)
        .map(|i| {
            format!(
                "method{i}(alphaParameter{i}: string, betaParameter{i}: number, gammaParameter{i}: boolean): void"
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        format!("declare const big: {{ {members} }};\nconst s: string = big;\nexport {{}};\n"),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        no_emit_config("\"es2020\"", "esnext", "\"a.ts\""),
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "a.ts(2,7): error TS2322: Type '{ method0(alphaParameter0: string, betaParameter0: number, gammaParameter0: boolean): void; method1(alphaParameter1: string, betaParameter1: number, gammaParameter1: boolean): void; ... 5 more ...; method7(alphaParameter7: string, betaParameter7: number, gammaParameter7: boolean): void; }' is not assignable to type 'string'.\n"
    );
}

#[test]
fn one_diagnostic_per_non_identical_inherited_property_like_tsgo() {
    // checkInheritedPropertiesAreIdentical reports one TS2320 per property
    // that two base types declare differently, and the diagnostics differ
    // only in their chains: tsgo's deduplication compares the whole chain
    // (EqualDiagnosticsNoRelatedInfo), so all three are kept, where tsc 6.0
    // compared the head text and kept the first. The expected bytes are
    // tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.d.ts"),
        concat!(
            "export {};\n",
            "interface Event { readonly type: string; }\n",
            "interface EventListener { (evt: Event): void; }\n",
            "interface EventListenerObject { handleEvent(object: Event): void; }\n",
            "interface EventListenerOptions { capture?: boolean; }\n",
            "interface AddEventListenerOptions extends EventListenerOptions { once?: boolean; }\n",
            "interface EventTarget {\n",
            "    addEventListener(type: string, listener: EventListener | EventListenerObject, options?: AddEventListenerOptions | boolean): void;\n",
            "    dispatchEvent(event: Event): boolean;\n",
            "    removeEventListener(type: string, listener: EventListener | EventListenerObject, options?: EventListenerOptions | boolean): void;\n",
            "}\n",
            "interface AbortSignalEventMap { \"abort\": Event; }\n",
            "type InternalEventTargetEventProperties<T> = { [K in keyof T & string as `on${K}`]: ((ev: T[K]) => void) | null; };\n",
            "type _AbortSignal = typeof globalThis extends { onmessage: any } ? {} : AbortSignal;\n",
            "interface AbortSignal extends EventTarget, InternalEventTargetEventProperties<AbortSignalEventMap> {\n",
            "    readonly aborted: boolean;\n",
            "    readonly reason: any;\n",
            "    throwIfAborted(): void;\n",
            "    addEventListener<K extends keyof AbortSignalEventMap>(type: K, listener: (ev: AbortSignalEventMap[K]) => void, options?: AddEventListenerOptions | boolean): void;\n",
            "    addEventListener(type: string, listener: EventListener | EventListenerObject, options?: AddEventListenerOptions | boolean): void;\n",
            "    removeEventListener<K extends keyof AbortSignalEventMap>(type: K, listener: (ev: AbortSignalEventMap[K]) => void, options?: EventListenerOptions | boolean): void;\n",
            "    removeEventListener(type: string, listener: EventListener | EventListenerObject, options?: EventListenerOptions | boolean): void;\n",
            "}\n",
            "declare global {\n",
            "    interface AbortSignal extends _AbortSignal {}\n",
            "}\n",
        ),
    )
    .expect("write a.d.ts");
    fs::write(
        tree.path("b.d.ts"),
        concat!(
            "export {};\n",
            "interface Event { readonly type: string; }\n",
            "interface EventListener { (evt: Event): void; }\n",
            "interface EventListenerObject { handleEvent(object: Event): void; }\n",
            "interface EventListenerOptions { capture?: boolean; }\n",
            "interface AddEventListenerOptions extends EventListenerOptions { once?: boolean; }\n",
            "interface EventTarget {\n",
            "    addEventListener(type: string, listener: EventListener | EventListenerObject, options?: AddEventListenerOptions | boolean): void;\n",
            "    dispatchEvent(event: Event): boolean;\n",
            "    removeEventListener(type: string, listener: EventListener | EventListenerObject, options?: EventListenerOptions | boolean): void;\n",
            "}\n",
            "type _AbortSignal = typeof globalThis extends { onmessage: any } ? {} : AbortSignal;\n",
            "interface AbortSignal extends EventTarget {\n",
            "    readonly aborted: boolean;\n",
            "    onabort: ((this: AbortSignal, ev: Event) => any) | null;\n",
            "    readonly reason: any;\n",
            "    throwIfAborted(): void;\n",
            "}\n",
            "declare global {\n",
            "    interface AbortSignal extends _AbortSignal {}\n",
            "}\n",
        ),
    )
    .expect("write b.d.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es6"],"module":"node16","target":"es6","noEmit":true,"strict":true},"files":["a.d.ts","b.d.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        concat!(
            "a.d.ts(25,15): error TS2320: Interface 'AbortSignal' cannot simultaneously extend types 'AbortSignal' and 'AbortSignal'.\n",
            "  Named property 'addEventListener' of types 'AbortSignal' and 'AbortSignal' are not identical.\n",
            "a.d.ts(25,15): error TS2320: Interface 'AbortSignal' cannot simultaneously extend types 'AbortSignal' and 'AbortSignal'.\n",
            "  Named property 'onabort' of types 'AbortSignal' and 'AbortSignal' are not identical.\n",
            "a.d.ts(25,15): error TS2320: Interface 'AbortSignal' cannot simultaneously extend types 'AbortSignal' and 'AbortSignal'.\n",
            "  Named property 'removeEventListener' of types 'AbortSignal' and 'AbortSignal' are not identical.\n",
        )
    );
}

#[test]
fn a_value_used_as_the_base_of_an_interface_suggests_typeof_like_tsgo() {
    // tsgo's parser turns the entity name of an interface `extends` element
    // into a qualified name (parser.go parseTypeHeritageClauseElement), so
    // resolveQualifiedName's `typeof` suggestion applies to it: `q.v` is a
    // value, and TS2749 is reported on the whole name, as for the type
    // annotation. The port kept the property access expression and reported
    // TS2694 on `v`. The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "declare namespace q { const v: { p: number }; }\n",
            "interface I extends q.v {}\n",
            "declare const x: q.v;\n",
            "export {};\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        no_emit_config("\"es2020\"", "esnext", "\"a.ts\""),
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        concat!(
            "a.ts(2,21): error TS2749: 'q.v' refers to a value, but is being used as a type here. Did you mean 'typeof q.v'?\n",
            "a.ts(3,18): error TS2749: 'q.v' refers to a value, but is being used as a type here. Did you mean 'typeof q.v'?\n",
        )
    );
}

#[test]
fn a_global_interface_whose_base_is_the_type_of_its_own_variable_follows_tsgo() {
    // DefinitelyTyped's `@types/node` web globals: `interface Console extends
    // console.Console {}` with `var console: Console` in the same global
    // block, where `console` is also the namespace import of a module whose
    // `export =` is `globalThis.console`. Resolving the base reaches the
    // declared type of `Console` while it is being computed — tsgo publishes
    // the shell of a class or interface type before its type parameters
    // (getDeclaredTypeOfClassOrInterface), where the port asserted — and
    // the `typeof` suggestion walks the value `console.Console` through the
    // global variable (tryGetQualifiedNameAsValue), which re-enters the base
    // types and reports the circularity at both declarations. The expected
    // bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("b.d.ts"),
        concat!(
            "declare module \"node:console\" {\n",
            "    global {\n",
            "        interface Console {\n",
            "            Console: console.ConsoleConstructor;\n",
            "            log(...data: any[]): void;\n",
            "        }\n",
            "        namespace console {\n",
            "            interface ConsoleConstructor {\n",
            "                prototype: Console;\n",
            "                new(): Console;\n",
            "            }\n",
            "        }\n",
            "        var console: Console;\n",
            "    }\n",
            "    export = globalThis.console;\n",
            "}\n",
        ),
    )
    .expect("write b.d.ts");
    fs::write(
        tree.path("c.d.ts"),
        concat!(
            "export {};\n",
            "\n",
            "import * as console from \"node:console\";\n",
            "\n",
            "declare global {\n",
            "    interface Console extends console.Console {}\n",
            "\n",
            "    var console: Console;\n",
            "}\n",
        ),
    )
    .expect("write c.d.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es6"],"module":"node16","target":"es6","noEmit":true,"strict":true},"files":["b.d.ts","c.d.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        concat!(
            "b.d.ts(3,19): error TS2310: Type 'Console' recursively references itself as a base type.\n",
            "c.d.ts(6,15): error TS2310: Type 'Console' recursively references itself as a base type.\n",
            "c.d.ts(6,31): error TS2749: 'console.Console' refers to a value, but is being used as a type here. Did you mean 'typeof console.Console'?\n",
        )
    );
}

#[test]
fn two_copies_of_a_module_with_export_assignments_resolve_a_qualified_value_like_tsgo() {
    // Two `@types/node` versions in one program declare `node:console` twice,
    // each with an `export =`: tsgo reports the duplicate assignments and,
    // resolving the base `console.Console` of the global `Console`, follows
    // the last assignment to the global variable (getDeclarationOfAliasSymbol
    // is the last alias declaration) and suggests `typeof console.Console`.
    // The expected bytes are tsgo's.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.d.ts"),
        concat!(
            "declare module \"node:console\" {\n",
            "    namespace console {\n",
            "        interface Console {\n",
            "            readonly Console: {\n",
            "                prototype: Console;\n",
            "                new(): Console;\n",
            "            };\n",
            "            log(...data: any[]): void;\n",
            "        }\n",
            "    }\n",
            "    var console: console.Console;\n",
            "    export = console;\n",
            "}\n",
        ),
    )
    .expect("write a.d.ts");
    fs::write(
        tree.path("b.d.ts"),
        concat!(
            "declare module \"node:console\" {\n",
            "    global {\n",
            "        interface Console {\n",
            "            Console: console.ConsoleConstructor;\n",
            "            log(...data: any[]): void;\n",
            "        }\n",
            "        namespace console {\n",
            "            interface ConsoleConstructor {\n",
            "                prototype: Console;\n",
            "                new(): Console;\n",
            "            }\n",
            "        }\n",
            "        var console: Console;\n",
            "    }\n",
            "    export = globalThis.console;\n",
            "}\n",
        ),
    )
    .expect("write b.d.ts");
    fs::write(
        tree.path("c.d.ts"),
        concat!(
            "export {};\n",
            "\n",
            "import * as console from \"node:console\";\n",
            "\n",
            "declare global {\n",
            "    interface Console extends console.Console {}\n",
            "\n",
            "    var console: Console;\n",
            "}\n",
        ),
    )
    .expect("write c.d.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"types":[],"lib":["es6"],"module":"node16","target":"es6","noEmit":true,"strict":true},"files":["a.d.ts","b.d.ts","c.d.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        concat!(
            "a.d.ts(12,14): error TS2300: Duplicate identifier 'export='.\n",
            "b.d.ts(3,19): error TS2310: Type 'Console' recursively references itself as a base type.\n",
            "b.d.ts(15,5): error TS2300: Duplicate identifier 'export='.\n",
            "c.d.ts(6,15): error TS2310: Type 'Console' recursively references itself as a base type.\n",
            "c.d.ts(6,31): error TS2749: 'console.Console' refers to a value, but is being used as a type here. Did you mean 'typeof console.Console'?\n",
        )
    );
}

#[test]
fn a_re_entered_base_constraint_is_circular_every_time_like_tsgo() {
    // While the constraint of `infer P` is inferred from
    // `DefineComponent<infer P>`, the distributive conditional
    // `ExtractDefaultPropTypes<P>` compares `P` with `object` (its
    // substitution `P & object`) several times, and each comparison asks for
    // the base constraint of `P`, whose resolution is in progress. tsgo's
    // getResolvedBaseConstraint returns the circular sentinel for a
    // re-entered resolution without caching it, so every request finds the
    // cycle again and the conditional type's constraint resolves to the
    // sentinel; its mapped type's parameter `K` is never asked for its own
    // constraint. tsc 6.0 cached the first sentinel, answered the later
    // requests from the cache, and so did the port, which went on to resolve
    // `K` inside the cycle and reported TS2313 on it (DefinitelyTyped's
    // `vue-writer`, `vue-draggable-resizable` and `vue3-carousel-3d` through
    // `@vue/runtime-core`). The expected bytes are tsgo's: no diagnostics.
    let tree = TempTree::new();
    fs::write(
        tree.path("a.ts"),
        concat!(
            "type DefaultKeys<T> = { [K in keyof T]: T[K] extends { default: any } ? T[K] extends { required: true } ? never : K : never }[keyof T];\n",
            "type ExtractDefaultPropTypes<O> = O extends object ? { [K in keyof Pick<O, DefaultKeys<O>>]: O[K] } : {};\n",
            "type EnsureNonVoid<T> = T extends void ? never : T;\n",
            "type DefineComponent<Props = {}, Mixin = unknown, Defaults = ExtractDefaultPropTypes<Props>> = Readonly<{ base: number } & EnsureNonVoid<Defaults>>;\n",
            "declare function define<T>(options: T): T extends DefineComponent<infer P> ? P : unknown;\n",
            "export {};\n",
        ),
    )
    .expect("write a.ts");
    fs::write(
        tree.path("tsconfig.json"),
        no_emit_config("\"es2020\"", "esnext", "\"a.ts\""),
    )
    .expect("write tsconfig.json");
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
}

#[test]
fn a_file_imported_three_times_is_explained_by_each_import_like_tsgo() {
    // tsgo adds the resolved file to the program once for every import of it
    // (processImportedModules, compiler/fileloader.go:928-940), so the file's
    // explanation lists one "Imported via" line per occurrence, and TS6059
    // keeps the explanation because the file has more than one reason. The
    // command listed one line per specifier. The expected bytes are tsgo's
    // (material-ui `scripts/buildLlmsDocs`: a value and a type re-export of
    // one specifier).
    let tree = TempTree::new();
    fs::create_dir_all(tree.path("src")).expect("create src");
    fs::write(
        tree.path("src/index.ts"),
        concat!(
            "export { create, build } from '../shared';\n",
            "export type { Options } from '../shared';\n",
            "import('../shared');\n",
        ),
    )
    .expect("write index.ts");
    fs::write(
        tree.path("shared.ts"),
        concat!(
            "export function create(): number { return 1; }\n",
            "export function build(): number { return 2; }\n",
            "export type Options = { a: number };\n",
        ),
    )
    .expect("write shared.ts");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"rootDir":"src","outDir":"out","module":"esnext","target":"es2020","lib":["es2020"],"types":[],"noEmit":true},"files":["src/index.ts"]}"#,
    )
    .expect("write tsconfig.json");
    let project = compiler_current_directory(&tree);
    let project = project.display();
    let output = run(&tree, &["--pretty", "false"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!(
            concat!(
                "src/index.ts(1,31): error TS6059: File '{project}/shared.ts' is not under 'rootDir' '{project}/src'. 'rootDir' is expected to contain all source files.\n",
                "  The file is in the program because:\n",
                "    Imported via '../shared' from file '{project}/src/index.ts'\n",
                "    Imported via '../shared' from file '{project}/src/index.ts'\n",
                "    Imported via '../shared' from file '{project}/src/index.ts'\n",
            ),
            project = project,
        )
    );
}

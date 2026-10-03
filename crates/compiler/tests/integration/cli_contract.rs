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

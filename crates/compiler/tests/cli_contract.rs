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
    Command::new(env!("CARGO_BIN_EXE_tsc-rs"))
        .current_dir(&tree.root)
        .args(arguments)
        .output()
        .expect("run tsc-rs binary")
}

fn run_typescript(tree: &TempTree, arguments: &[&str]) -> std::process::Output {
    let bundle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("vendor/typescript-6.0.3/lib/_tsc.js");
    Command::new("node")
        .current_dir(&tree.root)
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

fn assert_typescript_parity(tree: &TempTree, rust_arguments: &[&str], ts_arguments: &[&str]) {
    let rust = run(tree, rust_arguments);
    let typescript = run_typescript(tree, ts_arguments);
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
fn command_line_no_emit_overrides_the_config_and_missing_override_fails_closed() {
    let tree = TempTree::new();
    fs::write(tree.path("main.ts"), "const value: number = 1;\n").expect("write source");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"lib":["es5"]},"files":["main.ts"]}"#,
    )
    .expect("write config");

    let without_override = run(&tree, &["-p", "tsconfig.json"]);
    assert_eq!(without_override.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&without_override.stderr).contains("noEmit"));

    let with_override = run(&tree, &["--noEmit", "-p", "tsconfig.json"]);
    assert_eq!(with_override.status.code(), Some(0));
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
fn config_option_diagnostics_are_rendered_alongside_semantic_diagnostics() {
    let tree = TempTree::new();
    fs::write(tree.path("main.ts"), "const value: number = 'wrong';\n").expect("write source");
    fs::write(
        tree.path("tsconfig.json"),
        r#"{"compilerOptions":{"noEmit":true,"lib":["es5"],"moduleResolution":"node"},"files":["main.ts"]}"#,
    )
    .expect("write config");

    let output = run(&tree, &["--pretty=false"]);
    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let option = stdout
        .find("tsconfig.json(1,1): error TS5107:")
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

    let plain = run(&tree, &["--pretty=false"]);
    assert_eq!(plain.status.code(), Some(2));
    let plain_stdout = String::from_utf8_lossy(&plain.stdout);
    assert!(plain_stdout.starts_with("main.ts(1,7): error TS2322:"));
    assert!(!plain_stdout.contains('~'));

    let pretty = run(&tree, &["--pretty=true"]);
    assert_eq!(pretty.status.code(), Some(2));
    let pretty_stdout = String::from_utf8_lossy(&pretty.stdout);
    let pretty_text = strip_ansi_sgr(&pretty_stdout);
    assert!(pretty_text.contains("main.ts:1:7 - error TS2322:"));
    assert!(pretty_text.contains('~'));
    assert!(pretty_text.contains("Found 1 error in main.ts:1"));
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

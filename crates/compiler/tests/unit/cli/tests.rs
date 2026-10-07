use super::*;

fn run(arguments: &[&str]) -> CliOutput {
    run_cli(
        &arguments
            .iter()
            .map(|argument| (*argument).to_owned())
            .collect::<Vec<_>>(),
    )
}

#[test]
fn command_line_errors_are_tsgo_diagnostics_with_exit_status_one() {
    // tsgo has no `--name=value` form: the whole token is an unknown option
    // (too long for a spelling suggestion here).
    let output = run(&["--noEmit=false", "--pretty", "false"]);
    assert_eq!(output.exit_code(), EXIT_COMMAND_LINE);
    assert!(output.stderr().is_empty());
    assert_eq!(
        output.stdout(),
        "error TS5023: Unknown compiler option '--noEmit=false'.\n"
    );
    let output = run(&["--strct", "--pretty", "false"]);
    assert_eq!(output.exit_code(), EXIT_COMMAND_LINE);
    assert_eq!(
        output.stdout(),
        "error TS5025: Unknown compiler option '--strct'. Did you mean 'strict'?\n"
    );
    let output = run(&["--target", "unknown-target", "--pretty", "false"]);
    assert_eq!(output.exit_code(), EXIT_COMMAND_LINE);
    assert!(output
        .stdout()
        .starts_with("error TS6046: Argument for '--target' option must be: "));
    let output = run(&["--bogus", "--pretty", "false"]);
    assert_eq!(output.exit_code(), EXIT_COMMAND_LINE);
    assert_eq!(
        output.stdout(),
        "error TS5023: Unknown compiler option '--bogus'.\n"
    );
    // Unsupported modes stay usage errors of the port.
    let output = run(&["--watch"]);
    assert_eq!(output.exit_code(), EXIT_FAILURE);
    assert!(output.stderr().contains("unsupported option \"--watch\""));
}

#[test]
fn boolean_switches_consume_separate_values_without_turning_them_into_roots() {
    let parsed = parse_command_line(
        &[
            "--noEmit".to_owned(),
            "true".to_owned(),
            "--ignoreConfig".to_owned(),
            "true".to_owned(),
            "--pretty".to_owned(),
            "false".to_owned(),
            "main.ts".to_owned(),
        ],
        "/work".into(),
        true,
        &|_| None,
    );
    assert!(parsed.errors.is_empty());
    assert_eq!(parsed.option_bool("noEmit"), Some(true));
    assert_eq!(parsed.option_bool("ignoreConfig"), Some(true));
    assert_eq!(parsed.option_bool("pretty"), Some(false));
    assert_eq!(parsed.file_names, ["main.ts"]);
}

#[test]
fn explicit_emit_reports_a_missing_root_after_profile_selection() {
    let output = run(&[
        "--ignoreConfig",
        "--target",
        "esnext",
        "--module",
        "preserve",
        "--noLib",
        "missing.ts",
    ]);
    assert_eq!(output.exit_code(), EXIT_FAILURE);
    assert!(output.stderr().is_empty());
    assert!(output.stdout().contains("TS6053"));
}

#[test]
fn version_is_available_without_a_filesystem_host() {
    let output = run(&["--version"]);
    assert_eq!(output.exit_code(), EXIT_SUCCESS);
    assert!(output.stderr().is_empty());
    assert_eq!(
        output.stdout().trim(),
        format!("Version {TYPESCRIPT_VERSION}")
    );
}

#[test]
fn embedded_library_overlay_owns_the_pinned_catalog_bytes() {
    let filesystem = FsCompilerHost::from_process().expect("construct filesystem host");
    let host = CliCompilerHost::new(filesystem);
    assert_eq!(embedded_libraries::EMBEDDED_LIBRARIES.len(), 113);
    // tsgo's embedded library path (internal/bundled): the diagnostics name
    // a library file `bundled:///libs/<lib>`.
    assert_eq!(
        host.library_directory(),
        Path::new("bundled:///libs"),
        "the embedded library directory is tsgo's bundled path"
    );

    let embedded_path = host.library_directory().join("lib.es5.d.ts");
    let embedded = host
        .read_file(&embedded_path)
        .expect("read embedded library")
        .expect("embedded ES5 library exists");
    let vendored = std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../../vendor/typescript-native/7.1.0-dev-19dadef8/upstream/tsc/internal/bundled/libs/lib.es5.d.ts",
    ))
    .expect("read pinned ES5 library");
    assert!(host
        .file_exists(&host.library_directory().join("lib.es2026.full.d.ts"))
        .expect("query embedded ES2026 library"));
    assert_eq!(embedded, vendored);
    assert!(host
        .file_exists(&embedded_path)
        .expect("query embedded library"));
    assert!(!host
        .file_exists(&host.library_directory().join("lib.unknown.d.ts"))
        .expect("query absent embedded library"));
    assert_eq!(
        host.read_directory(host.library_directory())
            .expect("list embedded library directory")
            .len(),
        113
    );
}

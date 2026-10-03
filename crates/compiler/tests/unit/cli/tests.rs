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
fn argument_parser_selects_emit_and_the_admitted_target_ladder() {
    assert_eq!(
        parse_arguments(&["--noEmit=false".to_owned()])
            .expect("explicit false selects emit")
            .compiler_options
            .no_emit,
        Some(false)
    );
    assert!(matches!(
        parse_arguments(&["--watch".to_owned()]),
        Err(CliError::Usage(_))
    ));
    assert_eq!(
        parse_arguments(&["--target=es2015".to_owned()])
            .expect("the current H2 downlevel target is admitted")
            .compiler_options
            .target,
        Some(2)
    );
    assert_eq!(
        parse_arguments(&["--target=latest".to_owned()])
            .expect("the tsc latest alias is admitted")
            .compiler_options
            .target,
        Some(99)
    );
    assert_eq!(
        parse_arguments(&["--target=es5".to_owned()])
            .expect("the shared target catalog admits ES5")
            .compiler_options
            .target,
        Some(1)
    );
    assert!(matches!(
        parse_arguments(&["--target=unknown-target".to_owned()]),
        Err(CliError::Usage(message)) if message.contains("--target has an unknown value")
    ));
}

#[test]
fn boolean_switches_consume_separate_values_without_turning_them_into_roots() {
    let parsed = parse_arguments(&[
        "--noEmit".to_owned(),
        "true".to_owned(),
        "--ignoreConfig".to_owned(),
        "true".to_owned(),
        "--pretty".to_owned(),
        "false".to_owned(),
        "main.ts".to_owned(),
    ])
    .expect("separate boolean values are accepted");
    assert_eq!(parsed.compiler_options.no_emit, Some(true));
    assert!(parsed.ignore_config);
    assert_eq!(parsed.pretty, Some(false));
    assert_eq!(parsed.files, [PathBuf::from("main.ts")]);
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
    let current_directory = filesystem
        .current_directory()
        .expect("read current directory");
    let host = CliCompilerHost::new(filesystem, &current_directory);
    assert_eq!(embedded_libraries::EMBEDDED_LIBRARIES.len(), 113);

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

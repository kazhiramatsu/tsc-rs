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

/// tsgo `--pprofDir`: the run's CPU profile, its path after the run's
/// output (tsgo's memory profile is not supported).
#[cfg(unix)]
#[test]
fn pprof_dir_writes_the_runs_cpu_profile() {
    let _profiler = crate::pprof::TEST_PROFILER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let directory = std::env::temp_dir()
        .join(format!("tsc-rs-pprof-dir-test-{}", std::process::id()))
        .to_string_lossy()
        .into_owned();
    let output = run(&["--pprofDir", &directory, "--version"]);
    assert_eq!(output.exit_code(), EXIT_SUCCESS);
    let profile = format!("{directory}/{}-cpuprofile.pb.gz", std::process::id());
    assert_eq!(
        output.stdout(),
        format!("Version {TYPESCRIPT_VERSION}\nCPU profile: {profile}\n")
    );
    assert!(std::path::Path::new(&profile).exists());
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn embedded_library_overlay_owns_the_pinned_catalog_bytes() {
    let system = NativeSystem::from_process().expect("the process system");
    let host = system.compiler_host();
    // tsgo's embedded library path (internal/bundled): the diagnostics name
    // a library file `bundled:///libs/<lib>`.
    assert_eq!(
        system.default_library_path(),
        "bundled:///libs",
        "the embedded library directory is tsgo's bundled path"
    );
    let library_directory = Path::new(system.default_library_path());

    let embedded_path = library_directory.join("lib.es5.d.ts");
    let embedded = host
        .read_file(&embedded_path)
        .expect("read embedded library")
        .expect("embedded ES5 library exists");
    let vendored = std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../../vendor/typescript-native/7.1.0-dev-aa814927/upstream/tsc/internal/bundled/libs/lib.es5.d.ts",
    ))
    .expect("read pinned ES5 library");
    assert!(host
        .file_exists(&library_directory.join("lib.es2026.full.d.ts"))
        .expect("query embedded ES2026 library"));
    assert_eq!(embedded, vendored);
    assert!(host
        .file_exists(&embedded_path)
        .expect("query embedded library"));
    assert!(!host
        .file_exists(&library_directory.join("lib.unknown.d.ts"))
        .expect("query absent embedded library"));
    assert_eq!(
        host.read_directory(library_directory)
            .expect("list embedded library directory")
            .len(),
        116
    );
}

#[test]
fn checker_command_counts_clamp_to_one_instead_of_selecting_the_default() {
    assert_eq!(CommandBudgets::of(None, None).checkers, None);
    for (requested, expected) in [(-2.0, 1), (0.0, 1), (1.0, 1), (2.0, 2), (8.0, 8)] {
        assert_eq!(
            CommandBudgets::of(None, Some(requested))
                .checkers
                .unwrap()
                .get(),
            expected
        );
    }
    assert!(CommandBudgets::of(Some(true), Some(8.0)).single_threaded);
}

#[test]
fn checker_defaults_environment_and_option_precedence_follow_tsgo() {
    /// A checker budget reads two variables: the count and the fast preset.
    struct BudgetSystem {
        checkers: Option<&'static str>,
        fast: Option<&'static str>,
    }
    impl System for BudgetSystem {
        fn fs(&self) -> &dyn tsc_host::vfs::FileSystem {
            panic!("a checker budget does not read files")
        }
        fn current_directory(&self) -> &str {
            "/"
        }
        fn default_library_path(&self) -> &str {
            "/lib"
        }
        fn now(&self) -> std::time::SystemTime {
            std::time::SystemTime::UNIX_EPOCH
        }
        fn since_start(&self) -> std::time::Duration {
            std::time::Duration::ZERO
        }
        fn env_var(&self, name: &str) -> Option<String> {
            match name {
                CHECKERS_ENV => self.checkers,
                FAST_ENV => self.fast,
                other => panic!("a checker budget does not read {other}"),
            }
            .map(str::to_owned)
        }
        fn output_is_terminal(&self) -> bool {
            false
        }
        fn terminal_width(&self) -> Option<usize> {
            None
        }
        fn write_output(&self, _: &str) {}
        fn write_error(&self, _: &str) {}
    }
    let tsgo_rows = [
        (None, None, false, 4),
        (Some("bad"), None, false, 4),
        (Some("0"), None, false, 1),
        (Some("-2"), None, false, 1),
        (Some("2"), None, false, 2),
        (Some("8"), Some(2.0), false, 2),
        (Some("8"), Some(0.0), false, 1),
        (Some("8"), Some(8.0), true, 1),
        (Some("1000"), None, false, 256),
    ];
    // Without the preset (the variable unset, or holding anything but `1`)
    // the pool is tsgo's: its counts and its file association.
    for fast in [None, Some(""), Some("0"), Some("true")] {
        for (environment, explicit, single, expected) in tsgo_rows {
            let budget = CommandBudgets::of(Some(single), explicit).checker_budget(&BudgetSystem {
                checkers: environment,
                fast,
            });
            assert_eq!(
                budget.checkers(),
                expected,
                "environment={environment:?}, explicit={explicit:?}, single={single}, fast={fast:?}"
            );
            assert_eq!(
                budget.assignment(),
                CheckerAssignment::Tsgo,
                "environment={environment:?}, explicit={explicit:?}, single={single}, fast={fast:?}"
            );
        }
    }
    // The preset keeps a requested count and changes the association; with
    // no count its pool follows the Program, up to twelve checkers.
    // `--singleThreaded` is the serial mode with or without it.
    for fast in ["1", " 1 "] {
        for (environment, explicit, single, expected, assignment) in [
            (None, None, false, 12, CheckerAssignment::Locality),
            (Some("bad"), None, false, 12, CheckerAssignment::Locality),
            (Some("2"), None, false, 2, CheckerAssignment::Locality),
            (Some("8"), Some(2.0), false, 2, CheckerAssignment::Locality),
            (Some("1000"), None, false, 256, CheckerAssignment::Locality),
            (Some("8"), Some(8.0), true, 1, CheckerAssignment::Tsgo),
            (None, None, true, 1, CheckerAssignment::Tsgo),
        ] {
            let budget = CommandBudgets::of(Some(single), explicit).checker_budget(&BudgetSystem {
                checkers: environment,
                fast: Some(fast),
            });
            assert_eq!(
                budget.checkers(),
                expected,
                "environment={environment:?}, explicit={explicit:?}, single={single}, fast={fast:?}"
            );
            assert_eq!(
                budget.assignment(),
                assignment,
                "environment={environment:?}, explicit={explicit:?}, single={single}, fast={fast:?}"
            );
        }
    }
}

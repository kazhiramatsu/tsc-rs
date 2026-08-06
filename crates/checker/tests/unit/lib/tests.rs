use super::*;

#[test]
fn checker_input_and_source_file_share_the_exact_snapshot_arc() {
    let input = InputFile::new("main.ts", "const value = '😀';\n");
    let source = tsc_syntax::parse_source_file_from_snapshot(
        input.name.clone(),
        Arc::clone(input.snapshot()),
        tsc_syntax::ParseOptions::default(),
        None,
    );

    assert!(Arc::ptr_eq(input.snapshot(), source.snapshot()));
    assert!(Arc::ptr_eq(
        &input.snapshot().shared_text(),
        &source.snapshot().shared_text(),
    ));
    assert!(Arc::ptr_eq(
        &input.snapshot().shared_positions(),
        &source.snapshot().shared_positions(),
    ));
}

fn assert_one_cached_library_saved_work(
    owned: CheckWorkCounters,
    cached: CheckWorkCounters,
    _library_bytes: usize,
) {
    assert_eq!(owned.parsed_documents(), cached.parsed_documents() + 1);
    assert_eq!(owned.bound_documents(), cached.bound_documents() + 1);
    assert_eq!(owned.full_text_copies(), 0);
    assert_eq!(cached.full_text_copies(), 0);
    assert_eq!(owned.full_text_bytes_copied(), 0);
    assert_eq!(cached.full_text_bytes_copied(), 0);
}

#[test]
fn empty_engine_returns_no_diagnostics() {
    let result = check_program(&[], &CompilerOptions::default());
    assert!(result.diagnostics.is_empty());
    assert!(result.syntactic_diagnostics.is_empty());
    assert!(result.semantic_diagnostics.is_empty());
    assert!(result.global_diagnostics.is_empty());
    assert!(result.suggestion_diagnostics.is_empty());
    assert!(result.file_diagnostics.is_empty());
}

#[test]
fn owned_no_emit_entry_keeps_library_borrows_local_and_matches_file_getters() {
    let libs = [InputFile::new("/lib.d.ts".to_owned(), "interface IArguments {}\ninterface Array<T> {}\ninterface Object {}\ninterface Function {}\ninterface CallableFunction extends Function {}\ninterface NewableFunction extends Function {}\ninterface String {}\ninterface Number {}\ninterface Boolean {}\ninterface RegExp {}\n"
                .to_owned())];
    let files = [InputFile::new(
        "/main.ts".to_owned(),
        "const value: string = 1;\n".to_owned(),
    )];
    let options = CompilerOptions {
        no_emit: Some(true),
        ..CompilerOptions::default()
    };

    let cached = check_program_with_libs_at(&libs, &files, &options, "/");
    let owned = check_program_with_owned_libs_at(&libs, &files, &options, "/");

    assert_eq!(owned.syntactic_diagnostics, cached.syntactic_diagnostics);
    assert_eq!(owned.semantic_diagnostics, cached.semantic_diagnostics);
    assert!(owned.global_diagnostics.is_empty());
    assert_eq!(
        owned
            .semantic_diagnostics
            .iter()
            .map(Diagnostic::code)
            .collect::<Vec<_>>(),
        [2322]
    );
}

#[test]
fn owned_no_emit_entry_materializes_global_diagnostics_before_semantics() {
    let result = check_program_with_owned_libs_at(
        &[],
        &[InputFile::new(
            "/main.ts".to_owned(),
            "export {};\n".to_owned(),
        )],
        &CompilerOptions {
            no_emit: Some(true),
            ..CompilerOptions::default()
        },
        "/",
    );

    assert_eq!(
        result
            .global_diagnostics
            .iter()
            .map(Diagnostic::message_text)
            .collect::<Vec<_>>(),
        [
            "Cannot find global type 'Array'.",
            "Cannot find global type 'Boolean'.",
            "Cannot find global type 'CallableFunction'.",
            "Cannot find global type 'Function'.",
            "Cannot find global type 'IArguments'.",
            "Cannot find global type 'NewableFunction'.",
            "Cannot find global type 'Number'.",
            "Cannot find global type 'Object'.",
            "Cannot find global type 'RegExp'.",
            "Cannot find global type 'String'.",
        ]
    );
    assert!(result.semantic_diagnostics.is_empty());
}

#[test]
fn owned_no_emit_entry_materializes_globals_without_a_source_binder() {
    let result = check_program_with_owned_libs_at(
        &[],
        &[],
        &CompilerOptions {
            no_emit: Some(true),
            ..CompilerOptions::default()
        },
        "/",
    );

    assert_eq!(
        result
            .global_diagnostics
            .iter()
            .map(Diagnostic::message_text)
            .collect::<Vec<_>>(),
        [
            "Cannot find global type 'Array'.",
            "Cannot find global type 'Boolean'.",
            "Cannot find global type 'CallableFunction'.",
            "Cannot find global type 'Function'.",
            "Cannot find global type 'IArguments'.",
            "Cannot find global type 'NewableFunction'.",
            "Cannot find global type 'Number'.",
            "Cannot find global type 'Object'.",
            "Cannot find global type 'RegExp'.",
            "Cannot find global type 'String'.",
        ]
    );
    assert!(result.semantic_diagnostics.is_empty());

    let relaxed = check_program_with_owned_libs_at(
        &[],
        &[],
        &CompilerOptions {
            no_emit: Some(true),
            strict: Some(false),
            ..CompilerOptions::default()
        },
        "/",
    );
    assert_eq!(relaxed.global_diagnostics.len(), 8);
    assert!(relaxed.global_diagnostics.iter().all(|diagnostic| {
        !diagnostic.message_text().contains("CallableFunction")
            && !diagnostic.message_text().contains("NewableFunction")
    }));
}

#[test]
fn owned_no_emit_entry_keeps_located_global_shape_errors_semantic() {
    let result = check_program_with_owned_libs_at(
            &[],
            &[InputFile::new("/main.ts".to_owned(), "interface IArguments {}\ninterface Array {}\ninterface Object {}\ninterface Function {}\ninterface CallableFunction extends Function {}\ninterface NewableFunction extends Function {}\ninterface String {}\ninterface Number {}\ninterface Boolean {}\ninterface RegExp {}\n"
                    .to_owned())],
            &CompilerOptions {
                no_emit: Some(true),
                ..CompilerOptions::default()
            },
            "/",
        );

    assert!(result.global_diagnostics.is_empty());
    assert_eq!(
        result
            .semantic_diagnostics
            .iter()
            .map(Diagnostic::code)
            .collect::<Vec<_>>(),
        [2317]
    );
}

#[test]
fn observed_entry_reports_each_coarse_phase_once() {
    let mut phases = Vec::new();
    let result =
        check_program_with_libs_at_observed(&[], &[], &CompilerOptions::default(), "/", |phase| {
            phases.push(phase)
        });
    assert!(result.diagnostics.is_empty());
    assert_eq!(
        phases,
        [CheckPhase::Parse, CheckPhase::Bind, CheckPhase::Check]
    );
}

#[test]
fn public_getter_passes_keep_fixture_ordinal_before_global_sort() {
    let result = check_program(
        &[
            InputFile::new(
                "z.ts".to_owned(),
                "/// <reference path=\"/z-missing.d.ts\" />\n".to_owned(),
            ),
            InputFile::new(
                "a.ts".to_owned(),
                "/// <reference path=\"/a-missing.d.ts\" />\n".to_owned(),
            ),
        ],
        &CompilerOptions::default(),
    );

    assert_eq!(
        result
            .file_diagnostics
            .iter()
            .map(|file| file.file_name.as_str())
            .collect::<Vec<_>>(),
        ["z.ts", "a.ts"]
    );
    assert!(result
        .file_diagnostics
        .iter()
        .all(|file| file.syntactic.is_empty() && file.suggestion.is_empty()));
    assert_eq!(
        result
            .semantic_diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.file_name.as_deref(), diagnostic.code(),))
            .collect::<Vec<_>>(),
        [(Some("z.ts"), 6053), (Some("a.ts"), 6053)]
    );

    let mut assembled = result
        .file_diagnostics
        .iter()
        .flat_map(|file| {
            file.syntactic
                .iter()
                .chain(&file.semantic)
                .chain(&file.suggestion)
                .cloned()
        })
        .collect::<Vec<_>>();
    tsc_diagnostics::sort_and_dedupe_diagnostics(&mut assembled);
    assert_eq!(result.diagnostics, assembled);
}

#[test]
fn missing_leading_path_reference_reports_exact_6053() {
    let result = check_program(
        &[InputFile::new(
            "a.ts".to_owned(),
            "/// <reference path=\"/missing.d.ts\" />\n".to_owned(),
        )],
        &CompilerOptions::default(),
    );
    assert_eq!(result.diagnostics.len(), 1, "{:?}", result.diagnostics);
    let diagnostic = &result.diagnostics[0];
    assert_eq!(
        (
            diagnostic.file_name.as_deref(),
            diagnostic.code(),
            diagnostic.category(),
            diagnostic.start,
            diagnostic.length,
            diagnostic.message_text(),
        ),
        (
            Some("a.ts"),
            6053,
            DiagnosticCategory::Error,
            Some(21),
            Some(13),
            "File '/missing.d.ts' not found.",
        )
    );
    assert!(result.syntactic_diagnostics.is_empty());
}

#[test]
fn relative_single_quoted_path_reference_resolves_against_the_source() {
    let result = check_program(
        &[InputFile::new(
            "src/a.ts".to_owned(),
            "///<reference path='../typescript.ts' />\n".to_owned(),
        )],
        &CompilerOptions::default(),
    );
    assert_eq!(result.diagnostics.len(), 1, "{:?}", result.diagnostics);
    let diagnostic = &result.diagnostics[0];
    assert_eq!(
        (
            diagnostic.code(),
            diagnostic.start,
            diagnostic.length,
            diagnostic.message_text(),
        ),
        (6053, Some(20), Some(16), "File '/typescript.ts' not found.",)
    );
}

#[test]
fn existing_path_reference_is_loaded_without_a_missing_file_diagnostic() {
    let result = check_program(
        &[
            InputFile::new(
                "src/a.ts".to_owned(),
                "/// <reference path=\"./dep.d.ts\" />\n".to_owned(),
            ),
            InputFile::new(
                "src/dep.d.ts".to_owned(),
                "declare const dep: number;\n".to_owned(),
            ),
        ],
        &CompilerOptions::default(),
    );
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code() != 6053),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn path_reference_projection_stays_on_its_owned_pragma_face() {
    let result = check_program(
        &[InputFile::new(
            "a.ts".to_owned(),
            concat!(
                "/// <reference types=\"node\" path=\"/not-a-path-ref.d.ts\" />\n",
                "/// <reference path=\"/unsupported.html\" />\n",
                "const text = '/// <reference path=\"/inside-string.d.ts\" />';\n",
                "/// <reference path=\"/after-token.d.ts\" />\n",
            )
            .to_owned(),
        )],
        &CompilerOptions::default(),
    );
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code() != 6053),
        "{:?}",
        result.diagnostics
    );
}

/// Node posixCwd — path.posix.resolve's implicit base: the process
/// working directory untouched on POSIX; on Windows backslashes
/// flipped and the pre-"/" drive prefix dropped. The expectation
/// twin of the derivation in check_program_with_libs_at.
fn posix_process_cwd() -> String {
    let raw = std::env::current_dir()
        .expect("test process has a working directory")
        .to_string_lossy()
        .into_owned();
    if cfg!(windows) {
        let flipped = raw.replace('\\', "/");
        let root = flipped
            .find('/')
            .expect("an absolute Windows cwd has a separator");
        flipped[root..].to_owned()
    } else {
        raw
    }
}

fn cwd_probe_diagnostic_rows(current_directory: &str) -> Vec<(String, u32, u32, u32, String)> {
    let result = check_program_with_libs_at(
        &[],
        &[
            InputFile::new("b.ts".to_owned(), "export const bee = 1;\n".to_owned()),
            InputFile::new(
                "a.ts".to_owned(),
                "import * as b from \"./b\";\nb.nope;\n".to_owned(),
            ),
        ],
        &CompilerOptions::default(),
        current_directory,
    );
    result
        .diagnostics
        .iter()
        .map(|diag| {
            (
                diag.file_name.clone().unwrap_or_default(),
                diag.code(),
                diag.start.unwrap_or(u32::MAX),
                diag.length.unwrap_or(u32::MAX),
                diag.message_text().to_owned(),
            )
        })
        .collect()
}

#[test]
fn relative_cwd_roots_at_the_process_working_directory() {
    // The oracle host resolves ProgramJson cwd with
    // path.posix.resolve (program-host.mjs decodeProgram), so a
    // RELATIVE cwd roots at Node's posixCwd (drive-stripped on
    // Windows) — not "/". Must ride the PUBLIC entry: the check.rs
    // cwd pins set host_current_directory directly
    // (post-normalization) and cannot catch a regression at this
    // seam.
    let process_cwd = posix_process_cwd();
    assert_eq!(
            cwd_probe_diagnostic_rows("review-relative"),
            [(
                "a.ts".to_owned(),
                2339,
                28,
                4,
                format!(
                    "Property 'nope' does not exist on type 'typeof import(\"{process_cwd}/review-relative/b\")'."
                )
            )]
        );
}

#[test]
fn backslash_led_cwd_is_relative_under_posix_resolve() {
    // path.posix.resolve treats "\\" as an ordinary character, so a
    // "\\"-led cwd is RELATIVE — it joins onto posixCwd and the
    // later separator flip collapses "<cwd>/\\x" into "<cwd>/x".
    // Normalizing separators BEFORE the absoluteness test would
    // wrongly re-root it at "/" and drop the process cwd.
    let process_cwd = posix_process_cwd();
    assert_eq!(
            cwd_probe_diagnostic_rows("\\review-relative"),
            [(
                "a.ts".to_owned(),
                2339,
                28,
                4,
                format!(
                    "Property 'nope' does not exist on type 'typeof import(\"{process_cwd}/review-relative/b\")'."
                )
            )]
        );
}

#[test]
fn mixed_separator_cwd_resolves_dot_segments_before_backslash_flip() {
    // path.posix.resolve sees "\\" as a literal segment here, so
    // the following POSIX "/.." removes that segment and leaves
    // posixCwd unchanged. Flipping "\\" first would instead let
    // ".." remove the final segment of posixCwd.
    let process_cwd = posix_process_cwd();
    let module_path = state::CheckerState::normalize_program_path("b", &process_cwd);
    assert_eq!(
        cwd_probe_diagnostic_rows("\\/.."),
        [(
            "a.ts".to_owned(),
            2339,
            28,
            4,
            format!("Property 'nope' does not exist on type 'typeof import(\"{module_path}\")'.")
        )]
    );
}

#[test]
fn absolute_cwd_backslash_segments_stay_literal_during_dot_resolution() {
    // posix.resolve("/a\\b/..") = "/": "a\\b" is ONE literal
    // segment eaten by "..". Flipping "\\" first would split it
    // and leave "/a". Oracle-probed (driver.mjs): import("/b").
    assert_eq!(
        cwd_probe_diagnostic_rows("/a\\b/.."),
        [(
            "a.ts".to_owned(),
            2339,
            28,
            4,
            "Property 'nope' does not exist on type 'typeof import(\"/b\")'.".to_owned()
        )]
    );
}

#[test]
fn lib_bundle_key_projects_to_bind_observables() {
    use tsc_types::flags::ScriptTarget;
    // A lib name unique to this test: the cache is process-global.
    let lib = InputFile::new(
        "lib.bundle-key-probe.d.ts".to_owned(),
        "declare const bundleKeyProbe: number;\n".to_owned(),
    );
    let libs = [&lib];
    let base = CompilerOptions::default();
    let shared = lib_bundle(&libs, &base);
    assert_eq!(shared.sources[0].language_version, ScriptTarget::ES2025);

    // Bind-inert options reuse the bundle: the checker consumes
    // them per program, never through the cached prefix.
    let inert = CompilerOptions {
        strict_null_checks: Some(false),
        jsx: Some(2),
        no_emit: Some(true),
        module_resolution: Some(1),
        ..base.clone()
    };
    assert!(std::ptr::eq(shared, lib_bundle(&libs, &inert)));

    // ES3 and an absent target compute the same ES2025
    // languageVersion (options.rs:139) — one bundle.
    let es3 = CompilerOptions {
        target: Some(ScriptTarget::ES3.bits()),
        ..base.clone()
    };
    assert!(std::ptr::eq(shared, lib_bundle(&libs, &es3)));

    // Each bind-time observable splits the key.
    let es5 = CompilerOptions {
        target: Some(ScriptTarget::ES5.bits()),
        ..base.clone()
    };
    let es5_bundle = lib_bundle(&libs, &es5);
    assert!(!std::ptr::eq(shared, es5_bundle));
    assert_eq!(es5_bundle.sources[0].language_version, ScriptTarget::ES5);
    let loose = CompilerOptions {
        always_strict: Some(false),
        ..base.clone()
    };
    assert!(!std::ptr::eq(shared, lib_bundle(&libs, &loose)));
    let fallthrough = CompilerOptions {
        no_fallthrough_cases_in_switch: Some(true),
        ..base.clone()
    };
    assert!(!std::ptr::eq(shared, lib_bundle(&libs, &fallthrough)));
}

#[test]
fn lib_bundle_forced_fingerprint_collision_requires_exact_text() {
    fn collide_all_text(_: &str) -> u64 {
        0
    }

    let first = InputFile::new(
        "lib.bundle-collision-probe.d.ts".to_owned(),
        "declare const collisionProbe: string;\n".to_owned(),
    );
    let second = InputFile::new(
        first.name.clone(),
        "declare const collisionProbe: number;\n".to_owned(),
    );
    let options = CompilerOptions::default();

    let first_bundle = lib_bundle_with_fingerprint(&[&first], &options, collide_all_text);
    let second_bundle = lib_bundle_with_fingerprint(&[&second], &options, collide_all_text);
    let first_again = lib_bundle_with_fingerprint(&[&first], &options, collide_all_text);

    assert!(!std::ptr::eq(first_bundle, second_bundle));
    assert!(std::ptr::eq(first_bundle, first_again));
    assert_eq!(first_bundle.sources[0].text(), first.text());
    assert_eq!(second_bundle.sources[0].text(), second.text());
}

#[test]
fn prepared_harness_bundle_validates_exact_text_and_projected_options() {
    fn assert_handle_traits<T: Copy + Send + Sync + 'static>() {}

    assert_handle_traits::<PreparedHarnessLibBundle>();
    let original = InputFile::new(
        "lib.prepared-validation-probe.d.ts".to_owned(),
        "declare const preparedProbe: string;\n".to_owned(),
    );
    let changed = InputFile::new(
        original.name.clone(),
        "declare const preparedProbe: number;\n".to_owned(),
    );
    let base = CompilerOptions::default();
    let prepared = prepare_harness_lib_bundle(std::slice::from_ref(&original), &base).unwrap();
    let base_projection = lib_bundle_options(&base);

    assert!(prepared.validated(&[&original], &base_projection).is_some());
    assert!(prepared.validated(&[&changed], &base_projection).is_none());

    let bind_inert = CompilerOptions {
        strict_null_checks: Some(true),
        no_emit: Some(true),
        ..base.clone()
    };
    assert!(harness_lib_bundle_options_key(&base) == harness_lib_bundle_options_key(&bind_inert));
    assert!(prepared
        .validated(&[&original], &lib_bundle_options(&bind_inert))
        .is_some());

    let bind_observable = CompilerOptions {
        always_strict: Some(false),
        ..base.clone()
    };
    assert!(
        harness_lib_bundle_options_key(&base) != harness_lib_bundle_options_key(&bind_observable)
    );
    assert!(prepared
        .validated(&[&original], &lib_bundle_options(&bind_observable))
        .is_none());

    let second = InputFile::new(
        "lib.prepared-validation-second.d.ts".to_owned(),
        "declare const preparedSecond: boolean;\n".to_owned(),
    );
    let ordered = [original.clone(), second.clone()];
    let ordered_prepared = prepare_harness_lib_bundle(&ordered, &base).unwrap();
    assert!(ordered_prepared
        .validated(&[&ordered[0], &ordered[1]], &base_projection)
        .is_some());
    assert!(ordered_prepared
        .validated(&[&ordered[1], &ordered[0]], &base_projection)
        .is_none());
    let renamed = InputFile::new(
        "lib.prepared-validation-renamed.d.ts".to_owned(),
        second.text(),
    );
    assert!(ordered_prepared
        .validated(&[&ordered[0], &renamed], &base_projection)
        .is_none());
}

#[test]
fn stale_prepared_harness_bundle_falls_back_to_ordinary_exact_bundle() {
    fn rows(result: &CheckResult) -> Vec<(u32, String)> {
        result
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code(), diagnostic.message_text().to_owned()))
            .collect()
    }

    let original = InputFile::new(
        "lib.prepared-fallback-probe.d.ts".to_owned(),
        "declare const preparedFallbackProbe: string;\n".to_owned(),
    );
    let changed = InputFile::new(
        original.name.clone(),
        "declare const preparedFallbackProbe: number;\n".to_owned(),
    );
    let files = [InputFile::new(
        "/prepared-fallback.ts".to_owned(),
        "const value: string = preparedFallbackProbe;\n".to_owned(),
    )];
    let options = CompilerOptions::default();
    let prepared = prepare_harness_lib_bundle(std::slice::from_ref(&original), &options).unwrap();

    let ordinary =
        check_program_with_libs_at(std::slice::from_ref(&changed), &files, &options, "/");
    let hinted = check_program_with_prepared_harness_libs_at(
        std::slice::from_ref(&changed),
        &files,
        &options,
        "/",
        prepared,
    );

    assert_eq!(rows(&hinted), rows(&ordinary));
    assert!(rows(&hinted).iter().any(|(code, _)| *code == 2322));

    let mut observe_phase = |_| {};
    let cache_off = check_program_with_libs_at_observed_cache_mode_prepared(
        std::slice::from_ref(&changed),
        &files,
        &options,
        "/",
        false,
        Some(prepared),
        &mut observe_phase,
    );
    assert_eq!(rows(&cache_off), rows(&ordinary));

    let shadowing_file = [InputFile::new(
        original.name.clone(),
        "const localOnly = 1;\n".to_owned(),
    )];
    let ordinary_shadowed = check_program_with_libs_at(
        std::slice::from_ref(&original),
        &shadowing_file,
        &options,
        "/",
    );
    let hinted_shadowed = check_program_with_prepared_harness_libs_at(
        std::slice::from_ref(&original),
        &shadowing_file,
        &options,
        "/",
        prepared,
    );
    assert_eq!(rows(&hinted_shadowed), rows(&ordinary_shadowed));
}

#[test]
fn parallel_cold_lib_bundle_callers_share_one_exact_entry() {
    let lib = InputFile::new(
        "lib.bundle-parallel-cold-probe.d.ts".to_owned(),
        (0..512)
            .map(|index| format!("interface ColdProbe{index} {{ value: number }}\n"))
            .collect::<String>(),
    );
    let options = CompilerOptions::default();
    let start = std::sync::Barrier::new(3);

    let (first, second) = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            start.wait();
            lib_bundle(&[&lib], &options)
        });
        let second = scope.spawn(|| {
            start.wait();
            lib_bundle(&[&lib], &options)
        });
        start.wait();
        (
            first.join().expect("first cold cache caller"),
            second.join().expect("second cold cache caller"),
        )
    });

    assert!(std::ptr::eq(first, second));
}

#[test]
fn cache_off_owned_prefix_matches_cached_harness_result() {
    let libs = [InputFile::new("lib.cache-mode-probe.d.ts".to_owned(), "interface IArguments {}\ninterface Array<T> {}\ninterface Object {}\ninterface Function {}\ninterface CallableFunction extends Function {}\ninterface NewableFunction extends Function {}\ninterface String {}\ninterface Number {}\ninterface Boolean {}\ninterface RegExp {}\n"
                .to_owned())];
    let files = [InputFile::new(
        "cache-mode-probe.ts".to_owned(),
        "const value: string = 1;\n".to_owned(),
    )];
    let options = CompilerOptions::default();
    let mut cached_phases = Vec::new();
    let mut owned_phases = Vec::new();

    let cached = check_program_with_libs_at_observed_cache_mode(
        &libs,
        &files,
        &options,
        "/",
        true,
        &mut |phase| cached_phases.push(phase),
    );
    let owned = check_program_with_libs_at_observed_cache_mode(
        &libs,
        &files,
        &options,
        "/",
        false,
        &mut |phase| owned_phases.push(phase),
    );

    assert_eq!(owned, cached);
    assert_one_cached_library_saved_work(
        owned.work_counters,
        cached.work_counters,
        libs[0].text().len(),
    );
    assert_eq!(owned_phases, cached_phases);
    assert_eq!(
        owned_phases,
        [CheckPhase::Parse, CheckPhase::Bind, CheckPhase::Check]
    );
}

#[test]
fn authoritative_owned_and_harness_cached_modes_are_exactly_equivalent() {
    struct Provider {
        fail: bool,
    }

    impl AuthoritativeModuleProvider for Provider {
        fn resolve_module(
            &self,
            request: AuthoritativeModuleRequest<'_>,
        ) -> Result<AuthoritativeModuleResolution, AuthoritativeModuleLookupFailure> {
            assert_eq!(request.source_token, AuthoritativeSourceToken(1));
            assert_eq!(request.containing_file, "/main.ts");
            assert_eq!(request.specifier, "pkg");
            if self.fail {
                Err(AuthoritativeModuleLookupFailure::Missing)
            } else {
                Ok(AuthoritativeModuleResolution::NotFound(
                    AuthoritativeNotFoundModule::default(),
                ))
            }
        }
    }

    let libs = [InputFile::new("/lib.authoritative-cache-mode-probe.d.ts".to_owned(), "interface IArguments {}\ninterface Array<T> {}\ninterface Object {}\ninterface Function {}\ninterface CallableFunction extends Function {}\ninterface NewableFunction extends Function {}\ninterface String {}\ninterface Number {}\ninterface Boolean {}\ninterface RegExp {}\n"
                .to_owned())];
    let files = [InputFile::new(
        "/main.ts".to_owned(),
        "import 'pkg';\nconst value: string = 1;\n".to_owned(),
    )];
    let lib_metadata = [AuthoritativeSourceMetadata {
        token: AuthoritativeSourceToken(0),
        file_name: libs[0].name.clone(),
        may_be_emitted: false,
        implied_node_format: None,
        implied_node_format_for_emit: None,
    }];
    let file_metadata = [AuthoritativeSourceMetadata {
        token: AuthoritativeSourceToken(1),
        file_name: files[0].name.clone(),
        may_be_emitted: true,
        implied_node_format: None,
        implied_node_format_for_emit: None,
    }];
    let options = CompilerOptions {
        no_emit: Some(true),
        module: Some(1),
        module_resolution: Some(2),
        ..CompilerOptions::default()
    };
    let run = |cache_enabled, provider: &Provider| {
        check_program_with_authoritative_modules_at_cache_mode(
            &libs,
            &files,
            &lib_metadata,
            &file_metadata,
            &options,
            "/",
            provider,
            cache_enabled,
        )
    };

    let owned = run(false, &Provider { fail: false }).expect("owned authoritative result");
    let cached = run(true, &Provider { fail: false }).expect("cached authoritative result");
    assert_eq!(owned, cached);
    assert_one_cached_library_saved_work(
        owned.work_counters,
        cached.work_counters,
        libs[0].text().len(),
    );
    assert_eq!(
        cached
            .semantic_diagnostics
            .iter()
            .map(Diagnostic::code)
            .collect::<Vec<_>>(),
        [2882, 2322]
    );

    let owned_failure =
        run(false, &Provider { fail: true }).expect_err("owned authoritative failure");
    let cached_failure =
        run(true, &Provider { fail: true }).expect_err("cached authoritative failure");
    assert_eq!(owned_failure, cached_failure);
}

#[test]
fn authoritative_not_found_facts_reach_the_node10_diagnostic_chain() {
    struct Provider;

    impl AuthoritativeModuleProvider for Provider {
        fn resolve_module(
            &self,
            request: AuthoritativeModuleRequest<'_>,
        ) -> Result<AuthoritativeModuleResolution, AuthoritativeModuleLookupFailure> {
            assert_eq!(request.source_token, AuthoritativeSourceToken(1));
            assert_eq!(request.containing_file, "/index.ts");
            assert_eq!(request.specifier, "pkg");
            assert_eq!(request.mode, AuthoritativeResolutionMode::Unspecified);
            Ok(AuthoritativeModuleResolution::NotFound(
                AuthoritativeNotFoundModule {
                    alternate_result: Some(
                        "/node_modules/pkg/definitely-not-index.d.ts".to_owned(),
                    ),
                },
            ))
        }
    }

    let source = "import { pkg } from \"pkg\";\n";
    let files = [InputFile::new("/index.ts".to_owned(), source.to_owned())];
    let metadata = [AuthoritativeSourceMetadata {
        token: AuthoritativeSourceToken(1),
        file_name: files[0].name.clone(),
        may_be_emitted: true,
        implied_node_format: None,
        implied_node_format_for_emit: None,
    }];
    let result = check_program_with_authoritative_modules_at_cache_mode(
        &[],
        &files,
        &[],
        &metadata,
        &CompilerOptions {
            no_emit: Some(true),
            module_resolution: Some(2),
            ..CompilerOptions::default()
        },
        "/",
        &Provider,
        false,
    )
    .expect("authoritative alternate-result miss");

    let diagnostic = result
        .semantic_diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code() == 2307)
        .expect("module-not-found diagnostic");
    assert_eq!(
        (
            diagnostic.file_name.as_deref(),
            diagnostic.start,
            diagnostic.length,
            diagnostic.message_text(),
        ),
        (
            Some("/index.ts"),
            Some(source.find("\"pkg\"").expect("module specifier") as u32),
            Some("\"pkg\"".len() as u32),
            "Cannot find module 'pkg' or its corresponding type declarations.",
        )
    );
    assert_eq!(diagnostic.message.next.len(), 1);
    assert_eq!(
            (
                diagnostic.message.next[0].code,
                diagnostic.message.next[0].category,
                diagnostic.message.next[0].text.as_str(),
            ),
            (
                6280,
                DiagnosticCategory::Message,
                "There are types at '/node_modules/pkg/definitely-not-index.d.ts', but this result could not be resolved under your current 'moduleResolution' setting. Consider updating to 'node16', 'nodenext', or 'bundler'.",
            )
        );
}

#[test]
fn program_parser_receives_the_effective_script_target() {
    let files = [InputFile::new(
        "a.ts".to_owned(),
        "foo.\u{08a1};\n".to_owned(),
    )];
    let es5 = check_program(
        &files,
        &CompilerOptions {
            target: Some(tsc_types::ScriptTarget::ES5.bits()),
            ..CompilerOptions::default()
        },
    );
    let es2015 = check_program(
        &files,
        &CompilerOptions {
            target: Some(tsc_types::ScriptTarget::ES2015.bits()),
            ..CompilerOptions::default()
        },
    );

    assert_eq!(
        es5.syntactic_diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code(), diagnostic.start, diagnostic.length))
            .collect::<Vec<_>>(),
        vec![(1127, Some(4), Some(1))]
    );
    assert!(es2015.syntactic_diagnostics.is_empty());
}

#[test]
fn js_files_report_typescript_only_syntax() {
    // Pins from tsc program.getSyntacticDiagnostics on an allowJs program.
    let result = check_program(
            &[InputFile::new("a.js".to_owned(), "function f(x: number): string { return \"\"; }\ninterface I { a: string }\nenum E { A }\nvar x!;\nimport eq = require(\"m\");\n".to_owned())],
            &CompilerOptions {
                allow_js: true,
                ..CompilerOptions::default()
            },
        );
    let pins: Vec<(u32, u32, u32)> = result
        .syntactic_diagnostics
        .iter()
        .map(|d| (d.code(), d.start.unwrap_or(0), d.length.unwrap_or(0)))
        .collect();
    assert_eq!(
        pins,
        [
            (8010, 14, 6),
            (8010, 23, 6),
            (8006, 55, 1),
            (8006, 76, 1),
            (8002, 92, 25),
        ]
    );
}

#[test]
fn js_files_report_type_only_imports_and_export_equals() {
    let result = check_program(
            &[InputFile::new("a.js".to_owned(), "import type { A } from \"m\";\nimport { type B } from \"m\";\nexport type { C };\nexport = 5;\n".to_owned())],
            &CompilerOptions {
                allow_js: true,
                ..CompilerOptions::default()
            },
        );
    let pins: Vec<(u32, u32, u32)> = result
        .syntactic_diagnostics
        .iter()
        .map(|d| (d.code(), d.start.unwrap_or(0), d.length.unwrap_or(0)))
        .collect();
    assert_eq!(
        pins,
        [(8006, 0, 27), (8006, 37, 6), (8006, 56, 18), (8003, 75, 11)]
    );
}

fn codes_of(source: &str) -> Vec<u32> {
    codes_of_with_options(source, &CompilerOptions::default())
}

fn codes_of_with_options(source: &str, options: &CompilerOptions) -> Vec<u32> {
    let result = check_program(
        &[InputFile::new("a.ts".to_owned(), source.to_owned())],
        options,
    );
    result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.category() == DiagnosticCategory::Error)
        .map(|d| d.code())
        .collect()
}

#[test]
fn bom_before_arrow_at_line_end_does_not_create_a_line_terminator_error() {
    let without_bom = codes_of("const f = () =>\n  1;\n");
    let with_bom = codes_of("\u{feff}const f = () =>\n  1;\n");
    assert_eq!(with_bom, without_bom);
    assert!(!with_bom.contains(&1200));

    let invalid_without_bom = codes_of("const f = ()\n  => 1;\n");
    let invalid_with_bom = codes_of("\u{feff}const f = ()\n  => 1;\n");
    assert_eq!(invalid_with_bom, invalid_without_bom);
    assert!(invalid_with_bom.contains(&1200));
}

#[test]
fn host_package_json_accepts_one_leading_bom() {
    assert_eq!(
        parse_host_package_json("\u{feff}{\"type\":\"module\"}"),
        parse_host_package_json("{\"type\":\"module\"}")
    );
}

fn strict_options() -> CompilerOptions {
    CompilerOptions {
        strict: Some(true),
        no_implicit_any: Some(true),
        ..CompilerOptions::default()
    }
}

#[test]
fn typeof_import_follows_value_alias_reexports() {
    let result = check_program(
        &[
            InputFile::new("a.ts".to_owned(), "export const x = 1;\n".to_owned()),
            InputFile::new("b.ts".to_owned(), "export { x } from \"./a\";\n".to_owned()),
            InputFile::new(
                "main.ts".to_owned(),
                "type T = typeof import(\"./b\").x;\nlet y: T = \"bad\";\n".to_owned(),
            ),
        ],
        &CompilerOptions::default(),
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [2322]
    );
}

#[test]
fn implicit_external_modules_exclude_umd_global_aliases() {
    let run =
        |file_name: &str, file_text: &str, options: CompilerOptions, extra_files: &[InputFile]| {
            let mut files = vec![InputFile::new(
                "umd.d.ts".to_owned(),
                "export as namespace U;\nexport const s: unique symbol;\n".to_owned(),
            )];
            files.extend_from_slice(extra_files);
            files.push(InputFile::new(file_name.to_owned(), file_text.to_owned()));
            let result = check_program(&files, &options);
            result
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code() == 2741)
                .expect("the computed-property assignment should report 2741")
                .message_text()
                .to_owned()
        };
    let assignment = "declare let a: {};\nlet b: {\n  // @ts-ignore\n  [U.s]: number\n} = a;\n";
    let expected =
        "Property '[U.s]' is missing in type '{}' but required in type '{ [s]: number; }'.";

    // Auto mode: .mts/.cts are modules even without import/export.
    assert_eq!(
        run("a.mts", assignment, CompilerOptions::default(), &[]),
        expected
    );
    // Force mode: every non-declaration source file is a module.
    assert_eq!(
        run(
            "a.ts",
            assignment,
            CompilerOptions {
                module_detection: Some(3),
                ..CompilerOptions::default()
            },
            &[]
        ),
        expected
    );
    // Auto + React JSX: a real JSX tag is the indicator.
    assert_eq!(
        run(
            "a.tsx",
            &format!("{assignment}const element = <div />;\n"),
            CompilerOptions {
                jsx: Some(4),
                ..CompilerOptions::default()
            },
            &[]
        ),
        expected
    );
    // Auto + Node-flavored package lookup: a nearest `type: module`
    // package scope supplies an ESNext implied format.
    assert_eq!(
        run(
            "/src/a.ts",
            assignment,
            CompilerOptions {
                module: Some(7),
                module_resolution: Some(3),
                module_detection: Some(2),
                ..CompilerOptions::default()
            },
            &[InputFile::new(
                "/package.json".to_owned(),
                r#"{"type":"module"}"#.to_owned()
            )]
        ),
        expected
    );
    // Legacy mode intentionally retains syntax-only detection.
    assert_eq!(
        run(
            "a.mts",
            assignment,
            CompilerOptions {
                module_detection: Some(1),
                ..CompilerOptions::default()
            },
            &[]
        ),
        "Property '[U.s]' is missing in type '{}' but required in type '{ [U.s]: number; }'."
    );
}

#[test]
fn import_type_missing_member_uses_absolute_module_name() {
    let result = check_program(
        &[
            InputFile::new(
                "m.ts".to_owned(),
                "export interface Present {}\n".to_owned(),
            ),
            InputFile::new(
                "main.ts".to_owned(),
                "type T = import(\"./m\").Missing;\n".to_owned(),
            ),
        ],
        &CompilerOptions::default(),
    );
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code() == 2694)
        .expect("missing import-type member should report 2694");
    assert_eq!(
        diagnostic.message_text(),
        "Namespace '\"/m\"' has no exported member 'Missing'."
    );
}

#[test]
fn bare_import_defer_does_not_run_import_meta_module_checks() {
    let result = check_program(
        &[InputFile::new(
            "a.ts".to_owned(),
            "const x = import.defer;\n".to_owned(),
        )],
        &CompilerOptions {
            module: Some(1),
            ..CompilerOptions::default()
        },
    );
    assert!(!result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code() == 1343));
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [1005]
    );
}

#[test]
fn node16_plain_ts_uses_package_scope_for_import_meta() {
    let options = CompilerOptions {
        module: Some(100),
        module_resolution: Some(3),
        ..CompilerOptions::default()
    };
    let commonjs = check_program(
        &[InputFile::new(
            "src/main.ts".to_owned(),
            "const x = import.meta;\n".to_owned(),
        )],
        &options,
    );
    assert!(commonjs
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code() == 1470));

    let esm = check_program(
        &[
            InputFile::new(
                "package.json".to_owned(),
                "{\"type\":\"module\"}\n".to_owned(),
            ),
            InputFile::new(
                "src/main.ts".to_owned(),
                "const x = import.meta;\n".to_owned(),
            ),
        ],
        &options,
    );
    assert!(!esm
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code() == 1470));
}

#[test]
fn node16_windows_paths_use_package_scope_for_import_meta() {
    let result = check_program(
        &[
            InputFile::new(
                r"C:\pkg\package.json".to_owned(),
                "{\"type\":\"module\"}\n".to_owned(),
            ),
            InputFile::new(
                r"C:\pkg\main.ts".to_owned(),
                "const x = import.meta;\n".to_owned(),
            ),
        ],
        &CompilerOptions {
            module: Some(100),
            module_resolution: Some(3),
            ..CompilerOptions::default()
        },
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code() == 1470),
        "Windows path separators must not hide package.json: {:#?}",
        result.diagnostics
    );
}

#[test]
fn node16_package_commonjs_format_applies_to_default_import_and_export_equals() {
    let result = check_program(
        &[
            InputFile::new(
                "package.json".to_owned(),
                "{\"type\":\"commonjs\"}\n".to_owned(),
            ),
            InputFile::new(
                "dep.ts".to_owned(),
                "const value = { a: 1 };\nexport = value;\n".to_owned(),
            ),
            InputFile::new(
                "main.mts".to_owned(),
                "import value from \"./dep.js\";\nvalue.a;\n".to_owned(),
            ),
        ],
        &CompilerOptions {
            module: Some(100),
            module_resolution: Some(3),
            ..CompilerOptions::default()
        },
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|diagnostic| matches!(diagnostic.code(), 1192 | 1203)),
        "{:#?}",
        result.diagnostics
    );
}

#[test]
fn unrelated_package_inputs_do_not_hide_a_bare_module_miss() {
    let result = check_program(
        &[
            InputFile::new(
                "package.json".to_owned(),
                "{\"name\":\"unrelated\"}\n".to_owned(),
            ),
            InputFile::new(
                "node_modules/other/index.d.ts".to_owned(),
                "export {};\n".to_owned(),
            ),
            InputFile::new(
                "main.ts".to_owned(),
                "import { value } from \"definitely-missing\";\nvalue;\n".to_owned(),
            ),
        ],
        &CompilerOptions::default(),
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code() == 2307),
        "{:#?}",
        result.diagnostics
    );
}

#[test]
fn base_url_miss_without_a_paths_match_reports_2307() {
    let result = check_program(
        &[InputFile::new(
            "src/main.ts".to_owned(),
            "import { value } from \"definitely-missing\";\nvalue;\n".to_owned(),
        )],
        &CompilerOptions {
            base_url: Some("src".to_owned()),
            ..CompilerOptions::default()
        },
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code() == 2307),
        "{:#?}",
        result.diagnostics
    );
}

#[test]
fn checked_js_definite_relative_module_miss_is_public() {
    let result = check_program(
        &[
            InputFile::new("foo.js".to_owned(), "export const value = 1;\n".to_owned()),
            InputFile::new(
                "main.mjs".to_owned(),
                "import { value } from \"./foo\";\nvalue;\n".to_owned(),
            ),
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            module: Some(100),
            module_resolution: Some(3),
            ..CompilerOptions::default()
        },
    );
    let codes: Vec<u32> = result
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code())
        .collect();
    assert_eq!(codes, [2835], "{:#?}", result.diagnostics);
}

#[test]
fn checked_js_global_this_collision_is_public() {
    let result = check_program(
        &[InputFile::new(
            "globalThisCollision.js".to_owned(),
            "var globalThis;".to_owned(),
        )],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            no_emit: Some(true),
            ..CompilerOptions::default()
        },
    );
    let pins: Vec<(u32, u32, u32)> = result
        .diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
            )
        })
        .collect();
    assert_eq!(pins, [(2397, 4, 10)], "{:#?}", result.diagnostics);
}

#[test]
fn checked_js_publishes_namespace_export_declaration_bind_diagnostic() {
    let files = [
        InputFile::new("cls.js".to_owned(), "export class Foo {}\n".to_owned()),
        InputFile::new(
            "globalNs.js".to_owned(),
            "export * from \"./cls\";\nexport as namespace GLO;\n".to_owned(),
        ),
    ];
    let checked = check_program(
        &files,
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            module: Some(1),
            ..CompilerOptions::default()
        },
    );
    let pins = checked
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code() == 1315)
        .map(|diagnostic| {
            (
                diagnostic.file_name.as_deref(),
                diagnostic.start,
                diagnostic.length,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        pins,
        [(
            Some("globalNs.js"),
            Some(files[1].text().find("export as").expect("namespace export") as u32,),
            Some("export as namespace GLO;".len() as u32),
        )]
    );

    let plain = check_program(
        &files,
        &CompilerOptions {
            allow_js: true,
            module: Some(1),
            ..CompilerOptions::default()
        },
    );
    assert!(
        plain
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code() != 1315),
        "plain JS must retain the plainJSErrors publication surface: {:#?}",
        plain.diagnostics
    );
}

#[test]
fn checked_js_host_dependent_module_resolution_stays_suppressed() {
    let result = check_program(
        &[
            InputFile::new(
                "node_modules/pkg/index.js".to_owned(),
                "export const value = 1;\n".to_owned(),
            ),
            InputFile::new(
                "main.js".to_owned(),
                "import { value } from \"pkg\";\nvalue;\n".to_owned(),
            ),
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            module: Some(100),
            module_resolution: Some(3),
            ..CompilerOptions::default()
        },
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code() == 2307),
        "{:#?}",
        result.diagnostics
    );
}

#[test]
fn external_emit_helpers_validate_an_in_program_tslib() {
    let result = check_program(
        &[
            InputFile::new(
                "types.d.ts".to_owned(),
                "declare module \"tslib\" { export {}; }\n".to_owned(),
            ),
            InputFile::new("a.ts".to_owned(), "export {};\n".to_owned()),
            InputFile::new(
                "main.ts".to_owned(),
                "export * as ns from \"./a\";\n".to_owned(),
            ),
        ],
        &CompilerOptions {
            module: Some(1),
            import_helpers: Some(true),
            ..CompilerOptions::default()
        },
    );
    let helper = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code() == 2343)
        .expect("missing __importStar should report");
    assert!(helper.message_text().contains("__importStar"));
}

#[test]
fn external_emit_helpers_report_only_definite_tslib_misses() {
    let files = [
        InputFile::new("a.ts".to_owned(), "export {};\n".to_owned()),
        InputFile::new(
            "main.ts".to_owned(),
            "export * as ns from \"./a\";\n".to_owned(),
        ),
    ];
    let options = CompilerOptions {
        module: Some(1),
        import_helpers: Some(true),
        ..CompilerOptions::default()
    };
    let missing = check_program(&files, &options);
    assert!(
        missing
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code() == 2354),
        "{:#?}",
        missing.diagnostics
    );

    let mut host_dependent = files.to_vec();
    host_dependent.push(InputFile::new(
        "node_modules/tslib/index.d.ts".to_owned(),
        "export {};\n".to_owned(),
    ));
    let suppressed = check_program(&host_dependent, &options);
    assert!(
        suppressed
            .diagnostics
            .iter()
            .all(|diagnostic| !matches!(diagnostic.code(), 2343 | 2354 | 2807)),
        "{:#?}",
        suppressed.diagnostics
    );
}

#[test]
fn external_emit_helpers_check_spread_array_arity() {
    let result = check_program(
            &[
                InputFile::new("types.d.ts".to_owned(), "declare module \"tslib\" {\n  export function __spreadArray(to: any[], from: any[]): any[];\n}\n".to_owned()),
                InputFile::new("main.ts".to_owned(), "export {};\nconst values = [1, ...[2], 3];\n".to_owned()),
            ],
            &CompilerOptions {
                target: Some(tsc_types::ScriptTarget::ES5.bits()),
                import_helpers: Some(true),
                ..CompilerOptions::default()
            },
        );
    let helper = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code() == 2807)
        .expect("two-parameter __spreadArray should report");
    assert!(helper.message_text().contains("3 parameters"));
}

#[test]
fn external_emit_helpers_check_private_get_and_set_arity() {
    let tslib = InputFile::new("types.d.ts".to_owned(), concat!(
                "declare module \"tslib\" {\n",
                "  export function __classPrivateFieldGet<T extends object, V>(receiver: T, state: any): V;\n",
                "  export function __classPrivateFieldSet<T extends object, V>(receiver: T, state: any, value: V): V;\n",
                "}\n",
            )
            .to_owned());
    let cases = [
        (
            "instance.ts",
            concat!(
                "\nexport class C {\n",
                "    #a = 1;\n",
                "    #b() { this.#c = 42; }\n",
                "    set #c(v: number) { this.#a += v; }\n",
                "}\n",
            ),
            [
                (41, 7, "__classPrivateFieldSet", "5 parameters"),
                (81, 7, "__classPrivateFieldGet", "4 parameters"),
            ],
        ),
        (
            "static.ts",
            concat!(
                "\nexport class S {\n",
                "    static #a = 1;\n",
                "    static #b() { this.#a = 42; }\n",
                "    static get #c() { return S.#b(); }\n",
                "}\n",
            ),
            [
                (55, 7, "__classPrivateFieldSet", "5 parameters"),
                (100, 4, "__classPrivateFieldGet", "4 parameters"),
            ],
        ),
    ];
    let options = CompilerOptions {
        target: Some(tsc_types::ScriptTarget::ES2015.bits()),
        import_helpers: Some(true),
        isolated_modules: Some(true),
        ..CompilerOptions::default()
    };

    for (file_name, text, expected) in cases {
        let result = check_program(
            &[
                tslib.clone(),
                InputFile::new(file_name.to_owned(), text.to_owned()),
            ],
            &options,
        );
        let observed = result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 2807)
            .map(|diagnostic| {
                (
                    diagnostic.start.unwrap_or_default(),
                    diagnostic.length.unwrap_or_default(),
                    diagnostic.message_text(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(observed.len(), expected.len(), "{file_name}: {observed:#?}");
        for (observed, expected) in observed.iter().zip(expected) {
            assert_eq!((observed.0, observed.1), (expected.0, expected.1));
            assert!(
                observed.2.contains(expected.2),
                "{file_name}: {}",
                observed.2
            );
            assert!(
                observed.2.contains(expected.3),
                "{file_name}: {}",
                observed.2
            );
        }
    }

    let native = check_program(
        &[
            tslib,
            InputFile::new(
                "native.ts".to_owned(),
                "export class C { #x = 1; read() { return this.#x; } }\n".to_owned(),
            ),
        ],
        &CompilerOptions {
            target: Some(tsc_types::ScriptTarget::ES_NEXT.bits()),
            import_helpers: Some(true),
            isolated_modules: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert!(native
        .diagnostics
        .iter()
        .all(|diagnostic| diagnostic.code() != 2807));
}

#[test]
fn external_emit_helpers_cover_decorator_named_evaluation_helpers() {
    let result = check_program(
            &[
                InputFile::new("types.d.ts".to_owned(), "declare module \"tslib\" { export {}; }\n".to_owned()),
                InputFile::new("main.ts".to_owned(), "export {};\ndeclare let dec: any;\ndeclare let key: any;\n({ [key]: @dec class {} });\n".to_owned()),
            ],
            &CompilerOptions {
                target: Some(tsc_types::ScriptTarget::ES2022.bits()),
                module: Some(1),
                import_helpers: Some(true),
                ..CompilerOptions::default()
            },
        );
    let messages: Vec<&str> = result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code() == 2343)
        .map(|diagnostic| diagnostic.message_text())
        .collect();
    for helper in [
        "__esDecorate",
        "__runInitializers",
        "__setFunctionName",
        "__propKey",
    ] {
        assert!(
            messages.iter().any(|message| message.contains(helper)),
            "missing {helper}: {messages:#?}"
        );
    }
}

#[test]
fn parameter_initializer_ordering_reports_self_and_later_but_not_deferred() {
    assert_eq!(
        codes_of("function f(a = a, b = c, c = 1, d = () => e, e = 1) {}\n")
            .into_iter()
            .filter(|code| matches!(code, 2372 | 2373))
            .collect::<Vec<_>>(),
        [2372, 2373]
    );
}

#[test]
fn parameter_initializer_scope_change_honors_explicit_legacy_class_fields() {
    let result = check_program(
        &[InputFile::new(
            "a.ts".to_owned(),
            "class C {}\n((b = class extends C { static x = 1 }, d = x) => { var C; var x; })();\n"
                .to_owned(),
        )],
        &CompilerOptions {
            target: Some(tsc_types::ScriptTarget::ES_NEXT.bits()),
            use_define_for_class_fields: Some(false),
            ..CompilerOptions::default()
        },
    );
    let rows: Vec<(u32, u32, u32)> = result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code() == 2373)
        .map(|diagnostic| {
            (
                diagnostic.code(),
                diagnostic.start.unwrap_or_default(),
                diagnostic.length.unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(rows, [(2373, 31, 1), (2373, 55, 1)]);
}

#[test]
fn missing_import_meta_global_is_public_semantic_diagnostic() {
    assert_eq!(
        codes_of_with_options(
            "const x = import.meta;\n",
            &CompilerOptions {
                module: Some(99),
                ..CompilerOptions::default()
            },
        ),
        [2318]
    );
}

#[test]
fn missing_generator_fallback_global_is_public_semantic_diagnostic() {
    assert_eq!(codes_of("function* f() { yield 1; }\n"), [2318]);
}

#[test]
fn ts_nocheck_does_not_publish_missing_generator_globals() {
    let codes = codes_of("// @ts-nocheck\nfunction* f() { yield 1; }\n");
    assert!(codes.is_empty(), "{codes:?}");
}

#[test]
fn check_js_false_does_not_publish_missing_generator_globals() {
    let result = check_program(
        &[InputFile::new(
            "a.js".to_owned(),
            "function* f() { yield 1; }\n".to_owned(),
        )],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(false),
            ..CompilerOptions::default()
        },
    );
    assert!(result.diagnostics.is_empty(), "{:#?}", result.diagnostics);
}

#[test]
fn node16_esm_import_of_commonjs_has_synthetic_default_even_when_option_is_false() {
    let result = check_program(
        &[
            InputFile::new(
                "dep.cts".to_owned(),
                "declare const value: { x: number };\nexport = value;\n".to_owned(),
            ),
            InputFile::new(
                "main.mts".to_owned(),
                "import value from \"./dep.cjs\";\nvalue.x;\n".to_owned(),
            ),
        ],
        &CompilerOptions {
            module: Some(100),
            module_resolution: Some(3),
            allow_synthetic_default_imports: Some(false),
            es_module_interop: Some(false),
            ..CompilerOptions::default()
        },
    );
    let codes: Vec<u32> = result
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code())
        .collect();
    assert!(
        !codes.contains(&1259) && !codes.contains(&1192) && !codes.contains(&1203),
        "native ESM-to-CJS default interop should be accepted: {:#?}",
        result.diagnostics
    );
}

#[test]
fn node16_package_commonjs_target_has_synthetic_default() {
    let result = check_program(
        &[
            InputFile::new(
                "esm/package.json".to_owned(),
                "{\"type\":\"module\"}\n".to_owned(),
            ),
            InputFile::new(
                "cjs/package.json".to_owned(),
                "{\"type\":\"commonjs\"}\n".to_owned(),
            ),
            InputFile::new("cjs/dep.ts".to_owned(), "export const ok = 1;\n".to_owned()),
            InputFile::new(
                "esm/main.ts".to_owned(),
                "import value from \"../cjs/dep.js\";\nvalue.ok;\n".to_owned(),
            ),
        ],
        &CompilerOptions {
            module: Some(100),
            module_resolution: Some(3),
            allow_synthetic_default_imports: Some(false),
            es_module_interop: Some(false),
            ..CompilerOptions::default()
        },
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code() == 1192),
        "package-scoped CommonJS target should have a synthetic default: {:#?}",
        result.diagnostics
    );
}

#[test]
fn node16_mode_mismatch_details_preserve_package_type_evidence() {
    let run = |package_json: &str| {
        check_program(
            &[
                InputFile::new("/package.json".to_owned(), package_json.to_owned()),
                InputFile::new(
                    "/module.mts".to_owned(),
                    "export const value = 1;\n".to_owned(),
                ),
                InputFile::new(
                    "/common.cts".to_owned(),
                    "import { value } from \"./module.mjs\";\nvalue;\n".to_owned(),
                ),
                InputFile::new(
                    "/common.js".to_owned(),
                    "import { value } from \"./module.mjs\";\nvalue;\n".to_owned(),
                ),
                InputFile::new(
                    "/common.ts".to_owned(),
                    "import { value } from \"./module.mjs\";\nvalue;\n".to_owned(),
                ),
                InputFile::new(
                    "/common.tsx".to_owned(),
                    "import { value } from \"./module.mjs\";\nvalue;\n".to_owned(),
                ),
            ],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                module: Some(100),
                module_resolution: Some(3),
                target: Some(tsc_types::ScriptTarget::ES2022.bits()),
                ..CompilerOptions::default()
            },
        )
    };
    let detail_codes = |result: &CheckResult| {
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 1479)
            .map(|diagnostic| {
                (
                    diagnostic
                        .file_name
                        .as_deref()
                        .expect("mode mismatch is located")
                        .to_owned(),
                    diagnostic.message.next.first().map(|detail| detail.code),
                )
            })
            .collect::<Vec<_>>()
    };

    assert_eq!(
        detail_codes(&run("{}\n")),
        [
            ("/common.cts".to_owned(), None),
            ("/common.js".to_owned(), Some(1481)),
            ("/common.ts".to_owned(), Some(1481)),
            ("/common.tsx".to_owned(), Some(1482)),
        ]
    );
    assert_eq!(
        detail_codes(&run("{\"type\":\"commonjs\"}\n")),
        [
            ("/common.cts".to_owned(), None),
            ("/common.js".to_owned(), Some(1480)),
            ("/common.ts".to_owned(), Some(1480)),
            ("/common.tsx".to_owned(), Some(1483)),
        ]
    );
}

#[test]
fn node16_mode_mismatch_selects_construct_and_honors_overrides() {
    let result = check_program(
            &[
                InputFile::new("/module.mts".to_owned(), "export type T = number;\n".to_owned()),
                InputFile::new("/common.cts".to_owned(), "import value = require(\"./module.mjs\");\n\
                           import type {} from \"./module.mjs\";\n\
                           import type {} from \"./module.mjs\" with { \"resolution-mode\": \"import\" };\n\
                           type Plain = typeof import(\"./module.mjs\");\n\
                           type Overridden = typeof import(\"./module.mjs\", { with: { \"resolution-mode\": \"import\" } });\n\
                           const dynamic = import(\"./module.mjs\");\n\
                           void value;\nvoid dynamic;\n"
                        .to_owned()),
            ],
            &CompilerOptions {
                module: Some(100),
                module_resolution: Some(3),
                target: Some(tsc_types::ScriptTarget::ES2022.bits()),
                ..CompilerOptions::default()
            },
        );
    let mismatch_codes = result
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code())
        .filter(|code| matches!(code, 1471 | 1479 | 1541 | 1542))
        .collect::<Vec<_>>();
    assert_eq!(
        mismatch_codes,
        [1471, 1541, 1542],
        "{:#?}",
        result.diagnostics
    );
}

#[test]
fn node16_mode_mismatch_resolves_package_conditions_and_patterns_without_publishing_symbols() {
    let result = check_program(
            &[
                InputFile::new("/node_modules/pkg/package.json".to_owned(), "{\"exports\":{\"./exact\":{\"require\":\"./esm.mjs\",\"import\":\"./cjs.cjs\"},\"./pattern/*\":\"./*.mjs\"}}\n".to_owned()),
                InputFile::new("/node_modules/pkg/esm.mts".to_owned(), "export const exact = 1;\n".to_owned()),
                InputFile::new("/node_modules/pkg/value.mts".to_owned(), "export const pattern = 1;\n".to_owned()),
                InputFile::new("/consumer.cts".to_owned(), "import { exact } from \"pkg/exact\";\n\
                           import { pattern } from \"pkg/pattern/value\";\n\
                           exact;\npattern;\n"
                        .to_owned()),
            ],
            &CompilerOptions {
                module: Some(100),
                module_resolution: Some(3),
                target: Some(tsc_types::ScriptTarget::ES2022.bits()),
                ..CompilerOptions::default()
            },
        );
    let mismatch_codes = result
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code())
        .filter(|code| *code == 1479)
        .collect::<Vec<_>>();
    assert_eq!(mismatch_codes, [1479, 1479], "{:#?}", result.diagnostics);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|diagnostic| matches!(diagnostic.code(), 2305 | 2551)),
        "diagnostic-only package resolution must not publish target members: {:#?}",
        result.diagnostics
    );
}

#[test]
fn bundler_does_not_infer_plain_target_format_from_package_scope() {
    let result = check_program(
            &[
                InputFile::new("/package.json".to_owned(), "{\"type\":\"module\"}\n".to_owned()),
                InputFile::new("/plain.ts".to_owned(), "declare const plain: number;\nexport = plain;\n".to_owned()),
                InputFile::new("/decisive.mts".to_owned(), "declare const decisive: number;\nexport = decisive;\n".to_owned()),
                InputFile::new("/consumer.ts".to_owned(), "import plain from \"./plain\";\nimport decisive from \"./decisive.mts\";\nplain;\ndecisive;\n"
                        .to_owned()),
            ],
            &CompilerOptions {
                module: Some(99),
                module_resolution: Some(100),
                target: Some(tsc_types::ScriptTarget::ES2022.bits()),
                allow_synthetic_default_imports: Some(true),
                ..CompilerOptions::default()
            },
        );
    let rows: Vec<(String, u32, u32, u32)> = result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code() == 1192)
        .map(|diagnostic| {
            (
                diagnostic.file_name.clone().expect("located diagnostic"),
                diagnostic.code(),
                diagnostic.start.expect("located diagnostic"),
                diagnostic.length.expect("located diagnostic"),
            )
        })
        .collect();
    assert_eq!(rows, [("/consumer.ts".to_owned(), 1192, 36, 8)]);
}

#[test]
fn emit_format_distinguishes_explicit_commonjs_from_missing_package_type() {
    let result = check_program(
        &[
            InputFile::new(
                "/node_modules/cjs/package.json".to_owned(),
                "{\"type\":\"commonjs\"}\n".to_owned(),
            ),
            InputFile::new(
                "/node_modules/cjs/index.ts".to_owned(),
                "export const value = 1;\n".to_owned(),
            ),
            InputFile::new(
                "/node_modules/other/package.json".to_owned(),
                "{}\n".to_owned(),
            ),
            InputFile::new(
                "/node_modules/other/index.ts".to_owned(),
                "export const value = 1;\n".to_owned(),
            ),
        ],
        &CompilerOptions {
            module: Some(99),
            module_resolution: Some(100),
            target: Some(tsc_types::ScriptTarget::ES2022.bits()),
            verbatim_module_syntax: Some(true),
            ..CompilerOptions::default()
        },
    );
    let rows: Vec<(String, u32, u32, u32)> = result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code() == 1287)
        .map(|diagnostic| {
            (
                diagnostic.file_name.clone().expect("located diagnostic"),
                diagnostic.code(),
                diagnostic.start.expect("located diagnostic"),
                diagnostic.length.expect("located diagnostic"),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [("/node_modules/cjs/index.ts".to_owned(), 1287, 0, 6)]
    );
}

#[test]
fn node16_json_declaration_rejects_named_esm_imports() {
    let result = check_program(
        &[
            InputFile::new(
                "data.d.json.ts".to_owned(),
                "export const x: number;\n".to_owned(),
            ),
            InputFile::new(
                "main.mts".to_owned(),
                "import data, { x } from \"./data.d.json.ts\";\ndata.x;\nx;\n".to_owned(),
            ),
        ],
        &CompilerOptions {
            module: Some(100),
            module_resolution: Some(3),
            allow_importing_ts_extensions: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert!(result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code() == 1544));
}

#[test]
fn node18_json_default_import_requires_type_attribute() {
    let files = |main: &str| {
        vec![
            InputFile::new(
                "data.d.json.ts".to_owned(),
                "export const x: number;\n".to_owned(),
            ),
            InputFile::new("main.mts".to_owned(), main.to_owned()),
        ]
    };
    let options = CompilerOptions {
        module: Some(101),
        module_resolution: Some(3),
        allow_importing_ts_extensions: Some(true),
        ..CompilerOptions::default()
    };
    let missing = check_program(
        &files("import data from \"./data.d.json.ts\";\ndata.x;\n"),
        &options,
    );
    assert!(
        missing
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code() == 1543),
        "Node18 JSON import without an attribute should report 1543: {:#?}",
        missing.diagnostics
    );

    let attributed = check_program(
        &files("import data from \"./data.d.json.ts\" with { type: \"json\" };\ndata.x;\n"),
        &options,
    );
    assert!(
        !attributed
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code() == 1543),
        "a type: json attribute should satisfy the Node18 requirement: {:#?}",
        attributed.diagnostics
    );
}

#[test]
fn import_attributes_on_cjs_emit_report_2856_with_priority() {
    // tsc checkImportAttributes: the CommonJS-require row (2856)
    // rides the specifier's emit syntax and takes priority over
    // the type-only (2857) and resolution-mode (1454) rows. The
    // oracle-correction epoch made the row observable corpus-wide
    // (nodeModulesJson loosey.cts and the ImportAttributesMode
    // DeclarationEmit fixtures).
    let files = |main: &str| {
        vec![
            InputFile::new(
                "data.d.json.ts".to_owned(),
                "declare const _default: {};\nexport default _default;\n".to_owned(),
            ),
            InputFile::new("main.cts".to_owned(), main.to_owned()),
        ]
    };
    let options = CompilerOptions {
        module: Some(101),
        module_resolution: Some(3),
        allow_importing_ts_extensions: Some(true),
        ..CompilerOptions::default()
    };
    let plain = check_program(
        &files("import data from \"./data.d.json.ts\" with { type: \"json\" };\ndata;\n"),
        &options,
    );
    let codes: Vec<u32> = plain
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code())
        .collect();
    assert!(codes.contains(&2856), "{:#?}", plain.diagnostics);

    let type_only = check_program(
            &files(
                "import type data from \"./data.d.json.ts\" with { type: \"json\" };\nexport type T = typeof data;\n",
            ),
            &options,
        );
    let codes: Vec<u32> = type_only
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code())
        .collect();
    assert!(
        codes.contains(&2856) && !codes.contains(&2857),
        "the CommonJS-require row outranks the type-only row: {:#?}",
        type_only.diagnostics
    );
}

#[test]
fn node18_actual_json_module_is_resolved_and_typed() {
    let result = check_program(
        &[
            InputFile::new(
                "package.json".to_owned(),
                "{\"type\":\"module\"}\n".to_owned(),
            ),
            InputFile::new(
                "data.json".to_owned(),
                "{\"count\": 1, \"label\": \"ok\"}\n".to_owned(),
            ),
            InputFile::new(
                "main.ts".to_owned(),
                "import data from \"./data.json\";\n\
                           let count: number;\n\
                           count = data.count;\n\
                           let wrong: string;\n\
                           wrong = data.count;\n"
                    .to_owned(),
            ),
        ],
        &CompilerOptions {
            module: Some(101),
            module_resolution: Some(3),
            resolve_json_module: Some(true),
            ..CompilerOptions::default()
        },
    );
    let codes: Vec<u32> = result
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code())
        .collect();
    assert!(codes.contains(&1543), "{:#?}", result.diagnostics);
    assert!(codes.contains(&2322), "{:#?}", result.diagnostics);
    assert!(!codes.contains(&2307), "{:#?}", result.diagnostics);
}

#[test]
fn node20_commonjs_default_import_uses_module_exports_export() {
    let result = check_program(
        &[
            InputFile::new(
                "dep.mts".to_owned(),
                "const value = { a: 1 };\nexport { value as \"module.exports\" };\n".to_owned(),
            ),
            InputFile::new(
                "main.cts".to_owned(),
                "import value from \"./dep.mjs\";\nvalue.a;\n".to_owned(),
            ),
        ],
        &CompilerOptions {
            module: Some(102),
            module_resolution: Some(3),
            es_module_interop: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert!(
        result.diagnostics.is_empty(),
        "Node20 module.exports interop should resolve the default: {:#?}",
        result.diagnostics
    );
}

#[test]
fn node20_module_exports_default_import_requires_explicit_interop_when_disabled() {
    let result = check_program(
        &[
            InputFile::new(
                "dep.mts".to_owned(),
                "const value = { a: 1 };\nexport { value as \"module.exports\" };\n".to_owned(),
            ),
            InputFile::new(
                "main.cts".to_owned(),
                "import value from \"./dep.mjs\";\nvalue.a;\n".to_owned(),
            ),
        ],
        &CompilerOptions {
            module: Some(102),
            module_resolution: Some(3),
            es_module_interop: Some(false),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [1259],
        "{:#?}",
        result.diagnostics
    );
}

#[test]
fn node20_module_exports_precedes_syntactic_default() {
    let result = check_program(
        &[
            InputFile::new(
                "dep.mts".to_owned(),
                "export default function actual(x: string): string { return x; }\n\
                           const compat = (x: number) => x;\n\
                           export { compat as \"module.exports\" };\n"
                    .to_owned(),
            ),
            InputFile::new(
                "main.cts".to_owned(),
                "import fn from \"./dep.mjs\";\nfn(1);\nfn(\"x\");\n".to_owned(),
            ),
        ],
        &CompilerOptions {
            module: Some(102),
            module_resolution: Some(3),
            ..CompilerOptions::default()
        },
    );
    let errors: Vec<&tsc_diagnostics::Diagnostic> = result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code() == 2345)
        .collect();
    assert_eq!(errors.len(), 1, "{:#?}", result.diagnostics);
    assert_eq!(
        errors[0].message_text(),
        "Argument of type 'string' is not assignable to parameter of type 'number'."
    );
}

#[test]
fn checked_cjs_require_of_node20_esm_namespace_is_not_constructable() {
    for es_module_interop in [true, false] {
        let result = check_program(
            &[
                InputFile::new(
                    "/exporter.mts".to_owned(),
                    "export default class Foo {}\n\
                               const oops = \"oops\";\n\
                               export { oops as \"module.exports\" };\n"
                        .to_owned(),
                ),
                InputFile::new(
                    "/importer.cjs".to_owned(),
                    "const Foo = require(\"./exporter.mjs\");\nnew Foo();\n".to_owned(),
                ),
            ],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                module: Some(102),
                module_resolution: Some(3),
                es_module_interop: Some(es_module_interop),
                ..CompilerOptions::default()
            },
        );
        let errors: Vec<&tsc_diagnostics::Diagnostic> = result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 2351)
            .collect();
        assert_eq!(
            errors.len(),
            1,
            "diagnostics={:#?}\npartial={:#?}",
            result.diagnostics,
            result.partial_checks
        );
        assert_eq!(
            errors[0].message_text(),
            "This expression is not constructable."
        );
    }
}

#[test]
fn node20_namespace_import_uses_distinct_module_exports_export() {
    let result = check_program(
        &[
            InputFile::new(
                "dep.mts".to_owned(),
                "export default function actual(x: string): string { return x; }\n\
                           const compat = (x: number) => x;\n\
                           export { compat as \"module.exports\" };\n"
                    .to_owned(),
            ),
            InputFile::new(
                "main.cts".to_owned(),
                "import * as fn from \"./dep.mjs\";\nfn(1);\nfn(\"x\");\n".to_owned(),
            ),
        ],
        &CompilerOptions {
            module: Some(102),
            module_resolution: Some(3),
            ..CompilerOptions::default()
        },
    );
    let errors: Vec<&tsc_diagnostics::Diagnostic> = result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code() == 2345)
        .collect();
    assert_eq!(errors.len(), 1, "{:#?}", result.diagnostics);
    assert_eq!(
        errors[0].message_text(),
        "Argument of type 'string' is not assignable to parameter of type 'number'."
    );
}

#[test]
fn node20_namespace_import_uses_module_exports_even_when_it_aliases_default() {
    let result = check_program(
        &[
            InputFile::new(
                "dep.mts".to_owned(),
                "const compat = (x: number) => x;\n\
                           export default compat;\n\
                           export { compat as \"module.exports\" };\n"
                    .to_owned(),
            ),
            InputFile::new(
                "main.cts".to_owned(),
                "import * as fn from \"./dep.mjs\";\nfn(1);\n".to_owned(),
            ),
        ],
        &CompilerOptions {
            module: Some(102),
            module_resolution: Some(3),
            ..CompilerOptions::default()
        },
    );
    assert!(result.diagnostics.is_empty(), "{:#?}", result.diagnostics);
}

fn js_pair_diagnostics(js: &str, ts: &str) -> Vec<(u32, Option<String>)> {
    check_program(
        &[
            InputFile::new("a.js".to_owned(), js.to_owned()),
            InputFile::new("b.ts".to_owned(), ts.to_owned()),
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..strict_options()
        },
    )
    .diagnostics
    .into_iter()
    .map(|diagnostic| (diagnostic.code(), diagnostic.file_name))
    .collect()
}

#[test]
fn unrelated_destructuring_sibling_guard_keeps_property_miss() {
    assert_eq!(
        codes_of_with_options(
            "function f({a,b}:{a:boolean,b:number}){if(a){b.missing;}}",
            &strict_options(),
        ),
        [2339]
    );
}

#[test]
fn concrete_destructuring_equality_guard_keeps_property_miss() {
    assert_eq!(
        codes_of_with_options(
            "function f({a,b}:{a:boolean,b:number}){if(a===true){b.missing;}}",
            &strict_options(),
        ),
        [2339]
    );
}

#[test]
fn discriminated_destructuring_sibling_still_narrows() {
    assert_eq!(
        codes_of_with_options(
            "type A={kind:'A',payload:{a:number}}|{kind:'B',payload:{b:number}};\
                 function f({kind,payload}:A){if(kind==='A'){payload.a;}}",
            &strict_options(),
        ),
        Vec::<u32>::new()
    );
}

fn full_lib_bundle(target_libs: &[&str]) -> Vec<InputFile> {
    let base = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../vendor/typescript-6.0.3/lib/"
    );
    target_libs
        .iter()
        .map(|name| {
            InputFile::new(
                (*name).to_owned(),
                std::fs::read_to_string(format!("{base}{name}")).expect("vendored lib"),
            )
        })
        .collect()
}

#[test]
fn in_operator_missing_key_join_keeps_later_const_key_narrowing() {
    // controlFlowInOperator: the missing-key branch and the later
    // `a in c` branch are independent; the latter narrows to A so
    // `c[a]` remains valid.
    let libs = full_lib_bundle(&[
        "lib.es6.d.ts",
        "lib.es5.d.ts",
        "lib.es2015.d.ts",
        "lib.dom.d.ts",
        "lib.dom.iterable.d.ts",
        "lib.webworker.importscripts.d.ts",
        "lib.scripthost.d.ts",
        "lib.es2015.core.d.ts",
        "lib.es2015.collection.d.ts",
        "lib.es2015.generator.d.ts",
        "lib.es2015.iterable.d.ts",
        "lib.es2015.promise.d.ts",
        "lib.es2015.proxy.d.ts",
        "lib.es2015.reflect.d.ts",
        "lib.es2015.symbol.d.ts",
        "lib.es2015.symbol.wellknown.d.ts",
        "lib.es2018.asynciterable.d.ts",
        "lib.decorators.d.ts",
        "lib.decorators.legacy.d.ts",
    ]);
    let options = CompilerOptions {
        strict: Some(true),
        target: Some(tsc_types::ScriptTarget::ES2015.bits()),
        ..CompilerOptions::default()
    };
    let result = check_program_with_libs(
            &libs,
            &[InputFile::new("a.ts".to_owned(), "const a = 'a';\nconst b = 'b';\nconst d = 'd';\ntype A = { [a]: number; };\ntype B = { [b]: string; };\ndeclare const c: A | B;\nif ('d' in c) {\n    c;\n}\nif (a in c) {\n    c;\n    c[a];\n}\n".to_owned())],
            &options,
        );
    let rows: Vec<(String, u32)> = result
        .diagnostics
        .iter()
        .filter(|d| d.file_name.as_deref() == Some("a.ts"))
        .map(|d| (d.file_name.clone().unwrap_or_default(), d.code()))
        .collect();
    assert_eq!(rows, Vec::<(String, u32)>::new());
}

#[test]
fn const_key_in_narrowing_indexes_late_bound_members() {
    // `a in c` narrows to A and `c[a]` resolves (oracle-clean).
    let text = "const a = 'a';\nconst b = 'b';\nconst d = 'd';\ntype A = { [a]: number; };\ntype B = { [b]: string; };\ndeclare const c: A | B;\nif (a in c) {\n    c;\n    c[a];\n}\n";
    assert_eq!(
        lib_codes_of_with_options(text, &strict_options()),
        Vec::<u32>::new()
    );
}

#[test]
fn for_in_over_optional_chain_stays_clean() {
    // tsc #51941 (canary FP controlFlowOptionalChain f50): the
    // body's obj.main read must not 18048; the optional-chain
    // condition narrows the body read.
    let text = "type Test5 = {\n  main?: {\n    childs: Record<string, Test5>;\n  };\n};\nfunction f50(obj: Test5) {\n   for (const key in obj.main?.childs) {\n      if (obj.main.childs[key] === obj) {\n        return obj;\n      }\n   }\n   return null;\n}\n";
    assert_eq!(
        lib_codes_of_with_options(text, &strict_options()),
        Vec::<u32>::new()
    );
}

#[test]
fn overload_failure_promise_intersection_awaits_to_never() {
    // The combined overload-failure signature returns the
    // INTERSECTION of candidate returns (tsc 76907); awaiting it
    // unwraps through the intersected structural `then` to never,
    // so the loop-carried assignment stays silent — only the 2769
    // reports (oracle-exact; the un-unwrapped promise was the
    // 6.6f 2322 FP face).
    let libs = full_lib_bundle(&[
        "lib.es6.d.ts",
        "lib.es5.d.ts",
        "lib.es2015.d.ts",
        "lib.es2015.core.d.ts",
        "lib.es2015.collection.d.ts",
        "lib.es2015.generator.d.ts",
        "lib.es2015.iterable.d.ts",
        "lib.es2015.promise.d.ts",
        "lib.es2015.proxy.d.ts",
        "lib.es2015.reflect.d.ts",
        "lib.es2015.symbol.d.ts",
        "lib.es2015.symbol.wellknown.d.ts",
        "lib.es2018.asynciterable.d.ts",
        "lib.decorators.d.ts",
        "lib.decorators.legacy.d.ts",
    ]);
    let options = CompilerOptions {
        strict: Some(true),
        target: Some(tsc_types::ScriptTarget::ES2015.bits()),
        ..CompilerOptions::default()
    };
    let result = check_program_with_libs(
            &libs,
            &[InputFile::new("a.ts".to_owned(), "declare const cond: boolean;\ndeclare function foo(x: string): Promise<number>;\ndeclare function foo(x: number): Promise<string>;\nasync function g1() {\n    let x: string | number | boolean;\n    x = \"\";\n    while (cond) {\n        x = await foo(x);\n        x;\n    }\n    x;\n}\n".to_owned())],
            &options,
        );
    let rows: Vec<(u32, u32)> = result
        .diagnostics
        .iter()
        .filter(|d| d.file_name.as_deref() == Some("a.ts"))
        .map(|d| (d.code(), d.start.unwrap_or(0)))
        .collect();
    assert_eq!(rows, [(2769, 242)]);
}

#[test]
fn async_iteration_fixture_reports_no_spurious_2322() {
    let libs = full_lib_bundle(&[
        "lib.es6.d.ts",
        "lib.es5.d.ts",
        "lib.es2015.d.ts",
        "lib.es2015.core.d.ts",
        "lib.es2015.collection.d.ts",
        "lib.es2015.generator.d.ts",
        "lib.es2015.iterable.d.ts",
        "lib.es2015.promise.d.ts",
        "lib.es2015.proxy.d.ts",
        "lib.es2015.reflect.d.ts",
        "lib.es2015.symbol.d.ts",
        "lib.es2015.symbol.wellknown.d.ts",
        "lib.es2018.asynciterable.d.ts",
        "lib.decorators.d.ts",
        "lib.decorators.legacy.d.ts",
    ]);
    let options = CompilerOptions {
        strict: Some(true),
        target: Some(tsc_types::ScriptTarget::ES2015.bits()),
        ..CompilerOptions::default()
    };
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../ts-tests/tests/cases/conformance/controlFlow/controlFlowIterationErrorsAsync.ts"
    ))
    .expect("fixture")
    .lines()
    .filter(|line| !line.trim_start().starts_with("// @"))
    .collect::<Vec<_>>()
    .join("\n");
    let result =
        check_program_with_libs(&libs, &[InputFile::new("a.ts".to_owned(), text)], &options);
    let rows: Vec<u32> = result
        .diagnostics
        .iter()
        .filter(|d| d.file_name.as_deref() == Some("a.ts"))
        .map(|d| d.code())
        .collect();
    assert_eq!(
        rows.iter().filter(|&&c| c == 2322).count(),
        0,
        "rows: {rows:?}"
    );
}

#[test]
fn computed_key_destructuring_assignment_contains() {
    // The evaluation-order family (tsc PR #41094) defers to M6 —
    // the const-bb rows partial-mark instead of misreporting
    // (controlFlowAssignmentPatternOrder).
    let text = "let a: 0 | 1 = 0;\nlet b: 0 | 1 | 8 | 9;\n[{ [(a = 1)]: b } = [9, a] as const] = [[9, 8] as const];\nconst bb: 0 | 8 = b;\n";
    assert_eq!(
        lib_codes_of_with_options(text, &strict_options()),
        Vec::<u32>::new()
    );
}

#[test]
fn destructuring_assignment_reads_apparent_type_members() {
    // getTypeOfPropertyOfType has no receiver-flags guard (55803;
    // 6.6 review A1) — string.length resolves via the reduced
    // apparent type and the assigned type narrows; tsc is clean.
    assert_eq!(
        lib_codes_of_with_options(
            "let n: number | string = 0;\n({ length: n } = \"abc\");\nconst m: number = n;\n",
            &strict_options()
        ),
        Vec::<u32>::new()
    );
}

#[test]
fn body_predicate_narrows_reference_inside_compound_return() {
    // The inferred predicate narrows `u` before the array literal
    // is checked against the annotated return type.
    assert_eq!(
            lib_codes_of_with_options(
                "function isNum(x: string | number) { return typeof x === \"number\"; }\nfunction g(u: string | number): number[] { if (isNum(u)) { return [u]; } return [0]; }\n",
                &strict_options()
            ),
            Vec::<u32>::new()
        );
}

fn lib_codes_of_with_options(source: &str, options: &CompilerOptions) -> Vec<u32> {
    let result = check_program_with_libs(
        &[es5_lib()],
        &[InputFile::new("a.ts".to_owned(), source.to_owned())],
        options,
    );
    result.diagnostics.iter().map(|d| d.code()).collect()
}

// The three redeclaration pins below run WITH lib.es5 — the real
// autoArrayType (6.2) is Array<auto>, which needs the global Array
// to mint and render (`any[]`). The lib-less env degrades to a
// display partial, matching tsc --noLib's own no-2403 output.
#[test]
fn empty_array_redeclaration_still_reports_incompatible_type() {
    assert_eq!(
        lib_codes_of_with_options("var x = [];\nvar x = 1;\n", &strict_options()),
        [2403]
    );
}

#[test]
fn shadowed_array_function_does_not_trigger_evolving_array_containment() {
    assert_eq!(
        lib_codes_of_with_options(
            "function f(){function Array():number{return 1};var x=[];var x=Array();return x;}",
            &strict_options(),
        ),
        [2403]
    );
}

#[test]
fn array_returning_call_redeclaration_reports_2403() {
    // Pre-6.2 this scenario was CONTAINED (the evolving-array
    // stand-in rendered the wrong first-type face); the real
    // autoArrayType retires the escape and matches the oracle.
    assert_eq!(
        lib_codes_of_with_options(
            "declare function makeArray():number[];var x=[];var x=makeArray();",
            &strict_options(),
        ),
        [2403]
    );
}

#[test]
fn ts_const_function_expression_reads_assignment_members_normally() {
    assert_eq!(
        codes_of(
            "const f = function () { return true; };\n\
                 f.extra = 1;\n\
                 const value: number = f.extra;\n\
                 f.missing;\n"
        ),
        [2339]
    );
}

#[test]
fn expando_member_uses_annotated_parent_property_type() {
    assert_eq!(
        codes_of(
            "interface F { (): boolean; value: 123; }\n\
                 const f: F = () => true;\n\
                 f.value = 123;\n"
        ),
        Vec::<u32>::new()
    );
}

fn checked_js_codes(source: &str) -> Vec<u32> {
    check_program(
        &[InputFile::new("a.js".to_owned(), source.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            no_implicit_any: Some(true),
            ..CompilerOptions::default()
        },
    )
    .diagnostics
    .iter()
    .map(|diagnostic| diagnostic.code())
    .collect()
}

fn checked_js_codes_with_function_prototype(source: &str) -> Vec<u32> {
    // getPropertyOfType 59348-59389 augments a callable with the
    // global Function face. The upstream fixture uses the default
    // lib, whose lib.es5.d.ts:299 declares `prototype: any`.
    check_program_with_libs(
        &[es5_lib()],
        &[InputFile::new("a.js".to_owned(), source.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            no_implicit_any: Some(true),
            ..CompilerOptions::default()
        },
    )
    .diagnostics
    .iter()
    .map(|diagnostic| diagnostic.code())
    .collect()
}

#[test]
fn checked_js_bare_prototype_access_type_annotates_the_assignment_symbol() {
    // getWidenedTypeForAssignmentDeclaration 56247-56263 keeps a
    // bare access declaration as the expression, so its @type
    // participates in the earlier constructor assignment.
    assert_eq!(
        checked_js_codes_with_function_prototype(
            "function C() { this.x = false; }\n\
                 /** @type {number} */\n\
                 C.prototype.x;\n\
                 new C().x;\n"
        ),
        [2322]
    );
}

#[test]
fn checked_js_bare_prototype_access_without_type_does_not_constrain_the_assignment() {
    assert_eq!(
        checked_js_codes_with_function_prototype(
            "function C() { this.x = false; }\n\
                 C.prototype.x;\n\
                 new C().x;\n"
        ),
        Vec::<u32>::new()
    );
}

#[test]
fn checked_js_chained_prototype_replacement_uses_the_rightmost_object_literal() {
    // getAssignedJSPrototype 77594-77606 reads
    // getInitializerOfBinaryExpression, so both A and B acquire
    // the object-literal class face.
    assert_eq!(
        checked_js_codes(
            "var A = function A() {};\n\
                 var B = function B() {};\n\
                 A.prototype = B.prototype = {\n\
                   /** @param {number} n */\n\
                   m(n) { return n + 1; }\n\
                 };\n\
                 new A().m('bad');\n\
                 new B().m('bad');\n"
        ),
        [2345, 2345]
    );
}

#[test]
fn checked_js_non_object_chained_prototype_replacement_does_not_invent_members() {
    let codes = checked_js_codes(
        // isJSConstructor 77509-77522 requires an instance member:
        // establish constructability before the primitive prototype
        // assignment, then verify that the assignment neither removes
        // that face nor invents `missing`.
        "var A = function A() { this.a = 1; };\n\
             var B = function B() { this.b = 2; };\n\
             A.prototype = B.prototype = 0;\n\
             new A().missing;\n",
    );
    assert!(codes.contains(&2339), "{codes:?}");
    assert!(!codes.contains(&7009), "{codes:?}");
}

#[test]
fn checked_js_exported_arrow_expando_keeps_its_own_property_annotation() {
    // getTypeOfFuncClassEnumModule 56808-56827 publishes the
    // merged initializer/expando type on both link faces.
    assert_eq!(
        checked_js_codes(
            "/** @type {{ (): boolean; nuo: 789 }} */\n\
                 export const conflicting = () => true;\n\
                 /** @type {1000} */\n\
                 conflicting.nuo = 789;\n"
        ),
        [2322]
    );
}

#[test]
fn checked_js_exported_arrow_matching_expando_annotation_is_clean() {
    assert_eq!(
        checked_js_codes(
            "/** @type {{ (): boolean; nuo: 789 }} */\n\
                 export const matching = () => true;\n\
                 /** @type {789} */\n\
                 matching.nuo = 789;\n"
        ),
        Vec::<u32>::new()
    );
}

#[test]
fn function_return_annotation_is_not_an_expando_parent_annotation() {
    assert_eq!(
        lib_codes_of_with_options(
            "function f(): number { return 1; }\nf.toFixed = \"own\";\n",
            &CompilerOptions::default(),
        ),
        Vec::<u32>::new()
    );
}

#[test]
fn plain_js_object_reference_warning_requires_strict_equality() {
    let result = check_program(
        &[InputFile::new(
            "a.js".to_owned(),
            "if ({} === {}) {}\nif ({} == {}) {}\n".to_owned(),
        )],
        &CompilerOptions {
            allow_js: true,
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [2839]
    );
}

#[test]
fn js_declared_container_property_miss_in_ts_file_reports() {
    assert_eq!(
        js_pair_diagnostics("class C {}", "const c = new C(); c.missing;"),
        [(2339, Some("b.ts".to_owned()))]
    );
}

#[test]
fn js_assignment_declared_class_member_stays_available() {
    assert!(js_pair_diagnostics("class C {}\nC.extra = 1;", "C.extra;").is_empty());
}

#[test]
fn shadowed_js_class_assignment_does_not_open_outer_class() {
    assert_eq!(
        js_pair_diagnostics(
            "class C {}\nfunction f(){class C {}\nC.extra = 1;}",
            "C.extra;",
        ),
        [(2339, Some("b.ts".to_owned()))]
    );
}

#[test]
fn js_assignment_declared_function_member_stays_available() {
    assert!(js_pair_diagnostics("function F() {}\nF.extra = 1;", "F.extra;").is_empty());
}

#[test]
fn js_assignment_declared_prototype_member_stays_available() {
    assert!(js_pair_diagnostics("class C {}\nC.prototype.extra = 1;", "new C().extra;").is_empty());
}

#[test]
fn js_static_assignment_does_not_open_instance_side() {
    assert_eq!(
        js_pair_diagnostics("class C {}\nC.extra = 1;", "new C().extra;"),
        [(2339, Some("b.ts".to_owned()))]
    );
}

#[test]
fn js_prototype_assignment_does_not_open_static_side() {
    assert_eq!(
        js_pair_diagnostics("class C {}\nC.prototype.extra = 1;", "C.extra;"),
        [(2339, Some("b.ts".to_owned()))]
    );
}

#[test]
fn js_static_this_assignment_does_not_open_instance_side() {
    assert_eq!(
        js_pair_diagnostics("class C { static { this.extra = 1; } }", "new C().extra;",),
        [(2339, Some("b.ts".to_owned()))]
    );
}

#[test]
fn js_instance_this_assignment_does_not_open_static_side() {
    assert_eq!(
        js_pair_diagnostics("class C { constructor() { this.extra = 1; } }", "C.extra;",),
        [(2339, Some("b.ts".to_owned()))]
    );
}

#[test]
fn js_static_this_assignment_stays_available_on_static_side() {
    assert!(js_pair_diagnostics("class C { static { this.extra = 1; } }", "C.extra;",).is_empty());
}

#[test]
fn js_instance_this_assignment_stays_available_on_instance_side() {
    assert!(js_pair_diagnostics(
        "class C { constructor() { this.extra = 1; } }",
        "new C().extra;",
    )
    .is_empty());
}

#[test]
fn nested_non_arrow_function_this_does_not_open_class_instance() {
    let diagnostics = js_pair_diagnostics(
        "class C { method() { function nested() { this.extra = 1; } nested(); } }",
        "new C().extra;",
    );
    assert!(
        diagnostics.contains(&(2339, Some("b.ts".to_owned()))),
        "a nested function owns its `this`: {diagnostics:?}"
    );
}

#[test]
fn nested_js_assignment_does_not_open_direct_static_member() {
    assert_eq!(
        js_pair_diagnostics(
            "class C {}\nC.bucket = {};\nC.bucket.extra = 1;",
            "C.extra;",
        ),
        [(2339, Some("b.ts".to_owned()))]
    );
}

#[test]
fn nested_js_assignment_still_opens_its_actual_receiver() {
    assert!(js_pair_diagnostics(
        "class C {}\nC.bucket = {};\nC.bucket.extra = 1;",
        "C.bucket.extra;",
    )
    .is_empty());
}

#[test]
fn unresolved_module_augmentation_keeps_unrelated_property_miss() {
    let diagnostics = check_program(
            &[
                InputFile::new("augmentation.ts".to_owned(), "export {};\ndeclare module \"pkg\" { interface X { missing(): void } }\n(\"x\").missing;\n"
                        .to_owned()),
                // An unrelated package scope does not make "pkg"
                // resolvable and therefore must not hide 2664.
                InputFile::new("package.json".to_owned(), "{}".to_owned()),
            ],
            &CompilerOptions::default(),
        )
        .diagnostics;
    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [2664, 2339]
    );
}

#[test]
fn unresolved_module_augmentation_does_not_open_same_named_local_type() {
    let diagnostics = check_program(
            &[
                InputFile::new("node_modules/pkg/index.d.ts".to_owned(), "export interface X {}\n".to_owned()),
                InputFile::new("augmentation.ts".to_owned(), "export {};\ndeclare module \"pkg\" { interface X { missing(): void } }\ninterface X {}\ndeclare const local: X;\nlocal.missing;\n"
                        .to_owned()),
            ],
            &CompilerOptions::default(),
        )
        .diagnostics;
    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [2339]
    );
}

#[test]
fn unresolved_bare_augmentation_does_not_claim_same_spelled_workspace_file() {
    let diagnostics = check_program(
        &[
            InputFile::new(
                "node_modules/other/index.d.ts".to_owned(),
                "export {};\n".to_owned(),
            ),
            InputFile::new(
                "pkg.ts".to_owned(),
                "interface X {}\ndeclare const local: X;\nlocal.missing;\n".to_owned(),
            ),
            InputFile::new(
                "augmentation.ts".to_owned(),
                "export {};\ndeclare module \"pkg\" { interface X { missing(): void } }\n"
                    .to_owned(),
            ),
        ],
        &CompilerOptions::default(),
    )
    .diagnostics;
    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [2664, 2339]
    );
}

#[test]
fn unresolved_module_augmentation_contains_index_signature_property() {
    let diagnostics = check_program(
            &[
                InputFile::new("node_modules/pkg/index.d.ts".to_owned(), "export as namespace Pkg;\nexport interface X {}\n".to_owned()),
                InputFile::new("augmentation.d.ts".to_owned(), "import * as Pkg from \"pkg\";\ndeclare module \"pkg\" { interface X { [key: string]: unknown } }\n"
                        .to_owned()),
                InputFile::new("use.ts".to_owned(), "declare const value: Pkg.X;\nvalue.anything;\n".to_owned()),
            ],
            &CompilerOptions::default(),
        )
        .diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.category() == DiagnosticCategory::Error)
        .collect::<Vec<_>>();
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
}

#[test]
fn unresolved_module_augmentation_contains_computed_property() {
    let result = check_program(
            &[
                InputFile::new("node_modules/pkg/index.d.ts".to_owned(), "export as namespace Pkg;\nexport interface X {}\n".to_owned()),
                InputFile::new("augmentation.d.ts".to_owned(), "import * as Pkg from \"pkg\";\ndeclare const member: \"extra\";\ndeclare module \"pkg\" { interface X { [member](): void } }\n"
                        .to_owned()),
                InputFile::new("use.ts".to_owned(), "declare const value: Pkg.X;\nvalue.extra();\n".to_owned()),
            ],
            &CompilerOptions::default(),
        );
    let diagnostics = result
        .diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.category() == DiagnosticCategory::Error)
        .collect::<Vec<_>>();
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    assert!(
        result.partial_checks.is_empty(),
        "{:#?}",
        result.partial_checks
    );
}

#[test]
fn unresolved_module_augmentation_matches_export_equals_namespace_target() {
    let diagnostics = check_program(
            &[
                InputFile::new("node_modules/pkg/index.d.ts".to_owned(), "export as namespace Pkg;\nexport = Package;\ndeclare namespace Package { class X {} }\n"
                        .to_owned()),
                InputFile::new("augmentation.d.ts".to_owned(), "import * as Pkg from \"pkg\";\ndeclare module \"pkg\" { interface X { added(): void } }\n"
                        .to_owned()),
                InputFile::new("use.ts".to_owned(), "declare const value: Pkg.X;\nvalue.added();\nfunction use<T extends Pkg.X>(item: T) { item.added(); }\ndeclare const mixed: Pkg.X | { added(): void };\nmixed.added();\n"
                        .to_owned()),
            ],
            &CompilerOptions::default(),
        )
        .diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.category() == DiagnosticCategory::Error)
        .collect::<Vec<_>>();
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
}

#[test]
fn unresolved_module_augmentation_does_not_open_sibling_package_subpath() {
    let diagnostics = check_program(
            &[
                InputFile::new("node_modules/pkg/a.d.ts".to_owned(), "export as namespace PkgA;\nexport interface X {}\n".to_owned()),
                InputFile::new("node_modules/pkg/b.d.ts".to_owned(), "export as namespace PkgB;\nexport interface X {}\n".to_owned()),
                InputFile::new("augmentation.d.ts".to_owned(), "import * as PkgA from \"pkg/a\";\ndeclare module \"pkg/a\" { interface X { added(): void } }\n"
                        .to_owned()),
                InputFile::new("use.ts".to_owned(), "declare const aValue: PkgA.X;\naValue.added();\ndeclare const bValue: PkgB.X;\nbValue.added();\n"
                        .to_owned()),
            ],
            &CompilerOptions::default(),
        )
        .diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.category() == DiagnosticCategory::Error)
        .collect::<Vec<_>>();
    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [2339]
    );
}

#[test]
fn unresolved_module_augmentation_stays_with_nearest_package_instance() {
    let diagnostics = check_program(
            &[
                InputFile::new("app1/node_modules/pkg/index.d.ts".to_owned(), "export as namespace PkgOne;\nexport interface X {}\n".to_owned()),
                InputFile::new("app2/node_modules/pkg/index.d.ts".to_owned(), "export as namespace PkgTwo;\nexport interface X {}\n".to_owned()),
                InputFile::new("app1/augmentation.d.ts".to_owned(), "import * as PkgOne from \"pkg\";\ndeclare module \"pkg\" { interface X { added(): void } }\n"
                        .to_owned()),
                InputFile::new("app2/use.ts".to_owned(), "declare const one: PkgOne.X;\none.added();\ndeclare const two: PkgTwo.X;\ntwo.added();\n"
                        .to_owned()),
            ],
            &CompilerOptions::default(),
        )
        .diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.category() == DiagnosticCategory::Error)
        .collect::<Vec<_>>();
    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [2339]
    );
}

#[test]
fn unresolved_node_core_augmentation_matches_only_its_at_types_node_subpath() {
    let diagnostics = check_program(
            &[
                InputFile::new("node_modules/@types/node/fs.d.ts".to_owned(), "export as namespace NodeFs;\nexport interface X {}\n".to_owned()),
                InputFile::new("node_modules/@types/node/http.d.ts".to_owned(), "export as namespace NodeHttp;\nexport interface X {}\n".to_owned()),
                InputFile::new("augmentation.d.ts".to_owned(), "import * as NodeFs from \"node:fs\";\ndeclare module \"node:fs\" { interface X { added(): void } }\n"
                        .to_owned()),
                InputFile::new("use.ts".to_owned(), "declare const fsValue: NodeFs.X;\nfsValue.added();\ndeclare const httpValue: NodeHttp.X;\nhttpValue.added();\n"
                        .to_owned()),
            ],
            &CompilerOptions::default(),
        )
        .diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.category() == DiagnosticCategory::Error)
        .collect::<Vec<_>>();
    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [2591, 2339, 2339]
    );
}

#[test]
fn value_side_member_publication_survives_reentrant_base_resolution() {
    let diagnostics = codes_of(
            "class B {}\nclass A extends A.make() {\n  static make(): typeof B { return B; }\n}\nA.make();\n",
        );
    assert!(
        !diagnostics.contains(&2339),
        "staged exports must stay visible during base resolution: {diagnostics:?}"
    );
}

#[test]
fn truthy_this_guard_keeps_type_query_assignment_error() {
    assert_eq!(
        codes_of_with_options(
            "class C { m() { if (this) { const x: typeof this = 1; } } }",
            &strict_options(),
        ),
        [2322]
    );
}

#[test]
fn tuple_intersection_array_literal_keeps_element_error() {
    assert_eq!(
        codes_of_with_options(
            "const x: [string] & { p: number } = [1];",
            &strict_options(),
        ),
        [2322]
    );
}

#[test]
fn tuple_intersection_unrelated_member_reports_the_intersection_head() {
    // Oracle: one 2322 head with args '[number]' vs
    // '[number] & { p: string; }' (+ the missing-'p' chain in the
    // elided tail). The intersection member is an anonymous
    // object WITH members — rendered by the 9.3b display slice
    // (this pin was containment-until-9.3b after the pre-9.3a
    // syntax bridge retired).
    assert_eq!(
        codes_of_with_options(
            "const x: [number] & { p: string } = [1];",
            &strict_options(),
        ),
        [2322]
    );
}

#[test]
fn contextual_tuple_arity_gap_remains_contained() {
    assert_eq!(
        codes_of_with_options(
            "const x: [...number[]] & { length: 2 } = [0, 0];",
            &strict_options(),
        ),
        Vec::<u32>::new()
    );
}

#[test]
fn satisfies_literal_reports_elaborated_member_error() {
    assert_eq!(
        codes_of_with_options(
            "const x = { a: 1 } satisfies { a: string };",
            &strict_options(),
        ),
        [2322]
    );
}

#[test]
fn invalid_interface_computed_name_reports_resolution_error() {
    assert_eq!(codes_of("interface I { [NotThere.x](): void; }"), [2304]);
    assert_eq!(
        codes_of("declare const ns: {}; interface I { [ns.missing](): void; }"),
        [2339]
    );
}

#[test]
fn computed_object_setter_is_checked_without_a_use_site() {
    assert_eq!(
        codes_of_with_options(
            "declare const k: unique symbol; const o = { set [k](v) {} };",
            &strict_options(),
        ),
        [7032, 7006]
    );
}

#[test]
fn used_expect_error_consuming_a_real_row_stays_silent() {
    // Named for the KEEP-OFF era ("stays silent while checker is
    // incomplete") until the 2026-07-19 B32 amendment: the 2578
    // emitter is LIVE since 5.9d, and this shape is silent
    // because the directive consumes the real straight-line 2454
    // (use before assignment, live since 6.2) — a USED directive
    // reports nothing.
    assert_eq!(
        codes_of("let x: number;\n// @ts-expect-error\nx;\n"),
        Vec::<u32>::new()
    );
}

#[test]
fn eopt_widened_absent_property_takes_the_missing_flavor() {
    // m4-review A13: getUndefinedProperty types the context-added
    // absent property undefinedOrMissingType (tsc 67990). Under
    // exactOptionalPropertyTypes the widened first branch stays
    // assignable to `c?: string` (missing ⊂ string|missing where
    // plain undefined is not), the directive has nothing to
    // consume, and the unused 2578 surfaces — oracle row
    // (2578, 69, 19), probed vs vendored 6.0.3 (eOPT + strict,
    // noLib). The undefined flavor instead made the relation
    // reject, and the display-band containment of that report
    // marked the directive used — silence where the oracle
    // reports.
    let options = CompilerOptions {
        exact_optional_property_types: Some(true),
        ..CompilerOptions::default()
    };
    let result = check_program(
            &[InputFile::new("a.ts".to_owned(), "declare const b: boolean;\nconst o = b ? { a: 1 } : { a: 2, c: \"x\" };\n// @ts-expect-error\nconst t: { a: number; c?: string } = o;\n".to_owned())],
            &options,
        );
    let rows: Vec<(u32, Option<u32>, Option<u32>)> = result
        .diagnostics
        .iter()
        .map(|d| (d.code(), d.start, d.length))
        .collect();
    assert_eq!(rows, [(2578, Some(69), Some(19))]);
}

#[test]
fn partial_flow_check_does_not_hide_unrelated_unused_expect_error() {
    // The branch-dependent 2454 is REAL since 6.4b (the condition
    // arm is live and a plain boolean guard narrows nothing) and
    // no longer hides the unrelated 2578.
    assert_eq!(
            codes_of(
                "declare const c: boolean;\nlet x: number;\nif (c) { x = 1; }\nx;\n// @ts-expect-error\nconst y = 1;\n"
            ),
            [2454, 2578]
        );
}

#[test]
fn condition_join_reports_use_before_assignment() {
    // The if-without-else join and condition arm are live, and a
    // plain boolean guard narrows nothing — the join computes
    // number ∪
    // (number | undefined) and the ladder's 2454 fires like
    // tsc's. (The straight-line form reports since 6.2, the
    // condition-free try/catch join since 6.3 — pinned below.)
    let result = check_program(
        &[InputFile::new(
            "a.ts".to_owned(),
            "declare const c: boolean;\nlet x: number;\nif (c) { x = 1; }\nx;\n".to_owned(),
        )],
        &CompilerOptions {
            strict: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|d| d.code())
            .collect::<Vec<_>>(),
        [2454]
    );
    assert_eq!(result.partial_checks.len(), 0);
}

#[test]
fn const_variable_guard_inlines_into_the_condition() {
    // narrowType's Identifier arm (6.4h): `if (isStr)` narrows x
    // through the const's initializer (`typeof x === "string"`),
    // so the fs(x) argument checks clean — no diagnostic and no
    // containment (pre-6.4h the inline conditions flagged the
    // query and the failed-argument gate partial-marked).
    let result = check_program(
            &[InputFile::new("a.ts".to_owned(), "declare function fs(s: string): void;\ndeclare const x: string | number;\nconst isStr = typeof x === \"string\";\nif (isStr) { fs(x); }\n".to_owned())],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|d| d.code())
            .collect::<Vec<_>>(),
        Vec::<u32>::new()
    );
    assert_eq!(result.partial_checks.len(), 0);
}

#[test]
fn destructuring_query_does_not_inline_const_guards() {
    // The synthetic destructuring reference never const-inlines:
    // tsc's isConstantReference reads the factory node's
    // resolvedSymbol — never populated — and its access arm lands
    // on isReadonlySymbol(unknownSymbol) = false (70385). The
    // guard must NOT narrow p to string, so `p === 42` stays a
    // legal overlap (no 2367) exactly like tsc.
    let result = check_program(
            &[InputFile::new("a.ts".to_owned(), "declare const o: { p: string | number };\nconst isStr = typeof o.p === \"string\";\nif (isStr) {\n  const { p } = o;\n  if (p === 42) {}\n}\n".to_owned())],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|d| d.code())
            .collect::<Vec<_>>(),
        Vec::<u32>::new()
    );
    assert_eq!(
        result.partial_checks.len(),
        0,
        "{:?}",
        result.partial_checks
    );
}

#[test]
fn empty_string_typeof_case_witnesses_none() {
    // getSwitchClauseTypeOfWitnesses (69955): `case "":` is a
    // FALSY text — the witness is None like a default clause, the
    // clause narrows to never (tsc's `text ? ... : neverType`),
    // and the never-typed assignment checks clean. tsc reports
    // ONLY the case-comparability 2678 (oracle-verified). Pre-fix
    // the "" witness took the host-object fallback and narrowed
    // unknown to object — a 2322 FP alongside.
    let result = check_program(
            &[InputFile::new("a.ts".to_owned(), "declare const x: unknown;\nswitch (typeof x) {\n  case \"\": {\n    const y: never = x;\n    break;\n  }\n}\n".to_owned())],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.category() == DiagnosticCategory::Error)
            .map(|d| d.code())
            .collect::<Vec<_>>(),
        [2678]
    );
    assert_eq!(result.partial_checks.len(), 0);
}

#[test]
fn multi_signature_body_inference_resolves_the_selection() {
    // m6 7.6 flip: getEffectsSignature's some() sweep reaches the
    // LIVE body-inference arm per member — `!!v` infers no
    // predicate (its false branch survives reduction), so the
    // selection resolves to NO effects signature and BOTH uses
    // report their straight-line 2454, unflagged (oracle q2:
    // (2454, 137, 1) + (2454, 152, 1), vendored 6.0.3 strict).
    let result = check_program(
            &[InputFile::new("a.ts".to_owned(), "function f(v: unknown) { return !!v; }\nfunction g(v: unknown) { return !!v; }\ndeclare const h: typeof f & typeof g;\nlet x: number;\nif (h(x)) { x = 1; }\nx;\n".to_owned())],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|d| d.code())
            .collect::<Vec<_>>(),
        [2454, 2454]
    );
    assert_eq!(result.partial_checks.len(), 0);
}

#[test]
fn body_inference_resolves_the_runtime_trigger() {
    // `!!v` infers no predicate, so the guard call
    // carries no effects, and the trailing use reports its
    // straight-line 2454 for real alongside the argument use
    // (oracle q6: (2454, 60, 1) + (2454, 75, 1), vendored 6.0.3
    // strict). No partial mark remains.
    let result = check_program(
        &[InputFile::new(
            "a.ts".to_owned(),
            "function f(v: unknown) { return !!v; }\nlet x: number;\nif (f(x)) { x = 1; }\nx;\n"
                .to_owned(),
        )],
        &CompilerOptions {
            strict: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|d| d.code())
            .collect::<Vec<_>>(),
        [2454, 2454]
    );
    assert_eq!(result.partial_checks.len(), 0);
}

#[test]
fn join_dependent_auto_type_resolves_without_implicit_any() {
    // The auto-typed join computes number |
    // undefined for real — no implicit-any diagnostic and no
    // partial mark, like tsc.
    let result = check_program(
        &[InputFile::new(
            "a.ts".to_owned(),
            "declare const c: boolean;\nlet x;\nif (c) { x = 1; }\nx;\n".to_owned(),
        )],
        &CompilerOptions {
            strict: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|d| d.code())
            .collect::<Vec<_>>(),
        Vec::<u32>::new()
    );
    assert_eq!(result.partial_checks.len(), 0);
}

#[test]
fn join_dependent_auto_type_resolves_through_guard_calls() {
    // The guard call resolves through body inference
    // (no predicate from `!!v`), the auto-typed join computes
    // number | undefined for real, and tsc is CLEAN on this
    // shape (oracle q7, vendored 6.0.3 strict) — no rows, no
    // partial mark.
    let result = check_program(
        &[InputFile::new(
            "a.ts".to_owned(),
            "function f(v: unknown) { return !!v; }\nlet x;\nif (f(x)) { x = 1; }\nx;\n".to_owned(),
        )],
        &CompilerOptions {
            strict: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|d| d.code())
            .collect::<Vec<_>>(),
        Vec::<u32>::new()
    );
    assert_eq!(result.partial_checks.len(), 0);
}

#[test]
fn branch_join_reports_use_before_assignment_across_try_catch() {
    // try/catch joins carry no condition nodes (the try-path
    // antecedent terminates at the x=1 assignment arm; the
    // catch-path runs to Start), so the 6.3 branch label computes
    // the REAL union: number ∪ (number | undefined) → the ladder's
    // 2454 fires like tsc's.
    let result = check_program(
        &[InputFile::new(
            "a.ts".to_owned(),
            "let x: number;\ntry { x = 1; } catch {}\nx;\n".to_owned(),
        )],
        &CompilerOptions {
            strict: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|d| d.code())
            .collect::<Vec<_>>(),
        [2454]
    );
    assert_eq!(result.partial_checks.len(), 0);
}

#[test]
fn loop_fixpoint_converges_across_back_edges() {
    // The 6.3 loop-label fixpoint: `while (true)` binds no
    // condition node (the binder's literal-condition passthrough),
    // so both antecedents resolve through live arms. Entry assigns
    // "a" → string; the back edge re-assigns "b" → string; the
    // fixpoint converges to string and fs(x) is clean.
    let result = check_program(
            &[InputFile::new("a.ts".to_owned(), "declare function fs(s: string): void;\nlet x: string | number = \"a\";\nwhile (true) {\n  fs(x);\n  x = \"b\";\n}\n".to_owned())],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|d| d.code())
            .collect::<Vec<_>>(),
        Vec::<u32>::new()
    );
    assert_eq!(result.partial_checks.len(), 0);
}

#[test]
fn loop_fixpoint_accumulates_widening_back_edge_types() {
    // The divergent twin of the pin above: the back edge assigns a
    // NUMBER, so the fixpoint's second pass adds it and the union
    // reaches the declared string | number — fs(x) genuinely fails
    // under tsc (2345). Pins the accumulate-then-break direction
    // (an antecedent equal to the declared type stops the walk) —
    // AND the report surface: with the [FLOW M5] failure-face
    // gates retired at 6.6f, the true positive REPORTS
    // (oracle-exact: 2345 at the argument).
    let result = check_program(
            &[InputFile::new("a.ts".to_owned(), "declare function fs(s: string): void;\nlet x: string | number = \"a\";\nwhile (true) {\n  fs(x);\n  x = 1;\n}\n".to_owned())],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|d| d.code())
            .collect::<Vec<_>>(),
        [2345]
    );
    assert_eq!(
        result
            .partial_checks
            .iter()
            .map(|p| p.reason.as_str())
            .collect::<Vec<_>>(),
        Vec::<&str>::new()
    );
}

#[test]
fn speculative_overload_failure_in_fixpoint_leaves_no_signature_memo() {
    // The g2 shape of controlFlowIterationErrorsAsync: the bare
    // `x;` query's back-edge pull speculatively resolves foo(x),
    // whose overload failure stashes a failure-face
    // resolvedSignature (resolveCall 76629). The mid-fixpoint exit
    // must clear that stash (tsc 77505's `: cached`): if it
    // survived, the later assignment-statement check would hit the
    // memo, skip argument checking, and let the failure-face
    // return type reach the assignment relation — a 2322 tsc never
    // emits. Post-6.6f expected (oracle-exact): ONE 2769 (the
    // overload failure at the real call check), no 2322, no
    // partial marks.
    let result = check_program(
            &[InputFile::new("a.ts".to_owned(), "declare function foo(x: string): number;\ndeclare function foo(x: number): string;\ndeclare const cond: boolean;\nlet x: string | number | boolean;\nx = \"\";\nwhile (cond) {\n  x;\n  x = foo(x);\n}\n".to_owned())],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|d| d.code())
            .collect::<Vec<_>>(),
        [2769]
    );
    assert_eq!(
        result
            .partial_checks
            .iter()
            .map(|p| p.reason.as_str())
            .collect::<Vec<_>>(),
        Vec::<&str>::new()
    );
}

#[test]
fn loop_fixpoint_joins_evolving_arrays_incomplete_first_pass() {
    // Evolving arrays THROUGH the fixpoint: at tn(a) the loop
    // label joins {entry: evolving[never], back edge:
    // ArrayMutation(push 1)}. The mutation's input walk re-enters
    // this same label mid-back-edge and takes the in-progress arm
    // (the partial union tagged INCOMPLETE); the join then unions
    // element types into evolving[number], finalized to number[]
    // at the use — clean, like tsc.
    let result = check_program_with_libs(
            &[es5_lib()],
            &[InputFile::new("a.ts".to_owned(), "declare function tn(ns: number[]): void;\nlet a = [];\nwhile (true) {\n  tn(a);\n  a.push(1);\n}\n".to_owned())],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|d| d.code())
            .collect::<Vec<_>>(),
        Vec::<u32>::new()
    );
    assert_eq!(result.partial_checks.len(), 0);
}

#[test]
fn loop_fixpoint_reports_2454_through_live_conditions() {
    // 6.4b: the fixpoint through a LIVE (non-narrowing) boolean
    // condition computes the real per-use unions — both loop uses
    // report 2454 like tsc, nothing partial-marks, and the
    // second query may legitimately hit flowLoopCaches (same
    // key, unflagged).
    let result = check_program(
            &[InputFile::new("a.ts".to_owned(), "declare const cond: boolean;\nlet x: number;\nwhile (true) {\n  x;\n  x;\n  if (cond) { x = 1; }\n}\n".to_owned())],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|d| d.code())
            .collect::<Vec<_>>(),
        [2454, 2454]
    );
    assert_eq!(result.partial_checks.len(), 0);
}

#[test]
fn loop_fixpoint_reports_for_real_through_guard_calls() {
    // The guard call resolves through body inference (no
    // predicate), the loop fixpoint runs, and all
    // THREE uses report their 2454 exactly like tsc (oracle q5:
    // (2454, 71/76/87), vendored 6.0.3 strict).
    let result = check_program(
            &[InputFile::new("a.ts".to_owned(), "function f(v: unknown) { return !!v; }\nlet x: number;\nwhile (true) {\n  x;\n  x;\n  if (f(x)) { x = 1; }\n}\n".to_owned())],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|d| d.code())
            .collect::<Vec<_>>(),
        [2454, 2454, 2454]
    );
    assert_eq!(result.partial_checks.len(), 0);
}

#[test]
fn arithmetic_face_narrows_through_the_inferred_predicate() {
    // m6 7.6 flip of the M5 post-close D2 pin: isNum's predicate
    // is INFERRED for real, u narrows to number inside the
    // guard, and the arithmetic face is clean like tsc
    // (verify/d2_operator_face.ts + oracle q3).
    let result = check_program(
            &[InputFile::new("a.ts".to_owned(), "function isNum(x: unknown) { return typeof x === \"number\"; }\nfunction f(u: string | number) {\n    if (isNum(u)) {\n        const a = u * 2;\n    }\n}\n".to_owned())],
            &CompilerOptions::default(),
        );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.category() == DiagnosticCategory::Error)
            .map(|d| d.code())
            .collect::<Vec<_>>(),
        Vec::<u32>::new()
    );
    assert_eq!(result.partial_checks.len(), 0);
}

#[test]
fn assignment_face_relates_through_the_inferred_predicate() {
    // m6 7.6 flip of the M5 post-close D1 pin: isNum's predicate
    // is INFERRED for real, u narrows to number inside the
    // compound RHS, and the assignment face relates cleanly like
    // tsc (verify/d1_assignment_face.ts + oracle q4).
    let result = check_program(
            &[InputFile::new("a.ts".to_owned(), "function isNum(x: unknown) { return typeof x === \"number\"; }\nfunction g(u: string | number) {\n    let t: { p: number };\n    if (isNum(u)) {\n        t = { p: u };\n        void t;\n    }\n}\n".to_owned())],
            &CompilerOptions::default(),
        );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|d| d.code())
            .collect::<Vec<_>>(),
        Vec::<u32>::new()
    );
    assert_eq!(result.partial_checks.len(), 0);
}

#[test]
fn dependent_parameter_narrowing_types_rest_tuple_slices() {
    // getNarrowedTypeOfSymbol arm 2 (72040-72060) over a CONCRETE
    // union-of-tuples rest type — live since the 6.2 review fix
    // (pre-fix the whole reference stopped at a recovery boundary).
    // kind types as the [0]-slice "a" | "b", so takeAB accepts it.
    let result = check_program(
            &[InputFile::new("a.ts".to_owned(), "declare function f(cb: (...args: [\"a\", number] | [\"b\", string]) => void): void;\ndeclare function takeAB(x: \"a\" | \"b\"): void;\nf((kind, _data) => { takeAB(kind); });\n".to_owned())],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|d| d.code())
            .collect::<Vec<_>>(),
        Vec::<u32>::new()
    );
    assert_eq!(result.partial_checks.len(), 0);
}

#[test]
fn dependent_parameter_narrowing_skips_a_non_union_rest_type() {
    // Nearest non-firing side of the 72046 gate: a single tuple is
    // contextually indexed normally, but does not enter the
    // dependent union-of-tuples flow walk.
    let result = check_program(
        &[InputFile::new(
            "a.ts".to_owned(),
            "declare function f(cb: (...args: [\"a\", number]) => void): void;\n\
                       declare function takeA(x: \"a\"): void;\n\
                       f((kind, _data) => { takeA(kind); });\n"
                .to_owned(),
        )],
        &CompilerOptions {
            strict: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        Vec::<u32>::new()
    );
    assert_eq!(result.partial_checks.len(), 0);
}

#[test]
fn dependent_parameter_narrowing_stops_after_parameter_assignment() {
    // getNarrowedTypeOfSymbol 72043-72046: assignment to one of
    // the dependent parameters keeps the union-of-tuples rest
    // type on its non-firing path. The property access therefore
    // retains both tuple payloads and reports tsc 6.0.3's exact
    // chained 2339 rather than narrowing data from kind.
    let result = check_program(
            &[InputFile::new("a.ts".to_owned(), "declare function f(cb: (...args: [\"a\", { aOnly: 1 }] | [\"b\", { bOnly: 1 }]) => void): void;\nf((kind, data) => { kind = kind; if (kind === \"a\") { data.aOnly; } });\n".to_owned())],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
    let diagnostics = result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code() == 2339)
        .collect::<Vec<_>>();
    assert_eq!(diagnostics.len(), 1);
    let diagnostic = diagnostics[0];
    assert_eq!((diagnostic.start, diagnostic.length), (Some(150), Some(5)));
    assert_eq!(
        (diagnostic.message.code, diagnostic.message.text.as_str()),
        (
            2339,
            "Property 'aOnly' does not exist on type '{ aOnly: 1; } | { bOnly: 1; }'.",
        )
    );
    assert_eq!(diagnostic.message.next.len(), 1);
    let child = &diagnostic.message.next[0];
    assert_eq!(
        (child.code, child.text.as_str()),
        (
            2339,
            "Property 'aOnly' does not exist on type '{ bOnly: 1; }'.",
        )
    );
    assert!(child.next.is_empty());
    assert_eq!(result.partial_checks.len(), 0);
}

#[test]
fn unused_expect_error_reports_2578() {
    assert_eq!(codes_of("// @ts-expect-error\nconst x = 1;\n"), [2578]);
}

#[test]
fn suggestion_does_not_consume_or_hide_expect_error() {
    let result = check_program(
        &[InputFile::new(
            "a.ts".to_owned(),
            "export {};\n// @ts-expect-error\nconst dead = 1;\n".to_owned(),
        )],
        &CompilerOptions::default(),
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code(), diagnostic.category()))
            .collect::<Vec<_>>(),
        [
            (2578, DiagnosticCategory::Error),
            (6133, DiagnosticCategory::Suggestion),
        ]
    );
    assert_eq!(
        result
            .semantic_diagnostics
            .iter()
            .map(|diagnostic| { (diagnostic.code(), diagnostic.category()) })
            .collect::<Vec<_>>(),
        [(2578, DiagnosticCategory::Error)]
    );
    assert_eq!(
        result
            .suggestion_diagnostics
            .iter()
            .map(|diagnostic| { (diagnostic.code(), diagnostic.category()) })
            .collect::<Vec<_>>(),
        [(6133, DiagnosticCategory::Suggestion)]
    );
}

#[test]
fn expect_error_inside_contained_object_accessor_body_is_exempt() {
    // m4-review S8 (oracle: vendored tsc 6.0.3, noLib, strict,
    // 2026-07-19): clean — the directive consumes the body's
    // 2322. Since the A2 routing (checkAccessorDeclaration owns
    // the deferred obj-literal accessor) the body is genuinely
    // checked and the suppression marks the directive used —
    // tsc's own mechanism; the S8-era wholly-unchecked-subtree
    // exemption is retired.
    assert_eq!(
            codes_of(
                "const o = {\n    get x() {\n        // @ts-expect-error\n        let a: number = \"s\";\n        return 1;\n    },\n};\n"
            ),
            Vec::<u32>::new()
        );
}

#[test]
fn checked_js_marks_directives_from_the_full_diagnostic_stream() {
    let result = check_program_with_libs(
        &[es5_lib()],
        &[InputFile::new(
            "a.js".to_owned(),
            "// @ts-check\n// @ts-expect-error\n(1)();\n".to_owned(),
        )],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code() == 2578),
        "the suppressed checked-JS diagnostic must mark the directive used: {:#?}",
        result.diagnostics
    );
}

#[test]
fn contained_expect_error_target_does_not_report_2578() {
    assert_eq!(
        codes_of(
            "// @ts-expect-error\n\
                 const bad = (() => 1) satisfies number;\n"
        ),
        Vec::<u32>::new()
    );
}

#[test]
fn expect_error_on_a_curtained_2507_extends_is_exempt() {
    // oracle (vendored 6.0.3, strict, noLib, 2026-07-23): clean —
    // the directive consumes the 2507. The bigint-literal face
    // curtains the port's 2507, so the drop must mark the report
    // anchor partial or the directive accounting fabricates 2578
    // (9.3b5 review r1).
    assert_eq!(
        codes_of("declare const x: 1n;\n// @ts-expect-error\nclass C extends x {}\n"),
        Vec::<u32>::new()
    );
}

#[test]
fn expect_error_on_a_curtained_2509_base_return_is_exempt() {
    // oracle (vendored 6.0.3, strict, noLib, 2026-07-23): clean —
    // the directive consumes the 2509 (base constructor return
    // type 1n is not an object type). Same containment-marking
    // rule as the 2507 twin above.
    assert_eq!(
        codes_of("declare const x: new () => 1n;\n// @ts-expect-error\nclass C extends x {}\n"),
        Vec::<u32>::new()
    );
}

#[test]
fn directive_inside_a_checked_mapped_type_is_not_blanket_exempted() {
    assert_eq!(
        codes_of(
            "type M<T> = {\n\
                   // @ts-expect-error\n\
                   [K in keyof T]: number;\n\
                 };\n"
        ),
        [2578]
    );
}

#[test]
fn checked_js_exposes_supported_checker_call_diagnostics() {
    let result = check_program_with_libs(
        &[es5_lib()],
        &[InputFile::new(
            "a.js".to_owned(),
            "// @ts-check\n(1)();\n".to_owned(),
        )],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code() == 2349),
        "{:#?}",
        result.diagnostics
    );
}

#[test]
fn checked_js_publishes_symbol_free_property_misses() {
    let source = "const n = 1;\nn.missing;\n";
    let result = check_program_with_libs(
        &[es5_lib()],
        &[InputFile::new("a.js".to_owned(), source.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| (
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
            ))
            .collect::<Vec<_>>(),
        [(
            2339,
            source.find("missing").expect("missing property") as u32,
            "missing".len() as u32,
        )]
    );
}

#[test]
fn checked_js_contains_symbol_bearing_expando_property_misses() {
    let result = check_program_with_libs(
        &[es5_lib()],
        &[InputFile::new(
            "a.js".to_owned(),
            "const value = {};\nvalue.added = 1;\nvalue.added;\n".to_owned(),
        )],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code() != 2339),
        "{:#?}",
        result.diagnostics
    );
}

#[test]
fn checked_js_publishes_jsdoc_symbol_free_property_misses() {
    let source = "/** @type {number} */\nconst n = 1;\nn.missing;\n";
    let result = check_program_with_libs(
        &[es5_lib()],
        &[InputFile::new("a.js".to_owned(), source.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| (
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
            ))
            .collect::<Vec<_>>(),
        [(
            2339,
            source.find("missing").expect("missing property") as u32,
            "missing".len() as u32,
        )]
    );
}

#[test]
fn checked_js_publishes_property_misses_on_non_js_declared_types() {
    let source = "value.missing;\n";
    let result = check_program(
        &[
            InputFile::new(
                "types.d.ts".to_owned(),
                "interface Declared { known: number }\ndeclare const value: Declared;\n".to_owned(),
            ),
            InputFile::new("a.js".to_owned(), source.to_owned()),
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| (
                diagnostic.file_name.as_deref(),
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
            ))
            .collect::<Vec<_>>(),
        [(
            Some("a.js"),
            2339,
            source.find("missing").expect("missing property") as u32,
            "missing".len() as u32,
        )]
    );
}

#[test]
fn checked_js_non_js_declared_prototype_replacement_reports_assignment_type() {
    let source = "C.prototype = {};\nC.bar = 2;\n";
    let result = check_program(
        &[
            InputFile::new(
                "types.d.ts".to_owned(),
                "declare namespace C { function bar(): void }\n".to_owned(),
            ),
            InputFile::new("a.js".to_owned(), source.to_owned()),
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| (
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
            ))
            .collect::<Vec<_>>(),
        [(
            2322,
            source.find("C.bar").expect("typed assignment") as u32,
            "C.bar".len() as u32,
        )]
    );
}

#[test]
fn checked_js_publishes_plain_value_module_property_reads() {
    let source = "exports.missing();\nexports.created = 1;\n";
    let result = check_program(
        &[InputFile::new("a.js".to_owned(), source.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| (
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
            ))
            .collect::<Vec<_>>(),
        [(
            2339,
            source.find("missing").expect("missing property") as u32,
            "missing".len() as u32,
        )]
    );
}

#[test]
fn checked_js_contains_assignment_bearing_value_module_property_misses() {
    let result = check_program(
        &[InputFile::new(
            "a.js".to_owned(),
            "function C() { this.p = 1; }\n\
                       C.prototype = { q: 2 };\n\
                       const c = new C();\n\
                       c.q;\n"
                .to_owned(),
        )],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert!(result.diagnostics.is_empty(), "{:#?}", result.diagnostics);
}

#[test]
fn checked_js_publishes_assignment_bearing_class_property_reads() {
    let source = "class C { constructor() { this.p = 1; } }\n\
                      C.prototype = { q: 2 };\n\
                      const c = new C();\n\
                      c.q;\n";
    let result = check_program(
        &[InputFile::new("a.js".to_owned(), source.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| (
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
            ))
            .collect::<Vec<_>>(),
        [(2339, source.rfind('q').expect("missing property") as u32, 1,)]
    );
}

#[test]
fn checked_js_publishes_direct_this_class_property_reads() {
    let source = "class C { method() { this.missing; } }\n";
    let result = check_program(
        &[InputFile::new("a.js".to_owned(), source.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| (
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
            ))
            .collect::<Vec<_>>(),
        [(
            2339,
            source.find("missing").expect("missing property") as u32,
            "missing".len() as u32,
        )]
    );
}

#[test]
fn checked_js_publishes_imported_class_alias_expando_misses() {
    let source = "import { C, value } from \"./defs\";\n\
                      C.missing = 1;\n\
                      value.added = 1;\n";
    let result = check_program(
        &[
            InputFile::new(
                "defs.js".to_owned(),
                "export class C {}\nexport const value = {};\n".to_owned(),
            ),
            InputFile::new("main.js".to_owned(), source.to_owned()),
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    // TS 6.0.3 exact identity: both imported assignment sites are
    // rejected, producing these two 2339 rows.
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| (
                diagnostic.file_name.as_deref(),
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
            ))
            .collect::<Vec<_>>(),
        [
            (
                Some("main.js"),
                2339,
                source.find("missing").expect("missing property") as u32,
                "missing".len() as u32,
            ),
            (
                Some("main.js"),
                2339,
                source.find("added").expect("added property") as u32,
                "added".len() as u32,
            ),
        ]
    );
}

#[test]
fn checked_js_publishes_jsdoc_adjacent_private_name_misses() {
    let source = "class C {\n\
                        #known;\n\
                        method() {\n\
                          /** @type {string} */\n\
                          this.#missing;\n\
                          this.#known;\n\
                        }\n\
                      }\n";
    let result = check_program(
        &[InputFile::new("a.js".to_owned(), source.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| (
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
            ))
            .collect::<Vec<_>>(),
        [
            (
                7008,
                source.find("#known").expect("unused private field") as u32,
                "#known".len() as u32,
            ),
            (
                2339,
                source.find("#missing").expect("missing private name") as u32,
                "#missing".len() as u32,
            ),
        ]
    );
}

#[test]
fn checked_js_publishes_chained_this_assignment_misses() {
    let source = "this.x = {};\n\
                      this.x.missing = {};\n\
                      /** @constructor */\n\
                      function F() {\n\
                        this.x = {};\n\
                        this.x.alsoMissing = {};\n\
                      }\n";
    let result = check_program(
        &[InputFile::new("a.js".to_owned(), source.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| (
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
            ))
            .collect::<Vec<_>>(),
        [
            (
                2339,
                source.find("missing").expect("global chained miss") as u32,
                "missing".len() as u32,
            ),
            (
                2339,
                source
                    .find("alsoMissing")
                    .expect("constructor chained miss") as u32,
                "alsoMissing".len() as u32,
            ),
        ]
    );
}

#[test]
fn checked_js_publishes_chained_identifier_empty_assignment_misses() {
    let source = "let A;\n\
                      A = {};\n\
                      A.prototype.b = {};\n\
                      let B;\n\
                      B = {};\n\
                      B.direct = {};\n\
                      B.direct;\n";
    let result = check_program(
        &[InputFile::new("a.js".to_owned(), source.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    // TS 6.0.3 exact identity: prototype plus both direct access
    // sites produce three 2339 rows.
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| (
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
                diagnostic.message_text(),
            ))
            .collect::<Vec<_>>(),
        [
            (
                2339,
                source.find("prototype").expect("chained missing property") as u32,
                "prototype".len() as u32,
                "Property 'prototype' does not exist on type '{}'.",
            ),
            (
                2339,
                source.find("direct").expect("direct assignment miss") as u32,
                "direct".len() as u32,
                "Property 'direct' does not exist on type '{}'.",
            ),
            (
                2339,
                source.rfind("direct").expect("direct read miss") as u32,
                "direct".len() as u32,
                "Property 'direct' does not exist on type '{}'.",
            ),
        ]
    );
}

#[test]
fn checked_js_publishes_prototype_object_property_assignment_misses() {
    let source = "/** @constructor */\n\
                      var Multimap = function() {\n\
                        this._map = {};\n\
                        this._map;\n\
                        this.set;\n\
                        this.get;\n\
                        this.addon;\n\
                      };\n\
                      Multimap.prototype = {\n\
                        set: function() {},\n\
                        get() {}\n\
                      };\n\
                      Multimap.prototype.addon = function() {\n\
                        this._map;\n\
                        this.set;\n\
                        this.get;\n\
                        this.addon;\n\
                      };\n\
                      var Plain = function() {};\n\
                      Plain.prototype = { existing() {} };\n\
                      Plain.prototype.incremental = function() {};\n";
    let result = check_program(
        &[InputFile::new("a.js".to_owned(), source.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| (
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
                diagnostic.message_text(),
            ))
            .collect::<Vec<_>>(),
        [
            (
                2339,
                (source
                    .find("Multimap.prototype.addon")
                    .expect("missing prototype property")
                    + "Multimap.prototype.".len()) as u32,
                "addon".len() as u32,
                "Property 'addon' does not exist on type '{ set: () => void; get(): void; }'.",
            ),
            (
                2339,
                source
                    .find("incremental")
                    .expect("plain prototype property") as u32,
                "incremental".len() as u32,
                "Property 'incremental' does not exist on type '{ existing(): void; }'.",
            ),
        ]
    );
}

#[test]
fn checked_js_nested_constructor_this_uses_merged_prototype_members() {
    let source = "(function container() {\n\
                        /** @constructor */\n\
                        var Multimap = function() {\n\
                          this._map = {};\n\
                          this._map;\n\
                          this.set;\n\
                          this.get;\n\
                          this.addon;\n\
                        };\n\
                        Multimap.prototype = {\n\
                          set: function() {},\n\
                          get() {}\n\
                        };\n\
                        Multimap.prototype.addon = function() {};\n\
                      })();\n";
    let result = check_program(
        &[InputFile::new("a.js".to_owned(), source.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(true),
            ..CompilerOptions::default()
        },
    );
    // Preserve the existing assignment-LHS canary while proving
    // the earlier constructor read sees the inferred JS class's
    // complete prototype member set.
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| (
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
            ))
            .collect::<Vec<_>>(),
        [(
            2339,
            (source
                .find("Multimap.prototype.addon")
                .expect("missing prototype property")
                + "Multimap.prototype.".len()) as u32,
            "addon".len() as u32,
        )]
    );
}

#[test]
fn checked_js_publishes_jsdoc_satisfies_object_literal_property_reads() {
    let source = "const value = /** @satisfies {{ present: number }} */ ({ present: 1 });\n\
                      value.present;\n\
                      value.missing;\n\
                      const asserted = /** @type {{ present: number }} */ ({ present: 1 });\n\
                      asserted.hidden;\n";
    let result = check_program(
        &[InputFile::new("a.js".to_owned(), source.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| (
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
                diagnostic.message_text(),
            ))
            .collect::<Vec<_>>(),
        [
            (
                2339,
                source.find("missing").expect("satisfies-backed miss") as u32,
                "missing".len() as u32,
                "Property 'missing' does not exist on type '{ present: number; }'.",
            ),
            (
                2339,
                source.find("hidden").expect("type-assertion-backed miss") as u32,
                "hidden".len() as u32,
                "Property 'hidden' does not exist on type '{ present: number; }'.",
            ),
        ]
    );
}

#[test]
fn checked_js_valid_template_nested_prototype_read_is_parse_all_crash_guard() {
    // TypeScript 6.0.3 with ParseAll crashes in
    // typeToString -> lookupSymbolChainWorker while trying to
    // format the otherwise expected 2339 for `missing`. Keep this
    // fixture as a crash-free valid-JSDoc guard; the non-crashing
    // oracle face for the prototype read is pinned separately.
    let source = "/** @template T */\n\
                      class Outer {\n\
                        method() {\n\
                          class Inner {\n\
                            static check() {\n\
                              this.prototype.missing;\n\
                            }\n\
                          }\n\
                          Inner;\n\
                        }\n\
                      }\n";
    let result = check_program(
        &[InputFile::new("a.js".to_owned(), source.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| (
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
                diagnostic.message_text(),
            ))
            .collect::<Vec<_>>(),
        [(
            6133,
            source.find("@template").expect("unused template tag") as u32,
            12,
            "'T' is declared but its value is never read.",
        )]
    );
    assert!(
        result.partial_checks.is_empty(),
        "oracle-crash control flow is not partial-model audit debt: {:#?}",
        result.partial_checks
    );
}

#[test]
fn checked_js_outer_template_display_crash_does_not_stop_later_errors() {
    let source = "/** @template T */\n\
                      class Outer {\n\
                        method() {\n\
                          class Inner {\n\
                            static check() {\n\
                              this.prototype.missing;\n\
                            }\n\
                          }\n\
                          Inner;\n\
                        }\n\
                      }\n\
                      const later = { present: 1 };\n\
                      later.missing;\n";
    let result = check_program(
        &[InputFile::new("a.js".to_owned(), source.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| matches!(diagnostic.code(), 2339 | 6133))
            .map(|diagnostic| (
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
            ))
            .collect::<Vec<_>>(),
        [
            (
                6133,
                source.find("@template").expect("unused template tag") as u32,
                12,
            ),
            (
                2339,
                source.rfind("missing").expect("later independent miss") as u32,
                "missing".len() as u32,
            ),
        ]
    );
    assert!(result.partial_checks.is_empty());
}

#[test]
fn checked_js_outer_template_display_crash_consumes_preceding_expect_error_range_only() {
    let source = "/** @template T */\n\
                      class Outer {\n\
                        method() {\n\
                          class Inner {\n\
                            static check() {\n\
                              // @ts-expect-error\n\
                              this.prototype.missing;\n\
                            }\n\
                          }\n\
                          Inner;\n\
                        }\n\
                      }\n";
    let result = check_program(
        &[InputFile::new("a.js".to_owned(), source.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [6133],
        "the contained oracle crash must consume the directive without fabricating TS2578"
    );
    assert!(result.partial_checks.is_empty());
}

#[test]
fn checked_js_publishes_this_prototype_class_property_reads() {
    let source = "class Outer {\n\
                        method() {\n\
                          class Inner {\n\
                            static check() {\n\
                              this.prototype.missing;\n\
                            }\n\
                          }\n\
                          Inner;\n\
                        }\n\
                      }\n";
    let result = check_program(
        &[InputFile::new("a.js".to_owned(), source.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| (
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
                diagnostic.message_text(),
            ))
            .collect::<Vec<_>>(),
        [(
            2339,
            source.find("missing").expect("class prototype miss") as u32,
            "missing".len() as u32,
            "Property 'missing' does not exist on type 'Inner'.",
        )]
    );
    assert!(result.partial_checks.is_empty());
}

#[test]
fn checked_js_publishes_jsdoc_chained_static_assignment_this_reads() {
    let source = "function A() {\n\
                        this.instanceOnly = 1;\n\
                      }\n\
                      /** @param {number} n */\n\
                      A.s = A.t = function g(n) {\n\
                        return n + this.instanceOnly;\n\
                      };\n";
    let result = check_program(
        &[InputFile::new("a.js".to_owned(), source.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            no_implicit_any: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| (
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
                diagnostic.message_text(),
            ))
            .collect::<Vec<_>>(),
        [(
            2339,
            source
                .rfind("instanceOnly")
                .expect("static-side instance miss") as u32,
            "instanceOnly".len() as u32,
            "Property 'instanceOnly' does not exist on type 'typeof A'.",
        )]
    );
}

#[test]
fn checked_js_publishes_class_this_miss_from_jsdoc_this_annotated_arrow() {
    let source = "/** @typedef {{ fn(a: string): void }} T */\n\
                      class C {\n\
                        /**\n\
                         * @this {T}\n\
                         * @param {string} a\n\
                         */\n\
                        p = (a) => this.missing(a);\n\
                      }\n";
    let result = check_program(
        &[InputFile::new("a.js".to_owned(), source.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            no_implicit_any: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| (
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
                diagnostic.message_text(),
            ))
            .collect::<Vec<_>>(),
        [
            (
                2730,
                source.find("@this").expect("JSDoc this tag") as u32 + 1,
                "this".len() as u32,
                "An arrow function cannot have a 'this' parameter.",
            ),
            (
                2339,
                source.find("missing").expect("lexical class this miss") as u32,
                "missing".len() as u32,
                "Property 'missing' does not exist on type 'C'.",
            ),
        ]
    );
}

#[test]
fn checked_js_publishes_primitive_module_exports_assignment_misses() {
    let primitive = "module.exports = 1;\nmodule.exports.missing = 1;\n";
    let result = check_program(
        &[
            InputFile::new(
                "requires.d.ts".to_owned(),
                "declare var module: { exports: any };\n".to_owned(),
            ),
            InputFile::new("primitive.js".to_owned(), primitive.to_owned()),
            InputFile::new(
                "object.js".to_owned(),
                "module.exports = {};\nmodule.exports.allowed = 1;\n".to_owned(),
            ),
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| (
                diagnostic.file_name.as_deref(),
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
            ))
            .collect::<Vec<_>>(),
        [(
            Some("primitive.js"),
            2339,
            primitive.find("missing").expect("missing property") as u32,
            "missing".len() as u32,
        )]
    );
}

#[test]
fn checked_js_common_js_object_replacement_unions_direct_export_members() {
    let source = "const mod1 = require('./mod1');\n\
                      mod1.justExport.toFixed();\n\
                      mod1.bothBefore.toFixed();\n\
                      mod1.bothAfter.toFixed();\n\
                      mod1.justProperty.length;\n";
    let result = check_program_with_libs(
        &[es5_lib()],
        &[
            InputFile::new(
                "requires.d.ts".to_owned(),
                "declare var module: { exports: any };\n\
                           declare function require(name: string): any;\n"
                    .to_owned(),
            ),
            InputFile::new(
                "mod1.js".to_owned(),
                "module.exports.bothBefore = 'string';\n\
                           module.exports = {\n\
                               justExport: 1,\n\
                               bothBefore: 2,\n\
                               bothAfter: 3,\n\
                           };\n\
                           module.exports.bothAfter = 'string';\n\
                           module.exports.justProperty = 'string';\n"
                    .to_owned(),
            ),
            InputFile::new("a.js".to_owned(), source.to_owned()),
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| (
                diagnostic.file_name.clone(),
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
                diagnostic.message_text().to_owned(),
                diagnostic
                    .message
                    .next
                    .first()
                    .map(|message| message.text.clone()),
            ))
            .collect::<Vec<_>>(),
        [
            (
                Some("a.js".to_owned()),
                2339,
                source.find("bothBefore.toFixed").expect("before access") as u32
                    + "bothBefore.".len() as u32,
                "toFixed".len() as u32,
                "Property 'toFixed' does not exist on type 'number | \"string\"'.".to_owned(),
                Some("Property 'toFixed' does not exist on type '\"string\"'.".to_owned()),
            ),
            (
                Some("a.js".to_owned()),
                2339,
                source.find("bothAfter.toFixed").expect("after access") as u32
                    + "bothAfter.".len() as u32,
                "toFixed".len() as u32,
                "Property 'toFixed' does not exist on type 'number | \"string\"'.".to_owned(),
                Some("Property 'toFixed' does not exist on type '\"string\"'.".to_owned()),
            ),
        ]
    );
}

#[test]
fn checked_js_exposes_typed_declaration_arity_diagnostics() {
    let result = check_program(
        &[
            InputFile::new(
                "defs.d.ts".to_owned(),
                "declare function f1(p: void): void;\n\
                           declare function f2(p: undefined): void;\n\
                           declare function f3(p: unknown): void;\n\
                           declare function f4(p: any): void;\n\
                           interface I<T> { m(p: T): void; }\n\
                           declare const o1: I<void>;\n\
                           declare const o2: I<undefined>;\n\
                           declare const o3: I<unknown>;\n\
                           declare const o4: I<any>;\n"
                    .to_owned(),
            ),
            InputFile::new(
                "a.js".to_owned(),
                "f1();\no1.m();\nf2();\nf3();\nf4();\no2.m();\no3.m();\no4.m();\n".to_owned(),
            ),
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(true),
            ..CompilerOptions::default()
        },
    );
    let arity_rows = result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code() == 2554)
        .collect::<Vec<_>>();
    assert_eq!(arity_rows.len(), 6, "{:#?}", result.diagnostics);
    assert!(arity_rows
        .iter()
        .all(|diagnostic| { diagnostic.file_name.as_deref() == Some("a.js") }));
}

#[test]
fn checked_js_publishes_non_jsdoc_readonly_enum_expandos() {
    let source = "lf.Order = {};\nlf.Order.DESC = 0;\nlf.Order.ASC = 1;\n";
    let result = check_program(
        &[
            InputFile::new(
                "types.d.ts".to_owned(),
                "declare namespace lf { export enum Order { ASC, DESC } }\n".to_owned(),
            ),
            InputFile::new("enums.js".to_owned(), source.to_owned()),
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            target: Some(2),
            ..CompilerOptions::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| (
                diagnostic.file_name.as_deref(),
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
                diagnostic.message_text(),
            ))
            .collect::<Vec<_>>(),
        [
            (
                Some("enums.js"),
                2540,
                source.find("DESC").expect("DESC assignment") as u32,
                "DESC".len() as u32,
                "Cannot assign to 'DESC' because it is a read-only property.",
            ),
            (
                Some("enums.js"),
                2540,
                source.find("ASC =").expect("ASC assignment") as u32,
                "ASC".len() as u32,
                "Cannot assign to 'ASC' because it is a read-only property.",
            ),
        ]
    );
}

#[test]
fn complex_union_guards_report_across_intersection_template_and_tuple_paths() {
    let ten_objects = |suffix: u8| {
        ('a'..='j')
            .map(|name| format!("{{{name}{suffix}: any}}"))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let source = format!(
            "type U1 = {};\n\
             type U2 = {};\n\
             type U3 = {};\n\
             type U4 = {};\n\
             type U5 = {};\n\
             type U100000 = U1 & U2 & U3 & U4 & U5;\n\
             type D = 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9;\n\
             type D100000 = `${{D}}${{D}}${{D}}${{D}}${{D}}`;\n\
             type TD = [0] | [1] | [2] | [3] | [4] | [5] | [6] | [7] | [8] | [9];\n\
             type T100000 = [...TD, ...TD, ...TD, ...TD, ...TD];\n\
             type D20 = 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 15 | 16 | 17 | 18 | 19 | 20;\n\
             type Spacing = `0` | `${{number}}px` | `${{number}}rem` | `s${{D20}}`;\n\
             type SpacingShorthand = `${{Spacing}} ${{Spacing}} ${{Spacing}} ${{Spacing}}`;\n",
            ten_objects(1),
            ten_objects(2),
            ten_objects(3),
            ten_objects(4),
            ten_objects(5),
        );
    assert_eq!(codes_of(&source), [2590, 2590, 2590, 2590]);
}

#[test]
fn checked_js_jsdoc_type_checks_its_initializer() {
    let result = check_program(
        &[InputFile::new(
            "a.js".to_owned(),
            "// @ts-check\n/** @type {number} */\nlet value = \"wrong\";\n".to_owned(),
        )],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code() == 2322),
        "{:#?}",
        result.diagnostics
    );
}

#[test]
fn checked_js_does_not_treat_other_jsdoc_tags_as_type() {
    let result = check_program(
        &[InputFile::new(
            "a.js".to_owned(),
            "// @ts-check\n/** @types {number} */\nlet value = \"ok\";\n".to_owned(),
        )],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code() == 2322),
        "{:#?}",
        result.diagnostics
    );
}

#[test]
fn checked_js_jsdoc_augments_reports_only_effective_hosts() {
    let source = "/** @extends {A} */\n\
                      /** @constructor */\n\
                      class A {}\n\
                      /** @augments A */\n\
                      function f() {}\n\
                      class B {}\n\
                      /** @augments A */\n\
                      class C extends B {}\n\
                      /** @augments */\n\
                      class D extends A {}\n\
                      /** @extends {A} */\n";
    let result = check_program(
        &[InputFile::new("a.js".to_owned(), source.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            target: Some(99),
            ..CompilerOptions::default()
        },
    );
    let rows = result
        .diagnostics
        .iter()
        .filter(|diagnostic| matches!(diagnostic.code(), 8022 | 8023))
        .map(|diagnostic| {
            (
                diagnostic.file_name.as_deref(),
                diagnostic.start,
                diagnostic.length,
                diagnostic.code(),
                diagnostic.message_text(),
            )
        })
        .collect::<Vec<_>>();
    let function_name = source.find("f()").expect("function name") as u32;
    let mismatch_name = (source
        .find("@augments A */\nclass C")
        .expect("mismatch tag")
        + "@augments ".len()) as u32;
    let missing_name =
        (source.find("@augments */").expect("missing tag") + "@augments".len()) as u32;
    assert_eq!(
        rows,
        [
            (
                None,
                None,
                None,
                8022,
                "JSDoc '@extends' is not attached to a class.",
            ),
            (
                Some("a.js"),
                Some(function_name),
                Some(1),
                8022,
                "JSDoc '@augments' is not attached to a class.",
            ),
            (
                Some("a.js"),
                Some(mismatch_name),
                Some(1),
                8023,
                "JSDoc '@augments A' does not match the 'extends B' clause.",
            ),
            (
                Some("a.js"),
                Some(missing_name),
                Some(0),
                8023,
                "JSDoc '@augments ' does not match the 'extends A' clause.",
            ),
            (
                Some("a.js"),
                Some(source.len() as u32),
                Some(0),
                8022,
                "JSDoc '@extends' is not attached to a class.",
            ),
        ],
        "{:#?}",
        result.diagnostics
    );
}

#[test]
fn checked_js_detached_augments_document_keeps_fileless_8022() {
    let source = "class A {}\n\
                      /** @extends {A} */\n\
                      \n\
                      /** @constructor */\n\
                      class B extends A {}\n";
    let result = check_program(
        &[InputFile::new("a.js".to_owned(), source.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            target: Some(99),
            ..CompilerOptions::default()
        },
    );
    let rows = result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code() == 8022)
        .map(|diagnostic| {
            (
                diagnostic.file_name.as_deref(),
                diagnostic.start,
                diagnostic.length,
                diagnostic.message_text(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            None,
            None,
            None,
            "JSDoc '@extends' is not attached to a class.",
        )],
        "{:#?}",
        result.diagnostics
    );
}

#[test]
fn checked_js_detached_implements_document_keeps_fileless_8022() {
    let source = "class A {}\n\
                      /** @implements {A} */\n\
                      /** @constructor */\n\
                      class B {}\n\
                      /** @implements {A} */\n\
                      class C {}\n";
    let result = check_program(
        &[InputFile::new("a.js".to_owned(), source.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            target: Some(99),
            ..CompilerOptions::default()
        },
    );
    let rows = result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code() == 8022)
        .map(|diagnostic| {
            (
                diagnostic.file_name.as_deref(),
                diagnostic.start,
                diagnostic.length,
                diagnostic.message_text(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            None,
            None,
            None,
            "JSDoc '@implements' is not attached to a class.",
        )],
        "{:#?}",
        result.diagnostics
    );
}

#[test]
fn jsdoc_augments_projection_preserves_matching_siblings_and_typescript() {
    let valid_js = "class A {}\n\
                        /** @extends {A} */\n\
                        class B extends A {}\n\
                        /** @extends { A } */\n\
                        class C extends A {}\n\
                        /** @extends {A<{ value: string }>} */\n\
                        class Generic extends A {}\n\
                        /** prose @extends {B} */\n\
                        class D extends B {}\n";
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        target: Some(99),
        ..CompilerOptions::default()
    };
    for (name, text) in [
        ("a.js", valid_js),
        (
            "a.ts",
            "/** @augments Wrong */\nclass Typed extends Actual {}\n",
        ),
    ] {
        let result = check_program(
            &[InputFile::new(name.to_owned(), text.to_owned())],
            &options,
        );
        assert!(
            result
                .diagnostics
                .iter()
                .all(|diagnostic| !matches!(diagnostic.code(), 8022 | 8023)),
            "{name}: {:#?}",
            result.diagnostics
        );
    }
}

#[test]
fn checked_js_set_only_accessors_use_jsdoc_parameter_annotations() {
    let source = "// @ts-check\n\
                      class C {\n\
                        /** @param {string} value */\n\
                        set instance(value) {}\n\
                        /** @param {number} value */\n\
                        static set stat(value) {}\n\
                      }\n\
                      const c = new C();\n\
                      c.instance = 1;\n\
                      C.stat = \"bad\";\n";
    for target in [1, 2] {
        let result = check_program(
            &[InputFile::new("a.js".to_owned(), source.to_owned())],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                strict: Some(true),
                target: Some(target),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .filter(|diagnostic| matches!(diagnostic.code(), 2322 | 7032))
                .map(|diagnostic| (
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                ))
                .collect::<Vec<_>>(),
            [
                (
                    2322,
                    source.find("c.instance").expect("instance assignment") as u32,
                    "c.instance".len() as u32,
                ),
                (
                    2322,
                    source.find("C.stat").expect("static assignment") as u32,
                    "C.stat".len() as u32,
                ),
            ],
            "target {target}: {:#?}",
            result.diagnostics
        );
    }
}

#[test]
fn checked_js_super_call_uses_effective_jsdoc_extends_type_arguments() {
    let source = "// @ts-check\n\
                      /** @template T */\n\
                      class Base {\n\
                        /** @param {T} value */\n\
                        constructor(value) {}\n\
                      }\n\
                      /** @template U @extends {Base<U>} */\n\
                      class Derived extends Base {\n\
                        /** @param {U} value */\n\
                        constructor(value) { super(value); }\n\
                      }\n\
                      /** @extends {Base<number>} */\n\
                      class Fixed extends Base {\n\
                        constructor() { super(\"bad\"); }\n\
                      }\n";
    for target in [1, 2] {
        let result = check_program(
            &[InputFile::new("a.js".to_owned(), source.to_owned())],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                strict: Some(true),
                target: Some(target),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .filter(|diagnostic| matches!(diagnostic.code(), 2345 | 2346))
                .map(|diagnostic| (
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                ))
                .collect::<Vec<_>>(),
            [(
                2345,
                source.find("\"bad\"").expect("invalid super argument") as u32,
                "\"bad\"".len() as u32,
            )],
            "target {target}: {:#?}",
            result.diagnostics
        );
    }
}

#[test]
fn single_line_directive_suppresses_through_comment_lines() {
    // Walk crosses blank and `//` lines, exactly like tsc.
    assert_eq!(
        codes_of("// @ts-ignore\n// note\n\nlet x;\nlet x;\n"),
        [2451]
    );
}

#[test]
fn block_comment_shell_stops_the_directive_walk() {
    // tsc's markPrecedingCommentDirectiveLine stops at any line
    // that is non-empty and not a `//` comment — a block-comment
    // line between directive and diagnostic KEEPS the diagnostic
    // (the retired interim filter walked through these).
    assert_eq!(
        codes_of("// @ts-ignore\n/* shell */\nlet x;\nlet x;\n"),
        [2451, 2451]
    );
}

#[test]
fn trailing_comment_directive_suppresses_the_next_line() {
    // Scanner-collected: the directive comment trails code on its
    // own line, so a line-start scan would miss it.
    assert_eq!(
        codes_of("let a = 1; // @ts-ignore\nlet x;\nlet x;\n"),
        [2451]
    );
}

#[test]
fn multi_line_directive_keys_on_its_closing_line() {
    // Directive on the closing line: suppresses the next line.
    assert_eq!(
        codes_of("/*\n@ts-expect-error */\nlet x;\nlet x;\n"),
        [2451]
    );
    // Directive on an interior line is no directive at all.
    assert_eq!(
        codes_of("/*\n@ts-expect-error\n*/\nlet x;\nlet x;\n"),
        [2451, 2451]
    );
}

#[test]
fn template_literal_fake_directive_does_not_suppress() {
    // The `// @ts-ignore` line sits INSIDE a template literal: the
    // scanner collects nothing, and the walk treats the line as a
    // `//` comment and keeps climbing past it.
    assert_eq!(
        codes_of("const s = `\n// @ts-ignore\n`;\nlet x;\nlet x;\n"),
        [2451, 2451]
    );
}

#[test]
fn directive_on_the_diagnostic_line_itself_does_not_suppress() {
    // The walk starts one line ABOVE the diagnostic.
    assert_eq!(codes_of("let x;\nlet x; // @ts-ignore\n"), [2451, 2451]);
}

#[test]
fn ts_nocheck_suppresses_checked_js_diagnostics() {
    let result = check_program(
        &[InputFile::new(
            "a.js".to_owned(),
            "// @ts-nocheck\nlet x;\nlet x;\n".to_owned(),
        )],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );

    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
}

#[test]
fn jsdoc_parse_diagnostics_publish_only_for_checked_js_semantics() {
    let text = "/**\n * @typedef Name\n * @type {string}\n * @type {Oops}\n */";
    let checked = check_program(
        &[InputFile::new("a.js".to_owned(), text.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert!(
        checked
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code() == 8033),
        "{:#?}",
        checked.diagnostics
    );
    assert!(
        checked
            .syntactic_diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code() != 8033),
        "{:#?}",
        checked.syntactic_diagnostics
    );

    for (source, check_js) in [
        (text.to_owned(), false),
        (format!("// @ts-nocheck\n{text}"), true),
    ] {
        let result = check_program(
            &[InputFile::new("a.js".to_owned(), source)],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(check_js),
                ..CompilerOptions::default()
            },
        );
        assert!(
            result
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code() != 8033),
            "{:#?}",
            result.diagnostics
        );
    }
}

#[test]
fn ts_check_overrides_explicit_check_js_false() {
    let result = check_program(
        &[InputFile::new(
            "a.js".to_owned(),
            "// @ts-check\nlet x;\nlet x;\n".to_owned(),
        )],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(false),
            ..CompilerOptions::default()
        },
    );
    let pins: Vec<(u32, u32, u32)> = result
        .diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
                diagnostic.length.unwrap_or(u32::MAX),
            )
        })
        .collect();

    assert_eq!(pins, [(2451, 17, 1), (2451, 24, 1)]);
}

#[test]
fn checked_js_uses_comment_directives() {
    let result = check_program(
        &[InputFile::new(
            "a.js".to_owned(),
            "// @ts-check\n// @ts-ignore\nlet x;\nlet x;\n".to_owned(),
        )],
        &CompilerOptions {
            allow_js: true,
            ..CompilerOptions::default()
        },
    );
    let codes: Vec<u32> = result
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code())
        .collect();

    assert_eq!(codes, [2451]);
}

#[test]
fn check_js_option_uses_comment_directives() {
    let result = check_program(
        &[InputFile::new(
            "a.js".to_owned(),
            "// @ts-ignore\nlet x;\nlet x;\n".to_owned(),
        )],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        },
    );
    let codes: Vec<u32> = result
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code())
        .collect();

    assert_eq!(codes, [2451]);
}

#[test]
fn check_directive_matches_shebang_bom_and_unicode_line_breaks() {
    assert_eq!(
        check_directive("#!/usr/bin/env node\n// @ts-nocheck\n"),
        Some(CheckDirective::NoCheck)
    );
    assert_eq!(
        check_directive("\u{FEFF}// @ts-nocheck\n"),
        Some(CheckDirective::NoCheck)
    );
    assert_eq!(
        check_directive("\u{FEFF}#!/usr/bin/env node\n// @ts-nocheck\n"),
        None
    );
    assert_eq!(
        check_directive("// @ts-nocheck\u{2028}// @ts-check\u{2029}"),
        Some(CheckDirective::Check)
    );
    assert_eq!(
        check_directive("// @ts-check\u{2028}// @ts-nocheck\u{2029}"),
        Some(CheckDirective::NoCheck)
    );
}

#[test]
fn unicode_line_break_last_ts_check_restores_semantic_diagnostics() {
    let result = check_program(
        &[InputFile::new(
            "a.ts".to_owned(),
            "// @ts-nocheck\u{2028}// @ts-check\u{2028}const value: string = 1;".to_owned(),
        )],
        &CompilerOptions::default(),
    );

    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code() == 2322),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn bom_before_shebang_does_not_enable_following_ts_nocheck() {
    let result = check_program(
        &[InputFile::new(
            "a.ts".to_owned(),
            "\u{FEFF}#!/usr/bin/env node\n// @ts-nocheck\nconst value: string = 1;\n".to_owned(),
        )],
        &CompilerOptions::default(),
    );

    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code() == 2322),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn ts_nocheck_after_shebang_suppresses_semantic_diagnostics() {
    let result = check_program(
        &[InputFile::new(
            "a.ts".to_owned(),
            "#!/usr/bin/env node\n// @ts-nocheck\nconst value: string = 1;\n".to_owned(),
        )],
        &CompilerOptions::default(),
    );

    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
}

#[test]
fn skip_lib_check_preserves_syntax_errors_and_skips_semantic_errors() {
    let result = check_program(
        &[
            InputFile::new(
                "bad-syntax.d.ts".to_owned(),
                "declare const x: ;\n".to_owned(),
            ),
            InputFile::new(
                "bad-semantic.d.ts".to_owned(),
                "declare const y: Missing;\n".to_owned(),
            ),
            InputFile::new(
                "merge-a.d.ts".to_owned(),
                "declare let merged: number;\n".to_owned(),
            ),
            InputFile::new(
                "merge-b.d.ts".to_owned(),
                "declare let merged: string;\n".to_owned(),
            ),
        ],
        &CompilerOptions {
            skip_lib_check: Some(true),
            ..CompilerOptions::default()
        },
    );

    let pins: Vec<(String, u32, u32)> = result
        .diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.file_name.clone().unwrap_or_default(),
                diagnostic.code(),
                diagnostic.start.unwrap_or(u32::MAX),
            )
        })
        .collect();
    assert_eq!(pins, [("bad-syntax.d.ts".to_owned(), 1110, 17)]);
}

// ---- lib-loading L2: lib-backed programs (oracle-pinned) ----

fn es5_lib() -> InputFile {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../vendor/typescript-6.0.3/lib/lib.es5.d.ts"
    );
    InputFile::new(
        "lib.es5.d.ts".to_owned(),
        std::fs::read_to_string(path).expect("vendored lib.es5.d.ts"),
    )
}

fn lib_backed_diags(text: &str) -> Vec<(u32, u32, u32, String)> {
    let result = check_program_with_libs(
        &[es5_lib()],
        &[InputFile::new("a.ts".to_owned(), text.to_owned())],
        &CompilerOptions::default(),
    );
    result
        .diagnostics
        .iter()
        .map(|d| {
            (
                d.code(),
                d.start.unwrap_or(u32::MAX),
                d.length.unwrap_or(u32::MAX),
                d.message_text().to_owned(),
            )
        })
        .collect()
}

#[test]
fn lib_names_resolve_through_the_loaded_lib() {
    assert_eq!(
        lib_backed_diags(
            "interface I<T extends Date> { x: T }
"
        ),
        []
    );
}

#[test]
fn restricted_lib_set_reports_2583_with_the_lib_argument() {
    // Map is not in es5: the failure is GENUINE under this lib set
    // (the lib_globals gate stands down for lib-loaded programs)
    // and the suggested-lib arm supplies tsc's exact argument.
    let diags = lib_backed_diags(
        "interface I<T extends Map> { x: T }
",
    );
    assert_eq!(
            diags,
            [(
                2583,
                22,
                3,
                "Cannot find name 'Map'. Do you need to change your target library? Try changing the 'lib' compiler option to 'es2015' or later."
                    .to_owned()
            )]
        );
}

#[test]
fn lib_array_members_drive_variance_measurement() {
    // Mutable method parameters are bivariant, so es5 Array
    // measures covariant and `out` holds (oracle-pinned clean)...
    assert_eq!(
        lib_backed_diags(
            "interface Wrap<out T> { xs: T[] }
"
        ),
        []
    );
    // ...including when a fixture declaration MERGES into the lib
    // interface (both member sets resolve; oracle-pinned clean).
    assert_eq!(
        lib_backed_diags(
            "interface Array<T> { fixtureExtra: T }
interface Wrap<out T> { xs: T[] }
"
        ),
        []
    );
    assert_eq!(
        lib_backed_diags(
            "interface Array<T> { sink: (x: T) => void }
interface Wrap<out T> { xs: T[] }
"
        ),
        []
    );
    assert_eq!(
        lib_backed_diags(
            "interface Wrap<out T> { xs: ReadonlyArray<T> }
"
        ),
        []
    );
}

#[test]
fn lib_types_render_in_constraint_failure_args() {
    // Named object types print their symbol name in the 2344 args
    // (type_to_string_slice's named-object arm; oracle-pinned).
    let diags = lib_backed_diags("interface Foo<T extends number> { x: T }\ntype X = Foo<Date>;\n");
    assert_eq!(
        diags,
        [(
            2344,
            54,
            4,
            "Type 'Date' does not satisfy the constraint 'number'.".to_owned()
        )]
    );
}

#[test]
fn lib_array_in_parameter_position_reports_2636() {
    let diags = lib_backed_diags(
        "interface Wrap<out T> { f: (xs: T[]) => void }
",
    );
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2636, 15, 5));
    assert!(
        diags[0]
            .3
            .starts_with("Type 'Wrap<sub-T>' is not assignable to type 'Wrap<super-T>'"),
        "{}",
        diags[0].3
    );
}

#[test]
fn check_program_includes_parse_diagnostics() {
    let result = check_program(
        &[InputFile::new(
            "a.ts".to_owned(),
            "\"unterminated".to_owned(),
        )],
        &CompilerOptions::default(),
    );

    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.diagnostics[0].code(), 1002);
}

/// Promise<T> is declared in BOTH es2015.promise and
/// es2015.symbol.wellknown; the merged symbol must expose ONE T
/// (getSymbolOfDeclaration's getMergedSymbol chase inside
/// appendTypeParameters) — without the chase the declared type
/// read `Promise<T, T>` and every `Promise<X>` reference tripped
/// a spurious 2314 (lib-loading L2 find: the async-fixture FPs).
#[test]
fn merged_lib_interface_type_parameters_unify() {
    let vendor = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../vendor/typescript-6.0.3/lib/"
    );
    let lib = |name: &str| {
        InputFile::new(
            name.to_owned(),
            std::fs::read_to_string(format!("{vendor}{name}")).expect("vendored lib"),
        )
    };
    let result = check_program_with_libs(
        &[
            lib("lib.es5.d.ts"),
            lib("lib.es2015.promise.d.ts"),
            lib("lib.es2015.symbol.wellknown.d.ts"),
        ],
        &[InputFile::new(
            "a.ts".to_owned(),
            "type X = Promise<number>;\n".to_owned(),
        )],
        &CompilerOptions::default(),
    );
    assert_eq!(result.diagnostics, []);
}

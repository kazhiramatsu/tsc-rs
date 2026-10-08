use super::*;

#[test]
fn check_js_computes_allow_js_only_when_allow_js_is_absent() {
    for (settings, expected) in [
        (vec![("checkJs".to_owned(), "true".to_owned())], true),
        (
            vec![
                ("allowJs".to_owned(), "false".to_owned()),
                ("checkJs".to_owned(), "true".to_owned()),
            ],
            false,
        ),
    ] {
        let mut compiler_options = CompilerOptions::default();
        let mut program_options = ProgramOptions::default();
        apply_compiler_settings(
            &mut compiler_options,
            &mut program_options,
            "/.src",
            settings
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str())),
            false,
        )
        .expect("project effective allowJs");
        assert_eq!(compiler_options.allow_js, expected);
    }
}

#[test]
fn remove_comments_projects_into_compiler_options() {
    let mut compiler_options = CompilerOptions::default();
    let mut program_options = ProgramOptions::default();
    apply_compiler_setting(
        &mut compiler_options,
        &mut program_options,
        "/.src",
        "removeComments",
        "true",
    )
    .expect("removeComments is an admitted emit option");

    assert_eq!(compiler_options.remove_comments, Some(true));
}

#[test]
fn compiler_fixture_projects_lowercase_newline_directive() {
    let mut compiler_options = CompilerOptions::default();
    let mut program_options = ProgramOptions::default();
    apply_compiler_setting(
        &mut compiler_options,
        &mut program_options,
        "/.src",
        "newline",
        "LF",
    )
    .expect("compiler directives accept the upstream lowercase spelling");

    assert_eq!(compiler_options.new_line, Some(1));
}

#[test]
fn compiler_plan_projects_erasable_syntax_only_into_checker_options() {
    let mut compiler_options = CompilerOptions::default();
    let mut program_options = ProgramOptions::default();
    apply_compiler_setting(
        &mut compiler_options,
        &mut program_options,
        "/.src",
        "erasableSyntaxOnly",
        "true",
    )
    .expect("erasableSyntaxOnly is an admitted checker option");

    assert_eq!(compiler_options.erasable_syntax_only, Some(true));
}

#[test]
fn compiler_plan_retains_isolated_declaration_dependencies() {
    let mut compiler_options = CompilerOptions::default();
    let mut program_options = ProgramOptions::default();

    for (name, value) in [
        ("isolatedDeclarations", "true"),
        ("declaration", "true"),
        ("composite", "false"),
    ] {
        apply_compiler_setting(
            &mut compiler_options,
            &mut program_options,
            "/.src",
            name,
            value,
        )
        .unwrap_or_else(|setting_error| panic!("failed to project {name}: {setting_error}"));
    }

    assert_eq!(compiler_options.isolated_declarations, Some(true));
    assert_eq!(compiler_options.declaration, Some(true));
    assert_eq!(compiler_options.composite, Some(false));
}

#[test]
fn compiler_boolean_directives_preserve_non_true_lexemes_as_false() {
    let mut compiler_options = CompilerOptions::default();
    let mut program_options = ProgramOptions::default();

    apply_compiler_setting(
        &mut compiler_options,
        &mut program_options,
        "/.src",
        "declaration",
        "true;",
    )
    .expect("the upstream harness accepts every boolean directive lexeme");

    assert_eq!(compiler_options.declaration, Some(false));
    assert!(parse_compiler_bool("strict", "TRUE").unwrap());
    assert!(!parse_compiler_bool("strict", "false").unwrap());
    assert!(!parse_compiler_bool("strict", "not-a-boolean").unwrap());
}

#[test]
fn relative_path_exact_case_resolves_mixed_case_module_resolution_key() {
    let mut compiler_options = CompilerOptions::default();
    let mut program_options = ProgramOptions::default();
    apply_compiler_setting(
        &mut compiler_options,
        &mut program_options,
        "/.src",
        "ModuleResolution",
        "classic",
    )
    .expect("relativePathToDeclarationFile admits its exact fixture spelling");

    assert_eq!(compiler_options.module_resolution, Some(1));
}

#[test]
fn compiler_option_lookup_canonicalizes_ascii_case_once() {
    let mut compiler_options = CompilerOptions::default();
    let mut program_options = ProgramOptions::default();
    apply_compiler_setting(
        &mut compiler_options,
        &mut program_options,
        "/.src",
        "mOdUlErEsOlUtIoN",
        "bundler",
    )
    .expect("known compiler options use canonical ASCII keys");

    assert_eq!(compiler_options.module_resolution, Some(100));
}

#[test]
fn compiler_plan_projects_ordered_custom_conditions() {
    let mut compiler_options = CompilerOptions::default();
    let mut program_options = ProgramOptions::default();
    apply_compiler_setting(
        &mut compiler_options,
        &mut program_options,
        "/.src",
        "customConditions",
        "webpack, browser",
    )
    .expect("customConditions is a typed module-resolution option");

    // tsgo ParseListTypeOption keeps the list elements untrimmed.
    assert_eq!(
        compiler_options.custom_conditions.as_deref(),
        Some(&["webpack".into(), " browser".into()][..]),
    );
}

#[test]
fn suppress_output_path_check_is_a_compiler_option() {
    // tsgo's harness parses `@suppressOutputPathCheck` as a compiler option
    // (harnessutil.go:317-333), which turns off the Program's output-path
    // verification (compiler/program.go:1340).
    let mut compiler_options = CompilerOptions::default();
    let mut program_options = ProgramOptions::default();
    let original_program_options = program_options.clone();
    apply_compiler_setting(
        &mut compiler_options,
        &mut program_options,
        "/.src",
        "suppressOutputPathCheck",
        "true",
    )
    .expect("the option is accepted");

    assert_eq!(compiler_options.suppress_output_path_check, Some(true));
    assert_eq!(program_options, original_program_options);
}

#[test]
fn the_native_runner_projects_every_emit_directive() {
    let settings = [
        ("declaration", "true"),
        ("sourceMap", "true"),
        ("inlineSourceMap", "true"),
        ("inlineSources", "true"),
        ("sourceRoot", "/sources"),
        ("mapRoot", "/maps"),
        ("emitBOM", "true"),
        ("outFile", "bundle.js"),
        ("declarationMap", "true"),
        ("eMiTdEcLaRaTiOnOnLy", "true"),
        ("outDir", "out"),
        ("rootDir", "src"),
        ("declarationDir", "types"),
        ("stripInternal", "true"),
        ("noCheck", "true"),
    ];
    let mut actual = CompilerOptions::default();
    apply_compiler_settings(
        &mut actual,
        &mut ProgramOptions::default(),
        "/.src",
        settings,
        false,
    )
    .expect("project the directive values");
    let expected = CompilerOptions {
        declaration: Some(true),
        source_map: Some(true),
        inline_source_map: Some(true),
        inline_sources: Some(true),
        source_root: Some("/sources".into()),
        map_root: Some("/maps".into()),
        emit_bom: Some(true),
        out_file: Some("bundle.js".into()),
        declaration_map: Some(true),
        emit_declaration_only: Some(true),
        out_dir: Some("out".into()),
        root_dir: Some("/.src/src".into()),
        declaration_dir: Some("/.src/types".into()),
        strip_internal: Some(true),
        no_check: Some(true),
        ..CompilerOptions::default()
    };
    assert_eq!(actual, expected);
}

#[test]
fn compiler_vfs_mounts_trailing_aliases_for_fixture_directories() {
    let paths = [
        Path::new("/.src/data1.ts"),
        Path::new("/.src/nested/value.ts"),
    ];
    let aliases = compiler_vfs_trailing_directory_aliases(paths)
        .expect("derive compiler VFS directory aliases")
        .into_iter()
        .map(|path| path.into_os_string().into_string().expect("Unicode path"))
        .collect::<Vec<_>>();

    assert_eq!(aliases, ["/.src/", "/.src/nested/"]);
}

#[test]
fn compiler_fixture_paths_preserve_drive_roots_outside_posix_current_directory() {
    assert_eq!(
        normalize_compiler_fixture_path("/.src", "c:/root/src/file1.ts")
            .expect("normalize drive-rooted compiler fixture path"),
        "c:/root/src/file1.ts"
    );
    assert_eq!(
        normalize_compiler_fixture_path("/.src", r"C:\root\generated\src\file2.ts")
            .expect("normalize backslash drive-rooted compiler fixture path"),
        "C:/root/generated/src/file2.ts"
    );
    assert_eq!(
        normalize_compiler_fixture_path("/.src", "nested/../file3.ts")
            .expect("normalize relative compiler fixture path"),
        "/.src/file3.ts"
    );
    assert_eq!(
        normalize_compiler_fixture_path("/.src", r"\\server\share\folder\..\file4.ts")
            .expect("normalize UNC-rooted compiler fixture path"),
        "//server/share/file4.ts"
    );
    assert_eq!(
        normalize_compiler_fixture_path("/.src", "//?/C:/sdk/./file5.ts")
            .expect("normalize extended drive-rooted compiler fixture path"),
        "//?/C:/sdk/file5.ts"
    );
}

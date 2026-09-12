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
            EmitOptionFloor::Established,
        )
        .expect("project effective allowJs");
        assert_eq!(compiler_options.allow_js, expected);
    }
}

#[test]
fn qualified_emit_projects_remove_comments_into_compiler_options() {
    let mut compiler_options = CompilerOptions::default();
    let mut program_options = ProgramOptions::default();
    apply_compiler_setting(
        &mut compiler_options,
        &mut program_options,
        "/.src",
        "removeComments",
        "true",
        EmitOptionFloor::Established,
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
        EmitOptionFloor::Established,
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
        EmitOptionFloor::Established,
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
            EmitOptionFloor::Established,
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
        EmitOptionFloor::Established,
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
        EmitOptionFloor::Established,
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
        EmitOptionFloor::Established,
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
        EmitOptionFloor::Established,
    )
    .expect("customConditions is a typed module-resolution option");

    assert_eq!(
        compiler_options.custom_conditions.as_deref(),
        Some(&["webpack".to_owned(), "browser".to_owned()][..]),
    );
}

#[test]
fn jsdoc_exact_cases_accept_suppress_output_path_check_as_baseline_metadata() {
    const EXACT_CASES: [&str; 11] = [
        "checkJsdocOptionalParamOrder",
        "checkJsdocParamOnVariableDeclaredFunctionExpression",
        "checkJsdocParamTag1",
        "checkJsdocTypedefInParamTag1",
        "checkJsdocTypedefOnlySourceFile",
        "checkJsdocTypeTag1",
        "checkJsdocTypeTag2",
        "checkJsdocTypeTagOnObjectProperty1",
        "checkJsdocTypeTagOnObjectProperty2",
        "jsdocTypeTagCast",
        "salsa/malformedTags",
    ];

    for case_id in EXACT_CASES {
        let mut compiler_options = CompilerOptions::default();
        let original_compiler_options = compiler_options.clone();
        let mut program_options = ProgramOptions::default();
        let original_program_options = program_options.clone();
        apply_compiler_setting(
            &mut compiler_options,
            &mut program_options,
            "/.src",
            "suppressOutputPathCheck",
            "true",
            EmitOptionFloor::Established,
        )
        .unwrap_or_else(|setting_error| {
            panic!("{case_id} rejects baseline metadata: {setting_error}")
        });

        assert_eq!(compiler_options, original_compiler_options, "{case_id}");
        assert_eq!(program_options, original_program_options, "{case_id}");
    }
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

#[test]
fn current_map_floor_preserves_declaration_only_without_changing_earlier_floors() {
    // Test both directive and typed-config projection, preserving absent vs false.
    // DeclarationFamily keeps its existing outFile/declarationMap exclusion.
    for declaration_only in [None, Some(false), Some(true)] {
        for (floor, source_map, map_family, bom, bundle_family, declaration_mode) in [
            (
                EmitOptionFloor::Established,
                false,
                false,
                false,
                false,
                false,
            ),
            (EmitOptionFloor::SourceMap, true, false, false, false, false),
            (
                EmitOptionFloor::SourceMapWithOptions,
                true,
                true,
                false,
                false,
                false,
            ),
            (EmitOptionFloor::MapFamily, true, true, true, true, false),
            (
                EmitOptionFloor::MapFamilyWithDeclarationOnly,
                true,
                true,
                true,
                true,
                true,
            ),
            (
                EmitOptionFloor::DeclarationFamily,
                true,
                true,
                true,
                false,
                true,
            ),
        ] {
            let mut settings = vec![
                ("declaration", "true"),
                ("sourceMap", "true"),
                ("inlineSourceMap", "true"),
                ("inlineSources", "true"),
                ("sourceRoot", "/sources"),
                ("mapRoot", "/maps"),
                ("emitBOM", "true"),
                ("outFile", "bundle.js"),
                ("declarationMap", "true"),
            ];
            if let Some(value) = declaration_only {
                settings.push(("eMiTdEcLaRaTiOnOnLy", if value { "true" } else { "false" }));
            }
            let mut actual = CompilerOptions::default();
            apply_compiler_settings(
                &mut actual,
                &mut ProgramOptions::default(),
                "/.src",
                settings,
                false,
                floor,
            )
            .expect("project original directive values");
            let expected = CompilerOptions {
                declaration: Some(true),
                source_map: source_map.then_some(true),
                inline_source_map: map_family.then_some(true),
                inline_sources: map_family.then_some(true),
                source_root: map_family.then(|| "/sources".to_owned()),
                map_root: map_family.then(|| "/maps".to_owned()),
                emit_bom: bom.then_some(true),
                out_file: bundle_family.then(|| "bundle.js".to_owned()),
                declaration_map: bundle_family.then_some(true),
                emit_declaration_only: declaration_mode.then_some(declaration_only).flatten(),
                ..CompilerOptions::default()
            };
            assert_eq!(
                actual, expected,
                "{floor:?}, directive {declaration_only:?}"
            );

            let mut config = CompilerOptions {
                declaration: Some(true),
                source_map: Some(true),
                inline_source_map: Some(true),
                inline_sources: Some(true),
                source_root: Some("/sources".to_owned()),
                map_root: Some("/maps".to_owned()),
                emit_bom: Some(true),
                out_file: Some("bundle.js".to_owned()),
                declaration_map: Some(true),
                emit_declaration_only: declaration_only,
                ..CompilerOptions::default()
            };
            apply_emit_option_floor_to_config(&mut config, floor);
            assert_eq!(config, expected, "{floor:?}, config {declaration_only:?}");
        }
    }
}

#[test]
fn current_map_floor_projects_the_two_frozen_commonjs_declaration_only_inputs() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let qualification: Value = serde_json::from_slice(
        &fs::read(workspace.join("ratchets/h2-6c-qualification.v1.json"))
            .expect("frozen H2.6c qualification"),
    )
    .expect("H2.6c JSON");
    let cases = qualification["cases"].as_array().expect("H2.6c cases");
    assert_eq!(cases.len(), 643);
    let mut selected = BTreeMap::new();
    for case in cases {
        let Some(settings) = case["input"]["settings"].as_array() else {
            continue;
        };
        if !settings.iter().any(|setting| {
            setting["name"]
                .as_str()
                .is_some_and(|name| name.eq_ignore_ascii_case("emitDeclarationOnly"))
        }) {
            continue;
        }
        assert_eq!(case["execution_route"], "recorded-compiler-plan");
        let project = |floor| {
            let mut options = CompilerOptions::default();
            apply_compiler_settings(
                &mut options,
                &mut ProgramOptions::default(),
                case["input"]["current_directory"].as_str().unwrap(),
                settings.iter().map(|setting| {
                    (
                        setting["name"].as_str().unwrap(),
                        setting["value"].as_str().unwrap(),
                    )
                }),
                false,
                floor,
            )
            .expect("the unchanged frozen compiler settings are projectable");
            options
        };
        let current = project(EmitOptionFloor::MapFamilyWithDeclarationOnly);
        let mut historical = project(EmitOptionFloor::MapFamily);
        assert_eq!(historical.emit_declaration_only, None);
        assert_eq!(current.emit_declaration_only, Some(true));
        assert_eq!(current.declaration, Some(true));
        assert_eq!(current.source_map, Some(true));
        assert_eq!(current.module, Some(1));
        assert_eq!(current.out_file.as_deref(), Some("all.js"));
        // Every other projected setting remains identical to the old prepare.
        historical.emit_declaration_only = Some(true);
        assert_eq!(current, historical);
        selected.insert(case["case_id"].as_str().unwrap().to_owned(), current.target);
    }
    assert_eq!(
        selected,
        BTreeMap::from([
            (
                "typescript-6.0.3/compiler/outModuleConcatCommonjsDeclarationOnly.ts#target%3Des2015".to_owned(),
                Some(2),
            ),
            (
                "typescript-6.0.3/compiler/outModuleConcatCommonjsDeclarationOnly.ts#target%3Des5".to_owned(),
                Some(1),
            ),
        ]),
    );
}

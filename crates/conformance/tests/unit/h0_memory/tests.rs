use super::{
    bind_type_reference_host_outcome, implied_node_format, implied_node_format_for_emit,
    package_map_from_facts, program_options_from_program, supports_fixture, types_package_name,
    SUPPORTED_FIXTURES,
};
use std::collections::BTreeMap;
use std::path::Path;
use tsc_harness::{OptionValue, ProgramJson};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    CompilerOptions, ModuleExtension, ModuleResolver, PackageId, PackageJsonType, PackageMetadata,
    ProgramPath, ResolutionMode, ResolutionOutcome, SourceFileId,
};

#[test]
fn dedicated_route_is_exactly_the_reviewed_h0_fixtures() {
    assert!(SUPPORTED_FIXTURES
        .iter()
        .all(|fixture| supports_fixture(fixture)));
    for fixture in [
        "conformance/declarationEmit/typesVersionsDeclarationEmit.multiFileBackReferenceToSelf.ts",
        "conformance/declarationEmit/typesVersionsDeclarationEmit.multiFileBackReferenceToUnmapped.ts",
        "conformance/externalModules/rewriteRelativeImportExtensions/nodeModulesTsFiles.ts",
        "conformance/moduleResolution/packageJsonMain_isNonRecursive.ts",
        "conformance/moduleResolution/packageJsonMain.ts",
        "conformance/node/nodeModulesNoDirectoryModule.ts",
        "conformance/node/nodeModulesPackageExports.ts",
        "conformance/jsdoc/importTag17.ts",
        "conformance/typings/typingsLookup1.ts",
        "conformance/typings/typingsLookup3.ts",
        "conformance/externalModules/verbatimModuleSyntaxAmbientConstEnum.ts",
        "conformance/externalModules/verbatimModuleSyntaxConstEnumUsage.ts",
        "conformance/classes/members/privateNames/privateNameEmitHelpers.ts",
        "conformance/classes/members/privateNames/privateNameStaticEmitHelpers.ts",
        "conformance/es2020/modules/exportAsNamespace_missingEmitHelpers.ts",
        "conformance/moduleResolution/resolutionModeImportType1.ts",
        "conformance/moduleResolution/resolutionModeTypeOnlyImport1.ts",
        "conformance/moduleResolution/node10AlternateResult_noResolution.ts",
        "conformance/moduleResolution/node10Alternateresult_noTypes.ts",
        "conformance/salsa/namespaceAssignmentToRequireAlias.ts",
        "conformance/moduleResolution/untypedModuleImport_allowJs.ts",
        "conformance/moduleResolution/untypedModuleImport_withAugmentation.ts",
        "conformance/moduleResolution/untypedModuleImport.ts",
        "conformance/moduleResolution/untypedModuleImport_vsAmbient.ts",
    ] {
        assert!(supports_fixture(fixture), "missing H0 route: {fixture}");
    }
    for fixture in [
        "conformance/node/allowJs/nodeModulesAllowJsPackagePatternExportsTrailers.ts",
        "conformance/externalModules/rewriteRelativeImportExtensions/nonTSExtensions.ts",
        "conformance/declarationEmit/typesVersionsDeclarationEmit.multiFileBackReferenceToSelf.ts.backup",
        "conformance/moduleResolution/packageJsonMain_isNonRecursive.ts.backup",
        "conformance/node/nodeModulesPackagePatternExportsExclude.ts.backup",
        "conformance/externalModules/verbatimModuleSyntaxConstEnum.ts",
        "node/nodeModulesPackagePatternExportsExclude.ts",
    ] {
        assert!(!supports_fixture(fixture), "unexpected H0 route: {fixture}");
    }
}

#[test]
fn h0_type_reference_binding_accepts_all_typescript_source_extensions() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/@types/implementation/package.json",
            br#"{"name":"@types/implementation","version":"1.0.0","types":"index.ts"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/@types/implementation/index.ts",
            b"declare const implementation: true;".to_vec(),
        )
        .file(
            "/work/node_modules/@types/styles/package.json",
            br#"{"name":"@types/styles","version":"1.0.0","types":"index.css"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/@types/styles/index.d.css.ts",
            b"declare const styles: true;".to_vec(),
        )
        .build()
        .expect("build H0 type-reference host");
    let options = CompilerOptions {
        module: Some(199),
        module_resolution: Some(99),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &options).expect("create H0 resolver");

    for (index, (name, expected_path, expected_extension)) in [
        (
            "implementation",
            "/work/node_modules/@types/implementation/index.ts",
            ModuleExtension::Ts,
        ),
        (
            "styles",
            "/work/node_modules/@types/styles/index.d.css.ts",
            ModuleExtension::Arbitrary(".d.css.ts".to_owned()),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let outcome = resolver
            .resolve_type_reference(
                Path::new("/work/root.ts"),
                name,
                ResolutionMode::EsNext,
                None,
            )
            .expect("resolve H0 type-reference target");
        let ResolutionOutcome::Resolved(host_directive) = &outcome else {
            panic!("expected resolved type-reference target: {name}");
        };
        assert_eq!(host_directive.extension(), &expected_extension);
        let target = ProgramPath::from_trusted_parts(expected_path, expected_path)
            .expect("construct target identity");
        let source = SourceFileId::from_raw(
            u32::try_from(index + 1).expect("the focused source id fits u32"),
        );
        let source_by_canonical =
            BTreeMap::from([(target.canonical().as_path().to_path_buf(), source)]);
        assert!(matches!(
            bind_type_reference_host_outcome(outcome, &source_by_canonical)
                .expect("bind H0 type-reference target")
                .outcome(),
            ResolutionOutcome::Resolved(_)
        ));
    }
}

#[test]
fn types_versions_package_root_back_reference_uses_the_harness_directory_identity() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let vendor_lib_dir = workspace.join("vendor/typescript-6.0.3/lib");
    let fixture =
        "conformance/declarationEmit/typesVersionsDeclarationEmit.multiFileBackReferenceToSelf.ts";
    let programs = tsc_harness::expand_fixture_file(
        &workspace.join("ts-tests/tests/cases").join(fixture),
        &vendor_lib_dir,
    )
    .expect("expand the typesVersions back-reference fixture");
    assert_eq!(programs.len(), 1, "unexpected matrix expansion");

    let observed = crate::current_case_tsrs(fixture, &programs[0], &vendor_lib_dir)
        .expect("run the typesVersions back-reference fixture");
    assert_eq!(
        observed
            .all
            .iter()
            .map(|diagnostic| (
                diagnostic.file.as_deref(),
                diagnostic.code,
                diagnostic.start,
                diagnostic.length,
                diagnostic.line,
                diagnostic.col,
            ))
            .collect::<Vec<_>>(),
        [(Some("main.ts"), 2305, Some(9), Some(2), Some(0), Some(9))]
    );
    assert!(observed.syntactic.is_empty());
}

#[test]
fn const_enum_fixture_and_exact_control_match_the_reviewed_conformance_boundary() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let vendor_lib_dir = workspace.join("vendor/typescript-6.0.3/lib");

    let run_fixture = |fixture: &str| {
        let programs = tsc_harness::expand_fixture_file(
            &workspace.join("ts-tests/tests/cases").join(fixture),
            &vendor_lib_dir,
        )
        .expect("expand focused H0 fixture");
        assert_eq!(programs.len(), 1, "unexpected matrix expansion: {fixture}");
        crate::current_case_tsrs(fixture, &programs[0], &vendor_lib_dir)
            .expect("run focused H0 fixture")
    };

    let emitting =
        run_fixture("conformance/externalModules/verbatimModuleSyntaxAmbientConstEnum.ts");
    let observed = emitting
        .all
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.file.as_deref(),
                diagnostic.code,
                diagnostic.start,
                diagnostic.length,
                diagnostic.line,
                diagnostic.col,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        observed,
        [
            (Some("/a.ts"), 2748, Some(9), Some(1), Some(0), Some(9),),
            (Some("/a.ts"), 2748, Some(100), Some(1), Some(3), Some(0),),
            (Some("/b.ts"), 2748, Some(9), Some(1), Some(0), Some(9),),
        ]
    );
    assert!(emitting.syntactic.is_empty());

    let control = run_fixture("conformance/externalModules/verbatimModuleSyntaxConstEnumUsage.ts");
    assert!(control.all.is_empty());
    assert!(control.syntactic.is_empty());
}

#[test]
fn external_helper_fixtures_and_missing_tslib_control_match_the_reviewed_boundary() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let vendor_lib_dir = workspace.join("vendor/typescript-6.0.3/lib");

    let run_fixture = |fixture: &str| {
        let programs = tsc_harness::expand_fixture_file(
            &workspace.join("ts-tests/tests/cases").join(fixture),
            &vendor_lib_dir,
        )
        .expect("expand focused H0 fixture");
        assert_eq!(programs.len(), 1, "unexpected matrix expansion: {fixture}");
        crate::current_case_tsrs(fixture, &programs[0], &vendor_lib_dir)
            .expect("run focused H0 fixture")
    };

    for (fixture, expected) in [
        (
            "conformance/classes/members/privateNames/privateNameEmitHelpers.ts",
            vec![
                ("main.ts", 6133, 34, 2, 3, 4),
                ("main.ts", 2807, 41, 7, 3, 11),
                ("main.ts", 2807, 81, 7, 4, 24),
            ],
        ),
        (
            "conformance/classes/members/privateNames/privateNameStaticEmitHelpers.ts",
            vec![
                ("main.ts", 6133, 29, 2, 2, 11),
                ("main.ts", 2807, 55, 7, 3, 18),
                ("main.ts", 6133, 86, 2, 4, 15),
                ("main.ts", 2807, 100, 4, 4, 29),
            ],
        ),
    ] {
        let observed = run_fixture(fixture);
        assert_eq!(
            observed
                .all
                .iter()
                .map(|diagnostic| {
                    (
                        diagnostic.file.as_deref().unwrap_or_default(),
                        diagnostic.code,
                        diagnostic.start.unwrap_or_default(),
                        diagnostic.length.unwrap_or_default(),
                        diagnostic.line.unwrap_or_default(),
                        diagnostic.col.unwrap_or_default(),
                    )
                })
                .collect::<Vec<_>>(),
            expected,
            "unexpected external-helper stream: {fixture}"
        );
        assert!(observed.syntactic.is_empty());
    }

    let control = run_fixture("conformance/es2020/modules/exportAsNamespace_missingEmitHelpers.ts");
    assert_eq!(
        control
            .all
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.file.as_deref(),
                    diagnostic.code,
                    diagnostic.line,
                    diagnostic.col,
                )
            })
            .collect::<Vec<_>>(),
        [(Some("b.ts"), 2354, Some(0), Some(0))]
    );
    assert!(control.syntactic.is_empty());
}

#[test]
fn alternate_resolution_fixtures_and_controls_match_the_reviewed_boundary() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let vendor_lib_dir = workspace.join("vendor/typescript-6.0.3/lib");

    let run_fixture = |fixture: &str| {
        tsc_harness::expand_fixture_file(
            &workspace.join("ts-tests/tests/cases").join(fixture),
            &vendor_lib_dir,
        )
        .expect("expand focused H0 fixture")
        .into_iter()
        .map(|program| {
            let matrix_key = program.matrix_key.clone();
            let observed = crate::current_case_tsrs(fixture, &program, &vendor_lib_dir)
                .expect("run focused H0 fixture");
            (matrix_key, observed)
        })
        .collect::<Vec<_>>()
    };

    for (fixture, expected) in [
        (
            "conformance/moduleResolution/resolutionModeImportType1.ts",
            [(29, 5, 0, 29), (67, 5, 1, 28), (149, 5, 2, 29)],
        ),
        (
            "conformance/moduleResolution/resolutionModeTypeOnlyImport1.ts",
            [(34, 5, 0, 34), (74, 5, 1, 33), (152, 5, 2, 34)],
        ),
    ] {
        let cases = run_fixture(fixture);
        assert_eq!(cases.len(), 2, "unexpected matrix expansion: {fixture}");
        let bundler = cases
            .iter()
            .find(|(matrix_key, _)| matrix_key == "moduleResolution=bundler")
            .expect("bundler control");
        assert!(bundler.1.all.is_empty(), "{fixture}: bundler control");
        assert!(bundler.1.syntactic.is_empty());

        let classic = cases
            .iter()
            .find(|(matrix_key, _)| matrix_key == "moduleResolution=classic")
            .expect("classic emitting case");
        assert_eq!(
            classic
                .1
                .all
                .iter()
                .map(|diagnostic| {
                    (
                        diagnostic.file.as_deref(),
                        diagnostic.code,
                        diagnostic.start.unwrap_or_default(),
                        diagnostic.length.unwrap_or_default(),
                        diagnostic.line.unwrap_or_default(),
                        diagnostic.col.unwrap_or_default(),
                        diagnostic.chain.text.as_str(),
                    )
                })
                .collect::<Vec<_>>(),
            expected
                .into_iter()
                .map(|(start, length, line, col)| {
                    (
                        Some("/app.ts"),
                        2792,
                        start,
                        length,
                        line,
                        col,
                        "Cannot find module 'foo'. Did you mean to set the 'moduleResolution' option to 'nodenext', or to add aliases to the 'paths' option?",
                    )
                })
                .collect::<Vec<_>>(),
            "unexpected Classic stream: {fixture}"
        );
        assert!(classic.1.syntactic.is_empty());
    }

    let missing = run_fixture("conformance/moduleResolution/node10AlternateResult_noResolution.ts");
    assert_eq!(missing.len(), 1);
    assert_eq!(
        missing[0]
            .1
            .all
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.file.as_deref(),
                    diagnostic.code,
                    diagnostic.start,
                    diagnostic.length,
                    diagnostic.line,
                    diagnostic.col,
                    diagnostic.category.as_str(),
                    diagnostic.pass.as_deref(),
                    diagnostic.chain.text.as_str(),
                )
            })
            .collect::<Vec<_>>(),
        [
            (
                Some("/index.ts"),
                6133,
                Some(0),
                Some(26),
                Some(0),
                Some(0),
                "suggestion",
                None,
                "'pkg' is declared but its value is never read.",
            ),
            (
                Some("/index.ts"),
                2307,
                Some(20),
                Some(5),
                Some(0),
                Some(20),
                "error",
                None,
                "Cannot find module 'pkg' or its corresponding type declarations.",
            ),
        ]
    );
    let missing_module = missing[0]
        .1
        .all
        .iter()
        .find(|diagnostic| diagnostic.code == 2307)
        .expect("Node10 missing-module diagnostic");
    assert_eq!(
        missing_module
            .chain
            .next
            .iter()
            .map(|message| {
                (
                    message.code,
                    message.category.as_str(),
                    message.text.as_str(),
                )
            })
            .collect::<Vec<_>>(),
        [(
            6280,
            "message",
            "There are types at '/node_modules/pkg/definitely-not-index.d.ts', but this result could not be resolved under your current 'moduleResolution' setting. Consider updating to 'node16', 'nodenext', or 'bundler'.",
        )]
    );
    assert!(missing[0].1.syntactic.is_empty());

    let untyped = run_fixture("conformance/moduleResolution/node10Alternateresult_noTypes.ts");
    assert_eq!(untyped.len(), 1);
    assert_eq!(
        untyped[0]
            .1
            .all
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.file.as_deref(),
                    diagnostic.code,
                    diagnostic.start,
                    diagnostic.length,
                    diagnostic.line,
                    diagnostic.col,
                    diagnostic.category.as_str(),
                    diagnostic.pass.as_deref(),
                    diagnostic.chain.text.as_str(),
                )
            })
            .collect::<Vec<_>>(),
        [
            (
                Some("/index.ts"),
                6133,
                Some(0),
                Some(26),
                Some(0),
                Some(0),
                "suggestion",
                None,
                "'pkg' is declared but its value is never read.",
            ),
            (
                Some("/index.ts"),
                7016,
                Some(20),
                Some(5),
                Some(0),
                Some(20),
                "error",
                None,
                "Could not find a declaration file for module 'pkg'. '/node_modules/pkg/untyped.js' implicitly has an 'any' type.",
            ),
        ]
    );
    assert_eq!(
        untyped[0].1.all[1]
            .chain
            .next
            .iter()
            .map(|message| {
                (
                    message.code,
                    message.category.as_str(),
                    message.text.as_str(),
                )
            })
            .collect::<Vec<_>>(),
        [(
            6280,
            "message",
            "There are types at '/node_modules/pkg/definitely-not-index.d.ts', but this result could not be resolved under your current 'moduleResolution' setting. Consider updating to 'node16', 'nodenext', or 'bundler'.",
        )]
    );
    assert!(untyped[0].1.syntactic.is_empty());
}

#[test]
fn untyped_package_consumers_and_controls_match_the_reviewed_boundary() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let vendor_lib_dir = workspace.join("vendor/typescript-6.0.3/lib");

    let run_fixture = |fixture: &str| {
        let programs = tsc_harness::expand_fixture_file(
            &workspace.join("ts-tests/tests/cases").join(fixture),
            &vendor_lib_dir,
        )
        .expect("expand focused H0 fixture");
        assert_eq!(programs.len(), 1, "unexpected matrix expansion: {fixture}");
        crate::current_case_tsrs(fixture, &programs[0], &vendor_lib_dir)
            .expect("run focused H0 fixture")
    };

    let expected_diag = |file: &str,
                         code: u32,
                         start: u32,
                         length: u32,
                         line: u32,
                         col: u32,
                         category: &str,
                         text: &str| crate::GoldenDiag {
        file: Some(file.to_owned()),
        start: Some(start),
        length: Some(length),
        line: Some(line),
        col: Some(col),
        code,
        pass: None,
        category: category.to_owned(),
        chain: crate::GoldenMessageChain {
            text: text.to_owned(),
            code,
            category: category.to_owned(),
            next: Vec::new(),
        },
        related: Vec::new(),
        reports_unnecessary: false,
        reports_deprecated: false,
        source: None,
    };

    let cases = [
        (
            "conformance/salsa/namespaceAssignmentToRequireAlias.ts",
            vec![
                expected_diag(
                    "bug40140.js",
                    7016,
                    18,
                    9,
                    0,
                    18,
                    "suggestion",
                    "Could not find a declaration file for module 'untyped'. '/node_modules/untyped/index.js' implicitly has an 'any' type.",
                ),
                expected_diag(
                    "bug40140.js",
                    2339,
                    32,
                    10,
                    1,
                    2,
                    "error",
                    "Property 'assignment' does not exist on type 'typeof import(\"/node_modules/untyped/index\")'.",
                ),
                expected_diag(
                    "bug40140.js",
                    2339,
                    59,
                    7,
                    2,
                    2,
                    "error",
                    "Property 'noError' does not exist on type 'typeof import(\"/node_modules/untyped/index\")'.",
                ),
            ],
        ),
        (
            "conformance/moduleResolution/untypedModuleImport_allowJs.ts",
            vec![
                expected_diag(
                    "/a.ts",
                    7016,
                    16,
                    5,
                    0,
                    16,
                    "suggestion",
                    "Could not find a declaration file for module 'foo'. '/node_modules/foo/index.js' implicitly has an 'any' type.",
                ),
                expected_diag(
                    "/a.ts",
                    2339,
                    28,
                    3,
                    1,
                    4,
                    "error",
                    "Property 'bar' does not exist on type 'typeof import(\"/node_modules/foo/index\")'.",
                ),
            ],
        ),
        (
            "conformance/moduleResolution/untypedModuleImport_withAugmentation.ts",
            vec![
                expected_diag(
                    "/a.ts",
                    2665,
                    15,
                    5,
                    0,
                    15,
                    "error",
                    "Invalid module name in augmentation. Module 'foo' resolves to an untyped module at '/node_modules/foo/index.js', which cannot be augmented.",
                ),
                expected_diag(
                    "/a.ts",
                    7016,
                    74,
                    5,
                    3,
                    18,
                    "suggestion",
                    "Could not find a declaration file for module 'foo'. '/node_modules/foo/index.js' implicitly has an 'any' type.",
                ),
            ],
        ),
        (
            "conformance/moduleResolution/untypedModuleImport.ts",
            vec![
                expected_diag(
                    "/a.ts",
                    7016,
                    21,
                    5,
                    0,
                    21,
                    "suggestion",
                    "Could not find a declaration file for module 'foo'. '/node_modules/foo/index.js' implicitly has an 'any' type.",
                ),
                expected_diag(
                    "/b.ts",
                    7016,
                    21,
                    5,
                    0,
                    21,
                    "suggestion",
                    "Could not find a declaration file for module 'foo'. '/node_modules/foo/index.js' implicitly has an 'any' type.",
                ),
                expected_diag(
                    "/c.ts",
                    7016,
                    25,
                    5,
                    0,
                    25,
                    "suggestion",
                    "Could not find a declaration file for module 'foo'. '/node_modules/foo/index.js' implicitly has an 'any' type.",
                ),
            ],
        ),
        (
            "conformance/moduleResolution/untypedModuleImport_vsAmbient.ts",
            Vec::new(),
        ),
    ];

    for (fixture, expected) in cases {
        let observed = run_fixture(fixture);
        assert_eq!(observed.all, expected, "unexpected stream: {fixture}");
        assert!(
            observed.all_empty_related_information.is_empty(),
            "unexpected present-but-empty related information: {fixture}"
        );
        assert!(observed.syntactic.is_empty(), "{fixture}");
    }
}

#[test]
fn program_option_projection_preserves_types_and_normalizes_type_roots() {
    let program = ProgramJson {
        schema: 1,
        cwd: "/work/project".to_owned(),
        options: BTreeMap::from([
            ("noLib".to_owned(), OptionValue::Bool(true)),
            (
                "typeRoots".to_owned(),
                OptionValue::StringList(vec!["types".to_owned(), "/shared/types".to_owned()]),
            ),
            (
                "types".to_owned(),
                OptionValue::StringList(vec!["*".to_owned(), "explicit".to_owned()]),
            ),
        ]),
        libs: Vec::new(),
        files: Vec::new(),
        matrix_key: String::new(),
    };

    let options = program_options_from_program(&program, Path::new("/work/project"))
        .expect("project program options");
    assert_eq!(options.no_lib(), Some(true));
    let expected_types = vec!["*".to_owned(), "explicit".to_owned()];
    assert_eq!(options.types(), Some(expected_types.as_slice()));
    let roots = options.type_roots().expect("explicit type roots");
    assert_eq!(roots.len(), 2);
    assert_eq!(
        roots[0].canonical().as_path(),
        Path::new("/work/project/types")
    );
    assert_eq!(roots[1].canonical().as_path(), Path::new("/shared/types"));
}

#[test]
fn package_diagnostic_map_is_a_program_wide_exact_dts_fold() {
    let plain = PackageId::new("pkg", "index.js", "1.0.0");
    let bundled = PackageId::new("bundled", "index.d.ts", "1.0.0");
    let types = PackageId::new("@types/pkg", "index.d.mts", "1.0.0");
    let map = package_map_from_facts([
        (&plain, &ModuleExtension::Js),
        (&plain, &ModuleExtension::Dmts),
        (&bundled, &ModuleExtension::Dts),
        (&types, &ModuleExtension::Dmts),
    ]);

    assert_eq!(map.get("pkg"), Some(&false));
    assert_eq!(map.get("bundled"), Some(&true));
    assert_eq!(map.get("@types/pkg"), Some(&false));
    assert!(map.contains_key(&types_package_name("pkg")));
    assert_eq!(types_package_name("@scope/pkg"), "@types/scope__pkg");
}

#[test]
fn implied_format_uses_explicit_extensions_or_node_package_lookup() {
    fn package_scope(module_type: PackageJsonType) -> PackageMetadata {
        let package_json = ProgramPath::from_trusted_parts("/package.json", "/package.json")
            .expect("trusted package path");
        PackageMetadata::from_trusted_parsed(package_json, "{}", None, None, module_type)
    }

    let module_scope = package_scope(PackageJsonType::Module);
    let common_js_scope = package_scope(PackageJsonType::CommonJs);
    let other_scope = package_scope(PackageJsonType::Other);
    let unspecified_scope = package_scope(PackageJsonType::Unspecified);

    let common_js = CompilerOptions {
        module: Some(1),
        ..CompilerOptions::default()
    };
    assert_eq!(
        implied_node_format("/index.ts", Some(&module_scope), &common_js),
        None
    );
    assert_eq!(
        implied_node_format("/index.mts", Some(&module_scope), &common_js),
        Some(ResolutionMode::EsNext)
    );

    let node = CompilerOptions {
        module: Some(102),
        ..CompilerOptions::default()
    };
    assert_eq!(
        implied_node_format("/index.ts", Some(&module_scope), &node),
        Some(ResolutionMode::EsNext)
    );
    assert_eq!(
        implied_node_format("/node_modules/pkg/index.ts", None, &common_js),
        Some(ResolutionMode::CommonJs)
    );
    assert_eq!(
        implied_node_format_for_emit("/node_modules/pkg/index.ts", None, &common_js),
        None
    );

    let es_next = CompilerOptions {
        module: Some(99),
        module_resolution: Some(99),
        ..CompilerOptions::default()
    };
    assert_eq!(
        implied_node_format("/index.ts", Some(&common_js_scope), &es_next),
        Some(ResolutionMode::CommonJs)
    );
    assert_eq!(
        implied_node_format_for_emit("/index.ts", Some(&common_js_scope), &es_next),
        Some(ResolutionMode::CommonJs)
    );
    assert_eq!(
        implied_node_format("/index.ts", Some(&unspecified_scope), &es_next),
        Some(ResolutionMode::CommonJs)
    );
    assert_eq!(
        implied_node_format("/index.ts", Some(&other_scope), &es_next),
        Some(ResolutionMode::CommonJs)
    );
    assert_eq!(
        implied_node_format_for_emit("/index.ts", Some(&unspecified_scope), &es_next),
        None
    );
    assert_eq!(
        implied_node_format_for_emit("/index.ts", Some(&other_scope), &es_next),
        None
    );
    assert_eq!(
        implied_node_format("/index.ts", None, &es_next),
        Some(ResolutionMode::CommonJs)
    );
    assert_eq!(
        implied_node_format_for_emit("/index.ts", None, &es_next),
        None
    );

    let preserve = CompilerOptions {
        module: Some(200),
        module_resolution: Some(99),
        ..CompilerOptions::default()
    };
    assert_eq!(
        implied_node_format_for_emit("/index.ts", Some(&unspecified_scope), &preserve),
        None
    );
    assert_eq!(
        implied_node_format_for_emit("/index.ts", Some(&unspecified_scope), &node),
        Some(ResolutionMode::CommonJs)
    );
}

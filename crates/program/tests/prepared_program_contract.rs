use std::path::{Path, PathBuf};

use tsc_diagnostics::{Diagnostic, DiagnosticCategory, MessageChain, RelatedInfo};
use tsc_host::{HostError, HostErrorKind, HostOperation};
use tsc_program::{
    CompilerOptions, ModuleExtension, ModuleResolution, PackageId, PackageJsonType,
    PackageMetadata, PathContext, PathMapping, PreparationDiagnostics, PreparationErrorKind,
    PreparationOperation, PreparedAuxiliaryFile, PreparedProgram, PreparedProgramBuilder,
    PreparedRoot, PreparedSourceFile, ProgramOptions, ProgramPath, ResolutionError,
    ResolutionErrorKind, ResolutionKey, ResolutionMode, ResolutionOutcome, ResolutionRequestKind,
    ResolvedModule, ResolvedModuleTarget, ResolvedTypeReferenceDirective, SourceFileId,
    TypeReferenceResolution, TypeReferenceResolutionKey, UnloadedModuleReason,
};

fn path(display: &str, canonical: &str) -> ProgramPath {
    ProgramPath::from_trusted_parts(display, canonical).unwrap()
}

fn no_emit_options() -> CompilerOptions {
    CompilerOptions {
        no_emit: Some(true),
        ..CompilerOptions::default()
    }
}

fn builder() -> PreparedProgramBuilder {
    PreparedProgram::builder(
        PathContext::new(path("/Work", "/work"), false),
        no_emit_options(),
    )
}

fn diagnostic(code: u32) -> Diagnostic {
    Diagnostic::new(
        None,
        None,
        None,
        MessageChain {
            code,
            category: DiagnosticCategory::Error,
            text: format!("diagnostic {code}"),
            next_present: false,
            next: Vec::new(),
        },
    )
}

fn located_diagnostic(code: u32, file_name: &str) -> Diagnostic {
    Diagnostic::new(
        Some(file_name.to_owned()),
        Some(0),
        Some(1),
        MessageChain {
            code,
            category: DiagnosticCategory::Error,
            text: format!("diagnostic {code}"),
            next_present: false,
            next: Vec::new(),
        },
    )
}

#[test]
fn prepared_source_emit_eligibility_defaults_are_source_side_facts() {
    for file_name in [
        "/Work/types.d.ts",
        "/Work/types.d.mts",
        "/Work/types.d.cts",
        "/Work/types.d.css.ts",
    ] {
        assert!(
            !PreparedSourceFile::new(path(file_name, &file_name.to_ascii_lowercase()), "")
                .may_be_emitted(),
            "{file_name}"
        );
    }

    let node_modules_input = PreparedSourceFile::new(
        path(
            "/Work/node_modules/pkg/input.ts",
            "/work/node_modules/pkg/input.ts",
        ),
        "export {};",
    );
    assert!(node_modules_input.may_be_emitted());
    assert!(!node_modules_input
        .with_may_be_emitted(false)
        .may_be_emitted());
}

#[test]
fn preserves_final_program_order_independently_from_root_order() {
    let mut builder = builder();
    let lib = builder
        .add_source_file(PreparedSourceFile::new(
            path("/Work/lib.d.ts", "/work/lib.d.ts"),
            "declare const global: number;",
        ))
        .unwrap();
    let dependency = builder
        .add_source_file(PreparedSourceFile::new(
            path("/Work/B.ts", "/work/b.ts"),
            "export const b = 1;",
        ))
        .unwrap();
    let root = builder
        .add_source_file(PreparedSourceFile::new(
            path("/Work/A.ts", "/work/a.ts"),
            "import { b } from './B'; b;",
        ))
        .unwrap();
    builder.add_library_file(lib).unwrap();
    builder.add_root_file(root).unwrap();

    let explicit_empty_type_roots = Vec::new();
    builder.set_program_options(
        ProgramOptions::default()
            .with_no_lib(false)
            .with_type_roots(explicit_empty_type_roots)
            .with_config_file_path(path("/Work/tsconfig.json", "/work/tsconfig.json"))
            .with_root_dirs(vec![path("/Work/src", "/work/src")])
            .with_paths(vec![PathMapping::new("@app/*", vec!["src/*".to_owned()])]),
    );

    let program = builder.build().unwrap();
    assert_eq!(
        program
            .source_files()
            .iter()
            .map(|source| source.path().display())
            .collect::<Vec<_>>(),
        [
            Path::new("/Work/lib.d.ts"),
            Path::new("/Work/B.ts"),
            Path::new("/Work/A.ts"),
        ]
    );
    assert_eq!(program.library_files(), [lib]);
    assert_eq!(
        program
            .roots()
            .iter()
            .map(tsc_program::PreparedRoot::source)
            .collect::<Vec<_>>(),
        [Some(root)]
    );
    assert!(!program.path_context().use_case_sensitive_file_names());
    assert_eq!(program.current_directory().display(), Path::new("/Work"));
    assert_eq!(
        program.source_file(dependency).unwrap().text(),
        "export const b = 1;"
    );
    assert_eq!(program.program_options().no_lib(), Some(false));
    assert_eq!(program.program_options().type_roots(), Some([].as_slice()));
    assert_eq!(
        program
            .program_options()
            .config_file_path()
            .map(ProgramPath::display),
        Some(Path::new("/Work/tsconfig.json"))
    );
    assert_eq!(
        program.program_options().paths().unwrap()[0].substitutions(),
        ["src/*"]
    );
}

#[test]
fn root_requests_preserve_order_multiplicity_and_missing_entries() {
    let mut complete_builder = builder();
    let source = complete_builder
        .add_source_file(PreparedSourceFile::new(
            path("/Work/main.ts", "/work/main.ts"),
            "export {};",
        ))
        .unwrap();
    let missing = path("/Work/missing.ts", "/work/missing.ts");
    let missing_diagnostic = diagnostic(6053);
    complete_builder
        .add_root(PreparedRoot::missing(
            missing.clone(),
            missing_diagnostic.clone(),
        ))
        .unwrap();
    complete_builder.add_root_file(source).unwrap();
    complete_builder
        .add_root(PreparedRoot::missing(
            missing.clone(),
            missing_diagnostic.clone(),
        ))
        .unwrap();
    complete_builder.set_diagnostics(PreparationDiagnostics::new(
        Vec::new(),
        Vec::new(),
        vec![missing_diagnostic],
    ));

    let program = complete_builder.build().unwrap();
    assert_eq!(
        program
            .roots()
            .iter()
            .map(|root| (root.path().display(), root.source()))
            .collect::<Vec<_>>(),
        [
            (Path::new("/Work/missing.ts"), None),
            (Path::new("/Work/main.ts"), Some(source)),
            (Path::new("/Work/missing.ts"), None),
        ]
    );
    assert_eq!(program.diagnostics().program()[0].code(), 6053);
    assert_eq!(
        program.roots()[0].missing_diagnostic(),
        Some(&program.diagnostics().program()[0])
    );

    let mut missing_diagnostic_omitted = builder();
    missing_diagnostic_omitted
        .add_root(PreparedRoot::missing(
            path("/Work/missing.ts", "/work/missing.ts"),
            diagnostic(6053),
        ))
        .unwrap();
    let error = missing_diagnostic_omitted.build().unwrap_err();
    assert_eq!(error.kind(), PreparationErrorKind::InvalidReference);

    let mut hidden_owned_source = builder();
    hidden_owned_source
        .add_source_file(PreparedSourceFile::new(
            path("/Work/main.ts", "/work/main.ts"),
            "export {};",
        ))
        .unwrap();
    let error = hidden_owned_source
        .add_root(PreparedRoot::missing(
            path("/work/MAIN.ts", "/work/main.ts"),
            diagnostic(6053),
        ))
        .unwrap_err();
    assert_eq!(error.kind(), PreparationErrorKind::InvalidData);
}

#[test]
fn canonical_source_duplicates_collapse_only_when_facts_are_compatible() {
    let mut builder = builder();
    let first = builder
        .add_source_file(PreparedSourceFile::new(
            path("/Work/A.ts", "/work/a.ts"),
            "export {};",
        ))
        .unwrap();
    let duplicate = builder
        .add_source_file(PreparedSourceFile::new(
            path("/work/a.ts", "/work/a.ts"),
            "export {};",
        ))
        .unwrap();
    assert_eq!(first, duplicate);
    assert_eq!(
        builder
            .clone()
            .build()
            .unwrap()
            .source_file(first)
            .unwrap()
            .alternate_display_paths(),
        [PathBuf::from("/work/a.ts")]
    );

    let error = builder
        .add_source_file(PreparedSourceFile::new(
            path("/WORK/A.ts", "/work/a.ts"),
            "export const changed = true;",
        ))
        .unwrap_err();
    assert_eq!(error.kind(), PreparationErrorKind::IdentityConflict);
    assert_eq!(error.operation(), PreparationOperation::AddSourceFile);

    let error = builder
        .add_source_file(
            PreparedSourceFile::new(path("/work/a.ts", "/work/a.ts"), "export {};")
                .with_implied_node_format(ResolutionMode::EsNext),
        )
        .unwrap_err();
    assert_eq!(error.kind(), PreparationErrorKind::IdentityConflict);
}

#[test]
fn realpath_is_separate_and_part_of_source_compatibility() {
    let mut success_builder = builder();
    let source = PreparedSourceFile::new(path("/Work/link/a.ts", "/work/link/a.ts"), "export {};")
        .with_real_path(path("/Work/actual/a.ts", "/work/actual/a.ts"));
    let source_id = success_builder.add_source_file(source).unwrap();
    success_builder.add_root_file(source_id).unwrap();
    let program = success_builder.build().unwrap();
    assert_eq!(
        program
            .source_file(source_id)
            .unwrap()
            .real_path()
            .unwrap()
            .canonical()
            .as_path(),
        Path::new("/work/actual/a.ts")
    );
    assert_eq!(
        program
            .source_file(source_id)
            .unwrap()
            .path()
            .canonical()
            .as_path(),
        Path::new("/work/link/a.ts")
    );

    let mut physical_conflict = builder();
    physical_conflict
        .add_source_file(
            PreparedSourceFile::new(path("/Work/link/a.ts", "/work/link/a.ts"), "export {};")
                .with_real_path(path("/Work/actual/a.ts", "/work/actual/a.ts")),
        )
        .unwrap();
    let error = physical_conflict
        .add_source_file(
            PreparedSourceFile::new(
                path("/Work/other-link/a.ts", "/work/other-link/a.ts"),
                "export const incompatible = true;",
            )
            .with_real_path(path("/work/ACTUAL/a.ts", "/work/actual/a.ts")),
        )
        .unwrap_err();
    assert_eq!(error.kind(), PreparationErrorKind::IdentityConflict);
    assert_eq!(physical_conflict.build().unwrap_err(), error);

    let mut lexical_conflict = builder();
    lexical_conflict
        .add_source_file(
            PreparedSourceFile::new(path("/Work/link/a.ts", "/work/link/a.ts"), "export {};")
                .with_real_path(path("/Work/actual/a.ts", "/work/actual/a.ts")),
        )
        .unwrap();
    let error = lexical_conflict
        .add_source_file(
            PreparedSourceFile::new(path("/work/LINK/a.ts", "/work/link/a.ts"), "export {};")
                .with_real_path(path("/Work/other/a.ts", "/work/other/a.ts")),
        )
        .unwrap_err();
    assert_eq!(error.kind(), PreparationErrorKind::IdentityConflict);
    assert_eq!(lexical_conflict.build().unwrap_err(), error);
}

#[test]
fn requires_explicit_no_emit_and_a_library_prefix() {
    let missing_no_emit = PreparedProgram::builder(
        PathContext::new(path("/Work", "/work"), false),
        CompilerOptions::default(),
    )
    .build()
    .unwrap_err();
    assert_eq!(missing_no_emit.kind(), PreparationErrorKind::InvalidInput);
    assert_eq!(
        missing_no_emit.operation(),
        PreparationOperation::BuildPreparedProgram
    );

    let mut builder = builder();
    let root = builder
        .add_source_file(PreparedSourceFile::new(
            path("/Work/a.ts", "/work/a.ts"),
            "export {};",
        ))
        .unwrap();
    let misplaced_lib = builder
        .add_source_file(PreparedSourceFile::new(
            path("/Work/lib.d.ts", "/work/lib.d.ts"),
            "declare const global: number;",
        ))
        .unwrap();
    builder.add_root_file(root).unwrap();
    builder.add_library_file(misplaced_lib).unwrap();
    let error = builder.build().unwrap_err();
    assert_eq!(error.kind(), PreparationErrorKind::InvalidData);
    assert_eq!(
        error.operation(),
        PreparationOperation::BuildPreparedProgram
    );
}

#[test]
fn canonical_paths_must_match_the_host_case_profile() {
    let insensitive = PreparedProgram::builder(
        PathContext::new(path("/Work", "/Work"), false),
        no_emit_options(),
    )
    .build()
    .unwrap_err();
    assert_eq!(insensitive.kind(), PreparationErrorKind::InvalidData);
    assert_eq!(
        insensitive.operation(),
        PreparationOperation::BuildPreparedProgram
    );
    assert_eq!(insensitive.path(), Some(Path::new("/Work")));

    let mut invalid_option_path = builder();
    invalid_option_path.set_program_options(
        ProgramOptions::default().with_root_dirs(vec![path("/Work/src", "/Work/src")]),
    );
    let error = invalid_option_path.build().unwrap_err();
    assert_eq!(error.kind(), PreparationErrorKind::InvalidData);
    assert_eq!(error.path(), Some(Path::new("/Work/src")));

    let mut invalid_config_path = builder();
    invalid_config_path.set_program_options(
        ProgramOptions::default()
            .with_config_file_path(path("/Work/tsconfig.json", "/Work/tsconfig.json")),
    );
    let error = invalid_config_path.build().unwrap_err();
    assert_eq!(error.kind(), PreparationErrorKind::InvalidData);
    assert_eq!(error.path(), Some(Path::new("/Work/tsconfig.json")));

    let mut sensitive = PreparedProgram::builder(
        PathContext::new(path("/Work", "/Work"), true),
        no_emit_options(),
    );
    let source = sensitive
        .add_source_file(PreparedSourceFile::new(
            path("/Work/Main.TS", "/Work/Main.TS"),
            "export {};",
        ))
        .unwrap();
    sensitive.add_root_file(source).unwrap();
    assert!(sensitive.build().is_ok());

    let protected_unicode = PreparedProgram::builder(
        PathContext::new(path("/W/İıß", "/w/İıß"), false),
        no_emit_options(),
    )
    .build()
    .unwrap();
    assert!(!protected_unicode
        .path_context()
        .use_case_sensitive_file_names());
}

#[test]
fn resolution_keys_keep_mode_and_specifier_spelling_exact() {
    let mut builder = builder();
    let target = builder
        .add_source_file(PreparedSourceFile::new(
            path("/Work/pkg.d.ts", "/work/pkg.d.ts"),
            "export const value: number;",
        ))
        .unwrap();
    let source = builder
        .add_source_file(PreparedSourceFile::new(
            path("/Work/main.ts", "/work/main.ts"),
            "import { value } from 'Pkg';",
        ))
        .unwrap();
    builder.add_library_file(target).unwrap();
    builder.add_root_file(source).unwrap();
    let source_path = builder_path("/work/main.ts");

    let common_js = ResolutionKey::new(source_path.clone(), "Pkg", ResolutionMode::CommonJs);
    let es_next = ResolutionKey::new(source_path.clone(), "Pkg", ResolutionMode::EsNext);
    let unspecified = ResolutionKey::new(source_path.clone(), "Pkg", ResolutionMode::Unspecified);
    let different_case = ResolutionKey::new(source_path, "pkg", ResolutionMode::Unspecified);

    builder
        .add_module_resolution(
            common_js.clone(),
            Ok(ModuleResolution::resolved(ResolvedModule::new(
                ResolvedModuleTarget::Source {
                    source: target,
                    resolved_file: path("/Work/pkg.d.ts", "/work/pkg.d.ts"),
                },
                ModuleExtension::Dts,
            ))),
        )
        .unwrap();
    builder
        .add_module_resolution(es_next.clone(), Ok(ModuleResolution::not_found()))
        .unwrap();
    builder
        .add_module_resolution(unspecified.clone(), Ok(ModuleResolution::not_found()))
        .unwrap();
    builder
        .add_module_resolution(different_case.clone(), Ok(ModuleResolution::not_found()))
        .unwrap();

    let program = builder.build().unwrap();
    assert_eq!(
        program
            .resolutions()
            .require_module(&common_js)
            .unwrap()
            .outcome(),
        &ResolutionOutcome::Resolved(ResolvedModule::new(
            ResolvedModuleTarget::Source {
                source: target,
                resolved_file: path("/Work/pkg.d.ts", "/work/pkg.d.ts"),
            },
            ModuleExtension::Dts,
        ))
    );
    assert!(matches!(
        program
            .resolutions()
            .require_module(&es_next)
            .unwrap()
            .outcome(),
        ResolutionOutcome::NotFound
    ));
    assert_eq!(program.resolutions().module_len(), 4);
    assert!(program
        .resolutions()
        .require_module(&different_case)
        .is_ok());
}

fn builder_path(path: &str) -> tsc_program::CanonicalPath {
    tsc_program::CanonicalPath::from_trusted_normalized(path).unwrap()
}

#[test]
fn unloaded_targets_do_not_require_an_owned_source() {
    let mut builder = builder();
    let source = builder
        .add_source_file(PreparedSourceFile::new(
            path("/Work/main.ts", "/work/main.ts"),
            "import pkg from 'pkg';",
        ))
        .unwrap();
    builder.add_root_file(source).unwrap();
    let key = ResolutionKey::new(builder_path("/work/main.ts"), "pkg", ResolutionMode::EsNext);
    let external = ResolvedModule::new(
        ResolvedModuleTarget::unloaded(
            path(
                "/Work/node_modules/pkg/index.js",
                "/work/node_modules/pkg/index.js",
            ),
            UnloadedModuleReason::NodeModulesDepth,
        ),
        ModuleExtension::Js,
    )
    .with_external_library_import(true)
    .with_original_path(path(
        "/Work/node_modules/pkg/link.js",
        "/work/node_modules/pkg/link.js",
    ));
    builder
        .add_module_resolution(key.clone(), Ok(ModuleResolution::resolved(external)))
        .unwrap();

    let program = builder.build().unwrap();
    let ResolutionOutcome::Resolved(resolved) = program
        .resolutions()
        .require_module(&key)
        .unwrap()
        .outcome()
    else {
        panic!("expected resolved external target");
    };
    assert!(matches!(
        resolved.target(),
        ResolvedModuleTarget::Unloaded { .. }
    ));
    assert_eq!(
        resolved.target().unloaded_reason(),
        Some(UnloadedModuleReason::NodeModulesDepth)
    );
    assert_eq!(
        resolved.original_path().unwrap().canonical().as_path(),
        Path::new("/work/node_modules/pkg/link.js")
    );
}

#[test]
fn resolved_module_target_and_extension_metadata_are_validated_exactly() {
    fn owned_target_builder(target_path: ProgramPath) -> (PreparedProgramBuilder, SourceFileId) {
        let mut builder = builder();
        let target = builder
            .add_source_file(PreparedSourceFile::new(target_path, "export {};"))
            .unwrap();
        let source = builder
            .add_source_file(PreparedSourceFile::new(
                path("/Work/main.ts", "/work/main.ts"),
                "import './dep';",
            ))
            .unwrap();
        builder.add_library_file(target).unwrap();
        builder.add_root_file(source).unwrap();
        (builder, target)
    }

    let key = || {
        ResolutionKey::new(
            builder_path("/work/main.ts"),
            "./dep",
            ResolutionMode::Unspecified,
        )
    };

    let (mut mismatched_path, target) = owned_target_builder(path("/Work/dep.ts", "/work/dep.ts"));
    let error = mismatched_path
        .add_module_resolution(
            key(),
            Ok(ModuleResolution::resolved(ResolvedModule::new(
                ResolvedModuleTarget::Source {
                    source: target,
                    resolved_file: path("/Work/other.ts", "/work/other.ts"),
                },
                ModuleExtension::Ts,
            ))),
        )
        .unwrap_err();
    assert_eq!(error.kind(), PreparationErrorKind::InvalidData);

    let (mut mismatched_extension, target) =
        owned_target_builder(path("/Work/dep.d.ts", "/work/dep.d.ts"));
    let error = mismatched_extension
        .add_module_resolution(
            key(),
            Ok(ModuleResolution::resolved(ResolvedModule::new(
                ResolvedModuleTarget::Source {
                    source: target,
                    resolved_file: path("/Work/dep.d.ts", "/work/dep.d.ts"),
                },
                ModuleExtension::Ts,
            ))),
        )
        .unwrap_err();
    assert_eq!(error.kind(), PreparationErrorKind::InvalidData);

    let mut unloaded_targets = builder();
    let source = unloaded_targets
        .add_source_file(PreparedSourceFile::new(
            path("/Work/main.ts", "/work/main.ts"),
            "import data from './data.json'; import './theme.css';",
        ))
        .unwrap();
    unloaded_targets.add_root_file(source).unwrap();
    let json_key = ResolutionKey::new(
        builder_path("/work/main.ts"),
        "./data.json",
        ResolutionMode::Unspecified,
    );
    unloaded_targets
        .add_module_resolution(
            json_key.clone(),
            Ok(ModuleResolution::resolved(ResolvedModule::new(
                ResolvedModuleTarget::unloaded(
                    path("/Work/data.json", "/work/data.json"),
                    UnloadedModuleReason::JsonWithoutResolveJsonModule,
                ),
                ModuleExtension::Json,
            ))),
        )
        .unwrap();
    let css_key = ResolutionKey::new(
        builder_path("/work/main.ts"),
        "./theme.css",
        ResolutionMode::Unspecified,
    );
    unloaded_targets
        .add_module_resolution(
            css_key.clone(),
            Ok(ModuleResolution::resolved(ResolvedModule::new(
                ResolvedModuleTarget::unloaded(
                    path("/Work/theme.d.css.ts", "/work/theme.d.css.ts"),
                    UnloadedModuleReason::ArbitraryExtensionWithoutOption,
                ),
                ModuleExtension::Arbitrary(".d.css.ts".to_owned()),
            ))),
        )
        .unwrap();
    let program = unloaded_targets.build().unwrap();
    assert_eq!(program.source_files().len(), 1);
    assert!(program.resolutions().require_module(&json_key).is_ok());
    assert!(program.resolutions().require_module(&css_key).is_ok());

    let (mut unloaded_owned, _target) =
        owned_target_builder(path("/Work/dep.d.ts", "/work/dep.d.ts"));
    let error = unloaded_owned
        .add_module_resolution(
            key(),
            Ok(ModuleResolution::resolved(ResolvedModule::new(
                ResolvedModuleTarget::unloaded(
                    path("/Work/dep.d.ts", "/work/dep.d.ts"),
                    UnloadedModuleReason::ResolutionOnly,
                ),
                ModuleExtension::Dts,
            ))),
        )
        .unwrap_err();
    assert_eq!(error.kind(), PreparationErrorKind::InvalidData);

    let (mut arbitrary_extension, target) =
        owned_target_builder(path("/Work/dep.d.css.ts", "/work/dep.d.css.ts"));
    let arbitrary_key = key();
    arbitrary_extension
        .add_module_resolution(
            arbitrary_key.clone(),
            Ok(ModuleResolution::resolved(ResolvedModule::new(
                ResolvedModuleTarget::Source {
                    source: target,
                    resolved_file: path("/Work/dep.d.css.ts", "/work/dep.d.css.ts"),
                },
                ModuleExtension::Arbitrary(".d.css.ts".to_owned()),
            ))),
        )
        .unwrap();
    let program = arbitrary_extension.build().unwrap();
    let ResolutionOutcome::Resolved(module) = program
        .resolutions()
        .require_module(&arbitrary_key)
        .unwrap()
        .outcome()
    else {
        panic!("expected arbitrary-extension module");
    };
    assert_eq!(module.extension().as_str(), ".d.css.ts");

    let (mut case_preserved_arbitrary, target) =
        owned_target_builder(path("/Work/theme.d.CSS.ts", "/work/theme.d.css.ts"));
    let case_preserved_key = key();
    case_preserved_arbitrary
        .add_module_resolution(
            case_preserved_key.clone(),
            Ok(ModuleResolution::resolved(ResolvedModule::new(
                ResolvedModuleTarget::Source {
                    source: target,
                    resolved_file: path("/Work/theme.d.CSS.ts", "/work/theme.d.css.ts"),
                },
                ModuleExtension::Arbitrary(".d.CSS.ts".to_owned()),
            ))),
        )
        .unwrap();
    let program = case_preserved_arbitrary.build().unwrap();
    let ResolutionOutcome::Resolved(module) = program
        .resolutions()
        .require_module(&case_preserved_key)
        .unwrap()
        .outcome()
    else {
        panic!("expected a case-preserved arbitrary extension");
    };
    assert_eq!(module.extension().as_str(), ".d.CSS.ts");

    let (mut path_bearing_arbitrary, target) =
        owned_target_builder(path("/Work/dir.d.ext/.ts", "/work/dir.d.ext/.ts"));
    let path_bearing_key = key();
    path_bearing_arbitrary
        .add_module_resolution(
            path_bearing_key.clone(),
            Ok(ModuleResolution::resolved(ResolvedModule::new(
                ResolvedModuleTarget::Source {
                    source: target,
                    resolved_file: path("/Work/dir.d.ext/.ts", "/work/dir.d.ext/.ts"),
                },
                ModuleExtension::Arbitrary(".d.ext/.ts".to_owned()),
            ))),
        )
        .unwrap();
    let program = path_bearing_arbitrary.build().unwrap();
    let ResolutionOutcome::Resolved(module) = program
        .resolutions()
        .require_module(&path_bearing_key)
        .unwrap()
        .outcome()
    else {
        panic!("expected path-bearing arbitrary-extension module");
    };
    assert_eq!(module.extension().as_str(), ".d.ext/.ts");

    let (mut contradictory_arbitrary, target) =
        owned_target_builder(path("/Work/dep.d.css.ts", "/work/dep.d.css.ts"));
    let error = contradictory_arbitrary
        .add_module_resolution(
            key(),
            Ok(ModuleResolution::resolved(ResolvedModule::new(
                ResolvedModuleTarget::Source {
                    source: target,
                    resolved_file: path("/Work/dep.d.css.ts", "/work/dep.d.css.ts"),
                },
                ModuleExtension::Js,
            ))),
        )
        .unwrap_err();
    assert_eq!(error.kind(), PreparationErrorKind::InvalidData);
}

#[test]
fn resolved_targets_retain_realpaths_and_original_lexical_paths() {
    let mut lexical_identity_builder = builder();
    let lexical = path("/Work/link/dep.d.ts", "/work/link/dep.d.ts");
    let physical = path("/Work/actual/dep.d.ts", "/work/actual/dep.d.ts");
    let target = lexical_identity_builder
        .add_source_file(
            PreparedSourceFile::new(lexical.clone(), "export {};").with_real_path(physical.clone()),
        )
        .unwrap();
    let source = lexical_identity_builder
        .add_source_file(PreparedSourceFile::new(
            path("/Work/main.ts", "/work/main.ts"),
            "/// <reference types='dep' />",
        ))
        .unwrap();
    lexical_identity_builder.add_library_file(target).unwrap();
    lexical_identity_builder.add_root_file(source).unwrap();

    let module_key = ResolutionKey::new(
        builder_path("/work/main.ts"),
        "dep",
        ResolutionMode::Unspecified,
    );
    lexical_identity_builder
        .add_module_resolution(
            module_key.clone(),
            Ok(ModuleResolution::resolved(
                ResolvedModule::new(
                    ResolvedModuleTarget::Source {
                        source: target,
                        resolved_file: physical.clone(),
                    },
                    ModuleExtension::Dts,
                )
                .with_original_path(lexical.clone()),
            )),
        )
        .unwrap();
    let type_key = TypeReferenceResolutionKey::source(
        builder_path("/work/main.ts"),
        "dep",
        ResolutionMode::Unspecified,
    );
    lexical_identity_builder
        .add_type_reference_resolution(
            type_key.clone(),
            Ok(TypeReferenceResolution::resolved(
                ResolvedTypeReferenceDirective::new(physical.clone(), target)
                    .with_original_path(lexical.clone()),
            )),
        )
        .unwrap();

    let program = lexical_identity_builder.build().unwrap();
    let ResolutionOutcome::Resolved(module) = program
        .resolutions()
        .require_module(&module_key)
        .unwrap()
        .outcome()
    else {
        panic!("expected resolved module");
    };
    assert_eq!(module.target().resolved_file(), &physical);
    assert_eq!(module.original_path(), Some(&lexical));
    let ResolutionOutcome::Resolved(reference) = program
        .resolutions()
        .require_type_reference(&type_key)
        .unwrap()
        .outcome()
    else {
        panic!("expected resolved type reference");
    };
    assert_eq!(reference.target(), &physical);
    assert_eq!(reference.original_path(), Some(&lexical));

    let mut resolved_identity_builder = builder();
    let target = resolved_identity_builder
        .add_source_file(PreparedSourceFile::new(physical.clone(), "export {};"))
        .unwrap();
    let source = resolved_identity_builder
        .add_source_file(PreparedSourceFile::new(
            path("/Work/main.ts", "/work/main.ts"),
            "import 'dep';",
        ))
        .unwrap();
    resolved_identity_builder.add_library_file(target).unwrap();
    resolved_identity_builder.add_root_file(source).unwrap();
    resolved_identity_builder
        .add_module_resolution(
            ResolutionKey::new(
                builder_path("/work/main.ts"),
                "dep",
                ResolutionMode::Unspecified,
            ),
            Ok(ModuleResolution::resolved(
                ResolvedModule::new(
                    ResolvedModuleTarget::Source {
                        source: target,
                        resolved_file: physical.clone(),
                    },
                    ModuleExtension::Dts,
                )
                .with_original_path(lexical.clone()),
            )),
        )
        .unwrap();
    resolved_identity_builder
        .add_type_reference_resolution(
            TypeReferenceResolutionKey::source(
                builder_path("/work/main.ts"),
                "dep",
                ResolutionMode::Unspecified,
            ),
            Ok(TypeReferenceResolution::resolved(
                ResolvedTypeReferenceDirective::new(physical, target).with_original_path(lexical),
            )),
        )
        .unwrap();
    assert!(resolved_identity_builder.build().is_ok());
}

#[test]
fn original_paths_must_describe_the_lexical_to_physical_transition() {
    fn symlink_builder() -> (PreparedProgramBuilder, SourceFileId) {
        let mut builder = builder();
        let target = builder
            .add_source_file(
                PreparedSourceFile::new(
                    path("/Work/link/dep.d.ts", "/work/link/dep.d.ts"),
                    "export {};",
                )
                .with_real_path(path("/Work/actual/dep.d.ts", "/work/actual/dep.d.ts")),
            )
            .unwrap();
        let source = builder
            .add_source_file(PreparedSourceFile::new(
                path("/Work/main.ts", "/work/main.ts"),
                "import 'dep';",
            ))
            .unwrap();
        builder.add_library_file(target).unwrap();
        builder.add_root_file(source).unwrap();
        (builder, target)
    }

    let lexical = path("/Work/link/dep.d.ts", "/work/link/dep.d.ts");
    let physical = path("/Work/actual/dep.d.ts", "/work/actual/dep.d.ts");
    let wrong = path("/Work/other/dep.d.ts", "/work/other/dep.d.ts");
    for (target_path, original_path) in [
        (physical.clone(), Some(wrong.clone())),
        (physical.clone(), None),
        (lexical.clone(), Some(physical.clone())),
    ] {
        let (mut builder, target) = symlink_builder();
        let mut module = ResolvedModule::new(
            ResolvedModuleTarget::Source {
                source: target,
                resolved_file: target_path,
            },
            ModuleExtension::Dts,
        );
        if let Some(original_path) = original_path {
            module = module.with_original_path(original_path);
        }
        let error = builder
            .add_module_resolution(
                ResolutionKey::new(
                    builder_path("/work/main.ts"),
                    "dep",
                    ResolutionMode::Unspecified,
                ),
                Ok(ModuleResolution::resolved(module)),
            )
            .unwrap_err();
        assert_eq!(error.kind(), PreparationErrorKind::InvalidData);
    }

    for (target_path, original_path) in [
        (physical.clone(), Some(wrong)),
        (physical.clone(), None),
        (lexical, Some(physical)),
    ] {
        let (mut builder, target) = symlink_builder();
        let mut directive = ResolvedTypeReferenceDirective::new(target_path, target);
        if let Some(original_path) = original_path {
            directive = directive.with_original_path(original_path);
        }
        let error = builder
            .add_type_reference_resolution(
                TypeReferenceResolutionKey::source(
                    builder_path("/work/main.ts"),
                    "dep",
                    ResolutionMode::Unspecified,
                ),
                Ok(TypeReferenceResolution::resolved(directive)),
            )
            .unwrap_err();
        assert_eq!(error.kind(), PreparationErrorKind::InvalidData);
    }
}

#[test]
fn resolution_failures_cannot_be_inserted_as_misses() {
    fn resolution_builder() -> (PreparedProgramBuilder, ResolutionKey) {
        let mut builder = builder();
        let source = builder
            .add_source_file(PreparedSourceFile::new(
                path("/Work/main.ts", "/work/main.ts"),
                "import 'pkg';",
            ))
            .unwrap();
        builder.add_root_file(source).unwrap();
        let key = ResolutionKey::new(builder_path("/work/main.ts"), "pkg", ResolutionMode::EsNext);
        (builder, key)
    }

    let (mut unsupported_builder, key) = resolution_builder();
    let unsupported = unsupported_builder
        .add_module_resolution(
            key.clone(),
            Err(ResolutionError::unsupported(
                "package exports array",
                "not in the current H0 owner family",
            )),
        )
        .unwrap_err();
    assert_eq!(unsupported.kind(), PreparationErrorKind::ResolutionFailure);
    assert_eq!(
        unsupported.resolution().unwrap().kind(),
        ResolutionErrorKind::Unsupported
    );
    assert_eq!(
        unsupported_builder
            .add_module_resolution(key, Ok(ModuleResolution::not_found()))
            .unwrap_err(),
        unsupported
    );
    assert_eq!(unsupported_builder.build().unwrap_err(), unsupported);

    let denied = HostError::new(
        HostErrorKind::PermissionDenied,
        HostOperation::ReadFile,
        Some(PathBuf::from("/Work/node_modules/pkg/package.json")),
        "denied by test host",
    );
    let (mut host_builder, key) = resolution_builder();
    let host_failure = host_builder
        .add_module_resolution(key.clone(), Err(ResolutionError::from(denied.clone())))
        .unwrap_err();
    assert_eq!(host_failure.kind(), PreparationErrorKind::ResolutionFailure);
    assert_eq!(
        host_failure.resolution().unwrap(),
        &ResolutionError::Host(denied)
    );
    assert_eq!(
        host_builder
            .add_module_resolution(key, Ok(ModuleResolution::not_found()))
            .unwrap_err(),
        host_failure
    );
    assert_eq!(host_builder.build().unwrap_err(), host_failure);
}

#[test]
fn duplicate_resolution_keys_are_idempotent_but_conflicts_fail_closed() {
    let mut builder = builder();
    let source = builder
        .add_source_file(PreparedSourceFile::new(
            path("/Work/main.ts", "/work/main.ts"),
            "import './missing';",
        ))
        .unwrap();
    builder.add_root_file(source).unwrap();
    let key = ResolutionKey::new(
        builder_path("/work/main.ts"),
        "./missing",
        ResolutionMode::Unspecified,
    );
    let alternate = path("/Work/alternate.d.ts", "/work/alternate.d.ts");
    builder
        .add_module_resolution(
            key.clone(),
            Ok(ModuleResolution::not_found().with_alternate_result(alternate.clone())),
        )
        .unwrap();
    builder
        .add_module_resolution(
            key.clone(),
            Ok(ModuleResolution::not_found().with_alternate_result(alternate.clone())),
        )
        .unwrap();
    assert_eq!(
        builder
            .clone()
            .build()
            .unwrap()
            .resolutions()
            .require_module(&key)
            .unwrap()
            .alternate_result(),
        Some(&alternate)
    );

    let conflict = ModuleResolution::not_found()
        .with_alternate_result(path("/Work/other.d.ts", "/work/other.d.ts"));
    let error = builder
        .add_module_resolution(key, Ok(conflict))
        .unwrap_err();
    assert_eq!(error.kind(), PreparationErrorKind::IdentityConflict);
    assert_eq!(error.operation(), PreparationOperation::AddModuleResolution);
    assert_eq!(builder.build().unwrap_err(), error);
}

#[test]
fn resolution_sources_and_owned_targets_are_validated() {
    let mut unknown_origin_builder = builder();
    let source = unknown_origin_builder
        .add_source_file(PreparedSourceFile::new(
            path("/Work/main.ts", "/work/main.ts"),
            "import './dep';",
        ))
        .unwrap();
    unknown_origin_builder.add_root_file(source).unwrap();

    let unknown_source = ResolutionKey::new(
        builder_path("/work/unknown.ts"),
        "./dep",
        ResolutionMode::Unspecified,
    );
    let error = unknown_origin_builder
        .add_module_resolution(unknown_source, Ok(ModuleResolution::not_found()))
        .unwrap_err();
    assert_eq!(error.kind(), PreparationErrorKind::InvalidReference);
    assert_eq!(unknown_origin_builder.build().unwrap_err(), error);

    let mut unknown_target_builder = builder();
    let source = unknown_target_builder
        .add_source_file(PreparedSourceFile::new(
            path("/Work/main.ts", "/work/main.ts"),
            "import './dep';",
        ))
        .unwrap();
    unknown_target_builder.add_root_file(source).unwrap();
    let key = ResolutionKey::new(
        builder_path("/work/main.ts"),
        "./dep",
        ResolutionMode::Unspecified,
    );
    let error = unknown_target_builder
        .add_module_resolution(
            key,
            Ok(ModuleResolution::resolved(ResolvedModule::new(
                ResolvedModuleTarget::Source {
                    source: SourceFileId::from_raw(999),
                    resolved_file: path("/Work/dep.ts", "/work/dep.ts"),
                },
                ModuleExtension::Ts,
            ))),
        )
        .unwrap_err();
    assert_eq!(error.kind(), PreparationErrorKind::InvalidReference);
    assert_eq!(unknown_target_builder.build().unwrap_err(), error);

    let mut mismatched_type_target_builder = builder();
    let source = mismatched_type_target_builder
        .add_source_file(PreparedSourceFile::new(
            path("/Work/main.ts", "/work/main.ts"),
            "/// <reference types='types' />",
        ))
        .unwrap();
    mismatched_type_target_builder
        .add_root_file(source)
        .unwrap();
    let type_key = TypeReferenceResolutionKey::source(
        builder_path("/work/main.ts"),
        "types",
        ResolutionMode::Unspecified,
    );
    let error = mismatched_type_target_builder
        .add_type_reference_resolution(
            type_key,
            Ok(TypeReferenceResolution::resolved(
                ResolvedTypeReferenceDirective::new(
                    path("/Work/types/index.d.ts", "/work/types/index.d.ts"),
                    source,
                ),
            )),
        )
        .unwrap_err();
    assert_eq!(error.kind(), PreparationErrorKind::InvalidData);
    assert_eq!(mismatched_type_target_builder.build().unwrap_err(), error);
}

#[test]
fn source_package_scopes_must_reference_prepared_metadata() {
    let mut builder = builder();
    let source = builder
        .add_source_file(
            PreparedSourceFile::new(
                path(
                    "/Work/node_modules/pkg/index.d.ts",
                    "/work/node_modules/pkg/index.d.ts",
                ),
                "export {};",
            )
            .with_package_scope(builder_path("/work/node_modules/pkg/package.json")),
        )
        .unwrap();
    builder.add_root_file(source).unwrap();

    let error = builder.build().unwrap_err();
    assert_eq!(error.kind(), PreparationErrorKind::InvalidReference);
    assert_eq!(
        error.operation(),
        PreparationOperation::BuildPreparedProgram
    );
}

#[test]
fn module_and_type_reference_tables_keep_the_same_exact_key_independently() {
    let mut builder = builder();
    let types = builder
        .add_source_file(PreparedSourceFile::new(
            path("/Work/types/pkg/index.d.ts", "/work/types/pkg/index.d.ts"),
            "declare const pkg: number;",
        ))
        .unwrap();
    let source = builder
        .add_source_file(PreparedSourceFile::new(
            path("/Work/main.ts", "/work/main.ts"),
            "/// <reference types='pkg' />",
        ))
        .unwrap();
    builder.add_library_file(types).unwrap();
    builder.add_root_file(source).unwrap();
    let key = ResolutionKey::new(
        builder_path("/work/main.ts"),
        "pkg",
        ResolutionMode::Unspecified,
    );
    let type_key = TypeReferenceResolutionKey::source(
        builder_path("/work/main.ts"),
        "pkg",
        ResolutionMode::Unspecified,
    );
    builder
        .add_module_resolution(key.clone(), Ok(ModuleResolution::not_found()))
        .unwrap();
    builder
        .add_type_reference_resolution(
            type_key.clone(),
            Ok(TypeReferenceResolution::resolved(
                ResolvedTypeReferenceDirective::new(
                    path("/Work/types/pkg/index.d.ts", "/work/types/pkg/index.d.ts"),
                    types,
                )
                .with_primary(true),
            )),
        )
        .unwrap();

    let program = builder.build().unwrap();
    assert!(program.resolutions().require_module(&key).is_ok());
    let ResolutionOutcome::Resolved(reference) = program
        .resolutions()
        .require_type_reference(&type_key)
        .unwrap()
        .outcome()
    else {
        panic!("expected resolved type reference");
    };
    assert_eq!(reference.source(), types);
    assert!(reference.primary());
}

#[test]
fn automatic_type_references_use_a_synthetic_origin_and_missing_rows_are_typed_errors() {
    let mut complete_builder = builder();
    let types = complete_builder
        .add_source_file(PreparedSourceFile::new(
            path("/Work/types/pkg/index.d.ts", "/work/types/pkg/index.d.ts"),
            "declare const pkg: number;",
        ))
        .unwrap();
    let source = complete_builder
        .add_source_file(PreparedSourceFile::new(
            path("/Work/main.ts", "/work/main.ts"),
            "export {};",
        ))
        .unwrap();
    complete_builder.add_library_file(types).unwrap();
    complete_builder.add_root_file(source).unwrap();
    let automatic_key = TypeReferenceResolutionKey::automatic(
        builder_path("/work/__inferred type names__.ts"),
        "pkg",
    );
    assert_eq!(automatic_key.mode(), ResolutionMode::Unspecified);
    complete_builder
        .add_type_reference_resolution(
            automatic_key.clone(),
            Ok(TypeReferenceResolution::resolved(
                ResolvedTypeReferenceDirective::new(
                    path("/Work/types/pkg/index.d.ts", "/work/types/pkg/index.d.ts"),
                    types,
                )
                .with_primary(true),
            )),
        )
        .unwrap();

    let program = complete_builder.build().unwrap();
    assert!(automatic_key.origin().is_automatic());
    assert!(matches!(
        program
            .resolutions()
            .require_type_reference(&automatic_key)
            .unwrap()
            .outcome(),
        ResolutionOutcome::Resolved(_)
    ));

    let missing_module_key = ResolutionKey::new(
        builder_path("/work/main.ts"),
        "missing",
        ResolutionMode::CommonJs,
    );
    let missing_module = program
        .resolutions()
        .require_module(&missing_module_key)
        .unwrap_err();
    assert_eq!(missing_module.request_kind(), ResolutionRequestKind::Module);
    assert_eq!(missing_module.origin(), missing_module_key.source());
    assert_eq!(missing_module.type_reference_origin(), None);
    assert_eq!(missing_module.specifier(), "missing");
    assert_eq!(missing_module.mode(), ResolutionMode::CommonJs);

    let missing_type_key = TypeReferenceResolutionKey::automatic(
        builder_path("/work/__inferred type names__.ts"),
        "other",
    );
    let missing_type = program
        .resolutions()
        .require_type_reference(&missing_type_key)
        .unwrap_err();
    assert_eq!(
        missing_type.request_kind(),
        ResolutionRequestKind::TypeReference
    );
    assert_eq!(
        missing_type.origin(),
        missing_type_key.origin().canonical_path()
    );
    assert_eq!(
        missing_type.type_reference_origin(),
        Some(missing_type_key.origin())
    );
    assert_eq!(missing_type.specifier(), "other");
    assert_eq!(missing_type.mode(), ResolutionMode::Unspecified);

    let mut invalid_automatic = builder();
    let invalid_key = TypeReferenceResolutionKey::automatic(
        builder_path("/work/not-the-inferred-types-file.ts"),
        "pkg",
    );
    let error = invalid_automatic
        .add_type_reference_resolution(invalid_key, Ok(TypeReferenceResolution::not_found()))
        .unwrap_err();
    assert_eq!(error.kind(), PreparationErrorKind::InvalidData);
    assert_eq!(invalid_automatic.build().unwrap_err(), error);
}

#[test]
fn package_identity_includes_peer_dependencies() {
    let without_peers = PackageId::new("pkg", "index.d.ts", "1.0.0");
    let react_18 = without_peers
        .clone()
        .with_peer_dependencies("+react@18.3.1");
    let react_19 = without_peers
        .clone()
        .with_peer_dependencies("+react@19.0.0");

    assert_ne!(without_peers, react_18);
    assert_ne!(react_18, react_19);
    assert_eq!(react_18.peer_dependencies(), Some("+react@18.3.1"));
}

#[test]
fn package_metadata_and_diagnostic_buckets_remain_owned_and_distinct() {
    let mut builder = builder();
    let source = builder
        .add_source_file(
            PreparedSourceFile::new(path("/Work/main.mts", "/work/main.mts"), "export {};")
                .with_implied_node_format(ResolutionMode::EsNext)
                .with_package_scope(builder_path("/work/package.json")),
        )
        .unwrap();
    builder.add_root_file(source).unwrap();
    let package = PackageMetadata::from_trusted_parsed(
        path("/Work/package.json", "/work/package.json"),
        r#"{"name":"pkg","version":"1.0.0","type":"module"}"#,
        Some("pkg".to_owned()),
        Some("1.0.0".to_owned()),
        PackageJsonType::Module,
    );
    builder.add_package_metadata(package.clone()).unwrap();
    builder
        .add_package_metadata(PackageMetadata::from_trusted_parsed(
            path("/work/PACKAGE.json", "/work/package.json"),
            r#"{"name":"pkg","version":"1.0.0","type":"module"}"#,
            Some("pkg".to_owned()),
            Some("1.0.0".to_owned()),
            PackageJsonType::Module,
        ))
        .unwrap();
    builder.set_diagnostics(PreparationDiagnostics::new(
        vec![diagnostic(1001), diagnostic(1002)],
        vec![diagnostic(2001)],
        vec![diagnostic(3001), diagnostic(3002)],
    ));

    let program = builder.build().unwrap();
    assert_eq!(program.packages().count(), 1);
    let prepared_package = program
        .package(&builder_path("/work/package.json"))
        .unwrap();
    assert_eq!(prepared_package.text(), package.text());
    assert_eq!(prepared_package.name(), package.name());
    assert_eq!(prepared_package.version(), package.version());
    assert_eq!(prepared_package.module_type(), package.module_type());
    assert_eq!(
        prepared_package.alternate_display_paths(),
        [PathBuf::from("/work/PACKAGE.json")]
    );
    assert_eq!(
        program
            .diagnostics()
            .config()
            .iter()
            .map(Diagnostic::code)
            .collect::<Vec<_>>(),
        [1001, 1002]
    );
    assert_eq!(
        program
            .diagnostics()
            .options()
            .iter()
            .map(Diagnostic::code)
            .collect::<Vec<_>>(),
        [2001]
    );
    assert_eq!(
        program
            .diagnostics()
            .program()
            .iter()
            .map(Diagnostic::code)
            .collect::<Vec<_>>(),
        [3001, 3002]
    );
}

#[test]
fn every_located_preparation_diagnostic_keeps_owned_source_text() {
    let mut complete_builder = builder();
    let source = complete_builder
        .add_source_file(PreparedSourceFile::new(
            path("/Work/main.ts", "/work/main.ts"),
            "import './missing';",
        ))
        .unwrap();
    complete_builder.add_root_file(source).unwrap();
    complete_builder
        .add_auxiliary_file(PreparedAuxiliaryFile::new(
            path("/Work/tsconfig.json", "/work/tsconfig.json"),
            "{}",
        ))
        .unwrap();
    complete_builder
        .add_package_metadata(PackageMetadata::new(
            path("/Work/package.json", "/work/package.json"),
            r#"{"name":"pkg"}"#,
        ))
        .unwrap();
    complete_builder
        .add_package_metadata(PackageMetadata::new(
            path("/work/PACKAGE.json", "/work/package.json"),
            r#"{"name":"pkg"}"#,
        ))
        .unwrap();

    let mut config = located_diagnostic(1001, "/Work/tsconfig.json");
    config.related_information_present = true;
    config.related.push(RelatedInfo {
        file_name: Some("/work/PACKAGE.json".to_owned()),
        start: Some(0),
        length: Some(1),
        message: MessageChain {
            code: 1002,
            category: DiagnosticCategory::Message,
            text: "package scope".to_owned(),
            next_present: false,
            next: Vec::new(),
        },
    });
    complete_builder.set_diagnostics(PreparationDiagnostics::new(
        vec![config],
        Vec::new(),
        Vec::new(),
    ));
    complete_builder
        .add_module_resolution(
            ResolutionKey::new(
                builder_path("/work/main.ts"),
                "./missing",
                ResolutionMode::Unspecified,
            ),
            Ok(ModuleResolution::not_found()
                .with_diagnostics(vec![located_diagnostic(1003, "/Work/main.ts")])),
        )
        .unwrap();

    let program = complete_builder.build().unwrap();
    assert_eq!(
        program
            .auxiliary_file(&builder_path("/work/tsconfig.json"))
            .unwrap()
            .text(),
        "{}"
    );

    let mut missing_primary = builder();
    missing_primary.set_diagnostics(PreparationDiagnostics::new(
        vec![located_diagnostic(2001, "/Work/missing.json")],
        Vec::new(),
        Vec::new(),
    ));
    let error = missing_primary.build().unwrap_err();
    assert_eq!(error.kind(), PreparationErrorKind::InvalidReference);
    assert_eq!(error.path(), Some(Path::new("/Work/missing.json")));

    let mut missing_related = builder();
    missing_related
        .add_auxiliary_file(PreparedAuxiliaryFile::new(
            path("/Work/tsconfig.json", "/work/tsconfig.json"),
            "{}",
        ))
        .unwrap();
    let mut diagnostic = located_diagnostic(3001, "/Work/tsconfig.json");
    diagnostic.related.push(RelatedInfo {
        file_name: Some("/Work/missing-package.json".to_owned()),
        start: Some(0),
        length: Some(1),
        message: MessageChain {
            code: 3002,
            category: DiagnosticCategory::Message,
            text: "missing package".to_owned(),
            next_present: false,
            next: Vec::new(),
        },
    });
    missing_related.set_diagnostics(PreparationDiagnostics::new(
        vec![diagnostic],
        Vec::new(),
        Vec::new(),
    ));
    let error = missing_related.build().unwrap_err();
    assert_eq!(error.kind(), PreparationErrorKind::InvalidReference);
    assert_eq!(error.path(), Some(Path::new("/Work/missing-package.json")));

    let mut slash_equivalent = builder();
    slash_equivalent
        .add_auxiliary_file(PreparedAuxiliaryFile::new(
            path("C:/Work/tsconfig.json", "c:/work/tsconfig.json"),
            "{}",
        ))
        .unwrap();
    slash_equivalent.set_diagnostics(PreparationDiagnostics::new(
        vec![located_diagnostic(4001, "C:\\Work\\tsconfig.json")],
        Vec::new(),
        Vec::new(),
    ));
    assert!(slash_equivalent.build().is_ok());
}

#[test]
fn canonical_text_owners_cannot_disagree_across_roles() {
    let canonical = "/work/package.json";

    let mut source_then_auxiliary = builder();
    source_then_auxiliary
        .add_source_file(PreparedSourceFile::new(
            path("/Work/package.json", canonical),
            "source text",
        ))
        .unwrap();
    let error = source_then_auxiliary
        .add_auxiliary_file(PreparedAuxiliaryFile::new(
            path("/work/PACKAGE.json", canonical),
            "different auxiliary text",
        ))
        .unwrap_err();
    assert_eq!(error.kind(), PreparationErrorKind::IdentityConflict);
    assert_eq!(error.operation(), PreparationOperation::AddAuxiliaryFile);

    let mut auxiliary_then_package = builder();
    auxiliary_then_package
        .add_auxiliary_file(PreparedAuxiliaryFile::new(
            path("/Work/package.json", canonical),
            "auxiliary text",
        ))
        .unwrap();
    let error = auxiliary_then_package
        .add_package_metadata(PackageMetadata::new(
            path("/work/PACKAGE.json", canonical),
            "different package text",
        ))
        .unwrap_err();
    assert_eq!(error.kind(), PreparationErrorKind::IdentityConflict);
    assert_eq!(error.operation(), PreparationOperation::AddPackageMetadata);

    let mut package_then_source = builder();
    package_then_source
        .add_package_metadata(PackageMetadata::new(
            path("/Work/package.json", canonical),
            "package text",
        ))
        .unwrap();
    let error = package_then_source
        .add_source_file(PreparedSourceFile::new(
            path("/work/PACKAGE.json", canonical),
            "different source text",
        ))
        .unwrap_err();
    assert_eq!(error.kind(), PreparationErrorKind::IdentityConflict);
    assert_eq!(error.operation(), PreparationOperation::AddSourceFile);

    let mut physical_alias_conflict = builder();
    physical_alias_conflict
        .add_source_file(
            PreparedSourceFile::new(
                path("/Work/link/data.json", "/work/link/data.json"),
                "source text",
            )
            .with_real_path(path("/Work/actual/data.json", "/work/actual/data.json")),
        )
        .unwrap();
    let error = physical_alias_conflict
        .add_auxiliary_file(PreparedAuxiliaryFile::new(
            path("/Work/actual/data.json", "/work/actual/data.json"),
            "different auxiliary text",
        ))
        .unwrap_err();
    assert_eq!(error.kind(), PreparationErrorKind::IdentityConflict);
    assert_eq!(error.operation(), PreparationOperation::AddAuxiliaryFile);

    let mut compatible = builder();
    let source = compatible
        .add_source_file(PreparedSourceFile::new(
            path("/Work/package.json", canonical),
            "shared text",
        ))
        .unwrap();
    compatible
        .add_auxiliary_file(PreparedAuxiliaryFile::new(
            path("/work/PACKAGE.json", canonical),
            "shared text",
        ))
        .unwrap();
    compatible
        .add_package_metadata(PackageMetadata::new(
            path("/WORK/package.json", canonical),
            "shared text",
        ))
        .unwrap();
    compatible.add_root_file(source).unwrap();
    assert!(compatible.build().is_ok());
}

#[test]
fn prepared_program_is_owned_and_repeat_deterministic() {
    fn make_program() -> PreparedProgram {
        let display = String::from("/Work/a.ts");
        let canonical = String::from("/work/a.ts");
        let text = String::from("export const value = 1;");
        let mut builder = builder();
        let root = builder
            .add_source_file(PreparedSourceFile::new(
                ProgramPath::from_trusted_parts(display, canonical).unwrap(),
                text,
            ))
            .unwrap();
        builder.add_root_file(root).unwrap();
        builder.build().unwrap()
    }

    let first = make_program();
    let second = make_program();
    assert_eq!(first, second);
    assert_eq!(first.clone(), first);
    assert_eq!(first.source_files()[0].text(), "export const value = 1;");
}

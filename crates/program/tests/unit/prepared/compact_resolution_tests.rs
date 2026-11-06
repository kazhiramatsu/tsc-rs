use super::*;
use crate::{ModuleExtension, PackageId, ResolvedModule};

fn path(display: &str, canonical: &str) -> ProgramPath {
    ProgramPath::from_js_parts(display.into(), canonical.into()).unwrap()
}

fn builder() -> PreparedProgramBuilder {
    PreparedProgram::builder(
        PathContext::new(path("/Work", "/work"), false),
        CompilerOptions {
            no_emit: Some(true),
            ..CompilerOptions::default()
        },
    )
}

#[test]
fn compact_module_order_is_the_staging_key_order() {
    let mut builder = builder();
    let mut expected = BTreeMap::new();
    for file in ["z", "a", "m", "\u{e000}", "\u{10000}"] {
        let source_path = format!("/work/{file}.ts");
        let id = builder
            .add_source_file(PreparedSourceFile::new(
                path(&source_path, &source_path),
                "",
            ))
            .unwrap();
        builder.add_root_file(id).unwrap();
        for specifier in ["z", "A", "á", "\u{e000}", "\u{10000}"] {
            for mode in [
                ResolutionMode::Unspecified,
                ResolutionMode::EsNext,
                ResolutionMode::CommonJs,
            ] {
                let key = ResolutionKey::new(
                    path(&source_path, &source_path).canonical().clone(),
                    specifier,
                    mode,
                );
                expected.insert(key.clone(), ());
                builder
                    .add_module_resolution(key, Ok(ModuleResolution::not_found()))
                    .unwrap();
            }
        }
    }
    let program = builder.build().unwrap();
    let actual = program
        .resolutions()
        .modules()
        .map(|(key, _)| ResolutionKey::new(key.source().clone(), key.specifier(), key.mode()))
        .collect::<Vec<_>>();
    assert_eq!(actual, expected.into_keys().collect::<Vec<_>>());
}

#[test]
fn alias_keys_are_rejected_even_when_the_alias_maps_to_a_source() {
    let mut builder = builder();
    let mut file = PreparedSourceFile::new(path("/Work/own.ts", "/work/own.ts"), "");
    let alias = path("/Work/alias.ts", "/work/alias.ts");
    file.remember_package_redirect(alias.clone());
    let source = builder.add_source_file(file).unwrap();
    assert_eq!(
        builder.source_by_canonical.get(alias.canonical()),
        Some(&source)
    );
    let error = builder
        .add_module_resolution(
            ResolutionKey::new(alias.canonical().clone(), "pkg", ResolutionMode::EsNext),
            Ok(ModuleResolution::not_found()),
        )
        .unwrap_err();
    assert_eq!(error.kind(), PreparationErrorKind::InvalidReference);
}

#[test]
fn target_display_spelling_is_retained_even_when_the_canonical_path_matches() {
    let mut builder = builder();
    let importer = builder
        .add_source_file(PreparedSourceFile::new(
            path("/Work/main.ts", "/work/main.ts"),
            "",
        ))
        .unwrap();
    builder.add_root_file(importer).unwrap();
    let mut file = PreparedSourceFile::new(path("/Work/Target.ts", "/work/target.ts"), "");
    file.remember_display_alias("/Work/TARGET.ts".into());
    let target = builder.add_source_file(file).unwrap();
    let resolved = path("/Work/TARGET.ts", "/work/target.ts");
    let key = ResolutionKey::new(
        path("/Work/main.ts", "/work/main.ts").canonical().clone(),
        "./Target",
        ResolutionMode::EsNext,
    );
    builder
        .add_module_resolution(
            key.clone(),
            Ok(ModuleResolution::resolved(ResolvedModule::new(
                ResolvedModuleTarget::Source {
                    source: target,
                    resolved_file: resolved.clone(),
                },
                ModuleExtension::Ts,
            ))),
        )
        .unwrap();
    let program = builder.build().unwrap();
    let row = program.resolutions().require_module(&key).unwrap();
    let ResolutionOutcome::Resolved(module) = row.outcome() else {
        panic!("resolved")
    };
    assert_eq!(module.target().resolved_file(), &resolved);
    assert_ne!(
        module.target().resolved_file(),
        program.source_file(target).unwrap().path()
    );
}

#[test]
fn duplicate_rows_conflict_at_add_time() {
    let mut builder = builder();
    let file = path("/Work/main.ts", "/work/main.ts");
    let source = builder
        .add_source_file(PreparedSourceFile::new(file.clone(), ""))
        .unwrap();
    builder.add_root_file(source).unwrap();
    let key = ResolutionKey::new(file.canonical().clone(), "pkg", ResolutionMode::EsNext);
    builder
        .add_module_resolution(key.clone(), Ok(ModuleResolution::not_found()))
        .unwrap();
    builder
        .add_module_resolution(key.clone(), Ok(ModuleResolution::not_found()))
        .unwrap();
    // A resolved row with the same key conflicts before build(), rather than
    // silently replacing the first row during compaction.
    let resolution = ModuleResolution::resolved(ResolvedModule::new(
        ResolvedModuleTarget::Source {
            source,
            resolved_file: file,
        },
        ModuleExtension::Ts,
    ));
    assert!(builder.add_module_resolution(key, Ok(resolution)).is_err());
}

#[test]
fn lookups_are_confined_to_the_owning_sources_range_and_mode() {
    let mut builder = builder();
    let own = path("/Work/own.ts", "/work/own.ts");
    let alias = path("/Work/alias.ts", "/work/alias.ts");
    let mut file = PreparedSourceFile::new(own.clone(), "");
    file.remember_package_redirect(alias.clone());
    let source = builder.add_source_file(file).unwrap();
    builder.add_root_file(source).unwrap();
    let empty = builder
        .add_source_file(PreparedSourceFile::new(
            path("/Work/empty.ts", "/work/empty.ts"),
            "",
        ))
        .unwrap();
    for (mode, specifier) in [
        (ResolutionMode::CommonJs, "pkg"),
        (ResolutionMode::EsNext, "pkg"),
        (ResolutionMode::Unspecified, "other"),
    ] {
        builder
            .add_module_resolution(
                ResolutionKey::new(own.canonical().clone(), specifier, mode),
                Ok(if mode == ResolutionMode::CommonJs {
                    ModuleResolution::resolved(ResolvedModule::new(
                        ResolvedModuleTarget::Source {
                            source,
                            resolved_file: own.clone(),
                        },
                        ModuleExtension::Ts,
                    ))
                } else {
                    ModuleResolution::not_found()
                }),
            )
            .unwrap();
    }
    let program = builder.build().unwrap();
    for mode in [ResolutionMode::CommonJs, ResolutionMode::EsNext] {
        let resolution = program
            .module_resolution(source, "pkg".into(), mode)
            .unwrap();
        assert_eq!(
            matches!(resolution.outcome(), ResolutionOutcome::Resolved(_)),
            mode == ResolutionMode::CommonJs
        );
        assert!(program
            .module_resolution(empty, "pkg".into(), mode)
            .is_none());
        assert!(program
            .module_resolution(source, "missing".into(), mode)
            .is_none());
        assert!(program
            .module_resolution_by_path(alias.canonical(), "pkg".into(), mode)
            .is_none());
        assert!(program
            .module_resolution_by_path(own.canonical(), "pkg".into(), mode)
            .is_some());
    }
    assert!(program
        .module_resolution(source, "pkg".into(), ResolutionMode::Unspecified)
        .is_none());
    assert!(program
        .module_resolution(
            SourceFileId::from_raw(u32::MAX),
            "pkg".into(),
            ResolutionMode::EsNext
        )
        .is_none());
    // Reading and cloning expose only borrowed views; neither operation can
    // insert, remove or replace a row in the original program.
    let snapshot = program.clone();
    for _ in 0..3 {
        let _ = program.resolutions().modules().count();
    }
    assert_eq!(program, snapshot);
}

#[test]
#[should_panic(expected = "replacement source path")]
fn replacing_a_source_cannot_invalidate_resolution_ranges() {
    let mut builder = builder();
    let file = path("/Work/own.ts", "/work/own.ts");
    let source = builder
        .add_source_file(PreparedSourceFile::new(file, ""))
        .unwrap();
    builder.add_root_file(source).unwrap();
    let program = builder.build().unwrap();
    program.with_replaced_source(
        source,
        PreparedSourceFile::new(path("/Work/different.ts", "/work/different.ts"), ""),
    );
}

#[test]
fn empty_diagnostics_and_interned_packages_do_not_allocate_rare_boxes() {
    let mut builder = builder();
    let file = path("/Work/main.ts", "/work/main.ts");
    let source = builder
        .add_source_file(PreparedSourceFile::new(file.clone(), ""))
        .unwrap();
    builder.add_root_file(source).unwrap();
    for specifier in ["pkg", "pkg/other"] {
        builder
            .add_module_resolution(
                ResolutionKey::new(file.canonical().clone(), specifier, ResolutionMode::EsNext),
                Ok(ModuleResolution::resolved(
                    ResolvedModule::new(
                        ResolvedModuleTarget::Source {
                            source,
                            resolved_file: file.clone(),
                        },
                        ModuleExtension::Ts,
                    )
                    .with_package_id(PackageId::new("pkg", "main.ts", "1.0.0")),
                )
                .with_diagnostics(Vec::with_capacity(8))),
            )
            .unwrap();
    }
    let program = builder.build().unwrap();
    let stats = program.resolutions().module_storage();
    assert!(stats.row_size <= 88);
    assert_eq!(stats.rare_boxes, 0);
    assert_eq!(stats.package_ids, 1);
    let ResolutionOutcome::Resolved(module) = program
        .module_resolution(source, "pkg".into(), ResolutionMode::EsNext)
        .unwrap()
        .outcome()
    else {
        panic!("resolved")
    };
    assert!(std::ptr::eq(
        module.target().resolved_file(),
        program.source_file(source).unwrap().path()
    ));
}

#[test]
fn rare_facts_and_flags_round_trip_through_a_single_box() {
    let mut builder = builder();
    let file = path("/Work/main.ts", "/work/main.ts");
    let source = builder
        .add_source_file(PreparedSourceFile::new(file.clone(), ""))
        .unwrap();
    builder.add_root_file(source).unwrap();
    let own = path("/Work/physical.d.css.ts", "/work/physical.d.css.ts");
    let redirected = path("/Work/copy.d.css.ts", "/work/copy.d.css.ts");
    let mut target = PreparedSourceFile::new(own, "");
    target.remember_package_redirect(redirected.clone());
    let target = builder.add_source_file(target).unwrap();
    let diagnostic = Diagnostic::new(
        None,
        None,
        None,
        tsc_diagnostics::MessageChain::new(tsc_diagnostics::by_code(2209).unwrap(), &[]),
    );
    let row = ModuleResolution::resolved(
        ResolvedModule::new(
            ResolvedModuleTarget::Source {
                source: target,
                resolved_file: redirected,
            },
            ModuleExtension::Arbitrary(".d.css.ts".into()),
        )
        .with_original_path(path(
            "/Work/node_modules/pkg/index.d.css.ts",
            "/work/node_modules/pkg/index.d.css.ts",
        ))
        .with_package_id(
            PackageId::new("pkg", "index.d.css.ts", "1.0.0").with_peer_dependencies("+peer@2"),
        )
        .with_external_library_import(true)
        .with_resolved_using_ts_extension(true)
        .with_resolved_using_extra_extensions(true)
        .with_custom_resolution(true),
    )
    .with_alternate_result(path("/Work/alternate.ts", "/work/alternate.ts"))
    .with_types_package_exists(true)
    .with_package_bundles_types(true)
    .with_diagnostics(vec![diagnostic]);
    builder
        .add_module_resolution(
            ResolutionKey::new(file.canonical().clone(), "pkg", ResolutionMode::EsNext),
            Ok(row.clone()),
        )
        .unwrap();
    let program = builder.build().unwrap();
    assert_eq!(program.resolutions().module_storage().rare_boxes, 1);
    assert_eq!(
        program
            .module_resolution(source, "pkg".into(), ResolutionMode::EsNext)
            .unwrap()
            .to_owned(),
        row
    );
    let replaced =
        program.with_replaced_source(source, PreparedSourceFile::new(file, "// new text"));
    assert_eq!(program.resolutions(), replaced.resolutions());
    assert_eq!(
        program.module_resolution(source, "pkg".into(), ResolutionMode::EsNext),
        replaced.module_resolution(source, "pkg".into(), ResolutionMode::EsNext)
    );
    assert_eq!(
        replaced
            .module_resolution(source, "pkg".into(), ResolutionMode::EsNext)
            .unwrap()
            .to_owned(),
        row
    );
}

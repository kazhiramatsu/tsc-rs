use super::*;

fn legacy_build(
    staged: StagedModules,
    type_references: BTreeMap<TypeReferenceResolutionKey, TypeReferenceResolution>,
    sources: &[PreparedSourceFile],
) -> ResolutionTable {
    let StagedModules {
        mut rows,
        by_source,
    } = staged;
    drop(by_source);
    rows.sort_unstable_by(|a, b| {
        sources[a.source.index()]
            .path()
            .canonical()
            .cmp(sources[b.source.index()].path().canonical())
            .then_with(|| a.specifier.cmp(&b.specifier))
            .then_with(|| a.mode.cmp(&b.mode))
    });
    let mut modules = Vec::with_capacity(rows.len());
    let mut source_ranges = vec![0..0; sources.len()];
    let mut packages = BTreeMap::new();
    for StagedRow {
        source: importing_source,
        specifier,
        mode,
        resolution,
    } in rows
    {
        let start = u32::try_from(modules.len()).expect("module row index fits u32");
        let range = &mut source_ranges[importing_source.index()];
        if range.start == range.end {
            range.start = start;
        }
        range.end = start.checked_add(1).expect("module range fits u32");
        let ModuleResolution {
            outcome,
            alternate_result,
            types_package_exists,
            package_bundles_types,
            diagnostics,
        } = resolution;
        let mut rare = Rare {
            alternate_result,
            diagnostics,
            ..Rare::default()
        };
        let mut flags =
            flag(types_package_exists, TYPES_PACKAGE) | flag(package_bundles_types, BUNDLES_TYPES);
        let mut extension = ExtensionTag::Ts;
        let mut package_id = None;
        let target = match outcome {
            ResolutionOutcome::NotFound => Target::NotFound,
            ResolutionOutcome::Resolved(module) => {
                let ResolvedModule {
                    target,
                    extension: ext,
                    original_path,
                    is_external_library_import,
                    resolved_using_ts_extension,
                    resolved_using_extra_extensions,
                    is_custom_resolution,
                    package_id: package,
                } = module;
                (extension, rare.arbitrary_extension) = ExtensionTag::from_owned(ext);
                rare.original_path = original_path;
                flags |= flag(is_external_library_import, EXTERNAL)
                    | flag(resolved_using_ts_extension, TS_EXTENSION)
                    | flag(resolved_using_extra_extensions, EXTRA_EXTENSION)
                    | flag(is_custom_resolution, CUSTOM);
                if let Some(package) = package {
                    let next = NonZeroU32::new(
                        u32::try_from(packages.len() + 1).expect("package id fits u32"),
                    )
                    .unwrap();
                    package_id = Some(*packages.entry(package).or_insert(next));
                }
                match target {
                    ResolvedModuleTarget::Source {
                        source,
                        resolved_file,
                    } => {
                        // ProgramPath equality includes BOTH display and canonical.
                        if &resolved_file != sources[source.index()].path() {
                            rare.resolved_file = Some(resolved_file);
                        }
                        Target::Source(source)
                    }
                    ResolvedModuleTarget::Unloaded {
                        resolved_file,
                        reason,
                    } => Target::Unloaded {
                        path: Box::new(resolved_file),
                        reason,
                    },
                }
            }
        };
        modules.push(Row {
            specifier,
            mode,
            importing_source,
            target,
            extension,
            flags,
            package_id,
            rare: (!rare.is_empty()).then(|| Box::new(rare)),
        });
    }
    // The temporary interning index owns the identities during building;
    // consume it so the immutable side table owns each identity only once.
    let mut packages = packages
        .into_iter()
        .map(|(package, id)| (id, package))
        .collect::<Vec<_>>();
    packages.sort_unstable_by_key(|(id, _)| *id);
    let package_ids = packages.into_iter().map(|(_, package)| package).collect();
    ResolutionTable {
        modules,
        source_ranges,
        package_ids,
        type_references,
    }
}

#[test]
fn grouped_freeze_matches_the_old_table_for_shuffled_rows() {
    fn path(name: &str) -> ProgramPath {
        ProgramPath::from_js_parts(name.into(), name.into()).unwrap()
    }
    let sources: Vec<_> = [
        "/work/z.ts",
        "/work/a.ts",
        "/work/empty.ts",
        "/work/𐀀.ts",
        "/work/á.ts",
        "/work/\u{e000}.ts",
    ]
    .into_iter()
    .map(|name| PreparedSourceFile::new(path(name), ""))
    .collect();
    let source_by_canonical = sources
        .iter()
        .enumerate()
        .map(|(i, file)| {
            (
                file.path().canonical().clone(),
                SourceFileId::from_raw(i as u32),
            )
        })
        .collect();
    let modes = [
        ResolutionMode::Unspecified,
        ResolutionMode::EsNext,
        ResolutionMode::CommonJs,
    ];
    let specifiers = ["z", "A", "á", "𐀀", "\u{e000}", "pkg/other"];
    let mut entries = Vec::new();
    for (source, file) in sources.iter().enumerate() {
        if source == 2 {
            continue;
        }
        for (index, specifier) in specifiers.into_iter().enumerate() {
            for mode in modes {
                let key = ResolutionKey::new(file.path().canonical().clone(), specifier, mode);
                let resolution = match index % 3 {
                    0 => ModuleResolution::not_found(),
                    _ => {
                        let target = if index % 3 == 1 {
                            ResolvedModuleTarget::Source {
                                source: SourceFileId::from_raw(0),
                                resolved_file: path("/work/Z.ts"),
                            }
                        } else {
                            ResolvedModuleTarget::Unloaded {
                                resolved_file: path("/unloaded/a.ts"),
                                reason: UnloadedModuleReason::NoResolve,
                            }
                        };
                        ModuleResolution::resolved(
                            ResolvedModule::new(
                                target,
                                if index % 2 == 0 {
                                    ModuleExtension::Arbitrary(".d.css.ts".into())
                                } else {
                                    ModuleExtension::Ts
                                },
                            )
                            .with_package_id(PackageId::new(
                                format!("pkg{source}"),
                                "main.ts",
                                "1.0.0",
                            ))
                            .with_original_path(path("/original/a.ts"))
                            .with_custom_resolution(true)
                            .with_external_library_import(true)
                            .with_resolved_using_ts_extension(true)
                            .with_resolved_using_extra_extensions(true),
                        )
                    }
                }
                .with_types_package_exists(index % 2 == 0)
                .with_package_bundles_types(index % 2 == 1)
                .with_alternate_result(path("/alternate/a.ts"))
                .with_diagnostics(vec![tsc_diagnostics::Diagnostic::new(
                    None,
                    None,
                    None,
                    tsc_diagnostics::MessageChain::new(
                        tsc_diagnostics::by_code(2307).unwrap(),
                        &[specifier.to_owned()],
                    ),
                )]);
                entries.push((SourceFileId::from_raw(source as u32), key, resolution));
            }
        }
    }
    let mut state = 0x123456789abcdefu64;
    for round in 0..32 {
        // Deterministic Fisher-Yates insertion orders, including duplicate
        // equal keys and a conflicting row rejected before either freeze.
        for i in (1..entries.len()).rev() {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            entries.swap(i, state as usize % (i + 1));
        }
        let mut staged = StagedModules::default();
        for (source, key, resolution) in &entries {
            staged
                .insert(*source, key.clone(), resolution.clone())
                .unwrap();
            staged
                .insert(*source, key.clone(), resolution.clone())
                .unwrap();
        }
        let (source, key, _) = &entries[0];
        assert!(staged
            .insert(*source, key.clone(), ModuleResolution::not_found())
            .is_err());
        let expected = legacy_build(staged.clone(), BTreeMap::new(), &sources);
        let actual = ResolutionTable::build(staged, BTreeMap::new(), &sources);
        assert_eq!(
            actual, expected,
            "round {round}: complete rows, ranges, package IDs and rare facts"
        );
        let expected = expected.view(&sources, &source_by_canonical);
        let actual = actual.view(&sources, &source_by_canonical);
        assert_eq!(actual, expected);
        for source in 0..sources.len() {
            for specifier in specifiers.into_iter().chain(["missing"]) {
                for mode in modes {
                    assert_eq!(
                        actual.module(
                            SourceFileId::from_raw(source as u32),
                            specifier.into(),
                            mode
                        ),
                        expected.module(
                            SourceFileId::from_raw(source as u32),
                            specifier.into(),
                            mode
                        )
                    );
                }
            }
        }
    }
    let empty = ResolutionTable::build(StagedModules::default(), BTreeMap::new(), &sources);
    assert_eq!(
        empty,
        legacy_build(StagedModules::default(), BTreeMap::new(), &sources)
    );
}

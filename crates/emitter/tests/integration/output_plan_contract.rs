use std::path::{Path, PathBuf};

use tsc_emitter::{
    emit_files, emit_files_with_activity, get_source_files_to_emit, preflight_emit,
    validate_bootstrap_emit_options, DeclarationPathResolver, EmitBundle, EmitContractViolation,
    EmitDiagnosticGate, EmitFailure, EmitHost, EmitIoError, EmitIoOperation, EmitMode,
    EmitOutputPaths, EmitOutputPlan, EmitOutputUnit, EmitResolver, EmitResolverError,
    EmitResolverMethod, EmitResolverNode, EmitRoot, EmitSelection, EmitSource,
    EmitWriteDisposition, H2ActivityCanary, JavascriptOmission, MemoryOutputSink, OutputSink,
    PlanDeclarationPaths, UnavailableEmitResolver, UnsupportedEmitFeature,
};
use tsc_program::SourceFileId;
use tsc_syntax::{parse_source_file, SourceFile};
use tsc_types::CompilerOptions;

fn source(raw: u32) -> SourceFileId {
    SourceFileId::from_raw(raw)
}

fn script_unit(raw: u32, paths: EmitOutputPaths) -> EmitOutputUnit {
    EmitOutputUnit::new(EmitRoot::SourceFile(source(raw)), paths, EmitMode::Script)
}

#[test]
fn bootstrap_shape_is_whole_program_source_file_javascript_only() {
    let plan = EmitOutputPlan::whole_program(vec![script_unit(
        3,
        EmitOutputPaths::javascript("/project/out.js"),
    )]);

    assert_eq!(plan.selection(), EmitSelection::WholeProgram);
    assert_eq!(plan.units().len(), 1);
    assert_eq!(plan.units()[0].mode(), EmitMode::Script);
    assert_eq!(
        plan.units()[0].paths().javascript_path(),
        Some(std::path::Path::new("/project/out.js"))
    );
    assert_eq!(plan.validate_bootstrap_shape(), Ok(()));
}

#[test]
fn remove_comments_is_an_active_javascript_emit_option() {
    assert_eq!(
        validate_bootstrap_emit_options(&CompilerOptions {
            target: Some(99),
            module: Some(200),
            remove_comments: Some(true),
            ..CompilerOptions::default()
        }),
        Ok(()),
    );
}

#[test]
fn erasable_syntax_only_is_checker_policy_not_an_emit_preflight_axis() {
    assert_eq!(
        validate_bootstrap_emit_options(&CompilerOptions {
            target: Some(99),
            module: Some(200),
            erasable_syntax_only: Some(true),
            ..CompilerOptions::default()
        }),
        Ok(()),
    );
}

#[test]
fn declaration_is_admitted_while_composite_remains_refused() {
    assert_eq!(
        validate_bootstrap_emit_options(&CompilerOptions {
            target: Some(2),
            module: Some(1),
            isolated_declarations: Some(true),
            ..CompilerOptions::default()
        }),
        Ok(()),
    );

    assert_eq!(
        validate_bootstrap_emit_options(&CompilerOptions {
            target: Some(2),
            module: Some(1),
            isolated_declarations: Some(true),
            declaration: Some(true),
            ..CompilerOptions::default()
        }),
        Ok(()),
    );

    assert_eq!(
        validate_bootstrap_emit_options(&CompilerOptions {
            target: Some(2),
            module: Some(1),
            isolated_declarations: Some(true),
            composite: Some(true),
            ..CompilerOptions::default()
        }),
        Err(EmitFailure::UnsupportedCompilerOption {
            option: "composite"
        }),
    );
}

#[test]
fn every_dormant_axis_is_typed_and_rejected() {
    let javascript = || EmitOutputPaths::javascript("/project/out.js");
    let targeted = EmitOutputPlan::targeted(source(1), vec![script_unit(1, javascript())]);
    assert_eq!(
        targeted.validate_bootstrap_shape(),
        Err(EmitFailure::Unsupported(
            UnsupportedEmitFeature::TargetedSelection
        ))
    );

    let bundle = EmitOutputPlan::whole_program(vec![EmitOutputUnit::new(
        EmitRoot::Bundle(EmitBundle::new(vec![source(1), source(2)])),
        javascript(),
        EmitMode::Script,
    )]);
    assert_eq!(
        bundle.validate_bootstrap_shape(),
        Err(EmitFailure::Unsupported(UnsupportedEmitFeature::BundleRoot))
    );

    for (mode, feature) in [
        (
            EmitMode::DeclarationOnly,
            UnsupportedEmitFeature::DeclarationOnlyMode,
        ),
        (
            EmitMode::BuilderSignature,
            UnsupportedEmitFeature::BuilderSignatureMode,
        ),
        (
            EmitMode::BuildInfoOnly,
            UnsupportedEmitFeature::BuildInfoOnlyMode,
        ),
    ] {
        let plan = EmitOutputPlan::whole_program(vec![EmitOutputUnit::new(
            EmitRoot::SourceFile(source(1)),
            javascript(),
            mode,
        )]);
        assert_eq!(
            plan.validate_bootstrap_shape(),
            Err(EmitFailure::Unsupported(feature))
        );
    }

    let declaration = EmitOutputPlan::whole_program(vec![script_unit(
        1,
        javascript().with_declaration("/project/out.d.ts"),
    )]);
    assert_eq!(declaration.validate_bootstrap_shape(), Ok(()));

    for (paths, feature) in [
        (
            javascript().with_declaration_map("/project/out.d.ts.map"),
            UnsupportedEmitFeature::DeclarationMap,
        ),
        (
            javascript().with_build_info("/project/tsconfig.tsbuildinfo"),
            UnsupportedEmitFeature::BuildInfo,
        ),
    ] {
        let plan = EmitOutputPlan::whole_program(vec![script_unit(1, paths)]);
        assert_eq!(
            plan.validate_bootstrap_shape(),
            Err(EmitFailure::Unsupported(feature))
        );
    }
}

/// h2-6a-m-3 G8: the planned `.js.map` member left the dormant set —
/// a mapped unit passes the bootstrap-shape validation.
#[test]
fn a_planned_javascript_map_member_is_accepted() {
    let plan = EmitOutputPlan::whole_program(vec![script_unit(
        1,
        EmitOutputPaths::javascript("/project/out.js").with_javascript_map("/project/out.js.map"),
    )]);
    assert_eq!(plan.validate_bootstrap_shape(), Ok(()));
}

#[test]
fn malformed_active_slot_is_a_contract_failure() {
    let plan = EmitOutputPlan::whole_program(vec![script_unit(1, EmitOutputPaths::empty())]);
    assert_eq!(
        plan.validate_bootstrap_shape(),
        Err(EmitFailure::Contract(
            EmitContractViolation::ScriptOutputMissingJavaScriptPath
        ))
    );
}

#[test]
fn declaration_only_javascript_absence_requires_typed_plan_provenance() {
    let paths = EmitOutputPaths::empty().with_declaration("/project/out.d.ts");
    let unproven = EmitOutputPlan::whole_program(vec![script_unit(1, paths.clone())]);
    assert_eq!(
        unproven.validate_bootstrap_shape(),
        Err(EmitFailure::Contract(
            EmitContractViolation::ScriptOutputMissingJavaScriptPath
        ))
    );

    let proven = EmitOutputPlan::whole_program(vec![
        script_unit(1, paths).with_javascript_omitted(JavascriptOmission::EmitDeclarationOnly)
    ]);
    assert_eq!(proven.validate_bootstrap_shape(), Ok(()));
    assert_eq!(
        proven.units()[0].javascript_omitted(),
        Some(JavascriptOmission::EmitDeclarationOnly)
    );
}

#[test]
fn new_declaration_resolver_members_fail_closed_when_unavailable() {
    let resolver = UnavailableEmitResolver;
    assert!(matches!(
        resolver.has_global_name("GlobalName"),
        Err(EmitResolverError::UnavailableForName {
            method: EmitResolverMethod::HasGlobalName,
            ..
        })
    ));
    assert!(matches!(
        resolver.collect_linked_aliases(
            EmitResolverNode::new(source(7), tsc_syntax::NodeId(3)),
            true,
        ),
        Err(EmitResolverError::Unavailable {
            method: EmitResolverMethod::CollectLinkedAliases,
            ..
        })
    ));
    assert!(matches!(
        resolver.can_include_bind_and_check_diagnostics(source(7)),
        Err(EmitResolverError::UnavailableForSource {
            method: EmitResolverMethod::CanIncludeBindAndCheckDiagnostics,
            ..
        })
    ));
}

struct HostSource {
    path: PathBuf,
    canonical: PathBuf,
    may_be_emitted: bool,
    syntax: SourceFile,
}

struct TestEmitHost {
    options: CompilerOptions,
    current_directory: PathBuf,
    common_source_directory: PathBuf,
    config_file_path: Option<PathBuf>,
    case_sensitive: bool,
    ids: Vec<SourceFileId>,
    sources: Vec<HostSource>,
}

impl TestEmitHost {
    fn new(
        options: CompilerOptions,
        common_source_directory: &str,
        case_sensitive: bool,
        sources: &[(&str, bool)],
    ) -> Self {
        let sources = sources
            .iter()
            .map(|(path, may_be_emitted)| HostSource {
                path: PathBuf::from(path),
                canonical: PathBuf::from(if case_sensitive {
                    (*path).to_owned()
                } else {
                    path.to_lowercase()
                }),
                may_be_emitted: *may_be_emitted,
                syntax: parse_source_file(
                    *path,
                    "export const value = 1;\n",
                    Default::default(),
                    None,
                ),
            })
            .collect::<Vec<_>>();
        Self {
            options,
            current_directory: PathBuf::from("/project"),
            common_source_directory: PathBuf::from(common_source_directory),
            config_file_path: None,
            case_sensitive,
            ids: (0..sources.len())
                .map(|index| SourceFileId::from_raw(index as u32))
                .collect(),
            sources,
        }
    }

    fn with_config_file_path(mut self, path: &str) -> Self {
        self.config_file_path = Some(PathBuf::from(path));
        self
    }
}

impl EmitHost for TestEmitHost {
    fn compiler_options(&self) -> &CompilerOptions {
        &self.options
    }

    fn current_directory(&self) -> &Path {
        &self.current_directory
    }

    fn common_source_directory(&self) -> &Path {
        &self.common_source_directory
    }

    fn config_file_path(&self) -> Option<&Path> {
        self.config_file_path.as_deref()
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.case_sensitive
    }

    fn source_file_ids(&self) -> &[SourceFileId] {
        &self.ids
    }

    fn source_file(&self, id: SourceFileId) -> Option<EmitSource<'_>> {
        let source = self.sources.get(id.index())?;
        Some(EmitSource::new(
            id,
            &source.path,
            &source.canonical,
            source.may_be_emitted,
            None,
            Some(&source.syntax),
        ))
    }
}

#[test]
fn executable_planning_preserves_source_order_eligibility_and_out_dir_layout() {
    let host = TestEmitHost::new(
        CompilerOptions {
            out_dir: Some("/project/dist".to_owned()),
            ..CompilerOptions::default()
        },
        "/project/src",
        true,
        &[
            ("/project/src/zeta.ts", true),
            ("/project/src/types.d.ts", true),
            ("/project/src/external.ts", false),
            ("/project/src/alpha.ts", true),
        ],
    );
    assert_eq!(
        get_source_files_to_emit(&host, EmitSelection::WholeProgram).unwrap(),
        vec![source(0), source(3)]
    );
    let preflight = preflight_emit(&host, EmitSelection::WholeProgram).unwrap();
    let paths = preflight
        .plan()
        .units()
        .iter()
        .map(|unit| unit.paths().javascript_path().unwrap().to_path_buf())
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        [
            PathBuf::from("/project/dist/zeta.js"),
            PathBuf::from("/project/dist/alpha.js"),
        ]
    );
    assert!(preflight.diagnostics().is_empty());
}

#[test]
fn computed_resolve_json_module_is_validated_against_module_kind() {
    let host = TestEmitHost::new(
        CompilerOptions {
            module: Some(4),
            module_resolution: Some(100),
            ..CompilerOptions::default()
        },
        "/project",
        true,
        &[("/project/index.ts", true)],
    );

    let preflight = preflight_emit(&host, EmitSelection::WholeProgram).unwrap();
    assert_eq!(
        preflight
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [5071]
    );
}

#[test]
fn case_insensitive_planning_preserves_callback_visible_source_spelling() {
    let host = TestEmitHost::new(
        CompilerOptions {
            out_dir: Some("/project/dist".to_owned()),
            ..CompilerOptions::default()
        },
        "/project/src",
        false,
        &[("/Project/SRC/MixedCase.ts", true)],
    );

    let preflight = preflight_emit(&host, EmitSelection::WholeProgram).unwrap();
    assert_eq!(
        preflight.plan().units()[0].paths().javascript_path(),
        Some(Path::new("/project/dist/MixedCase.js"))
    );
}

#[test]
fn h2_3a_javascript_families_keep_their_runtime_extensions_when_relocated() {
    let host = TestEmitHost::new(
        CompilerOptions {
            allow_js: true,
            out_dir: Some("/project/dist".to_owned()),
            ..CompilerOptions::default()
        },
        "/project/src",
        true,
        &[
            ("/project/src/plain.js", true),
            ("/project/src/module.mjs", true),
            ("/project/src/common.cjs", true),
        ],
    );

    let preflight = preflight_emit(&host, EmitSelection::WholeProgram).unwrap();
    assert_eq!(
        preflight
            .plan()
            .units()
            .iter()
            .map(|unit| unit.paths().javascript_path().unwrap().to_path_buf())
            .collect::<Vec<_>>(),
        [
            PathBuf::from("/project/dist/plain.js"),
            PathBuf::from("/project/dist/module.mjs"),
            PathBuf::from("/project/dist/common.cjs"),
        ]
    );
    assert!(preflight.diagnostics().is_empty());
}

#[test]
fn plan_declaration_paths_prefers_declaration_then_javascript_then_source() {
    let host = TestEmitHost::new(
        CompilerOptions {
            declaration: Some(true),
            out_dir: Some("/project/dist".to_owned()),
            ..CompilerOptions::default()
        },
        "/project/src",
        true,
        &[
            ("/project/src/code.ts", true),
            ("/project/src/data.json", true),
            ("/project/src/not-emitted.ts", false),
        ],
    );
    let preflight = preflight_emit(&host, EmitSelection::WholeProgram).unwrap();
    let paths = PlanDeclarationPaths::new(&host, &preflight);

    assert_eq!(
        paths.declaration_file_path(source(0)),
        Some(PathBuf::from("/project/dist/code.d.ts"))
    );
    assert_eq!(
        paths.reference_target_path(source(0)),
        Some(PathBuf::from("/project/dist/code.d.ts"))
    );
    assert_eq!(
        paths.reference_target_path(source(1)),
        Some(PathBuf::from("/project/dist/data.json"))
    );
    assert_eq!(
        paths.reference_target_path(source(2)),
        Some(PathBuf::from("/project/src/not-emitted.ts"))
    );
}

#[test]
fn declaration_collision_preflight_covers_overwrite_duplicate_case_and_js_suppression() {
    let overwrite = TestEmitHost::new(
        CompilerOptions {
            declaration: Some(true),
            ..CompilerOptions::default()
        },
        "/project",
        true,
        &[("/project/value.ts", true), ("/project/value.d.ts", false)],
    );
    let preflight = preflight_emit(&overwrite, EmitSelection::WholeProgram).unwrap();
    assert_eq!(
        preflight
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [5055]
    );
    assert!(!preflight.is_emit_blocked(&overwrite, Path::new("/project/value.js")));
    assert!(preflight.is_emit_blocked(&overwrite, Path::new("/project/value.d.ts")));

    let canonical_case = TestEmitHost::new(
        CompilerOptions {
            declaration: Some(true),
            ..CompilerOptions::default()
        },
        "/project",
        false,
        &[("/project/value.ts", true), ("/PROJECT/VALUE.D.TS", false)],
    );
    let preflight = preflight_emit(&canonical_case, EmitSelection::WholeProgram).unwrap();
    assert_eq!(preflight.diagnostics()[0].code(), 5055);
    assert!(preflight.is_emit_blocked(&canonical_case, Path::new("/PROJECT/Value.D.TS")));

    let duplicate = TestEmitHost::new(
        CompilerOptions {
            allow_js: true,
            declaration: Some(true),
            emit_declaration_only: Some(true),
            ..CompilerOptions::default()
        },
        "/project",
        false,
        &[("/project/Value.ts", true), ("/project/value.ts", true)],
    );
    let preflight = preflight_emit(&duplicate, EmitSelection::WholeProgram).unwrap();
    assert_eq!(
        preflight
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [5056]
    );
    assert!(preflight.is_emit_blocked(&duplicate, Path::new("/project/value.d.ts")));

    let declaration_only = TestEmitHost::new(
        CompilerOptions {
            declaration: Some(true),
            emit_declaration_only: Some(true),
            ..CompilerOptions::default()
        },
        "/project",
        true,
        &[("/project/value.ts", true), ("/project/value.js", false)],
    );
    let preflight = preflight_emit(&declaration_only, EmitSelection::WholeProgram).unwrap();
    assert!(preflight.diagnostics().is_empty());
    assert_eq!(
        preflight.plan().units()[0].javascript_omitted(),
        Some(JavascriptOmission::EmitDeclarationOnly)
    );
    assert_eq!(preflight.plan().validate_bootstrap_shape(), Ok(()));

    let no_emit = TestEmitHost::new(
        CompilerOptions {
            declaration: Some(true),
            no_emit: Some(true),
            ..CompilerOptions::default()
        },
        "/project",
        true,
        &[("/project/value.ts", true), ("/project/value.d.ts", false)],
    );
    assert!(preflight_emit(&no_emit, EmitSelection::WholeProgram)
        .unwrap()
        .diagnostics()
        .is_empty());
}

#[test]
fn refused_option_sets_leave_every_activity_counter_and_sink_write_at_zero() {
    for (options, expected) in [
        (
            CompilerOptions {
                declaration_map: Some(true),
                ..CompilerOptions::default()
            },
            "declarationMap",
        ),
        (
            CompilerOptions {
                out_file: Some("/project/bundle.js".to_owned()),
                ..CompilerOptions::default()
            },
            "outFile",
        ),
        (
            CompilerOptions {
                emit_declaration_only: Some(true),
                ..CompilerOptions::default()
            },
            "emitDeclarationOnly",
        ),
    ] {
        let host = TestEmitHost::new(options, "/project", true, &[("/project/value.ts", true)]);
        let preflight = preflight_emit(&host, EmitSelection::WholeProgram).unwrap();
        let mut activity = H2ActivityCanary::h2_7b_profile();
        let mut sink = MemoryOutputSink::new();
        assert_eq!(
            emit_files_with_activity(
                &UnavailableEmitResolver,
                &host,
                preflight,
                EmitSelection::WholeProgram,
                &EmitDiagnosticGate::default(),
                &mut sink,
                &mut activity,
            ),
            Err(EmitFailure::UnsupportedCompilerOption { option: expected })
        );
        assert!(activity.counters().all_zero(), "{expected}: activity");
        assert!(sink.writes().is_empty(), "{expected}: sink writes");
    }
}

#[test]
fn overwrite_and_case_aware_duplicate_outputs_are_blocked_before_writes() {
    let overwrite = TestEmitHost::new(
        CompilerOptions::default(),
        "/project",
        true,
        &[("/project/value.ts", true), ("/project/value.js", false)],
    );
    let preflight = preflight_emit(&overwrite, EmitSelection::WholeProgram).unwrap();
    assert_eq!(
        preflight
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [5055]
    );
    assert!(preflight.diagnostics()[0].message.next_present);
    assert_eq!(preflight.diagnostics()[0].message.next[0].code, 5068);
    assert!(preflight.is_emit_blocked(&overwrite, Path::new("/project/value.js")));

    let configured = TestEmitHost::new(
        CompilerOptions::default(),
        "/project",
        true,
        &[("/project/value.ts", true), ("/project/value.js", false)],
    )
    .with_config_file_path("/project/tsconfig.json");
    let preflight = preflight_emit(&configured, EmitSelection::WholeProgram).unwrap();
    assert!(!preflight.diagnostics()[0].message.next_present);
    assert!(preflight.diagnostics()[0].message.next.is_empty());

    let duplicate = TestEmitHost::new(
        CompilerOptions::default(),
        "/project",
        false,
        &[("/project/Value.ts", true), ("/project/value.ts", true)],
    );
    let preflight = preflight_emit(&duplicate, EmitSelection::WholeProgram).unwrap();
    assert_eq!(
        preflight
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [5056]
    );
    assert!(preflight.is_emit_blocked(&duplicate, Path::new("/project/value.js")));
}

#[test]
fn duplicate_output_preflight_reaches_no_sink_and_obeys_no_emit_on_error() {
    let options = CompilerOptions {
        target: Some(99),
        module: Some(200),
        list_emitted_files: Some(true),
        ..CompilerOptions::default()
    };
    let host = TestEmitHost::new(
        options.clone(),
        "/project",
        false,
        &[("/project/Value.ts", true), ("/project/value.ts", true)],
    );
    let preflight = preflight_emit(&host, EmitSelection::WholeProgram).unwrap();
    let mut sink = MemoryOutputSink::new();
    let outcome = emit_files(
        &UnavailableEmitResolver,
        &host,
        preflight,
        EmitSelection::WholeProgram,
        &EmitDiagnosticGate::default(),
        &mut sink,
    )
    .unwrap();
    assert!(outcome.emit_skipped());
    assert!(outcome.diagnostics().is_empty());
    assert_eq!(outcome.emitted_files(), Some([].as_slice()));
    assert!(sink.writes().is_empty());

    let host = TestEmitHost::new(
        CompilerOptions {
            no_emit_on_error: Some(true),
            ..options
        },
        "/project",
        false,
        &[("/project/Value.ts", true), ("/project/value.ts", true)],
    );
    let preflight = preflight_emit(&host, EmitSelection::WholeProgram).unwrap();
    let mut sink = MemoryOutputSink::new();
    let outcome = emit_files(
        &UnavailableEmitResolver,
        &host,
        preflight,
        EmitSelection::WholeProgram,
        &EmitDiagnosticGate::default(),
        &mut sink,
    )
    .unwrap();
    assert!(outcome.emit_skipped());
    assert_eq!(
        outcome
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [5056]
    );
    assert_eq!(outcome.emitted_files(), Some([].as_slice()));
    assert!(sink.writes().is_empty());
}

#[derive(Default)]
struct ObservedSink {
    paths: Vec<PathBuf>,
    fail_index: Option<usize>,
    skip_index: Option<usize>,
}

impl OutputSink for ObservedSink {
    fn write(
        &mut self,
        artifact: tsc_emitter::EmitArtifact,
    ) -> Result<EmitWriteDisposition, EmitIoError> {
        let index = self.paths.len();
        self.paths.push(artifact.path().to_path_buf());
        if self.fail_index == Some(index) {
            return Err(EmitIoError::new(
                EmitIoOperation::WriteFile,
                artifact.path(),
                "injected failure",
            ));
        }
        if self.skip_index == Some(index) {
            return Ok(EmitWriteDisposition::SkippedUnchanged);
        }
        Ok(EmitWriteDisposition::Written)
    }
}

#[test]
fn sink_errors_continue_and_emitted_files_remain_independent_from_disposition() {
    let host = TestEmitHost::new(
        CompilerOptions {
            target: Some(99),
            module: Some(200),
            list_emitted_files: Some(true),
            ..CompilerOptions::default()
        },
        "/project",
        true,
        &[("/project/first.ts", true), ("/project/second.ts", true)],
    );

    let preflight = preflight_emit(&host, EmitSelection::WholeProgram).unwrap();
    let mut failing = ObservedSink {
        fail_index: Some(0),
        ..ObservedSink::default()
    };
    let outcome = emit_files(
        &UnavailableEmitResolver,
        &host,
        preflight,
        EmitSelection::WholeProgram,
        &EmitDiagnosticGate::default(),
        &mut failing,
    )
    .unwrap();
    assert_eq!(
        failing.paths,
        [
            PathBuf::from("/project/first.js"),
            PathBuf::from("/project/second.js"),
        ]
    );
    assert_eq!(
        outcome
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [5033]
    );
    assert!(!outcome.emit_skipped());
    assert_eq!(
        outcome.emitted_files(),
        Some(
            [
                PathBuf::from("/project/first.js"),
                PathBuf::from("/project/second.js"),
            ]
            .as_slice()
        )
    );

    let preflight = preflight_emit(&host, EmitSelection::WholeProgram).unwrap();
    let mut skipping = ObservedSink {
        skip_index: Some(0),
        ..ObservedSink::default()
    };
    let outcome = emit_files(
        &UnavailableEmitResolver,
        &host,
        preflight,
        EmitSelection::WholeProgram,
        &EmitDiagnosticGate::default(),
        &mut skipping,
    )
    .unwrap();
    assert_eq!(skipping.paths, failing.paths);
    assert!(outcome.diagnostics().is_empty());
    assert_eq!(
        outcome.emitted_files(),
        Some([PathBuf::from("/project/second.js")].as_slice())
    );
}

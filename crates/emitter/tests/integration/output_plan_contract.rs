use std::path::{Path, PathBuf};

use tsc_emitter::{
    emit_files, get_source_files_to_emit, preflight_emit, EmitBundle, EmitContractViolation,
    EmitDiagnosticGate, EmitFailure, EmitHost, EmitIoError, EmitIoOperation, EmitMode,
    EmitOutputPaths, EmitOutputPlan, EmitOutputUnit, EmitRoot, EmitSelection, EmitSource,
    EmitWriteDisposition, MemoryOutputSink, OutputSink, UnavailableEmitResolver,
    UnsupportedEmitFeature,
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

    for (paths, feature) in [
        (
            javascript().with_javascript_map("/project/out.js.map"),
            UnsupportedEmitFeature::JavaScriptMap,
        ),
        (
            javascript().with_declaration("/project/out.d.ts"),
            UnsupportedEmitFeature::Declaration,
        ),
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

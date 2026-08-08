use tsc_diagnostics::{gen, sort_and_dedupe_diagnostics, Diagnostic, DiagnosticList, MessageChain};
use tsc_types::{CompilerOptions, ScriptTarget};

use crate::builtins::get_script_transformers_with_activity;
use crate::{
    create_printer, transform_nodes, DisabledSourceMapRecorder, EmitArtifact,
    EmitContractViolation, EmitFailure, EmitHost, EmitOutcome, EmitPreflight, EmitResolver,
    EmitRoot, EmitSelection, EmitTextMetadata, EmitWriteDisposition, H2ActivityCanary, NewLineKind,
    OutputSink, PrintRequest, PrinterOptions, TransformArena, TransformRoot,
};

const MODULE_COMMON_JS: i32 = 1;
const MODULE_ES_NEXT: i32 = 99;
const MODULE_PRESERVE: i32 = 200;

/// The four public diagnostic getter streams consumed by
/// `handleNoEmitOptions`, kept separate so output-preflight diagnostics can
/// join the options bucket without disturbing cross-bucket order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EmitDiagnosticGate {
    options: DiagnosticList,
    syntactic: DiagnosticList,
    global: DiagnosticList,
    semantic: DiagnosticList,
}

impl EmitDiagnosticGate {
    pub fn new(
        options: DiagnosticList,
        syntactic: DiagnosticList,
        global: DiagnosticList,
        semantic: DiagnosticList,
    ) -> Self {
        Self {
            options,
            syntactic,
            global,
            semantic,
        }
    }

    fn collect_with_preflight(&self, preflight: &[Diagnostic]) -> DiagnosticList {
        let mut options = self.options.clone();
        options.extend_from_slice(preflight);
        sort_and_dedupe_diagnostics(&mut options);
        let capacity =
            options.len() + self.syntactic.len() + self.global.len() + self.semantic.len();
        let mut diagnostics = Vec::with_capacity(capacity);
        diagnostics.extend(options);
        diagnostics.extend(self.syntactic.iter().cloned());
        diagnostics.extend(self.global.iter().cloned());
        diagnostics.extend(self.semantic.iter().cloned());
        diagnostics
    }
}

/// Reject every effective option outside the frozen JavaScript-only bootstrap
/// before output planning, checker-to-emitter borrowing, or sink dispatch.
pub fn validate_bootstrap_emit_options(options: &CompilerOptions) -> Result<(), EmitFailure> {
    if options.emit_script_target() != ScriptTarget::ES_NEXT {
        return unsupported("target");
    }
    if !matches!(
        options.emit_module_kind(),
        MODULE_PRESERVE | MODULE_ES_NEXT | MODULE_COMMON_JS
    ) {
        return unsupported("module");
    }
    if options.use_define_for_class_fields == Some(false) {
        return unsupported("useDefineForClassFields");
    }
    if !matches!(options.new_line, None | Some(0 | 1)) {
        return unsupported("newLine");
    }

    for (active, name) in [
        (options.no_emit == Some(true), "noEmit"),
        (options.allow_js, "allowJs"),
        (options.experimental_decorators, "experimentalDecorators"),
        (options.import_helpers == Some(true), "importHelpers"),
        (options.no_emit_helpers == Some(true), "noEmitHelpers"),
        (options.no_check == Some(true), "noCheck"),
        (
            options.erasable_syntax_only == Some(true),
            "erasableSyntaxOnly",
        ),
        (options.isolated_modules == Some(true), "isolatedModules"),
        (
            options.verbatim_module_syntax == Some(true),
            "verbatimModuleSyntax",
        ),
        (
            options.allow_importing_ts_extensions == Some(true),
            "allowImportingTsExtensions",
        ),
        (
            options.rewrite_relative_import_extensions == Some(true),
            "rewriteRelativeImportExtensions",
        ),
        (
            options.resolve_json_module == Some(true),
            "resolveJsonModule",
        ),
        (options.remove_comments == Some(true), "removeComments"),
        (
            options.no_implicit_use_strict == Some(true),
            "noImplicitUseStrict",
        ),
        (options.source_map == Some(true), "sourceMap"),
        (options.inline_source_map == Some(true), "inlineSourceMap"),
        (options.inline_sources == Some(true), "inlineSources"),
        (options.declaration == Some(true), "declaration"),
        (options.declaration_map == Some(true), "declarationMap"),
        (
            options.emit_declaration_only == Some(true),
            "emitDeclarationOnly",
        ),
        (
            options.isolated_declarations == Some(true),
            "isolatedDeclarations",
        ),
        (
            options.stable_type_ordering == Some(true),
            "stableTypeOrdering",
        ),
        (options.strip_internal == Some(true), "stripInternal"),
        (options.incremental == Some(true), "incremental"),
        (options.composite == Some(true), "composite"),
        (
            options.assume_changes_only_affect_direct_dependencies == Some(true),
            "assumeChangesOnlyAffectDirectDependencies",
        ),
        (
            options.preserve_value_imports == Some(true),
            "preserveValueImports",
        ),
        (
            options.emit_decorator_metadata == Some(true),
            "emitDecoratorMetadata",
        ),
    ] {
        if active {
            return unsupported(name);
        }
    }
    for (present, name) in [
        (options.jsx.is_some(), "jsx"),
        (options.out_dir.is_some(), "outDir"),
        (options.root_dir.is_some(), "rootDir"),
        (options.source_root.is_some(), "sourceRoot"),
        (options.map_root.is_some(), "mapRoot"),
        (options.declaration_dir.is_some(), "declarationDir"),
        (options.out_file.is_some(), "outFile"),
        (options.out.is_some(), "out"),
        (options.ts_build_info_file.is_some(), "tsBuildInfoFile"),
        (
            options.imports_not_used_as_values.is_some(),
            "importsNotUsedAsValues",
        ),
    ] {
        if present {
            return unsupported(name);
        }
    }
    Ok(())
}

/// Validate the option profile and the admitted `.ts` source family before
/// the checker constructs an emit resolver.
pub fn validate_bootstrap_emit_request(host: &dyn EmitHost) -> Result<(), EmitFailure> {
    validate_bootstrap_emit_options(host.compiler_options())?;
    for source_id in host.source_file_ids() {
        let source = host.source_file(*source_id).ok_or(EmitFailure::Contract(
            EmitContractViolation::PlannedSourceMissing(*source_id),
        ))?;
        if !crate::source_file_may_be_emitted(source) {
            continue;
        }
        let name = source.path().to_string_lossy().to_ascii_lowercase();
        if !name.ends_with(".ts")
            || name.ends_with(".d.ts")
            || name.ends_with(".d.mts")
            || name.ends_with(".d.cts")
        {
            return Err(EmitFailure::UnsupportedSourceExtension {
                path: source.path().to_path_buf(),
            });
        }
    }
    Ok(())
}

fn unsupported<T>(option: &'static str) -> Result<T, EmitFailure> {
    Err(EmitFailure::UnsupportedCompilerOption { option })
}

/// tsc-port: emitFiles @6.0.3
/// tsc-hash: 62e93c3a8e9e2840b759bbaa0fa6de5e548ebd565748dbbddb47a933a1cf442c
/// tsc-span: _tsc.js:116530-116858
///
/// The profile-only preconstruction of every JavaScript artifact is a
/// fail-closed Rust ownership adaptation: an unsupported later source cannot
/// leave earlier callback writes behind. Once all artifacts exist, callback
/// order follows the ported output-unit order exactly.
pub fn emit_files(
    resolver: &dyn EmitResolver,
    host: &dyn EmitHost,
    preflight: EmitPreflight,
    selection: EmitSelection,
    diagnostic_gate: &EmitDiagnosticGate,
    sink: &mut dyn OutputSink,
) -> Result<EmitOutcome, EmitFailure> {
    let mut activity = H2ActivityCanary::h2_1a_profile();
    activity.construct_emit_session();
    activity.construct_output_plan();
    if !preflight.plan().units().is_empty() {
        activity.borrow_emit_resolver();
    }
    emit_files_with_activity(
        resolver,
        host,
        preflight,
        selection,
        diagnostic_gate,
        sink,
        &mut activity,
    )
}

/// Compiler-owned entry which carries one observer from request construction
/// through callback completion.
#[doc(hidden)]
pub fn emit_files_with_activity(
    resolver: &dyn EmitResolver,
    host: &dyn EmitHost,
    preflight: EmitPreflight,
    selection: EmitSelection,
    diagnostic_gate: &EmitDiagnosticGate,
    sink: &mut dyn OutputSink,
    activity: &mut H2ActivityCanary,
) -> Result<EmitOutcome, EmitFailure> {
    validate_bootstrap_emit_request(host)?;
    preflight.plan().validate_bootstrap_shape()?;
    if preflight.plan().selection() != selection {
        return Err(EmitFailure::Unsupported(
            crate::UnsupportedEmitFeature::TargetedSelection,
        ));
    }

    let options = host.compiler_options();
    let emitted_files_enabled = options.list_emitted_files == Some(true);
    if options.no_emit_on_error == Some(true) {
        let diagnostics = diagnostic_gate.collect_with_preflight(preflight.diagnostics());
        if !diagnostics.is_empty() {
            return Ok(EmitOutcome::new(
                diagnostics,
                true,
                emitted_files_enabled.then(Vec::new),
                None,
                activity.counters(),
            ));
        }
    }

    let new_line = match options.new_line {
        Some(0) => NewLineKind::CarriageReturnLineFeed,
        None | Some(1) => NewLineKind::LineFeed,
        Some(_) => return unsupported("newLine"),
    };
    activity.construct_printer();
    let printer = create_printer(
        PrinterOptions::new(new_line)
            .with_remove_comments(options.remove_comments == Some(true))
            .with_no_implicit_use_strict(options.no_implicit_use_strict == Some(true))
            .with_no_emit_helpers(options.no_emit_helpers == Some(true)),
    );

    let mut artifacts = Vec::with_capacity(preflight.plan().units().len());
    let mut emit_skipped = false;
    for unit in preflight.plan().units() {
        let EmitRoot::SourceFile(source_id) = unit.root() else {
            return Err(EmitFailure::Unsupported(
                crate::UnsupportedEmitFeature::BundleRoot,
            ));
        };
        let javascript_path = unit.paths().javascript_path().ok_or(EmitFailure::Contract(
            EmitContractViolation::ScriptOutputMissingJavaScriptPath,
        ))?;
        if preflight.is_emit_blocked(host, javascript_path) {
            emit_skipped = true;
            continue;
        }
        let source = host.source_file(*source_id).ok_or(EmitFailure::Contract(
            EmitContractViolation::PlannedSourceMissing(*source_id),
        ))?;
        let syntax = source.syntax().ok_or(EmitFailure::Contract(
            EmitContractViolation::CheckedSyntaxUnavailable(*source_id),
        ))?;

        let mut arena = TransformArena::new();
        let transform_source = arena.add_source(syntax, Some(*source_id));
        let transformers =
            get_script_transformers_with_activity(options, resolver, host, activity)?;
        activity.construct_transform_context();
        let mut transformation = transform_nodes(
            arena,
            vec![TransformRoot::SourceFile(transform_source)],
            transformers,
            false,
        )?;
        let transform_diagnostics = transformation.diagnostics().to_vec();
        let printed = printer.print(
            &mut transformation,
            PrintRequest::SourceFile(transform_source),
            &mut DisabledSourceMapRecorder,
        )?;
        activity.create_javascript_artifact();
        artifacts.push(EmitArtifact::javascript(
            javascript_path,
            printed.text(),
            options.emit_bom == Some(true),
            Some(vec![source.path().to_path_buf()]),
            EmitTextMetadata::new(transform_diagnostics, None),
        ));
    }

    let mut diagnostics: DiagnosticList = Vec::new();
    let mut emitted_files = emitted_files_enabled.then(Vec::new);
    for artifact in artifacts {
        let path = artifact.path().to_path_buf();
        activity.attempt_output_sink_write();
        let include_in_emitted_files = match sink.write(artifact) {
            Ok(EmitWriteDisposition::Written) => true,
            Ok(EmitWriteDisposition::SkippedUnchanged) => false,
            Err(error) => {
                activity.observe_output_sink_failure();
                diagnostics.push(write_diagnostic(&path, error.message()));
                // TypeScript records the attempted output after the host's
                // error callback returns; a callback error is not an
                // unchanged-write suppression.
                true
            }
        };
        if include_in_emitted_files {
            let Some(emitted_files) = emitted_files.as_mut() else {
                continue;
            };
            emitted_files.push(path);
        }
    }

    Ok(EmitOutcome::new(
        diagnostics,
        emit_skipped,
        emitted_files,
        None,
        activity.counters(),
    ))
}

fn write_diagnostic(path: &std::path::Path, message: &str) -> Diagnostic {
    Diagnostic::new(
        None,
        None,
        None,
        MessageChain::new(
            &gen::Could_not_write_file_0_1,
            &[path.to_string_lossy().into_owned(), message.to_owned()],
        ),
    )
}

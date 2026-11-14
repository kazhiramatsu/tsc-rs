use tsc_diagnostics::JsStr;

use tsc_diagnostics::Diagnostic;
use tsc_program::SourceFileId;
use tsc_syntax::{for_each_child, NodeData, NodeId, SyntaxKind};

use crate::{
    create_printer, transform_nodes, EmitArtifact, EmitContractViolation, EmitFailure, EmitHost,
    EmitPreflight, EmitResolver, EmitResolverNode, EmitTextMetadata, NewLineKind, PrinterOptions,
    SourceFileTextMode, TransformArena, TransformError, TransformRoot, TransformationResult,
};

use super::{get_declaration_transformers, DeclarationCustomTransformers, DeclarationPathResolver};

/// Mount the declaration unit's own source. Any other program source a
/// resolver answer reaches is mounted on first use
/// (`TransformArena::mount_parse_tree_transform_node`), so the transform of
/// one file never copies the syntax of every file.
fn mount_declaration_program_sources(
    arena: &mut TransformArena,
    host: &dyn EmitHost,
    source: SourceFileId,
) -> Result<crate::TransformSourceId, EmitFailure> {
    let emit_source = host.source_file(source).ok_or(EmitFailure::Contract(
        EmitContractViolation::PlannedSourceMissing(source),
    ))?;
    let syntax = emit_source.syntax().ok_or(EmitFailure::Contract(
        EmitContractViolation::CheckedSyntaxUnavailable(source),
    ))?;
    Ok(arena.add_source(syntax, Some(source)))
}

/// The five upstream inputs recorded at the declaration-blocking boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclBlockedInputs {
    pub diagnostics_len: usize,
    pub is_emit_blocked_evaluated: bool,
    pub is_emit_blocked: bool,
    pub no_emit: Option<bool>,
    pub decl_blocked: bool,
}

/// Harness result for one dormant declaration transform window.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclarationTransformOutcome {
    pub root: TransformRoot,
    pub diagnostics: Vec<Diagnostic>,
    pub decl_blocked: bool,
    pub decl_blocked_inputs: DeclBlockedInputs,
}

/// Production result for one independently executed declaration member.
pub(crate) struct DeclarationUnitEmit {
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) decl_blocked: bool,
    pub(crate) artifact: Option<EmitArtifact>,
    pub(crate) map_artifact: Option<EmitArtifact>,
    pub(crate) map_observation: Option<crate::SourceMapObservation>,
}

pub(crate) enum DeclarationWork {
    Complete(Result<DeclarationUnitEmit, EmitFailure>),
    Pending(crate::print_pool::PendingPrint<Result<DeclarationUnitEmit, EmitFailure>>),
}
impl From<DeclarationUnitEmit> for DeclarationWork {
    fn from(value: DeclarationUnitEmit) -> Self {
        Self::Complete(Ok(value))
    }
}
impl DeclarationWork {
    pub(crate) fn is_ready(&mut self) -> bool {
        match self {
            Self::Complete(_) => true,
            Self::Pending(work) => work.is_ready(),
        }
    }
    pub(crate) fn is_pending(&self) -> bool {
        matches!(self, Self::Pending(_))
    }
    pub(crate) fn has_known_failure(&self) -> bool {
        matches!(self, Self::Complete(Err(_)))
    }
    pub(crate) fn finish(self) -> Result<DeclarationUnitEmit, EmitFailure> {
        match self {
            Self::Complete(result) => result,
            Self::Pending(pending) => pending.finish().0,
        }
    }
}

enum DeclarationMapOptions<'a> {
    Borrowed(&'a tsc_types::CompilerOptions),
    Owned(Box<tsc_types::CompilerOptions>),
}
impl DeclarationMapOptions<'_> {
    fn as_options(&self) -> &tsc_types::CompilerOptions {
        match self {
            Self::Borrowed(options) => options,
            Self::Owned(options) => options,
        }
    }
    fn into_owned(self) -> DeclarationMapOptions<'static> {
        let options = self.as_options();
        DeclarationMapOptions::Owned(Box::new(tsc_types::CompilerOptions {
            declaration_map: options.declaration_map,
            source_root: options.source_root.clone(),
            map_root: options.map_root.clone(),
            emit_bom: options.emit_bom,
            ..Default::default()
        }))
    }
}
struct DeclarationPrintInputs<'a> {
    path: crate::execute::OutputPath<'a>,
    map_path: Option<crate::execute::OutputPath<'a>>,
    source_files: Vec<tsc_types::JsString>,
    diagnostics: Vec<Diagnostic>,
    decl_blocked: bool,
    new_line: NewLineKind,
    emit_bom: bool,
    map_lane: Option<crate::MapLaneInputs>,
    map_options: Option<DeclarationMapOptions<'a>>,
    recording: Option<crate::SourceMapRecordingInputs>,
    span: Option<tsc_types::tracing::Span>,
    worker_trace: Option<std::sync::Arc<tsc_types::tracing::Tracing>>,
}
impl DeclarationPrintInputs<'_> {
    fn into_owned(self) -> DeclarationPrintInputs<'static> {
        DeclarationPrintInputs {
            path: self.path.into_owned(),
            map_path: self.map_path.map(crate::execute::OutputPath::into_owned),
            source_files: self.source_files,
            diagnostics: self.diagnostics,
            decl_blocked: self.decl_blocked,
            new_line: self.new_line,
            emit_bom: self.emit_bom,
            map_lane: self.map_lane,
            map_options: self.map_options.map(DeclarationMapOptions::into_owned),
            recording: self.recording,
            span: self.span,
            worker_trace: self.worker_trace,
        }
    }
    fn input_charge(&self) -> Option<usize> {
        if self
            .recording
            .as_ref()
            .is_some_and(|v| v.original_positions.is_some())
        {
            return None;
        }
        let mut usage = tsc_diagnostics::MemoryUsage::default();
        usage.add(
            "declaration paths",
            1,
            0,
            self.path.as_js().as_bytes().len() * 2,
        );
        if let Some(path) = &self.map_path {
            usage.add("declaration paths", 1, 0, path.as_js().as_bytes().len() * 2);
        }
        usage.vec("declaration sources", &self.source_files);
        for path in &self.source_files {
            usage.js_string("declaration sources", path);
        }
        usage.diagnostics(&self.diagnostics);
        if let Some(lane) = &self.map_lane {
            usage.js_string("declaration map paths", &lane.common_source_directory);
            usage.js_string("declaration map paths", &lane.current_directory);
        }
        if let Some(options) = &self.map_options {
            usage.add(
                "declaration map options",
                1,
                0,
                std::mem::size_of::<tsc_types::CompilerOptions>(),
            );
            for path in [
                &options.as_options().source_root,
                &options.as_options().map_root,
            ]
            .into_iter()
            .flatten()
            {
                usage.js_string("declaration map paths", path);
            }
        }
        if let Some(recording) = &self.recording {
            for path in [
                &recording.file,
                &recording.source_root,
                &recording.sources_directory_path,
                &recording.current_directory,
            ] {
                usage.js_string("declaration recording paths", path);
            }
        }
        usage.total().requested.checked_mul(2)?.checked_add(4096)
    }
}

/// Run the declaration transform solely for diagnostics, without constructing
/// a printer or applying the per-output path blocking rules.
///
/// The source is filtered by getSourceFilesToEmit and the JSON exclusion.
/// Program callers prepare the per-file resolver before reaching this worker.
/// tsc-port: getDeclarationDiagnostics @6.0.3
/// tsc-hash: b34e4107425e1377261d1f20e0c23e3d970722b026812172da73afad146434a8
/// tsc-span: _tsc.js:114249-114263
pub fn get_declaration_diagnostics(
    resolver: &dyn EmitResolver,
    host: &dyn EmitHost,
    paths: &dyn DeclarationPathResolver,
    source: SourceFileId,
) -> Result<Vec<Diagnostic>, EmitFailure> {
    let emit_source = host.source_file(source).ok_or(EmitFailure::Contract(
        EmitContractViolation::PlannedSourceMissing(source),
    ))?;
    if !crate::get_source_files_to_emit(host, crate::EmitSelection::TargetSourceFile(source))?
        .contains(&source)
        || crate::builtins::is_json_file_name(emit_source.path())
    {
        return Ok(Vec::new());
    }
    let mut arena = TransformArena::new();
    arena.keep_node_builder_cache();
    let transform_source = mount_declaration_program_sources(&mut arena, host, source)?;
    let transformers = get_declaration_transformers(
        host.compiler_options(),
        resolver,
        host,
        paths,
        &DeclarationCustomTransformers::none(),
    )?;
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(transform_source)],
        transformers,
        false,
    )?;
    let diagnostics = result.diagnostics().to_vec();
    result.dispose();
    Ok(diagnostics)
}

/// tsc-port: emitDeclarationFileOrBundle @6.0.3
/// tsc-hash: 8275307ffb4a07e3c7d8b7a5d7f2acf16bfe01c5f746285165c54dc225904434
/// tsc-span: _tsc.js:116640-116715
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_declaration_unit(
    resolver: &dyn EmitResolver,
    host: &dyn EmitHost,
    preflight: &EmitPreflight,
    paths: &dyn DeclarationPathResolver,
    planned_root: &crate::EmitRoot,
    declaration_path: JsStr<'_>,
    declaration_map_path: Option<JsStr<'_>>,
    force_dts_emit: bool,
) -> Result<DeclarationUnitEmit, EmitFailure> {
    prepare_declaration_unit(
        resolver,
        host,
        preflight,
        paths,
        planned_root,
        declaration_path,
        declaration_map_path,
        force_dts_emit,
        None,
        None,
        None,
    )?
    .finish()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare_declaration_unit(
    resolver: &dyn EmitResolver,
    host: &dyn EmitHost,
    preflight: &EmitPreflight,
    paths: &dyn DeclarationPathResolver,
    planned_root: &crate::EmitRoot,
    declaration_path: JsStr<'_>,
    declaration_map_path: Option<JsStr<'_>>,
    force_dts_emit: bool,
    pool: Option<&crate::print_pool::EmitPrintPool>,
    credit: Option<crate::print_pool::PrintCredit>,
    span: Option<tsc_types::tracing::Span>,
) -> Result<DeclarationWork, EmitFailure> {
    let mut files_for_emit = Vec::new();
    for &source in planned_root.source_files() {
        let file = host.source_file(source).ok_or(EmitFailure::Contract(
            EmitContractViolation::PlannedSourceMissing(source),
        ))?;
        let syntax = file.syntax().ok_or(EmitFailure::Contract(
            EmitContractViolation::CheckedSyntaxUnavailable(source),
        ))?;
        if force_dts_emit || !crate::builtins::is_json_file_name(&syntax.file_name) {
            files_for_emit.push(source);
        }
    }
    let root = match planned_root {
        crate::EmitRoot::SourceFile(_) if files_for_emit.is_empty() => {
            return Ok(DeclarationUnitEmit {
                diagnostics: Vec::new(),
                decl_blocked: false,
                artifact: None,
                map_artifact: None,
                map_observation: None,
            }
            .into());
        }
        crate::EmitRoot::SourceFile(source) => crate::EmitRoot::SourceFile(*source),
    };
    let options = host.compiler_options();
    for &source in &files_for_emit {
        // Seed declaration visibility when checking is skipped, including
        // ordinary noCheck Programs whose .ts sources can carry diagnostics.
        if force_dts_emit
            || options.no_check == Some(true)
            || !resolver
                .can_include_bind_and_check_diagnostics(source)
                .map_err(TransformError::from)?
        {
            let syntax = host
                .source_file(source)
                .and_then(crate::EmitSource::syntax)
                .ok_or(EmitFailure::Contract(
                    EmitContractViolation::CheckedSyntaxUnavailable(source),
                ))?;
            collect_linked_aliases_for_declaration(resolver, source, syntax)?;
        }
    }
    let mut arena = TransformArena::new();
    arena.keep_node_builder_cache();
    // Other program sources are mounted on first use
    // (`TransformArena::mount_parse_tree_transform_node`).
    let transform_root = crate::execute::mount_emit_root(&mut arena, host, &root, false)?;
    let transformers = get_declaration_transformers(
        options,
        resolver,
        host,
        paths,
        &DeclarationCustomTransformers::none(),
    )?;
    let mut result = transform_nodes(arena, vec![transform_root], transformers, false)?;
    // tsgo emitFiles adds the declaration diagnostics to the emitter's
    // DiagnosticsCollection (compiler/emitter.go:241), which keeps one of the
    // equal ones; the written file's diagnostics are its sorted list.
    let mut diagnostics = result.diagnostics().to_vec();
    tsc_diagnostics::sort_and_dedupe_equal_diagnostics(&mut diagnostics);
    let diagnostics_blocked = !diagnostics.is_empty();
    let path_blocked = !diagnostics_blocked && preflight.is_emit_blocked(host, declaration_path);
    let decl_blocked = diagnostics_blocked || path_blocked || options.no_emit == Some(true);
    if decl_blocked && !force_dts_emit {
        result.dispose();
        return Ok(DeclarationUnitEmit {
            diagnostics,
            decl_blocked: true,
            artifact: None,
            map_artifact: None,
            map_observation: None,
        }
        .into());
    }
    if result.roots().len() != 1 {
        result.dispose();
        return Err(EmitFailure::Transform(Box::new(
            TransformError::UnsupportedCompilerOption {
                option: "declaration transformer contract",
                detail: "declaration transform must produce exactly one root",
            },
        )));
    }
    let transformed_root = result.roots()[0].clone();
    let source_files = crate::execute::transformed_source_paths(&result, &transformed_root, host)?;
    let source_path = source_files[0].as_js();
    let new_line = match options.new_line {
        Some(0) => NewLineKind::CarriageReturnLineFeed,
        None | Some(1) => NewLineKind::LineFeed,
        Some(_) => {
            result.dispose();
            return Err(EmitFailure::UnsupportedCompilerOption { option: "newLine" });
        }
    };
    let printer_options = PrinterOptions::new(new_line)
        .with_remove_comments(options.remove_comments == Some(true))
        .with_no_emit_helpers(true)
        .with_declaration_syntax(true)
        .with_only_print_js_doc_style(true)
        .with_omit_brace_source_map_positions(true)
        .with_module_kind(options.emit_module_kind())
        .with_target(options.emit_script_target())
        .with_source_file_text_mode(SourceFileTextMode::Canonical);
    let global_name_oracle = crate::execute::ResolverGlobalNameOracle(resolver);
    let recording_enabled =
        options.declaration_map == Some(true) && !source_path.ends_with(".json");
    let map_lane = recording_enabled.then(|| crate::execute::map_lane_inputs(host));
    let recording = map_lane.as_ref().map(|lane| {
        let mut inputs = crate::declaration_map_recording_inputs_for(
            lane,
            options,
            declaration_path,
            source_path,
        );
        let crate::EmitRoot::SourceFile(declared) = &root;
        inputs.original_positions = host
            .content_mapper_original(*declared)
            .and_then(|original| {
                let declared = host.source_file(*declared)?.syntax()?;
                Some(crate::OriginalPositions::new(
                    declared.file_name.clone(),
                    original.file_name.to_owned(),
                    std::sync::Arc::clone(original.text),
                    original.span_map.clone(),
                ))
            });
        inputs
    });
    let mut printer = create_printer(printer_options);
    let TransformRoot::SourceFile(root_source) = transformed_root;
    let prepared = match printer.prepare_source_file_print(
        &mut result,
        root_source,
        Some(&global_name_oracle),
    ) {
        Ok(prepared) => prepared,
        Err(error) => {
            result.dispose();
            return Err(error.into());
        }
    };
    let mut inputs = DeclarationPrintInputs {
        path: crate::execute::OutputPath::Borrowed(declaration_path),
        map_path: declaration_map_path.map(crate::execute::OutputPath::Borrowed),
        source_files,
        diagnostics,
        decl_blocked,
        new_line,
        emit_bom: options.emit_bom == Some(true),
        map_options: map_lane
            .as_ref()
            .map(|_| DeclarationMapOptions::Borrowed(options)),
        map_lane,
        recording,
        span,
        worker_trace: None,
    };
    if let (Some(pool), Some(credit)) = (pool, credit) {
        if result.has_inert_print_hooks() {
            // The native serialized-type cache contains handles and tracking
            // facts, not syntax ownership. All builder requests are complete.
            drop(result.arena_mut()?.take_node_builder_cache());
            let bytes = result
                .print_input_charge()
                .and_then(|tree| tree.checked_add(inputs.input_charge()?));
            if let Some((bytes, charge)) =
                bytes.and_then(|bytes| credit.try_charge(bytes).map(|charge| (bytes, charge)))
            {
                if let Some(owned) = result.take_owned_print() {
                    if let Some(span) = inputs.span.as_mut() {
                        span.set_arg("ownedPrintCharge", bytes as u64);
                    }
                    drop(inputs.span.take());
                    inputs.worker_trace = host.tracing().cloned();
                    let inputs = inputs.into_owned();
                    return Ok(DeclarationWork::Pending(pool.submit(credit, move |_| {
                        let _charge = charge;
                        print_prepared_declaration(owned.into_result(), printer, prepared, inputs)
                    })));
                }
            }
        }
    }
    Ok(DeclarationWork::Complete(print_prepared_declaration(
        result, printer, prepared, inputs,
    )))
}

fn print_prepared_declaration(
    mut result: TransformationResult<'_>,
    mut printer: crate::Printer,
    prepared: crate::printer::PreparedSourceFilePrint,
    inputs: DeclarationPrintInputs<'_>,
) -> Result<DeclarationUnitEmit, EmitFailure> {
    let DeclarationPrintInputs {
        path,
        map_path,
        source_files,
        diagnostics,
        decl_blocked,
        new_line,
        emit_bom,
        map_lane,
        map_options,
        recording,
        span: _span,
        worker_trace,
    } = inputs;
    let _worker_span = worker_trace.as_ref().map(|tracing| {
        tracing.begin(
            tsc_types::tracing::Phase::Emit,
            "printPreparedDeclaration",
            tsc_types::tracing::Args::new().with(
                "declarationFilePath",
                path.as_js().to_string_lossy().into_owned(),
            ),
        )
    });
    let printed = printer.print_prepared_source_file(&mut result, prepared, recording);
    result.dispose();
    // Maps keep their own recording inputs; the transformed tree is done.
    drop(result);
    let printed = printed?;
    let (artifact, map_artifact, map_observation) = if let Some(map_lane) = &map_lane {
        let map_path = map_path.as_ref().ok_or(EmitFailure::Contract(
            crate::EmitContractViolation::DeclarationMapPathMissing,
        ))?;
        let mapped = crate::finish_declaration_map(
            map_lane,
            map_options
                .as_ref()
                .expect("declaration map options")
                .as_options(),
            path.as_js(),
            map_path.as_js(),
            source_files[0].as_js(),
            printed,
            diagnostics.clone(),
            new_line,
        )?;
        (
            mapped.declaration,
            Some(mapped.map),
            Some(mapped.observation),
        )
    } else {
        (
            EmitArtifact::declaration(
                path.into_string(),
                crate::artifact::EmitCallbackText::from_generated(printed.into_generated_text()),
                emit_bom,
                Some(source_files),
                EmitTextMetadata::new(diagnostics.clone(), None),
            ),
            None,
            None,
        )
    };
    Ok(DeclarationUnitEmit {
        diagnostics,
        decl_blocked,
        artifact: Some(artifact),
        map_artifact,
        map_observation,
    })
}

/// tsc-port: collectLinkedAliases @6.0.3
/// tsc-hash: 4e2c2a3777eb8c8337cfbe0780bdfc63b4a49564bf6bd7c34e720fd69ec72c5b
/// tsc-span: _tsc.js:116716-116735
fn collect_linked_aliases_for_declaration(
    resolver: &dyn EmitResolver,
    source: SourceFileId,
    syntax: &tsc_syntax::SourceFile,
) -> Result<(), EmitFailure> {
    let mut stack = vec![syntax.root];
    while let Some(node) = stack.pop() {
        match syntax.arena.node(node).data() {
            NodeData::ExportAssignment(data) => {
                if let Some(expression) = data.expression.filter(|expression| {
                    syntax.arena.node(*expression).kind == SyntaxKind::Identifier
                }) {
                    resolver
                        .collect_linked_aliases(EmitResolverNode::new(source, expression), true)
                        .map_err(TransformError::from)?;
                }
                continue;
            }
            NodeData::ExportSpecifier(data) => {
                if let Some(name) = data.property_name.or(data.name) {
                    resolver
                        .collect_linked_aliases(EmitResolverNode::new(source, name), true)
                        .map_err(TransformError::from)?;
                }
                continue;
            }
            _ => {}
        }
        let mut children = Vec::<NodeId>::new();
        for_each_child(&syntax.arena, syntax.arena.node(node), |child| {
            children.push(child);
            false
        });
        stack.extend(children.into_iter().rev());
    }
    Ok(())
}

/// tsc-port: emitDeclarationFileOrBundle @6.0.3
/// Transform one source's declarations and return the blocking decision
/// with the printable arena; the caller prints (test harness entry).
/// tsc-hash: 8275307ffb4a07e3c7d8b7a5d7f2acf16bfe01c5f746285165c54dc225904434
/// tsc-span: _tsc.js:116640-116715
#[doc(hidden)]
pub fn transform_declaration_unit_for_harness<'t>(
    resolver: &'t dyn EmitResolver,
    host: &'t dyn EmitHost,
    preflight: &EmitPreflight,
    paths: &'t dyn DeclarationPathResolver,
    source: SourceFileId,
) -> Result<(DeclarationTransformOutcome, TransformationResult<'t>), EmitFailure> {
    let emit_source = host.source_file(source).ok_or(EmitFailure::Contract(
        EmitContractViolation::PlannedSourceMissing(source),
    ))?;
    let syntax = emit_source.syntax().ok_or(EmitFailure::Contract(
        EmitContractViolation::CheckedSyntaxUnavailable(source),
    ))?;
    let declaration_path = paths
        .declaration_file_path(source)
        .unwrap_or_else(|| syntax.file_name.clone());
    let options = host.compiler_options();

    let mut arena = TransformArena::new();
    arena.keep_node_builder_cache();
    let transform_source = mount_declaration_program_sources(&mut arena, host, source)?;
    let transformers = get_declaration_transformers(
        options,
        resolver,
        host,
        paths,
        &DeclarationCustomTransformers::none(),
    )?;
    let result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(transform_source)],
        transformers,
        false,
    )
    .map_err(|error| EmitFailure::Transform(Box::new(error)))?;
    let diagnostics = result.diagnostics().to_vec();
    let diagnostics_len = diagnostics.len();

    // Preserve upstream's short-circuit order: diagnostics first, then the
    // host blocking lookup, then `noEmit`.  The evaluated bit is observable
    // in the harness tuple even when an earlier arm already blocked output.
    let diagnostics_blocked = diagnostics_len != 0;
    let is_emit_blocked_evaluated = !diagnostics_blocked;
    let is_emit_blocked = if is_emit_blocked_evaluated {
        preflight.is_emit_blocked(host, declaration_path.as_js())
    } else {
        false
    };
    let no_emit = options.no_emit;
    let decl_blocked = diagnostics_blocked || is_emit_blocked || no_emit == Some(true);
    let root = result.roots().first().cloned().ok_or_else(|| {
        EmitFailure::Transform(Box::new(TransformError::UnknownSource(transform_source)))
    })?;

    let outcome = DeclarationTransformOutcome {
        root,
        diagnostics,
        decl_blocked,
        decl_blocked_inputs: DeclBlockedInputs {
            diagnostics_len,
            is_emit_blocked_evaluated,
            is_emit_blocked,
            no_emit,
            decl_blocked,
        },
    };
    Ok((outcome, result))
}

#[cfg(test)]
#[path = "../../tests/unit/declaration_print.rs"]
mod print_tests;

use std::path::{Path, PathBuf};

use tsc_diagnostics::Diagnostic;
use tsc_program::SourceFileId;
use tsc_syntax::{for_each_child, NodeData, NodeId, SyntaxKind};

use crate::{
    create_printer, transform_nodes, DeclarationPrintHandlers, EmitArtifact, EmitContractViolation,
    EmitFailure, EmitHost, EmitPreflight, EmitResolver, EmitResolverNode, EmitTextMetadata,
    GlobalNameOracle, H2ActivityCanary, H2RuntimeSlice, NewLineKind, PrinterOptions,
    SourceFileTextMode, TransformArena, TransformError, TransformRoot, TransformationResult,
};

use super::{
    get_declaration_transformers, get_declaration_transformers_with_observer, BoundaryEvent,
    DeclarationCustomTransformers, DeclarationPathResolver,
};

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
    let transform_source = arena.add_source(syntax, Some(source));
    for &other in host.source_file_ids() {
        if other == source {
            continue;
        }
        if let Some(syntax) = host.source_file(other).and_then(|source| source.syntax()) {
            arena.add_source(syntax, Some(other));
        }
    }
    Ok(transform_source)
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

struct ResolverGlobalNameOracle<'resolver>(&'resolver dyn EmitResolver);

impl GlobalNameOracle for ResolverGlobalNameOracle<'_> {
    fn has_global_name(&self, name: &str) -> Result<bool, crate::EmitResolverError> {
        self.0.has_global_name(name)
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
    activity: &mut H2ActivityCanary,
) -> Result<Vec<Diagnostic>, EmitFailure> {
    let emit_source = host.source_file(source).ok_or(EmitFailure::Contract(
        EmitContractViolation::PlannedSourceMissing(source),
    ))?;
    if !crate::get_source_files_to_emit(host, crate::EmitSelection::TargetSourceFile(source))?
        .contains(&source)
        || emit_source
            .path()
            .to_string_lossy()
            .to_ascii_lowercase()
            .ends_with(".json")
    {
        return Ok(Vec::new());
    }
    let mut arena = TransformArena::new();
    let transform_source = mount_declaration_program_sources(&mut arena, host, source)?;
    let transformers = get_declaration_transformers(
        host.compiler_options(),
        resolver,
        host,
        paths,
        &DeclarationCustomTransformers::none(),
    )?;
    activity.observe_runtime_slice(H2RuntimeSlice::H2_7b);
    activity.construct_transform_context();
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
    source: SourceFileId,
    declaration_path: &Path,
    declaration_map_path: Option<&Path>,
    activity: &mut H2ActivityCanary,
    force_dts_emit: bool,
) -> Result<DeclarationUnitEmit, EmitFailure> {
    let emit_source = host.source_file(source).ok_or(EmitFailure::Contract(
        EmitContractViolation::PlannedSourceMissing(source),
    ))?;
    let syntax = emit_source.syntax().ok_or(EmitFailure::Contract(
        EmitContractViolation::CheckedSyntaxUnavailable(source),
    ))?;
    if !force_dts_emit && syntax.file_name.to_ascii_lowercase().ends_with(".json") {
        return Ok(DeclarationUnitEmit {
            diagnostics: Vec::new(),
            decl_blocked: false,
            artifact: None,
            map_artifact: None,
            map_observation: None,
        });
    }

    activity.observe_runtime_slice(H2RuntimeSlice::H2_7b);

    // This forced route is declaration-only, so it skips source checking
    // and must collect linked aliases before transforming (:116649-116653).
    if force_dts_emit
        || !resolver
            .can_include_bind_and_check_diagnostics(source)
            .map_err(TransformError::from)?
    {
        collect_linked_aliases_for_declaration(resolver, source, syntax)?;
    }

    let options = host.compiler_options();
    let mut arena = TransformArena::new();
    let transform_source = mount_declaration_program_sources(&mut arena, host, source)?;
    let transformers = get_declaration_transformers(
        options,
        resolver,
        host,
        paths,
        &DeclarationCustomTransformers::none(),
    )?;
    activity.construct_transform_context();
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(transform_source)],
        transformers,
        false,
    )
    .map_err(|error| EmitFailure::Transform(Box::new(error)))?;
    let diagnostics = result.diagnostics().to_vec();
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
        });
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
    let TransformRoot::SourceFile(root_source) = result.roots()[0] else {
        result.dispose();
        return Err(EmitFailure::Transform(Box::new(
            TransformError::UnsupportedCompilerOption {
                option: "declaration transformer contract",
                detail: "declaration transform root must be a source file",
            },
        )));
    };
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
        .with_target(options.emit_script_target())
        .with_source_file_text_mode(SourceFileTextMode::Canonical);
    activity.construct_printer();
    let global_name_oracle = ResolverGlobalNameOracle(resolver);
    let recording_enabled =
        options.declaration_map == Some(true) && !syntax.file_name.ends_with(".json");
    let map_lane = recording_enabled.then(|| crate::execute::map_lane_inputs(host));
    let recording = map_lane.as_ref().map(|lane| {
        crate::declaration_map_recording_inputs_for(
            lane,
            options,
            declaration_path,
            emit_source.path(),
        )
    });
    let printed = create_printer(printer_options).print_declaration_with_recording(
        &mut result,
        root_source,
        DeclarationPrintHandlers::new(&global_name_oracle),
        recording,
    );
    result.dispose();
    let printed = printed?;
    let (artifact, map_artifact, map_observation) = if let Some(map_lane) = &map_lane {
        // TS prints before getSourceMappingURL's Debug.checkDefined(mapPath).
        // Keep that failure distinct from a write error and from map absence.
        let map_path = declaration_map_path.ok_or(EmitFailure::Contract(
            crate::EmitContractViolation::DeclarationMapPathMissing,
        ))?;
        let mapped = crate::finish_declaration_map(
            map_lane,
            options,
            declaration_path,
            map_path,
            emit_source.path(),
            &printed,
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
                declaration_path,
                printed.text(),
                options.emit_bom == Some(true),
                Some(vec![emit_source.path().to_path_buf()]),
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
        match &syntax.arena.node(node).data {
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
/// The owned slice is the diagnostic/blocking seam only.
/// tsc-hash: 8275307ffb4a07e3c7d8b7a5d7f2acf16bfe01c5f746285165c54dc225904434
/// tsc-span: _tsc.js:116640-116715
#[doc(hidden)]
pub fn transform_declaration_unit_for_harness<'t>(
    resolver: &'t dyn EmitResolver,
    host: &'t dyn EmitHost,
    preflight: &EmitPreflight,
    paths: &'t dyn DeclarationPathResolver,
    source: SourceFileId,
) -> Result<DeclarationTransformOutcome, EmitFailure> {
    let emit_source = host.source_file(source).ok_or(EmitFailure::Contract(
        EmitContractViolation::PlannedSourceMissing(source),
    ))?;
    let syntax = emit_source.syntax().ok_or(EmitFailure::Contract(
        EmitContractViolation::CheckedSyntaxUnavailable(source),
    ))?;
    let declaration_path = paths
        .declaration_file_path(source)
        .unwrap_or_else(|| PathBuf::from(&syntax.file_name));
    let options = host.compiler_options();

    let mut arena = TransformArena::new();
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
        preflight.is_emit_blocked(host, &declaration_path)
    } else {
        false
    };
    let no_emit = options.no_emit;
    let decl_blocked = diagnostics_blocked || is_emit_blocked || no_emit == Some(true);
    let root = result.roots().first().cloned().ok_or_else(|| {
        EmitFailure::Transform(Box::new(TransformError::UnknownSource(transform_source)))
    })?;

    Ok(DeclarationTransformOutcome {
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
    })
}

/// Run one dormant declaration transform while retaining its printable arena
/// and reporting the two declaration visitor boundaries to a harness observer.
/// tsrs-native: observer-armed declaration replay bridge (h2-7a-m-4 P3).
#[doc(hidden)]
pub fn transform_declaration_unit_with_observer_for_harness<'t>(
    resolver: &'t dyn EmitResolver,
    host: &'t dyn EmitHost,
    preflight: &EmitPreflight,
    paths: &'t dyn DeclarationPathResolver,
    source: SourceFileId,
    observer: &'t mut dyn FnMut(BoundaryEvent),
) -> Result<(DeclarationTransformOutcome, TransformationResult<'t>), EmitFailure> {
    let emit_source = host.source_file(source).ok_or(EmitFailure::Contract(
        EmitContractViolation::PlannedSourceMissing(source),
    ))?;
    let syntax = emit_source.syntax().ok_or(EmitFailure::Contract(
        EmitContractViolation::CheckedSyntaxUnavailable(source),
    ))?;
    let declaration_path = paths
        .declaration_file_path(source)
        .unwrap_or_else(|| PathBuf::from(&syntax.file_name));
    let options = host.compiler_options();

    let mut arena = TransformArena::new();
    let transform_source = mount_declaration_program_sources(&mut arena, host, source)?;
    let transformers = get_declaration_transformers_with_observer(
        options,
        resolver,
        host,
        paths,
        &DeclarationCustomTransformers::none(),
        observer,
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

    let diagnostics_blocked = diagnostics_len != 0;
    let is_emit_blocked_evaluated = !diagnostics_blocked;
    let is_emit_blocked = if is_emit_blocked_evaluated {
        preflight.is_emit_blocked(host, &declaration_path)
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

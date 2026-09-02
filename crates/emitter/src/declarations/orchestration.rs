use std::path::PathBuf;

use tsc_diagnostics::Diagnostic;
use tsc_program::SourceFileId;

use crate::{
    transform_nodes, EmitContractViolation, EmitFailure, EmitHost, EmitPreflight, EmitResolver,
    TransformArena, TransformError, TransformRoot,
};

use super::{get_declaration_transformers, DeclarationCustomTransformers, DeclarationPathResolver};

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
    let transform_source = arena.add_source(syntax, Some(source));
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

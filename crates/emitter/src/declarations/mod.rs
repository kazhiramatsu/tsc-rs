//! Dormant TypeScript declaration-transform foundation.

// P1 is intentionally dormant until P2 supplies root/statement orchestration.
// Keep its internally complete call graph warning-clean while it has no
// production entry point.
#![allow(dead_code)]

mod diagnostics;
mod ensure;
mod state;
mod subtree;
mod tracker;

use std::path::PathBuf;

use tsc_program::SourceFileId;
use tsc_syntax::SyntaxKind;
use tsc_types::CompilerOptions;

use crate::{
    EmitHost, EmitResolver, TransformError, TransformFlags, TransformNode, TransformRoot,
    TransformationContext, Transformer,
};

use self::state::{TransformState, VisitResult};
use self::tracker::DeclarationSymbolTracker;

/// Caller-owned declaration-output paths. The dormant transformer deliberately
/// does not reconstruct output planning.
pub trait DeclarationPathResolver {
    /// tsrs-native: dormant declaration-output path injection (h2-7a-m-4 §5.8).
    fn declaration_file_path(&self, source: SourceFileId) -> Option<PathBuf>;

    /// tsrs-native: effective declaration/JavaScript/source reference target
    /// injection (h2-7a-m-4 §5.8).
    fn reference_target_path(&self, source: SourceFileId) -> Option<PathBuf>;
}

/// One declaration visitor boundary observation. `output_ref == None` is the
/// empty-output sentinel; array results produce one event per output node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundaryEvent {
    pub input_ref: TransformNode,
    pub output_ref: Option<TransformNode>,
    pub has_original: bool,
    pub transform_flags: TransformFlags,
}

/// Declaration-emission transformer owner. It remains production-dormant until
/// the H2.7b selection/orchestration rung.
pub struct DeclarationTransformer<'t> {
    options: &'t CompilerOptions,
    resolver: &'t dyn EmitResolver,
    host: &'t dyn EmitHost,
    paths: &'t dyn DeclarationPathResolver,
    state: Option<TransformState>,
    tracker: DeclarationSymbolTracker<'t>,
    boundary_observer: Option<&'t mut dyn FnMut(BoundaryEvent)>,
}

impl<'t> DeclarationTransformer<'t> {
    /// tsc-port: transformDeclarations @6.0.3
    /// tsc-hash: 83b01352c568eb256aba9d60253fd28955a5b2b2899543f70867cc8661e817a8
    /// tsc-span: _tsc.js:114265-115802
    pub fn new(
        options: &'t CompilerOptions,
        resolver: &'t dyn EmitResolver,
        host: &'t dyn EmitHost,
        paths: &'t dyn DeclarationPathResolver,
    ) -> Self {
        Self {
            options,
            resolver,
            host,
            paths,
            state: None,
            tracker: DeclarationSymbolTracker::new(options, host),
            boundary_observer: None,
        }
    }

    /// tsrs-native: harness-only L1 boundary observation injection.
    #[doc(hidden)]
    pub fn with_boundary_observer(mut self, observer: &'t mut dyn FnMut(BoundaryEvent)) -> Self {
        self.boundary_observer = Some(observer);
        self
    }

    fn state(&self) -> Result<&TransformState, TransformError> {
        self.state
            .as_ref()
            .ok_or(TransformError::UnsupportedCompilerOption {
                option: "declaration transformer",
                detail: "per-file state has not been initialized",
            })
    }

    fn state_mut(&mut self) -> Result<&mut TransformState, TransformError> {
        self.state
            .as_mut()
            .ok_or(TransformError::UnsupportedCompilerOption {
                option: "declaration transformer",
                detail: "per-file state has not been initialized",
            })
    }

    fn kind(
        &self,
        cx: &TransformationContext,
        node: TransformNode,
    ) -> Result<SyntaxKind, TransformError> {
        Ok(cx.arena().node(node)?.kind)
    }

    fn parent(
        &self,
        cx: &TransformationContext,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        Ok(cx
            .arena()
            .node(node)?
            .parent
            .and_then(|parent| cx.arena().node_ref(node.source(), parent)))
    }

    fn required_resolver_node(
        &self,
        cx: &TransformationContext,
        node: TransformNode,
    ) -> Result<crate::EmitResolverNode, TransformError> {
        cx.arena().require_parse_tree_resolver_node(node)
    }

    fn current_enclosing_resolver_node(
        &self,
        cx: &TransformationContext,
    ) -> Result<crate::EmitResolverNode, TransformError> {
        let enclosing = self.state()?.enclosing_declaration.ok_or(
            TransformError::UnsupportedCompilerOption {
                option: "declaration transformer",
                detail: "an enclosing declaration is required",
            },
        )?;
        self.required_resolver_node(cx, enclosing)
    }

    fn observe_boundary(
        &mut self,
        cx: &TransformationContext,
        input: TransformNode,
        result: &VisitResult,
    ) {
        let Some(observer) = self.boundary_observer.as_deref_mut() else {
            return;
        };
        let outputs: &[TransformNode] = match result {
            VisitResult::Node(output) if *output == input => return,
            VisitResult::Node(output) => std::slice::from_ref(output),
            VisitResult::Nodes(outputs) => outputs,
            VisitResult::None => &[],
        };
        if outputs.is_empty() {
            observer(BoundaryEvent {
                input_ref: input,
                output_ref: None,
                has_original: false,
                transform_flags: TransformFlags::NONE,
            });
            return;
        }
        for &output in outputs {
            observer(BoundaryEvent {
                input_ref: input,
                output_ref: Some(output),
                has_original: cx
                    .arena()
                    .metadata(output)
                    .and_then(crate::EmitMetadata::original)
                    .is_some(),
                transform_flags: cx.arena().transform_flags(output),
            });
        }
    }

    fn contract(detail: &'static str) -> TransformError {
        // TransformError::Contract is owned by a later transform protocol
        // expansion; this typed existing refusal keeps P1 inside transform.rs's
        // hard boundary.
        TransformError::UnsupportedCompilerOption {
            option: "declaration transformer contract",
            detail,
        }
    }
}

impl Transformer for DeclarationTransformer<'_> {
    fn name(&self) -> &'static str {
        "declarations"
    }

    fn transform_root(
        &mut self,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        root_p2::transform_root(self, context, root)
    }

    fn dispose(&mut self) {}
}

// P2 owns this module's implementation. Keeping the seam inline avoids
// creating or editing P2's root.rs in the P1 lane.
mod root_p2 {
    use super::*;

    /// tsrs-native: P2 transformRoot implementation seam.
    pub(crate) fn transform_root(
        transformer: &mut DeclarationTransformer<'_>,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        // P2
        let _ = (transformer.options, transformer.paths, context);
        match root {
            TransformRoot::Bundle(_) => Err(TransformError::Unsupported(
                crate::UnsupportedEmitFeature::BundleRoot,
            )),
            TransformRoot::SourceFile(_) => Ok(root),
        }
    }
}

// P2 owns statements.rs. These narrow seams let the P1 subtree compile and
// make the two source-equivalent direct-return paths testable.
mod statements_p2 {
    use super::*;

    /// tsrs-native: P2 visitDeclarationStatements implementation seam.
    pub(crate) fn visit_declaration_statement(
        _transformer: &mut DeclarationTransformer<'_>,
        _cx: &mut TransformationContext,
        input: TransformNode,
    ) -> Result<VisitResult, TransformError> {
        // P2
        Ok(VisitResult::Node(input))
    }

    /// tsrs-native: P2 transformTopLevelDeclaration implementation seam.
    pub(crate) fn transform_top_level_declaration(
        _transformer: &mut DeclarationTransformer<'_>,
        _cx: &mut TransformationContext,
        input: TransformNode,
    ) -> Result<VisitResult, TransformError> {
        // P2
        Ok(VisitResult::Node(input))
    }

    /// tsrs-native: P2 transformImportDeclaration implementation seam.
    pub(crate) fn transform_import_declaration(
        _transformer: &mut DeclarationTransformer<'_>,
        _cx: &mut TransformationContext,
        input: TransformNode,
    ) -> Result<VisitResult, TransformError> {
        // P2
        Ok(VisitResult::Node(input))
    }

    /// tsrs-native: P2 transformImportEqualsDeclaration implementation seam.
    pub(crate) fn transform_import_equals_declaration(
        _transformer: &mut DeclarationTransformer<'_>,
        _cx: &mut TransformationContext,
        input: TransformNode,
    ) -> Result<VisitResult, TransformError> {
        // P2
        Ok(VisitResult::Node(input))
    }

    /// tsrs-native: P2 rewriteModuleSpecifier2 implementation seam.
    pub(crate) fn rewrite_module_specifier(
        _transformer: &mut DeclarationTransformer<'_>,
        _cx: &mut TransformationContext,
        _parent: TransformNode,
        input: Option<TransformNode>,
    ) -> Result<Option<TransformNode>, TransformError> {
        // P2
        Ok(input)
    }

    /// tsrs-native: P2 transformAndReplaceLatePaintedStatements seam.
    pub(crate) fn transform_and_replace_late_painted_statements(
        _transformer: &mut DeclarationTransformer<'_>,
        _cx: &mut TransformationContext,
        inputs: Vec<TransformNode>,
    ) -> Result<Vec<TransformNode>, TransformError> {
        // P2
        Ok(inputs)
    }

    /// tsrs-native: P2 stripExportModifiers implementation seam.
    pub(crate) fn strip_export_modifiers(
        _transformer: &mut DeclarationTransformer<'_>,
        _cx: &mut TransformationContext,
        input: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        // P2
        Ok(input)
    }

    /// tsrs-native: P2 updateModuleDeclarationAndKeyword seam.
    pub(crate) fn update_module_declaration_and_keyword(
        _transformer: &mut DeclarationTransformer<'_>,
        _cx: &mut TransformationContext,
        input: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        // P2
        Ok(input)
    }

    /// tsrs-native: P2 transformVariableStatement implementation seam.
    pub(crate) fn transform_variable_statement(
        _transformer: &mut DeclarationTransformer<'_>,
        _cx: &mut TransformationContext,
        input: TransformNode,
    ) -> Result<VisitResult, TransformError> {
        // P2
        Ok(VisitResult::Node(input))
    }

    /// tsrs-native: P2 recreateBindingPattern implementation seam.
    pub(crate) fn recreate_binding_pattern(
        _transformer: &mut DeclarationTransformer<'_>,
        _cx: &mut TransformationContext,
        _name: TransformNode,
    ) -> Result<VisitResult, TransformError> {
        // P2
        Ok(VisitResult::Nodes(Vec::new()))
    }

    /// tsrs-native: P2 recreateBindingElement implementation seam.
    pub(crate) fn recreate_binding_element(
        _transformer: &mut DeclarationTransformer<'_>,
        _cx: &mut TransformationContext,
        input: TransformNode,
    ) -> Result<VisitResult, TransformError> {
        // P2
        Ok(VisitResult::Node(input))
    }

    /// tsrs-native: P2 isScopeMarker2 implementation seam.
    pub(crate) fn is_scope_marker(
        _transformer: &DeclarationTransformer<'_>,
        _cx: &TransformationContext,
        _input: TransformNode,
    ) -> Result<bool, TransformError> {
        // P2
        Ok(false)
    }

    /// tsrs-native: P2 hasScopeMarker2 implementation seam.
    pub(crate) fn has_scope_marker(
        _transformer: &DeclarationTransformer<'_>,
        _cx: &TransformationContext,
        _input: TransformNode,
    ) -> Result<bool, TransformError> {
        // P2
        Ok(false)
    }

    /// tsrs-native: P2 transformHeritageClauses implementation seam.
    pub(crate) fn transform_heritage_clauses(
        _transformer: &mut DeclarationTransformer<'_>,
        _cx: &mut TransformationContext,
        inputs: Vec<TransformNode>,
    ) -> Result<Vec<TransformNode>, TransformError> {
        // P2
        Ok(inputs)
    }

    /// tsrs-native: P2 shouldEmitFunctionProperties implementation seam.
    pub(crate) fn should_emit_function_properties(
        _transformer: &DeclarationTransformer<'_>,
        _cx: &TransformationContext,
        _input: TransformNode,
    ) -> Result<bool, TransformError> {
        // P2
        Ok(false)
    }

    /// tsrs-native: P2 isPreservedDeclarationStatement implementation seam.
    pub(crate) fn is_preserved_declaration_statement(
        _transformer: &DeclarationTransformer<'_>,
        _cx: &TransformationContext,
        _input: TransformNode,
    ) -> Result<bool, TransformError> {
        // P2
        Ok(false)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/declarations/tests.rs"]
mod tests;

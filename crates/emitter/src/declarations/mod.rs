//! Dormant declaration-transformer foundation.
//!
//! The production JavaScript emitter does not select this module yet.  The
//! harness-only entry point in `orchestration.rs` is the intentionally narrow
//! activation seam for P2.

mod orchestration;
pub(crate) mod root;
mod selection;
mod statements;

use std::collections::BTreeMap;
use std::path::PathBuf;

use tsc_diagnostics::Diagnostic;
use tsc_syntax::NodeId;
use tsc_types::CompilerOptions;

use crate::{
    EmitHost, EmitResolver, EmitSymbolTracker, TransformError, TransformNode, TransformRoot,
    TransformSourceId, TransformationContext, Transformer,
};

pub use orchestration::{
    transform_declaration_unit_for_harness, DeclBlockedInputs, DeclarationTransformOutcome,
};
pub use root::DeclarationPathResolver;
pub(crate) use selection::get_declaration_transformers;

/// Typed API1 placeholder for the custom `afterDeclarations` chain.
///
/// P2 can observe whether the chain is empty, but it does not own the custom
/// transformer ABI.  Keeping the vector typed, rather than adding a boolean,
/// preserves the upstream control surface for the activation slice.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DeclarationCustomTransformers {
    pub after_declarations: Vec<()>,
}

impl DeclarationCustomTransformers {
    /// tsrs-native: P2 admits only the typed, empty API1 control.
    pub const fn none() -> Self {
        Self {
            after_declarations: Vec::new(),
        }
    }

    /// tsrs-native: report whether the typed API1 control has any entries.
    pub const fn is_empty(&self) -> bool {
        self.after_declarations.is_empty()
    }
}

/// The declaration transformer's per-file mutable state.
///
/// `state.rs`, `tracker.rs`, `diagnostics.rs`, `subtree.rs`, and `ensure.rs`
/// are P1-owned files.  This checkout predates P1, so the minimum shared
/// state/tracker surface is kept here under an explicit compatibility marker
/// until those files land.  P2-owned behavior remains in `root.rs` and
/// `statements.rs`.
// P1 compatibility scaffold: replace these fields with the landed P1 state
// and tracker modules without changing the P2 call sites.
#[allow(dead_code)]
#[derive(Debug, Default)]
pub(crate) struct TransformState {
    pub(crate) current_source_file: Option<TransformSourceId>,
    pub(crate) enclosing_declaration: Option<TransformNode>,
    pub(crate) late_marked_statements: Option<Vec<TransformNode>>,
    pub(crate) late_statement_replacement: BTreeMap<NodeId, VisitResult>,
    pub(crate) needs_declare: bool,
    pub(crate) is_bundled_emit: bool,
    pub(crate) result_has_external_module_indicator: bool,
    pub(crate) needs_scope_fix_marker: bool,
    pub(crate) result_has_scope_marker: bool,
    pub(crate) suppress_new_diagnostic_contexts: bool,
    pub(crate) references: root::RawFileReferences,
}

impl TransformState {
    /// tsrs-native: reset the P1 compatibility state at a SourceFile boundary.
    pub(crate) fn reset(&mut self, source: TransformSourceId) {
        *self = Self {
            current_source_file: Some(source),
            needs_declare: true,
            ..Self::default()
        };
        self.is_bundled_emit = false;
    }
}

/// A declaration visitor result, matching TypeScript's `VisitResult` shape.
pub(crate) type VisitResult = Option<Vec<TransformNode>>;

/// Diagnostic-context tag used by the P1 diagnostic channel.
// P1 compatibility scaffold: the full diagnostic materialization protocol is
// supplied by P1; P2 only needs to preserve the context transitions.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum DiagnosticContext {
    #[default]
    None,
    ForNode(TransformNode),
    ForNodeName(TransformNode),
    JsFile(TransformSourceId),
    DefaultExport(TransformNode),
    ClassExtends(TransformNode),
}

/// First declaration-transformer tracker implementation.
// P1 compatibility scaffold: P1's tracker file will own the report queue and
// diagnostic specs; the resolver protocol remains the stable dependency.
#[allow(dead_code)]
#[derive(Debug, Default)]
pub(crate) struct DeclarationSymbolTracker {
    pub(crate) diagnostic_context: DiagnosticContext,
    pub(crate) error_name_node: Option<TransformNode>,
    pub(crate) error_fallback_node: Option<TransformNode>,
    pub(crate) error_fallback_stack: Vec<Option<crate::EmitTrackerNodeDescription>>,
}

impl EmitSymbolTracker for DeclarationSymbolTracker {
    fn can_track_symbol(&self) -> bool {
        true
    }

    fn push_error_fallback_node(&mut self, node: Option<crate::EmitTrackerNodeDescription>) {
        self.error_fallback_stack.push(node);
    }

    fn pop_error_fallback_node(&mut self) {
        let _ = self.error_fallback_stack.pop();
    }
}

/// Rust owner for the upstream `transformDeclarations` closure.
pub struct DeclarationTransformer<'t> {
    #[allow(dead_code)]
    pub(crate) options: &'t CompilerOptions,
    pub(crate) resolver: &'t dyn EmitResolver,
    pub(crate) host: &'t dyn EmitHost,
    pub(crate) paths: &'t dyn DeclarationPathResolver,
    pub(crate) state: TransformState,
    pub(crate) tracker: DeclarationSymbolTracker,
    pub(crate) current_output_path: Option<PathBuf>,
    /// Bundle branches are accounted for by the source-only owner but cannot
    /// be entered in P2; `transform_root` refuses them before iteration.
    pub(crate) is_bundled_emit: bool,
}

impl<'t> DeclarationTransformer<'t> {
    /// tsc-port: transformDeclarations @6.0.3
    /// tsc-hash: 83b01352c568eb256aba9d60253fd28955a5b2b2899543f70867cc8661e817a8
    /// tsc-span: _tsc.js:114265-115802
    pub(crate) fn new(
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
            state: TransformState::default(),
            tracker: DeclarationSymbolTracker::default(),
            current_output_path: None,
            is_bundled_emit: false,
        }
    }

    /// tsrs-native: project a transformed node back to the resolver-owned tree.
    pub(crate) fn resolver_node(
        &self,
        context: &TransformationContext,
        node: TransformNode,
    ) -> Result<crate::EmitResolverNode, TransformError> {
        context.arena().require_parse_tree_resolver_node(node)
    }

    /// tsrs-native: adapt the resolver protocol error to the transform error.
    pub(crate) fn resolver_error(error: crate::EmitResolverError) -> TransformError {
        TransformError::Resolver(error)
    }

    #[allow(dead_code)]
    /// tsrs-native: P1 diagnostic queue compatibility seam.
    pub(crate) fn add_diagnostic(
        &self,
        _context: &mut TransformationContext,
        _diagnostic: Diagnostic,
    ) -> Result<(), TransformError> {
        // P1 compatibility seam.  P2 has no declaration diagnostic producer
        // that can materialize a checked diagnostic without P1's spec queue.
        Ok(())
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
        root::transform_root(self, context, root)
    }
}

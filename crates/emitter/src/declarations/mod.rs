//! Dormant TypeScript declaration-transform foundation.

mod bundle;
mod diagnostics;
mod ensure;
mod expando;
mod export_assignment;
mod isolated;
mod javascript;
mod orchestration;
mod paths;
pub(crate) mod root;
mod selection;
mod state;
mod statements;
mod subtree;
mod tracker;

use tsc_diagnostics::JsString;

use tsc_program::SourceFileId;
use tsc_syntax::SyntaxKind;
use tsc_types::CompilerOptions;

use crate::{
    EmitHost, EmitResolver, TransformError, TransformNode, TransformRoot, TransformationContext,
    Transformer,
};

pub(crate) use self::orchestration::emit_declaration_unit;
pub use self::orchestration::get_declaration_diagnostics;
pub use self::orchestration::{
    transform_declaration_unit_for_harness, DeclBlockedInputs, DeclarationTransformOutcome,
};
pub use self::paths::PlanDeclarationPaths;
pub(crate) use self::selection::get_declaration_transformers;
use self::state::{TransformState, VisitResult};
use self::tracker::DeclarationSymbolTracker;

/// Caller-owned declaration-output paths. The dormant transformer deliberately
/// does not reconstruct output planning.
pub trait DeclarationPathResolver {
    /// Forced declaration path for the whole output bundle. Source-file
    /// diagnostic getters retain their separate per-source paths.
    fn bundle_declaration_file_path(&self) -> Option<JsString> {
        None
    }

    /// tsrs-native: dormant declaration-output path injection (h2-7a-m-4 §5.8).
    fn declaration_file_path(&self, source: SourceFileId) -> Option<JsString>;

    /// tsrs-native: effective declaration/JavaScript/source reference target
    /// injection (h2-7a-m-4 §5.8).
    fn reference_target_path(&self, source: SourceFileId) -> Option<JsString>;
}

/// Typed API1 control for the custom `afterDeclarations` chain.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DeclarationCustomTransformers {
    pub after_declarations: Vec<()>,
}

impl DeclarationCustomTransformers {
    /// tsrs-native: the dormant declaration lane admits only an empty API1 control.
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

/// Declaration-emission transformer owner. It remains production-dormant until
/// the H2.7b selection/orchestration rung.
pub struct DeclarationTransformer<'t> {
    options: &'t CompilerOptions,
    resolver: &'t dyn EmitResolver,
    host: &'t dyn EmitHost,
    paths: &'t dyn DeclarationPathResolver,
    state: Option<TransformState>,
    tracker: DeclarationSymbolTracker<'t>,
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
        }
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

    fn contract(detail: &'static str) -> TransformError {
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
        root::transform_root(self, context, root)
    }

    fn transform_bundle(
        &mut self,
        context: &mut TransformationContext,
        bundle: crate::TransformBundle,
    ) -> Result<crate::TransformBundle, TransformError> {
        bundle::transform_bundle(self, context, bundle)
    }

    fn dispose(&mut self) {}
}

#[cfg(test)]
#[path = "../../tests/unit/declarations/tests.rs"]
mod tests;

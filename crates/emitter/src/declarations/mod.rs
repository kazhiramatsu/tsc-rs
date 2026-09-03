//! Dormant TypeScript declaration-transform foundation.

#![allow(dead_code)]

mod diagnostics;
mod ensure;
mod orchestration;
pub(crate) mod root;
mod selection;
mod state;
mod statements;
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

pub use self::orchestration::{
    transform_declaration_unit_for_harness, transform_declaration_unit_with_observer_for_harness,
    DeclBlockedInputs, DeclarationTransformOutcome,
};
pub(crate) use self::selection::{
    get_declaration_transformers, get_declaration_transformers_with_observer,
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

/// One declaration visitor boundary observation. `output_ref == None` is the
/// empty-output sentinel; array results produce one event per output node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundaryEvent {
    pub is_top_level: bool,
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
        is_top_level: bool,
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
                is_top_level,
                input_ref: input,
                output_ref: None,
                has_original: false,
                transform_flags: TransformFlags::NONE,
            });
            return;
        }
        for &output in outputs {
            observer(BoundaryEvent {
                is_top_level,
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

    fn dispose(&mut self) {}
}

#[cfg(test)]
#[path = "../../tests/unit/declarations/tests.rs"]
mod tests;

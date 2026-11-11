//! Mutable facts of one emit, separate from the detached syntax it reads.
//!
//! Source slots scope raw node/array IDs. Flags keep their existing dense,
//! range-relative representation and lazy growth; metadata remains sparse.
//! Preparing a source transfers its source facts together with its syntax.

use std::cell::{OnceCell, RefCell};
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use tsc_syntax::NodeId;

use crate::{EmitMetadata, LiteralNodeProperties, TransformNode};

/// Source-local session facts, stored inside the emitter-only TransformSource.
#[derive(Clone, Debug, Default)]
pub(crate) struct SourceEmitState {
    pub transform_flags: Vec<i32>,
    pub array_transform_flags: Vec<i32>,
    /// Synthesized updates with whole-subtree aggregate flags. Creation-site
    /// hints alone do not establish completeness.
    pub complete_synthesized: FxHashSet<NodeId>,
    /// Absolute ends classified so far; zero requests the initial root walk.
    pub classified_transform_flags: (u32, u32),
    /// Identifier texts through the last scanned node, shared with allocators.
    pub identifier_census: RefCell<IdentifierCensus>,
    /// Parsed identifiers only, initialized once by their collector.
    pub parsed_identifier_names: OnceCell<Arc<FxHashSet<String>>>,
    /// Whether source text contains a Unicode code-point escape.
    pub text_has_extended_unicode_escape: OnceCell<bool>,
    /// Monotonic guard for generated-binding metadata merged onto parsed ids.
    pub generated_binding_on_parsed_node: bool,
}

/// Append-only census shared with name allocators through copy-on-write Arcs.
#[derive(Clone, Debug, Default)]
pub(crate) struct IdentifierCensus {
    pub names: Arc<FxHashSet<String>>,
    pub scanned: usize,
}

/// A declaration resolver's serialized-node cache, bounded by arena identity.
/// A clone must never reuse handles cached for the original arena.
#[derive(Default)]
pub(crate) struct NodeBuilderCache {
    pub kept: bool,
    pub cache: Option<Box<dyn std::any::Any + Send + Sync>>,
}

impl std::fmt::Debug for NodeBuilderCache {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NodeBuilderCache")
            .field("kept", &self.kept)
            .field("cache", &self.cache.is_some())
            .finish()
    }
}

#[derive(Debug, Default)]
pub(crate) struct EmitSession {
    pub metadata: FxHashMap<TransformNode, EmitMetadata>,
    pub literal_properties: FxHashMap<TransformNode, LiteralNodeProperties>,
    pub next_generated_binding_id: u64,
    pub node_builder_cache: NodeBuilderCache,
}

impl Clone for EmitSession {
    fn clone(&self) -> Self {
        Self {
            metadata: self.metadata.clone(),
            literal_properties: self.literal_properties.clone(),
            next_generated_binding_id: self.next_generated_binding_id,
            node_builder_cache: NodeBuilderCache::default(),
        }
    }
}

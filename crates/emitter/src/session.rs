//! Mutable facts of one emit, separate from the detached syntax it reads.
//!
//! Source slots scope raw node/array IDs. Flags keep their existing dense,
//! range-relative representation and lazy growth; metadata remains sparse.
//! Preparing a source transfers its source facts together with its syntax.

use std::cell::{Cell, OnceCell};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use rustc_hash::{FxHashMap, FxHashSet};
use tsc_syntax::nodes::{IdentifierData, Node};
use tsc_syntax::NodeId;
use tsc_types::EscapedName;

use crate::{EmitMetadata, LiteralNodeProperties, TransformNode};

/// Source-local session facts, stored inside the emitter-only TransformSource.
#[derive(Clone, Debug, Default)]
pub(crate) struct SourceEmitState {
    /// Absence proof for the sparse metadata table, relative to this source.
    pub metadata_presence: MetadataPresence,
    pub transform_flags: Vec<i32>,
    pub array_transform_flags: Vec<Cell<i32>>,
    pub deferred_array_flags: Vec<Cell<u64>>,
    pub deferred_array_count: Cell<usize>,
    pub deferred_array_owner: u32,
    /// Synthesized updates with whole-subtree aggregate flags. Creation-site
    /// hints alone do not establish completeness.
    pub complete_synthesized: FxHashSet<NodeId>,
    /// Absolute ends classified so far; zero requests the initial root walk.
    pub classified_transform_flags: (u32, u32),
    /// The identifiers through the last scanned node, shared with allocators.
    pub identifier_census: SharedIdentifierCensus,
    /// Parsed identifiers only, initialized once by their collector.
    pub parsed_identifier_names: OnceCell<Arc<FxHashSet<String>>>,
    /// Whether source text contains a Unicode code-point escape.
    pub text_has_extended_unicode_escape: OnceCell<bool>,
    /// Monotonic guard for generated-binding metadata merged onto parsed ids.
    pub generated_binding_on_parsed_node: bool,
}

impl SourceEmitState {
    // This adds storage sizes, independent of identifier iteration order.
    #[allow(clippy::iter_over_hash_type)]
    pub(crate) fn print_input_usage(&self, u: &mut tsc_diagnostics::MemoryUsage) {
        u.vec("metadata presence", &self.metadata_presence.nodes);
        u.vec("flags", &self.transform_flags);
        u.vec("flags", &self.array_transform_flags);
        u.vec("deferred array flags", &self.deferred_array_flags);
        u.hash_set("synthesized completeness", &self.complete_synthesized);
        self.identifier_census.print_input_usage(u);
        if let Some(names) = self.parsed_identifier_names.get() {
            if u.arc("parsed identifiers", names) {
                u.hash_set("parsed identifier lookup", names);
                for name in names.iter() {
                    u.string("parsed identifier text", name);
                }
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct MetadataPresence {
    // Match NodeId's one-based representation for the hot subtraction.
    base_encoded: u32,
    nodes: Vec<bool>,
}

impl MetadataPresence {
    pub(crate) fn new(base: u32, end: u32) -> Self {
        Self {
            base_encoded: base.wrapping_add(1),
            nodes: vec![false; (end - base) as usize],
        }
    }

    pub(crate) fn contains(&self, node: NodeId) -> Option<bool> {
        // Below-base ids wrap beyond the exact allocated span. Keeping that
        // span unrounded lets the vector bound check prove absence safely.
        self.nodes
            .get(node.index().wrapping_add(1).wrapping_sub(self.base_encoded) as usize)
            .copied()
    }

    pub(crate) fn insert(&mut self, node: NodeId) -> bool {
        let Some(present) = self
            .nodes
            .get_mut(node.index().wrapping_add(1).wrapping_sub(self.base_encoded) as usize)
        else {
            return false;
        };
        *present = true;
        true
    }

    pub(crate) fn cover_through(&mut self, end: u32) {
        self.nodes.resize(
            end.wrapping_add(1).wrapping_sub(self.base_encoded) as usize,
            false,
        );
    }

    pub(crate) fn clear(&mut self) {
        self.nodes.fill(false);
    }
}

/// The identifiers of an emit source in node order: an append-only census
/// shared with the name allocators, each of which starts from the identifiers
/// the source had when the allocator was made (a prefix of `names`).
///
/// Covering a node costs one push. The texts are hashed when an allocator
/// first asks whether a spelling is used, which most emitted files never do
/// (tsgo collects a file's identifiers on its first unique-name request too).
#[derive(Clone, Debug, Default)]
struct IdentifierCensus {
    /// The names of the identifier nodes among the first `scanned` nodes, in
    /// node order, each read when the census first covered its node.
    names: Vec<EscapedName>,
    scanned: usize,
    /// For each text among the first `hashed` names, the index of the first
    /// name that spells it.
    first: FxHashMap<&'static str, u32>,
    hashed: usize,
}

impl IdentifierCensus {
    fn hash_through(&mut self, len: u32) {
        while self.hashed < len as usize {
            let name = IdentifierData {
                escaped_text: self.names[self.hashed],
            };
            let index = u32::try_from(self.hashed).expect("identifier census exceeds u32");
            self.first.entry(name.text()).or_insert(index);
            self.hashed += 1;
        }
    }
}

fn lock(census: &Mutex<IdentifierCensus>) -> MutexGuard<'_, IdentifierCensus> {
    census.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A source's identifier census, shared with the snapshots taken of it. A
/// cloned source owns a copy: the nodes it appends are not the original's.
#[derive(Debug, Default)]
pub(crate) struct SharedIdentifierCensus(Arc<Mutex<IdentifierCensus>>);

impl Clone for SharedIdentifierCensus {
    fn clone(&self) -> Self {
        Self(Arc::new(Mutex::new(lock(&self.0).clone())))
    }
}

impl SharedIdentifierCensus {
    fn print_input_usage(&self, u: &mut tsc_diagnostics::MemoryUsage) {
        if !u.arc("identifier census", &self.0) {
            return;
        }
        let census = lock(&self.0);
        u.vec("identifier census names", &census.names);
        u.hash_map("identifier census lookup", &census.first);
    }

    /// Cover the nodes appended since the last call (`nodes` is every node
    /// of the source) and take the snapshot of the identifiers so far.
    pub(crate) fn snapshot(&self, nodes: &[Node]) -> IdentifierCensusSnapshot {
        let mut census = lock(&self.0);
        if census.scanned < nodes.len() {
            let scanned = census.scanned;
            for node in &nodes[scanned..] {
                if let Some(data) = node.as_identifier() {
                    census.names.push(data.escaped_text);
                }
            }
            census.scanned = nodes.len();
        }
        let len = u32::try_from(census.names.len()).expect("identifier census exceeds u32");
        drop(census);
        IdentifierCensusSnapshot {
            census: Arc::clone(&self.0),
            len,
        }
    }
}

/// The identifier texts of the nodes an emit source had when the snapshot
/// was taken: the first `len` names of its census. Identifiers the source
/// gains later, and a later spelling of a node the census has covered, are
/// not part of it.
#[derive(Clone, Debug, Default)]
pub(crate) struct IdentifierCensusSnapshot {
    census: Arc<Mutex<IdentifierCensus>>,
    len: u32,
}

impl IdentifierCensusSnapshot {
    pub(crate) fn contains(&self, name: &str) -> bool {
        let mut census = lock(&self.census);
        census.hash_through(self.len);
        census
            .first
            .get(name)
            .is_some_and(|&first| first < self.len)
    }
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
    /// Public metadata_mut also accepts not-yet-allocated and unmounted keys.
    pub exceptional_metadata: FxHashSet<TransformNode>,
    pub literal_properties: FxHashMap<TransformNode, LiteralNodeProperties>,
    pub next_generated_binding_id: u64,
    pub node_builder_cache: NodeBuilderCache,
}

impl Clone for EmitSession {
    fn clone(&self) -> Self {
        Self {
            metadata: self.metadata.clone(),
            exceptional_metadata: self.exceptional_metadata.clone(),
            literal_properties: self.literal_properties.clone(),
            next_generated_binding_id: self.next_generated_binding_id,
            node_builder_cache: NodeBuilderCache::default(),
        }
    }
}

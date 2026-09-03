use std::collections::BTreeMap;

use tsc_program::SourceFileId;
use tsc_syntax::nodes::*;
use tsc_syntax::FileReference;
use tsc_syntax::{
    for_each_observable_field, try_visit_each_child, Node, NodeArray, NodeArrayId, NodeData,
    NodeDataChildVisitor, NodeId, ObservableField, SourceFile, SyntaxKind, TypeReferenceDirective,
};
use tsc_types::{ModifierFlags, NodeFlags};

use crate::{
    transform::GeneratedBindingId, EmitFlags, EmitMetadata, EmitResolverNode, JavaScriptString,
    SourceRange, TransformError, TransformFlags,
};

/// Typed spelling for an unscoped emit-helper reference.
///
/// Callers select a semantic helper here instead of constructing a printable
/// identifier and remembering to attach TypeScript's `HelperName` provenance.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum EmitHelperName {
    ImportStar,
    ImportDefault,
    ExportStar,
    Rest,
    RewriteRelativeImportExtension,
    Read,
    Awaiter,
    PropKey,
    ClassPrivateFieldGet,
    ClassPrivateFieldSet,
    ClassPrivateFieldIn,
    SetFunctionName,
    AsyncValues,
    AsyncDelegator,
    AsyncGenerator,
    Await,
    Decorate,
    Metadata,
    Param,
    AddDisposableResource,
    DisposeResources,
    EsDecorate,
    RunInitializers,
    Generator,
    Values,
    Extends,
    SpreadArray,
    MakeTemplateObject,
    Assign,
}

impl EmitHelperName {
    const fn text(self) -> &'static str {
        match self {
            Self::ImportStar => "__importStar",
            Self::ImportDefault => "__importDefault",
            Self::ExportStar => "__exportStar",
            Self::Rest => "__rest",
            Self::RewriteRelativeImportExtension => "__rewriteRelativeImportExtension",
            Self::Read => "__read",
            Self::Awaiter => "__awaiter",
            Self::PropKey => "__propKey",
            Self::ClassPrivateFieldGet => "__classPrivateFieldGet",
            Self::ClassPrivateFieldSet => "__classPrivateFieldSet",
            Self::ClassPrivateFieldIn => "__classPrivateFieldIn",
            Self::SetFunctionName => "__setFunctionName",
            Self::AsyncValues => "__asyncValues",
            Self::AsyncDelegator => "__asyncDelegator",
            Self::AsyncGenerator => "__asyncGenerator",
            Self::Await => "__await",
            Self::Decorate => "__decorate",
            Self::Metadata => "__metadata",
            Self::Param => "__param",
            Self::AddDisposableResource => "__addDisposableResource",
            Self::DisposeResources => "__disposeResources",
            Self::EsDecorate => "__esDecorate",
            Self::RunInitializers => "__runInitializers",
            Self::Generator => "__generator",
            Self::Values => "__values",
            Self::Extends => "__extends",
            Self::SpreadArray => "__spreadArray",
            Self::MakeTemplateObject => "__makeTemplateObject",
            Self::Assign => "__assign",
        }
    }
}

/// Emit-session source identity. It scopes otherwise local arena IDs.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TransformSourceId(u32);

impl TransformSourceId {
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// A node identity paired with its emit-session source arena.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TransformNode {
    source: TransformSourceId,
    node: NodeId,
}

impl TransformNode {
    /// Construct a source-scoped handle. Factory consumers validate the raw
    /// identity against the arena before attaching it as a child.
    pub const fn new(source: TransformSourceId, node: NodeId) -> Self {
        Self { source, node }
    }

    pub const fn source(self) -> TransformSourceId {
        self.source
    }

    pub const fn node(self) -> NodeId {
        self.node
    }
}

/// A node-array identity paired with its emit-session source arena.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TransformNodeArray {
    source: TransformSourceId,
    array: NodeArrayId,
}

impl TransformNodeArray {
    /// Construct a source-scoped array handle. Typed faces validate it before
    /// observing or attaching the array.
    pub const fn new(source: TransformSourceId, array: NodeArrayId) -> Self {
        Self { source, array }
    }

    pub const fn source(self) -> TransformSourceId {
        self.source
    }

    pub const fn array(self) -> NodeArrayId {
        self.array
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransformSource {
    program_source: Option<SourceFileId>,
    parsed_node_base: u32,
    parsed_node_end: u32,
    source: SourceFile,
}

impl TransformSource {
    pub const fn program_source(&self) -> Option<SourceFileId> {
        self.program_source
    }

    pub const fn syntax(&self) -> &SourceFile {
        &self.source
    }

    pub const fn contains_parsed_node(&self, node: NodeId) -> bool {
        node.0 >= self.parsed_node_base && node.0 < self.parsed_node_end
    }
}

/// Emit-only mutable syntax copies plus sparse transform/emit side tables.
/// Parsed `SourceFile` values and their identity leases are never mutated.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TransformArena {
    sources: Vec<TransformSource>,
    node_transform_flags: BTreeMap<TransformNode, TransformFlags>,
    array_transform_flags: BTreeMap<TransformNodeArray, TransformFlags>,
    metadata: BTreeMap<TransformNode, EmitMetadata>,
    next_generated_binding_id: u64,
}

impl TransformArena {
    pub const fn new() -> Self {
        Self {
            sources: Vec::new(),
            node_transform_flags: BTreeMap::new(),
            array_transform_flags: BTreeMap::new(),
            metadata: BTreeMap::new(),
            next_generated_binding_id: 0,
        }
    }

    /// Allocate an emit-session generated-binding identity independently of
    /// transformation-context lifecycle. Checker-built arenas use the same
    /// identity channel as transformer-built arenas.
    /// tsrs-native: arena ownership required by createUniqueName @6.0.3.
    pub(crate) fn allocate_generated_binding_id(&mut self) -> GeneratedBindingId {
        let id = GeneratedBindingId::new(self.next_generated_binding_id);
        self.next_generated_binding_id = self
            .next_generated_binding_id
            .checked_add(1)
            .expect("generated binding identity overflow");
        id
    }

    /// Borrow a synthetic-node constructor over this arena. The checker's
    /// dormant NodeBuilder foundation (h2-7a-m-3) builds serialized
    /// declaration `TypeNode`s through this seam; production transforms keep
    /// obtaining their factory from the `TransformationContext`.
    /// tsrs-native: consumer seam for arena-owned node construction.
    pub fn factory(&mut self) -> NodeFactory<'_> {
        NodeFactory::new(self)
    }

    /// Project a checker-side parse-tree identity into this arena's mounted
    /// copy of that parse tree — the validated inverse of
    /// [`parse_tree_resolver_node`](Self::parse_tree_resolver_node). Returns
    /// `None` when no mounted source carries the resolver node's Program
    /// file; a node id outside the mounted parse lease is an error, never a
    /// silent synthetic alias.
    /// tsrs-native: resolver-to-transform projection for reuse clones
    /// (h2-7a-m-3 §4).
    pub fn parse_tree_transform_node(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        for (index, source) in self.sources.iter().enumerate() {
            if source.program_source() != Some(node.source()) {
                continue;
            }
            let transform_source = TransformSourceId(
                u32::try_from(index).expect("transform source count exceeds u32"),
            );
            let candidate = TransformNode {
                source: transform_source,
                node: node.node(),
            };
            if !source.contains_parsed_node(node.node()) {
                return Err(TransformError::ResolverNodeNotInParseTree(candidate));
            }
            return Ok(Some(candidate));
        }
        Ok(None)
    }

    pub fn add_source(
        &mut self,
        source: &SourceFile,
        program_source: Option<SourceFileId>,
    ) -> TransformSourceId {
        let id = TransformSourceId(
            u32::try_from(self.sources.len()).expect("transform source count exceeds u32"),
        );
        let parsed_node_base = source.arena.node_base();
        let parsed_node_end = source.arena.node_end();
        let mut detached = source.clone();
        detached.arena = std::mem::take(&mut detached.arena).into_detached();
        self.sources.push(TransformSource {
            program_source,
            parsed_node_base,
            parsed_node_end,
            source: detached,
        });
        id
    }

    pub fn source(&self, id: TransformSourceId) -> Result<&TransformSource, TransformError> {
        self.sources
            .get(id.0 as usize)
            .ok_or(TransformError::UnknownSource(id))
    }

    pub(crate) fn source_mut(
        &mut self,
        id: TransformSourceId,
    ) -> Result<&mut TransformSource, TransformError> {
        self.sources
            .get_mut(id.0 as usize)
            .ok_or(TransformError::UnknownSource(id))
    }

    pub fn sources(&self) -> &[TransformSource] {
        &self.sources
    }

    pub fn root(&self, source: TransformSourceId) -> Result<TransformNode, TransformError> {
        let node = self.source(source)?.source.root;
        Ok(TransformNode { source, node })
    }

    pub fn replace_root(
        &mut self,
        source: TransformSourceId,
        root: TransformNode,
    ) -> Result<(), TransformError> {
        if root.source != source {
            return Err(TransformError::CrossSourceNode {
                expected: source,
                actual: root.source,
            });
        }
        if self.node(root)?.kind != SyntaxKind::SourceFile {
            return Err(TransformError::RootKindExpected {
                actual: self.node(root)?.kind,
            });
        }
        self.source_mut(source)?.source.root = root.node;
        Ok(())
    }

    pub fn node(&self, node: TransformNode) -> Result<&Node, TransformError> {
        let source = self.source(node.source)?;
        if !source.source.arena.contains_node(node.node) {
            return Err(TransformError::UnknownNode(node));
        }
        Ok(source.source.arena.node(node.node))
    }

    /// Whether this identity belongs to the immutable parse tree originally
    /// mounted into the emit session, rather than to a node appended by a
    /// transformer or emit-time substitution.
    pub fn is_parsed_node(&self, node: TransformNode) -> Result<bool, TransformError> {
        let source = self.source(node.source)?;
        Ok(source.contains_parsed_node(node.node))
    }

    /// Project a transform node into the immutable parse-tree identity accepted
    /// by the checker-owned emit resolver.
    ///
    /// A detached transform arena appends synthetic nodes immediately after
    /// its mounted parse lease. That numeric `NodeId` can therefore equal the
    /// first parsed node in the next Program source. Parse ownership, rather
    /// than the raw integer, is the discriminant. Following the original-node
    /// chain first preserves resolver access for updated/cloned parse nodes;
    /// an unanchored synthetic node has no checker identity and returns `None`.
    ///
    /// tsc-port: getParseTreeNode @6.0.3
    /// tsc-hash: b035994937956a4423ea3e73cac6618339c610416a4740d68e02560b96b3422d
    /// tsc-span: _tsc.js:11423-11435
    pub fn parse_tree_resolver_node(
        &self,
        node: TransformNode,
    ) -> Result<Option<EmitResolverNode>, TransformError> {
        let Some(original) = self.parse_tree_node(node)? else {
            return Ok(None);
        };
        let source = self.source(original.source())?;
        let program_source = source
            .program_source()
            .ok_or(TransformError::MissingProgramSource(original))?;
        Ok(Some(EmitResolverNode::new(program_source, original.node())))
    }

    /// Required counterpart for resolver sites whose syntax contract admits
    /// only a node anchored in the immutable parse tree.
    pub fn require_parse_tree_resolver_node(
        &self,
        node: TransformNode,
    ) -> Result<EmitResolverNode, TransformError> {
        self.parse_tree_resolver_node(node)?
            .ok_or(TransformError::ResolverNodeNotInParseTree(node))
    }

    pub(crate) fn set_generated_identifier_text(
        &mut self,
        node: TransformNode,
        text: &str,
    ) -> Result<(), TransformError> {
        let record = self
            .source_mut(node.source)?
            .source
            .arena
            .node_mut(node.node);
        let NodeData::Identifier(data) = &mut record.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::Identifier,
                actual: record.kind,
            });
        };
        data.escaped_text = tsc_syntax::escape_leading_underscores(text);
        data.text.clear();
        data.text.push_str(text);
        Ok(())
    }

    /// The label-literal analog of [`Self::set_generated_identifier_text`]:
    /// a text-only completion of a deliberately-deferred synthesized
    /// numeric literal. The Generators machine mints upstream's
    /// `Number.MAX_SAFE_INTEGER` label placeholders and
    /// `updateLabelExpressions` (its sole caller) assigns the final case
    /// numbers here once the build resolves them
    /// (`docs/design/greenfield/slices/h2-5h-b-b-3.md` §12.4).
    pub(crate) fn set_numeric_literal_text(
        &mut self,
        node: TransformNode,
        text: &str,
    ) -> Result<(), TransformError> {
        let record = self
            .source_mut(node.source)?
            .source
            .arena
            .node_mut(node.node);
        let NodeData::NumericLiteral(data) = &mut record.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::NumericLiteral,
                actual: record.kind,
            });
        };
        data.text.clear();
        data.text.push_str(text);
        Ok(())
    }

    /// tsc-port: moveSyntheticComments @6.0.3
    /// tsc-hash: dbec5c77db1209731faea7ecc4bbe067a09abe111ed885ca1c4dfb7b7b90677a
    /// tsc-span: _tsc.js:25388-25395
    ///
    /// Replace `node`'s synthetic comment lists with `original`'s and clear
    /// `original`'s — a METADATA relocation through the sanctioned arena
    /// surface (the `set_generated_identifier_text` discipline); the sole
    /// production caller is the ES2015 arrow expression-body return
    /// statement (`docs/design/greenfield/slices/h2-5h-b-b-4.md` §12.6).
    #[allow(dead_code)] // the production caller arrives with the B-4 owner
    pub(crate) fn move_synthetic_comments(&mut self, node: TransformNode, original: TransformNode) {
        let (leading, trailing) = {
            let source = self.metadata_mut(original);
            (
                std::mem::take(&mut source.leading_comments),
                std::mem::take(&mut source.trailing_comments),
            )
        };
        let target = self.metadata_mut(node);
        target.leading_comments = leading;
        target.trailing_comments = trailing;
    }

    pub fn node_array(&self, array: TransformNodeArray) -> Result<&NodeArray, TransformError> {
        let source = self.source(array.source)?;
        if array.array.0 < source.source.arena.array_base()
            || array.array.0 >= source.source.arena.array_end()
        {
            return Err(TransformError::UnknownNodeArray(array));
        }
        Ok(source.source.arena.node_array(array.array))
    }

    pub fn node_ref(&self, source: TransformSourceId, node: NodeId) -> Option<TransformNode> {
        self.source(source)
            .ok()
            .filter(|source| source.source.arena.contains_node(node))
            .map(|_| TransformNode { source, node })
    }

    pub fn node_array_ref(
        &self,
        source: TransformSourceId,
        array: NodeArrayId,
    ) -> Option<TransformNodeArray> {
        self.source(source)
            .ok()
            .filter(|source| {
                array.0 >= source.source.arena.array_base()
                    && array.0 < source.source.arena.array_end()
            })
            .map(|_| TransformNodeArray { source, array })
    }

    pub fn transform_flags(&self, node: TransformNode) -> TransformFlags {
        self.node_transform_flags
            .get(&node)
            .copied()
            .unwrap_or(TransformFlags::NONE)
    }

    pub fn set_transform_flags(&mut self, node: TransformNode, flags: TransformFlags) {
        if flags.is_empty() {
            self.node_transform_flags.remove(&node);
        } else {
            self.node_transform_flags.insert(node, flags);
        }
    }

    pub fn array_transform_flags(&self, array: TransformNodeArray) -> TransformFlags {
        self.array_transform_flags
            .get(&array)
            .copied()
            .unwrap_or(TransformFlags::NONE)
    }

    pub fn set_array_transform_flags(&mut self, array: TransformNodeArray, flags: TransformFlags) {
        if flags.is_empty() {
            self.array_transform_flags.remove(&array);
        } else {
            self.array_transform_flags.insert(array, flags);
        }
    }

    pub fn metadata(&self, node: TransformNode) -> Option<&EmitMetadata> {
        self.metadata.get(&node)
    }

    /// Inspect the source-numbered base attached to a generated identifier.
    /// This keeps the opaque binding identity private while allowing factory
    /// consumers to verify `createUniqueName` provenance.
    pub fn generated_binding_base(&self, node: TransformNode) -> Option<&str> {
        self.metadata(node)
            .and_then(EmitMetadata::generated_binding_base)
    }

    pub fn metadata_mut(&mut self, node: TransformNode) -> &mut EmitMetadata {
        self.metadata.entry(node).or_default()
    }

    /// tsc-port: isCallToHelper @6.0.3
    /// tsc-hash: 65c471809533a93e4ad2d44931471cb8a169cf9c93c9b291bc7a7dbdeede8fef
    /// tsc-span: _tsc.js:26566-26568
    pub(crate) fn is_call_to_emit_helper(
        &self,
        expression: TransformNode,
        helper_name: EmitHelperName,
    ) -> Result<bool, TransformError> {
        let NodeData::CallExpression(call) = &self.node(expression)?.data else {
            return Ok(false);
        };
        let Some(callee) = call
            .expression
            .and_then(|callee| self.node_ref(expression.source, callee))
        else {
            return Ok(false);
        };
        let NodeData::Identifier(identifier) = &self.node(callee)?.data else {
            return Ok(false);
        };
        Ok(self
            .metadata(callee)
            .is_some_and(|metadata| metadata.flags().contains(EmitFlags::HELPER_NAME))
            && identifier.escaped_text
                == tsc_syntax::escape_leading_underscores(helper_name.text()))
    }

    pub fn clear_session_metadata(&mut self) {
        self.metadata.clear();
        self.node_transform_flags.clear();
        self.array_transform_flags.clear();
    }

    /// tsc-port: getOriginalNode @6.0.3
    /// tsc-hash: e6e639e966314faf444b9b68796893745ffb06eb0adcf1180d6935332d8797a3
    /// tsc-span: _tsc.js:11400-11410
    pub fn get_original_node(&self, mut node: TransformNode) -> TransformNode {
        let mut remaining = self.metadata.len().saturating_add(1);
        while let Some(original) = self.metadata.get(&node).and_then(EmitMetadata::original) {
            if remaining == 0 || original == node {
                break;
            }
            node = original;
            remaining -= 1;
        }
        node
    }

    /// Return the first immutable parse-tree identity in an original-node
    /// chain. This is deliberately different from `get_original_node`, which
    /// follows the chain to its semantic endpoint: tsc's `getParseTreeNode`
    /// stops as soon as it reaches a parsed node, even when that node itself
    /// carries emit-session metadata.
    ///
    /// tsc-port: getParseTreeNode @6.0.3
    /// tsc-hash: b035994937956a4423ea3e73cac6618339c610416a4740d68e02560b96b3422d
    /// tsc-span: _tsc.js:11423-11435
    pub(crate) fn parse_tree_node(
        &self,
        mut node: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        let mut remaining = self.metadata.len().saturating_add(1);
        loop {
            let source = self.source(node.source())?;
            let record = self.node(node)?;
            let range = SourceRange::from_raw(record.pos, record.end, source.syntax().positions())
                .map_err(|error| TransformError::InvalidSourceRange { node, error })?;
            if source.contains_parsed_node(node.node())
                && !NodeFlags::from_bits(record.flags).contains(NodeFlags::SYNTHESIZED)
                && matches!(range, SourceRange::Original(_))
            {
                return Ok(Some(node));
            }

            let Some(original) = self.metadata.get(&node).and_then(EmitMetadata::original) else {
                return Ok(None);
            };
            if remaining == 0 || original == node {
                return Ok(None);
            }
            node = original;
            remaining -= 1;
        }
    }

    /// tsc-port: setOriginalNode @6.0.3
    /// tsc-hash: 8ef5d40b9635be7af9ec133e0cb89a40498944062d5e9570facb5c3468121129
    /// tsc-span: _tsc.js:25208-25217
    pub fn set_original_node(
        &mut self,
        node: TransformNode,
        original: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        self.node(node)?;
        if let Some(original) = original {
            self.node(original)?;
            // h2-7a-m-3 §4 seam: single-pool original provenance.
            // TypeScript's one node pool permits a reused clone to retain an
            // original from another mounted source. Both handles have already
            // been validated against this arena; an absent (cross-arena)
            // handle still fails above.
        }
        if self.metadata.get(&node).and_then(EmitMetadata::original) == original {
            return Ok(());
        }
        let source_metadata = original.and_then(|original| self.metadata.get(&original).cloned());
        let metadata = self.metadata.entry(node).or_default();
        metadata.original = original;
        if let Some(source_metadata) = source_metadata {
            metadata.merge_from(&source_metadata);
            metadata.original = original;
        }
        Ok(())
    }

    /// Attach only the parse-tree semantic provenance used to project later
    /// resolver queries back into the checker-owned tree.
    ///
    /// Unlike [`Self::set_original_node`], this Rust-specific bridge does not
    /// merge emit metadata and does not change the node's raw text range. It
    /// therefore transfers no comment, synthetic-comment, or source-map range
    /// ownership from `original`; the generated node remains lexically
    /// synthetic.
    pub(crate) fn set_semantic_original_node(
        &mut self,
        node: TransformNode,
        original: TransformNode,
    ) -> Result<(), TransformError> {
        self.node(node)?;
        self.node(original)?;
        if node.source != original.source {
            return Err(TransformError::CrossSourceNode {
                expected: node.source,
                actual: original.source,
            });
        }
        if self.metadata.get(&node).and_then(EmitMetadata::original) == Some(original) {
            return Ok(());
        }
        self.metadata.entry(node).or_default().original = Some(original);
        Ok(())
    }

    /// tsc-port: propagateChildFlags @6.0.3
    /// tsc-hash: 8ddb64c96b023e53f3d136865d331f4ff32cc68182cf51faa166e2023be5abb0
    /// tsc-span: _tsc.js:25110-25114
    pub fn propagate_child_flags(
        &self,
        child: TransformNode,
    ) -> Result<TransformFlags, TransformError> {
        let child_node = self.node(child)?;
        let child_flags =
            self.transform_flags(child) & !TransformFlags::subtree_exclusions(child_node.kind);
        let Some(name) = named_declaration_name(child_node) else {
            return Ok(child_flags);
        };
        let Some(name) = self.node_ref(child.source, name) else {
            return Ok(child_flags);
        };
        if !is_property_name(self.node(name)?.kind) {
            return Ok(child_flags);
        }
        Ok(child_flags
            | (self.transform_flags(name) & TransformFlags::PROPERTY_NAME_PROPAGATING_FLAGS))
    }

    /// The EA-GAP-FLAGS postorder classifier: the transform flags a freshly
    /// synthesized node carries, computed from tsc's factory creation
    /// tables — field-aware child aggregation through
    /// [`Self::propagate_child_flags`] (name fields drop the identifier's
    /// possible-top-level-await bit, function-like bodies drop it wholesale)
    /// plus the per-created-kind facet additions. Kinds outside the table
    /// carry pure child aggregation, exactly like their upstream creators.
    ///
    /// `declared_flags` is the NodeFlags word the caller will publish on the
    /// created node (`Let`/`Const` discriminate the variable-list facets).
    /// The private-identifier-in-expression facet is deliberately absent:
    /// [`private_identifier_expression_flags`] already adds it inside
    /// `create_node`. Owner-called creators whose tsc bodies add only bits
    /// outside this port's facet lattice inherit the aggregation fallback.
    /// tsc-port: createIdentifier @6.0.3
    /// tsc-hash: dd1baeac5d32597682b2f4f1acf9729f109bc958a82c19a446911a4bc94e709d
    /// tsc-span: _tsc.js:21609-21625
    /// tsc-port: createBinaryExpression @6.0.3
    /// tsc-hash: cb3f974e1006db9345bc4a2144f1ee6daae9ac0241cb0f6bffd6e476b3db09cb
    /// tsc-span: _tsc.js:22785-22811
    /// tsc-port: createVariableDeclarationList @6.0.3
    /// tsc-hash: 324d5fd5f464f3047fb5d2b3c5761cd3a469334262e88077a32c862a5c1051d8
    /// tsc-span: _tsc.js:23287-23299
    /// tsc-port: createFunctionExpression @6.0.3
    /// tsc-hash: 6bb6a6e8f55a98f4b9aac262fd2462253655a1dcc80052f7e29afe280998790f
    /// tsc-span: _tsc.js:22676-22697
    /// tsc-port: createArrowFunction @6.0.3
    /// tsc-hash: 86ab9adbb9da5a28bf8a8dad687363b83f315bfa06def479590c1f305a367d72
    /// tsc-span: _tsc.js:22701-22719
    /// tsc-port: createYieldExpression @6.0.3
    /// tsc-hash: 5b8b52de29c67a91327401c6ab7f3a3d71d4c150fe95ef171af7d64087ea2a5f
    /// tsc-span: _tsc.js:22907-22914
    /// tsc-port: createSpreadElement @6.0.3
    /// tsc-hash: c8947fee51b004c691da2d44736f0f207f4c7c48c5be9a7b0e01ceea7d0d9a43
    /// tsc-span: _tsc.js:22918-22923
    /// tsc-port: createObjectBindingPattern @6.0.3
    /// tsc-hash: 23d7a5579cd4dfaa4635b01de2c532bbaeabc4068a99d8ababa9f708fc61c827
    /// tsc-span: _tsc.js:22407-22415
    /// tsc-port: createBindingElement @6.0.3
    /// tsc-hash: 53d5a02d4d99e09ae5b8f52777e56e6bdb06131543f763f573cf767b5735b7df
    /// tsc-span: _tsc.js:22428-22437
    /// tsc-port: createForOfStatement @6.0.3
    /// tsc-hash: 7d1c85363e35295cece0d7cd397221ccc26f8142cd9971516ee6e943186cedd1
    /// tsc-span: _tsc.js:23157-23170
    /// tsc-port: createMethodDeclaration @6.0.3
    /// tsc-hash: ab2bb8f981f84e6971291e817bb111793868f0254f394570ed3056fc5bc28544
    /// tsc-span: _tsc.js:21924-21951
    /// tsc-port: createConstructorDeclaration @6.0.3
    /// tsc-hash: c5eefe07225bf0585ce38867bc66134c557c7da79684697fa32e2cc899276163
    /// tsc-span: _tsc.js:21982-22001
    /// tsc-port: createParameterDeclaration @6.0.3
    /// tsc-hash: 31cde0f942a7460844092dcd2b95e2e97bafbcd5a2dcc6f4f5e0a1b7f5f11ef5
    /// tsc-span: _tsc.js:21838-21853
    #[allow(dead_code)] // consumers arrive with the B-3/B-4 owners
    pub(crate) fn classify_created_node_flags(
        &self,
        node: TransformNode,
        declared_flags: NodeFlags,
    ) -> Result<TransformFlags, TransformError> {
        use tsc_syntax::observable_fields::{for_each_observable_field, ObservableField};

        let record = self.node(node)?;
        let kind = record.kind;
        let mut node_fields: Vec<(&'static str, NodeId)> = Vec::new();
        let mut array_fields: Vec<(&'static str, NodeArrayId)> = Vec::new();
        for_each_observable_field(record, |field, value| match value {
            ObservableField::Node(id) => node_fields.push((field, id)),
            ObservableField::NodeArray(id) => array_fields.push((field, id)),
            ObservableField::Bool(_) | ObservableField::String(_) => {}
        });

        let function_like = matches!(
            kind,
            SyntaxKind::ArrowFunction
                | SyntaxKind::FunctionExpression
                | SyntaxKind::FunctionDeclaration
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::Constructor
                | SyntaxKind::GetAccessor
                | SyntaxKind::SetAccessor
        );
        let child_of = |id: NodeId| TransformNode {
            source: node.source,
            node: id,
        };
        let kind_of = |id: NodeId| -> Result<SyntaxKind, TransformError> {
            let child = child_of(id);
            Ok(self.node(child)?.kind)
        };

        let mut flags = TransformFlags::NONE;
        for (field, id) in &node_fields {
            let mut child_flags = self.propagate_child_flags(child_of(*id))?;
            if matches!(*field, "name" | "propertyName") && kind_of(*id)? == SyntaxKind::Identifier
            {
                child_flags = child_flags & !TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT;
            }
            if function_like && *field == "body" {
                child_flags = child_flags & !TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT;
            }
            flags |= child_flags;
        }
        for (_, id) in &array_fields {
            flags |= self.array_transform_flags(TransformNodeArray {
                source: node.source,
                array: *id,
            });
        }

        let has_node_field = |name: &str| node_fields.iter().any(|(field, _)| *field == name);
        let modifiers_include = |wanted: SyntaxKind| -> Result<bool, TransformError> {
            let Some((_, array)) = array_fields.iter().find(|(field, _)| *field == "modifiers")
            else {
                return Ok(false);
            };
            let elements = self
                .node_array(TransformNodeArray {
                    source: node.source,
                    array: *array,
                })?
                .nodes
                .clone();
            for element in elements {
                if kind_of(element)? == wanted {
                    return Ok(true);
                }
            }
            Ok(false)
        };
        let is_super_keyword = |id: Option<NodeId>| -> Result<bool, TransformError> {
            Ok(match id {
                Some(id) => kind_of(id)? == SyntaxKind::SuperKeyword,
                None => false,
            })
        };
        let is_super_property = |id: Option<NodeId>| -> Result<bool, TransformError> {
            let Some(id) = id else { return Ok(false) };
            if !matches!(
                kind_of(id)?,
                SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
            ) {
                return Ok(false);
            }
            let inner = match &self.node(child_of(id))?.data {
                NodeData::PropertyAccessExpression(data) => data.expression,
                NodeData::ElementAccessExpression(data) => data.expression,
                _ => None,
            };
            is_super_keyword(inner)
        };
        let function_facets = |is_async: bool, is_generator: bool| {
            if is_async && is_generator {
                TransformFlags::CONTAINS_ES_2018
            } else if is_async {
                TransformFlags::CONTAINS_ES_2017
            } else if is_generator {
                TransformFlags::CONTAINS_GENERATOR
            } else {
                TransformFlags::NONE
            }
        };

        let additions = match &record.data {
            NodeData::Identifier(data) => {
                // createIdentifier rows: the "await" facet plus the
                // extended-unicode-escape ES2015 facet
                // (`_tsc.js:21618-21623`; NodeFlags 256).
                let mut flags = if data.escaped_text == "await" {
                    TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT
                } else {
                    TransformFlags::NONE
                };
                if declared_flags.contains(NodeFlags::IDENTIFIER_HAS_EXTENDED_UNICODE_ESCAPE) {
                    flags |= TransformFlags::CONTAINS_ES_2015;
                }
                flags
            }
            NodeData::MetaProperty(data) => {
                // createMetaProperty keyword rows (`_tsc.js:23009-23026`):
                // `new.target` is ES2015; `import.meta` is ES2020.
                match data.keyword_token {
                    SyntaxKind::NewKeyword => TransformFlags::CONTAINS_ES_2015,
                    SyntaxKind::ImportKeyword => TransformFlags::CONTAINS_ES_2020,
                    _ => TransformFlags::NONE,
                }
            }
            NodeData::NumericLiteral(data) => {
                if data.text.starts_with("0b")
                    || data.text.starts_with("0B")
                    || data.text.starts_with("0o")
                    || data.text.starts_with("0O")
                {
                    TransformFlags::CONTAINS_ES_2015
                } else {
                    TransformFlags::NONE
                }
            }
            NodeData::StringLiteral(data) => {
                if data.has_extended_unicode_escape == Some(true) {
                    TransformFlags::CONTAINS_ES_2015
                } else {
                    TransformFlags::NONE
                }
            }
            NodeData::PropertyAccessExpression(data) => {
                if is_super_keyword(data.expression)? {
                    TransformFlags::CONTAINS_ES_2017 | TransformFlags::CONTAINS_ES_2018
                } else {
                    TransformFlags::NONE
                }
            }
            NodeData::ElementAccessExpression(data) => {
                if is_super_keyword(data.expression)? {
                    TransformFlags::CONTAINS_ES_2017 | TransformFlags::CONTAINS_ES_2018
                } else {
                    TransformFlags::NONE
                }
            }
            NodeData::CallExpression(data) => {
                let mut extra = TransformFlags::NONE;
                if is_super_property(data.expression)? {
                    extra |= TransformFlags::CONTAINS_LEXICAL_THIS;
                }
                if let Some(expression) = data.expression {
                    if kind_of(expression)? == SyntaxKind::ImportKeyword {
                        extra |= TransformFlags::CONTAINS_DYNAMIC_IMPORT;
                    }
                }
                extra
            }
            NodeData::NewExpression(_) => TransformFlags::CONTAINS_ES_2020,
            NodeData::TaggedTemplateExpression(data) => {
                let mut extra = TransformFlags::CONTAINS_ES_2015;
                if data.question_dot_token.is_some() {
                    extra |= TransformFlags::CONTAINS_ES_2018;
                }
                extra
            }
            NodeData::TemplateExpression(_) => TransformFlags::CONTAINS_ES_2015,
            NodeData::ComputedPropertyName(_) => {
                TransformFlags::CONTAINS_ES_2015 | TransformFlags::CONTAINS_COMPUTED_PROPERTY_NAME
            }
            NodeData::BinaryExpression(data) => {
                let operator = match data.operator_token {
                    Some(token) => kind_of(token)?,
                    None => SyntaxKind::Unknown,
                };
                let left_kind = match data.left {
                    Some(left) => kind_of(left)?,
                    None => SyntaxKind::Unknown,
                };
                let assignment_pattern_flags =
                    |left: Option<NodeId>| -> Result<TransformFlags, TransformError> {
                        let Some(left) = left else {
                            return Ok(TransformFlags::NONE);
                        };
                        Ok(
                            if self
                                .transform_flags(child_of(left))
                                .contains(TransformFlags::CONTAINS_OBJECT_REST_OR_SPREAD)
                            {
                                TransformFlags::CONTAINS_OBJECT_REST_OR_SPREAD
                            } else {
                                TransformFlags::NONE
                            },
                        )
                    };
                match operator {
                    SyntaxKind::QuestionQuestionToken => TransformFlags::CONTAINS_ES_2020,
                    SyntaxKind::EqualsToken if left_kind == SyntaxKind::ObjectLiteralExpression => {
                        TransformFlags::CONTAINS_ES_2015
                            | TransformFlags::CONTAINS_ES_2018
                            | TransformFlags::CONTAINS_DESTRUCTURING_ASSIGNMENT
                            | assignment_pattern_flags(data.left)?
                    }
                    SyntaxKind::EqualsToken if left_kind == SyntaxKind::ArrayLiteralExpression => {
                        TransformFlags::CONTAINS_ES_2015
                            | TransformFlags::CONTAINS_DESTRUCTURING_ASSIGNMENT
                            | assignment_pattern_flags(data.left)?
                    }
                    SyntaxKind::AsteriskAsteriskToken | SyntaxKind::AsteriskAsteriskEqualsToken => {
                        TransformFlags::CONTAINS_ES_2016
                    }
                    SyntaxKind::BarBarEqualsToken
                    | SyntaxKind::AmpersandAmpersandEqualsToken
                    | SyntaxKind::QuestionQuestionEqualsToken => TransformFlags::CONTAINS_ES_2021,
                    _ => TransformFlags::NONE,
                }
            }
            NodeData::PrefixUnaryExpression(data) => {
                if matches!(
                    data.operator,
                    SyntaxKind::PlusPlusToken | SyntaxKind::MinusMinusToken
                ) && data
                    .operand
                    .map(kind_of)
                    .transpose()?
                    .is_some_and(|kind| kind == SyntaxKind::Identifier)
                {
                    TransformFlags::CONTAINS_UPDATE_EXPRESSION_FOR_IDENTIFIER
                } else {
                    TransformFlags::NONE
                }
            }
            NodeData::PostfixUnaryExpression(data) => {
                if data
                    .operand
                    .map(kind_of)
                    .transpose()?
                    .is_some_and(|kind| kind == SyntaxKind::Identifier)
                {
                    TransformFlags::CONTAINS_UPDATE_EXPRESSION_FOR_IDENTIFIER
                } else {
                    TransformFlags::NONE
                }
            }
            NodeData::AwaitExpression(_) => {
                TransformFlags::CONTAINS_ES_2017
                    | TransformFlags::CONTAINS_ES_2018
                    | TransformFlags::CONTAINS_AWAIT
            }
            NodeData::YieldExpression(_) => {
                TransformFlags::CONTAINS_ES_2015
                    | TransformFlags::CONTAINS_ES_2018
                    | TransformFlags::CONTAINS_YIELD
            }
            NodeData::SpreadElement(_) => {
                TransformFlags::CONTAINS_ES_2015 | TransformFlags::CONTAINS_REST_OR_SPREAD
            }
            NodeData::SpreadAssignment(_) => {
                TransformFlags::CONTAINS_ES_2018 | TransformFlags::CONTAINS_OBJECT_REST_OR_SPREAD
            }
            NodeData::ShorthandPropertyAssignment(_) => TransformFlags::CONTAINS_ES_2015,
            NodeData::ObjectBindingPattern(_) => {
                let mut extra =
                    TransformFlags::CONTAINS_ES_2015 | TransformFlags::CONTAINS_BINDING_PATTERN;
                if flags.contains(TransformFlags::CONTAINS_REST_OR_SPREAD) {
                    extra |= TransformFlags::CONTAINS_ES_2018
                        | TransformFlags::CONTAINS_OBJECT_REST_OR_SPREAD;
                }
                extra
            }
            NodeData::ArrayBindingPattern(_) => {
                TransformFlags::CONTAINS_ES_2015 | TransformFlags::CONTAINS_BINDING_PATTERN
            }
            NodeData::BindingElement(data) => {
                let mut extra = TransformFlags::CONTAINS_ES_2015;
                if data.dot_dot_dot_token.is_some() {
                    extra |= TransformFlags::CONTAINS_REST_OR_SPREAD;
                }
                extra
            }
            NodeData::Parameter(data) => {
                if data.dot_dot_dot_token.is_some() || data.initializer.is_some() {
                    TransformFlags::CONTAINS_ES_2015
                } else {
                    TransformFlags::NONE
                }
            }
            NodeData::VariableDeclarationList(_) => {
                let mut extra = TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION;
                if declared_flags.intersects(NodeFlags::BLOCK_SCOPED) {
                    extra |= TransformFlags::CONTAINS_ES_2015
                        | TransformFlags::CONTAINS_BLOCK_SCOPED_BINDING;
                }
                if declared_flags.intersects(NodeFlags::USING) {
                    extra |= TransformFlags::CONTAINS_ES_NEXT;
                }
                extra
            }
            NodeData::ReturnStatement(_) => {
                TransformFlags::CONTAINS_ES_2018
                    | TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION
            }
            NodeData::BreakStatement(_) | NodeData::ContinueStatement(_) => {
                TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION
            }
            NodeData::CatchClause(data) => {
                if data.variable_declaration.is_none() {
                    TransformFlags::CONTAINS_ES_2019
                } else {
                    TransformFlags::NONE
                }
            }
            NodeData::ForOfStatement(data) => {
                let mut extra = TransformFlags::CONTAINS_ES_2015;
                if data.await_modifier.is_some() {
                    extra |= TransformFlags::CONTAINS_ES_2018;
                }
                extra
            }
            NodeData::ClassExpression(_) | NodeData::ClassDeclaration(_) => {
                TransformFlags::CONTAINS_ES_2015
            }
            NodeData::ArrowFunction(_) => {
                let mut extra = TransformFlags::CONTAINS_ES_2015;
                if modifiers_include(SyntaxKind::AsyncKeyword)? {
                    extra |=
                        TransformFlags::CONTAINS_ES_2017 | TransformFlags::CONTAINS_LEXICAL_THIS;
                }
                extra
            }
            NodeData::FunctionExpression(_) | NodeData::FunctionDeclaration(_) => {
                let is_async = modifiers_include(SyntaxKind::AsyncKeyword)?;
                let is_generator = has_node_field("asteriskToken");
                function_facets(is_async, is_generator)
                    | TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION
            }
            NodeData::MethodDeclaration(_) => {
                let is_async = modifiers_include(SyntaxKind::AsyncKeyword)?;
                let is_generator = has_node_field("asteriskToken");
                function_facets(is_async, is_generator) | TransformFlags::CONTAINS_ES_2015
            }
            NodeData::Constructor(_) => TransformFlags::CONTAINS_ES_2015,
            NodeData::GetAccessor(_) | NodeData::SetAccessor(_) => TransformFlags::NONE,
            _ => TransformFlags::NONE,
        };
        Ok(flags | additions)
    }
}

/// tsc-port: createToken @6.0.3
/// tsc-hash: c78e317d4226871f44d628bcae0862e3c7c7a3b4d67c798042153e85455dc041
/// tsc-span: _tsc.js:21710-21766
///
/// The per-kind facet a freshly created bare token carries (the
/// EA-GAP-FLAGS token half). Kinds outside tsc's switch carry none.
#[allow(dead_code)] // consumers arrive with the B-3/B-4 owners
pub(crate) const fn classify_created_token_flags(kind: SyntaxKind) -> TransformFlags {
    match kind {
        SyntaxKind::AsyncKeyword => TransformFlags::from_bits(
            TransformFlags::CONTAINS_ES_2017.bits() | TransformFlags::CONTAINS_ES_2018.bits(),
        ),
        SyntaxKind::UsingKeyword => TransformFlags::CONTAINS_ES_NEXT,
        SyntaxKind::PublicKeyword
        | SyntaxKind::PrivateKeyword
        | SyntaxKind::ProtectedKeyword
        | SyntaxKind::ReadonlyKeyword
        | SyntaxKind::AbstractKeyword
        | SyntaxKind::DeclareKeyword
        | SyntaxKind::ConstKeyword
        | SyntaxKind::AnyKeyword
        | SyntaxKind::NumberKeyword
        | SyntaxKind::BigIntKeyword
        | SyntaxKind::NeverKeyword
        | SyntaxKind::ObjectKeyword
        | SyntaxKind::InKeyword
        | SyntaxKind::OutKeyword
        | SyntaxKind::OverrideKeyword
        | SyntaxKind::StringKeyword
        | SyntaxKind::BooleanKeyword
        | SyntaxKind::SymbolKeyword
        | SyntaxKind::VoidKeyword
        | SyntaxKind::UnknownKeyword
        | SyntaxKind::UndefinedKeyword => TransformFlags::CONTAINS_TYPE_SCRIPT,
        SyntaxKind::SuperKeyword => TransformFlags::from_bits(
            TransformFlags::CONTAINS_ES_2015.bits() | TransformFlags::CONTAINS_LEXICAL_SUPER.bits(),
        ),
        SyntaxKind::StaticKeyword => TransformFlags::CONTAINS_ES_2015,
        SyntaxKind::AccessorKeyword => TransformFlags::CONTAINS_CLASS_FIELDS,
        SyntaxKind::ThisKeyword => TransformFlags::CONTAINS_LEXICAL_THIS,
        _ => TransformFlags::NONE,
    }
}

fn named_declaration_name(node: &Node) -> Option<NodeId> {
    let mut name = None;
    for_each_observable_field(node, |field, value| {
        if field == "name" {
            if let ObservableField::Node(node) = value {
                name = Some(node);
            }
        }
    });
    name
}

const fn is_property_name(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Identifier
            | SyntaxKind::PrivateIdentifier
            | SyntaxKind::StringLiteral
            | SyntaxKind::NumericLiteral
            | SyntaxKind::ComputedPropertyName
    )
}

/// Local transform flag produced by expression forms that use a private name
/// at runtime. A `PrivateIdentifier` is deliberately not flagged by itself:
/// the same syntax node also names private declarations, and those names do
/// not require the legacy-decorator helper to remain in the class body.
/// `ElementAccessExpression` deliberately does not participate: tsc produces
/// this fact only for private property access and the `#name in value` form.
///
/// tsc-port: createBasePropertyAccessExpression @6.0.3
/// tsc-hash: 62c0288e0c2c8af918be4f39ac85edfdbc62c8e545366f7d6143c15f147b4e1b
/// tsc-span: _tsc.js:22464-22470
/// tsc-port: createBinaryExpression @6.0.3
/// tsc-hash: dca056c920de60b7104debd82eb40887c1c51cbed8e4ad5cca24124dd24460d9
/// tsc-span: _tsc.js:22785-22808
pub(crate) fn private_identifier_expression_flags(
    arena: &TransformArena,
    source: TransformSourceId,
    data: &NodeData,
) -> Result<TransformFlags, TransformError> {
    let kind_of = |id: Option<NodeId>| -> Result<Option<SyntaxKind>, TransformError> {
        let Some(id) = id else {
            return Ok(None);
        };
        let node = arena
            .node_ref(source, id)
            .ok_or_else(|| TransformError::UnknownNode(TransformNode::new(source, id)))?;
        Ok(Some(arena.node(node)?.kind))
    };
    let contains_private = match data {
        NodeData::PropertyAccessExpression(data) => {
            kind_of(data.name)? == Some(SyntaxKind::PrivateIdentifier)
        }
        NodeData::BinaryExpression(data) => {
            kind_of(data.operator_token)? == Some(SyntaxKind::InKeyword)
                && kind_of(data.left)? == Some(SyntaxKind::PrivateIdentifier)
        }
        _ => false,
    };
    Ok(if contains_private {
        TransformFlags::CONTAINS_PRIVATE_IDENTIFIER_IN_EXPRESSION
    } else {
        TransformFlags::NONE
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Associativity {
    Left,
    Right,
}

const PRECEDENCE_INVALID: i8 = -1;
const PRECEDENCE_COMMA: i8 = 0;
const PRECEDENCE_SPREAD: i8 = 1;
const PRECEDENCE_YIELD: i8 = 2;
const PRECEDENCE_ASSIGNMENT: i8 = 3;
const PRECEDENCE_CONDITIONAL: i8 = 4;
const PRECEDENCE_RELATIONAL: i8 = 11;
const PRECEDENCE_UNARY: i8 = 16;
const PRECEDENCE_UPDATE: i8 = 17;
const PRECEDENCE_LEFT_HAND_SIDE: i8 = 18;
const PRECEDENCE_MEMBER: i8 = 19;
const PRECEDENCE_PRIMARY: i8 = 20;

/// Optional facets accepted by `createUniqueName`. The low three kind bits
/// are factory-owned and therefore intentionally unavailable to callers.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GeneratedIdentifierFlags(u8);

impl GeneratedIdentifierFlags {
    pub const NONE: Self = Self(0);
    pub const RESERVED_IN_NESTED_SCOPES: Self = Self(8);
    pub const OPTIMISTIC: Self = Self(16);
    pub const FILE_LEVEL: Self = Self(32);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for GeneratedIdentifierFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

/// The type-only half of TypeScript's `createParenthesizerRules`.
///
/// Every decision is kind-driven except the two upstream structural probes:
/// JSDoc postfix-question propagation and the leading generic function-type
/// argument check. Wrappers are always fresh `ParenthesizedType` nodes.
pub struct TypeParenthesizer;

impl TypeParenthesizer {
    fn kind(factory: &NodeFactory<'_>, node: TransformNode) -> Result<SyntaxKind, TransformError> {
        Ok(factory.arena.node(node)?.kind)
    }

    /// tsc-port: parenthesizeCheckTypeOfConditionalType @6.0.3
    /// tsc-hash: 157ce0811dcc1565701b7dfb866c4327d16c319b8b93e7e3f98d5fc45599f4ae
    /// tsc-span: _tsc.js:20524-20532
    pub fn parenthesize_check_type_of_conditional_type(
        factory: &mut NodeFactory<'_>,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if matches!(
            Self::kind(factory, node)?,
            SyntaxKind::FunctionType | SyntaxKind::ConstructorType | SyntaxKind::ConditionalType
        ) {
            factory.create_parenthesized_type(node.source, node)
        } else {
            Ok(node)
        }
    }

    /// tsc-port: parenthesizeExtendsTypeOfConditionalType @6.0.3
    /// tsc-hash: df215134e50a89e2f66339a064ea5077520521a0c51c236140f849e090c76eaa
    /// tsc-span: _tsc.js:20533-20539
    pub fn parenthesize_extends_type_of_conditional_type(
        factory: &mut NodeFactory<'_>,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if Self::kind(factory, node)? == SyntaxKind::ConditionalType {
            factory.create_parenthesized_type(node.source, node)
        } else {
            Ok(node)
        }
    }

    /// tsc-port: parenthesizeConstituentTypeOfUnionType @6.0.3
    /// tsc-hash: 6a071b4a7c2eebb30005580cc9d725278da358dddc6e0a5a2543d51c9b33f0c3
    /// tsc-span: _tsc.js:20540-20548
    pub fn parenthesize_constituent_type_of_union_type(
        factory: &mut NodeFactory<'_>,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if matches!(
            Self::kind(factory, node)?,
            SyntaxKind::UnionType | SyntaxKind::IntersectionType
        ) {
            factory.create_parenthesized_type(node.source, node)
        } else {
            Self::parenthesize_check_type_of_conditional_type(factory, node)
        }
    }

    /// tsc-port: parenthesizeConstituentTypeOfIntersectionType @6.0.3
    /// tsc-hash: 16e074b57e8f85b242dacff120927e95da3283869f2623e6a1f48e715bddb0b4
    /// tsc-span: _tsc.js:20552-20560
    pub fn parenthesize_constituent_type_of_intersection_type(
        factory: &mut NodeFactory<'_>,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if matches!(
            Self::kind(factory, node)?,
            SyntaxKind::UnionType | SyntaxKind::IntersectionType
        ) {
            factory.create_parenthesized_type(node.source, node)
        } else {
            Self::parenthesize_constituent_type_of_union_type(factory, node)
        }
    }

    /// tsc-port: parenthesizeOperandOfTypeOperator @6.0.3
    /// tsc-hash: 63a1c7a5a630f81ce3a1e2fe93f2bdf4091c7efab7a5eaa7a7b763c2c1bfdbb6
    /// tsc-span: _tsc.js:20564-20570
    pub fn parenthesize_operand_of_type_operator(
        factory: &mut NodeFactory<'_>,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if Self::kind(factory, node)? == SyntaxKind::IntersectionType {
            factory.create_parenthesized_type(node.source, node)
        } else {
            Self::parenthesize_constituent_type_of_intersection_type(factory, node)
        }
    }

    /// tsc-port: parenthesizeOperandOfReadonlyTypeOperator @6.0.3
    /// tsc-hash: 698f5d1853244b3042270eeaf1390ffc39a580368bedce9d8189a4256eb5a59b
    /// tsc-span: _tsc.js:20571-20577
    pub fn parenthesize_operand_of_readonly_type_operator(
        factory: &mut NodeFactory<'_>,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if Self::kind(factory, node)? == SyntaxKind::TypeOperator {
            factory.create_parenthesized_type(node.source, node)
        } else {
            Self::parenthesize_operand_of_type_operator(factory, node)
        }
    }

    /// tsc-port: parenthesizeNonArrayTypeOfPostfixType @6.0.3
    /// tsc-hash: bfea293b8c62f6bf0135f40014a1398503089f618d07dd609b26ca786155ece9
    /// tsc-span: _tsc.js:20578-20587
    pub fn parenthesize_non_array_type_of_postfix_type(
        factory: &mut NodeFactory<'_>,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if matches!(
            Self::kind(factory, node)?,
            SyntaxKind::InferType | SyntaxKind::TypeOperator | SyntaxKind::TypeQuery
        ) {
            factory.create_parenthesized_type(node.source, node)
        } else {
            Self::parenthesize_operand_of_type_operator(factory, node)
        }
    }

    fn has_jsdoc_postfix_question(
        factory: &NodeFactory<'_>,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        let source = node.source;
        let child = |id: Option<NodeId>| id.and_then(|id| factory.arena.node_ref(source, id));
        let data = &factory.arena.node(node)?.data;
        let nested = match data {
            NodeData::JSDocNullableType(data) => return Ok(data.postfix),
            NodeData::NamedTupleMember(data) => child(data.r#type),
            NodeData::FunctionType(data) => child(data.r#type),
            NodeData::ConstructorType(data) => child(data.r#type),
            NodeData::TypeOperator(data) => child(data.r#type),
            NodeData::ConditionalType(data) => child(data.false_type),
            NodeData::UnionType(data) => factory
                .array_node_handles(source, data.types)?
                .and_then(|nodes| nodes.last().copied()),
            NodeData::IntersectionType(data) => factory
                .array_node_handles(source, data.types)?
                .and_then(|nodes| nodes.last().copied()),
            NodeData::InferType(data) => child(data.type_parameter).and_then(|parameter| {
                let NodeData::TypeParameter(data) = &factory.arena.node(parameter).ok()?.data
                else {
                    return None;
                };
                child(data.constraint)
            }),
            _ => None,
        };
        match nested {
            Some(nested) => Self::has_jsdoc_postfix_question(factory, nested),
            None => Ok(false),
        }
    }

    /// tsc-port: parenthesizeElementTypeOfTupleType @6.0.3
    /// tsc-hash: 0f276639c36bda09e21f1daa7c900c734ce7b21fdf7ccb2ee934a73a7ccd0393
    /// tsc-span: _tsc.js:20591-20594
    pub fn parenthesize_element_type_of_tuple_type(
        factory: &mut NodeFactory<'_>,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if Self::has_jsdoc_postfix_question(factory, node)? {
            factory.create_parenthesized_type(node.source, node)
        } else {
            Ok(node)
        }
    }

    /// tsc-port: parenthesizeTypeOfOptionalType @6.0.3
    /// tsc-hash: 1b37b2c079682c48a5d78dc5432fc7966478742bda22faf5f8872132a29a0c04
    /// tsc-span: _tsc.js:20602-20606
    pub fn parenthesize_type_of_optional_type(
        factory: &mut NodeFactory<'_>,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if Self::has_jsdoc_postfix_question(factory, node)? {
            factory.create_parenthesized_type(node.source, node)
        } else {
            Self::parenthesize_non_array_type_of_postfix_type(factory, node)
        }
    }

    /// tsc-port: parenthesizeLeadingTypeArgument @6.0.3
    /// tsc-hash: ddfd02d218841742dbccc0eab0bbb2245c34fa8339524d0ce413566b3babe969
    /// tsc-span: _tsc.js:20607-20609
    pub fn parenthesize_leading_type_argument(
        factory: &mut NodeFactory<'_>,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let has_type_parameters = match &factory.arena.node(node)?.data {
            NodeData::FunctionType(data) => data.type_parameters.is_some(),
            NodeData::ConstructorType(data) => data.type_parameters.is_some(),
            _ => false,
        };
        if has_type_parameters {
            factory.create_parenthesized_type(node.source, node)
        } else {
            Ok(node)
        }
    }

    fn map_array(
        factory: &mut NodeFactory<'_>,
        array: TransformNodeArray,
        mut rule: impl FnMut(
            &mut NodeFactory<'_>,
            TransformNode,
        ) -> Result<TransformNode, TransformError>,
    ) -> Result<TransformNodeArray, TransformError> {
        let original = factory.arena.node_array(array)?.clone();
        let mut changed = false;
        let mut nodes = Vec::with_capacity(original.nodes.len());
        for id in original.nodes {
            let node = TransformNode::new(array.source, id);
            let mapped = rule(factory, node)?;
            changed |= mapped != node;
            nodes.push(mapped);
        }
        if changed {
            factory.create_node_array(array.source, nodes)
        } else {
            Ok(array)
        }
    }

    /// tsc-port: parenthesizeConstituentTypesOfUnionType @6.0.3
    /// tsc-hash: 4d1b0a46b52a261b197eafbcf44f75154fb99e165aa7ca54631ffe2ef9c43277
    /// tsc-span: _tsc.js:20549-20551
    pub fn parenthesize_constituent_types_of_union_type(
        factory: &mut NodeFactory<'_>,
        array: TransformNodeArray,
    ) -> Result<TransformNodeArray, TransformError> {
        Self::map_array(
            factory,
            array,
            Self::parenthesize_constituent_type_of_union_type,
        )
    }

    /// tsc-port: parenthesizeConstituentTypesOfIntersectionType @6.0.3
    /// tsc-hash: 028652b3bf941fb7111a8ff5d194115cfd2321cd32da24cb0a6cdbda4644e2c4
    /// tsc-span: _tsc.js:20561-20563
    pub fn parenthesize_constituent_types_of_intersection_type(
        factory: &mut NodeFactory<'_>,
        array: TransformNodeArray,
    ) -> Result<TransformNodeArray, TransformError> {
        Self::map_array(
            factory,
            array,
            Self::parenthesize_constituent_type_of_intersection_type,
        )
    }

    /// tsc-port: parenthesizeElementTypesOfTupleType @6.0.3
    /// tsc-hash: 75c7d31e894ea6dc5226fb2cae08f967660ff8309f4c465570b2e3a4f1d0b9b1
    /// tsc-span: _tsc.js:20588-20590
    pub fn parenthesize_element_types_of_tuple_type(
        factory: &mut NodeFactory<'_>,
        array: TransformNodeArray,
    ) -> Result<TransformNodeArray, TransformError> {
        Self::map_array(
            factory,
            array,
            Self::parenthesize_element_type_of_tuple_type,
        )
    }

    /// tsc-port: parenthesizeTypeArguments/parenthesizeOrdinalTypeArgument @6.0.3
    /// tsc-hash: ea57c54b8172ed67a3011e7c347d1dc919bd7c2cdb730579be2325973a9f8080
    /// tsc-span: _tsc.js:20610-20617
    pub fn parenthesize_type_arguments(
        factory: &mut NodeFactory<'_>,
        array: Option<TransformNodeArray>,
    ) -> Result<Option<TransformNodeArray>, TransformError> {
        let Some(array) = array else { return Ok(None) };
        if factory.arena.node_array(array)?.nodes.is_empty() {
            return Ok(None);
        }
        let mut index = 0usize;
        Self::map_array(factory, array, |factory, node| {
            let result = if index == 0 {
                Self::parenthesize_leading_type_argument(factory, node)
            } else {
                Ok(node)
            };
            index += 1;
            result
        })
        .map(Some)
    }
}

/// Synthetic-node constructor scoped to one mutable transform arena.
pub struct NodeFactory<'arena> {
    arena: &'arena mut TransformArena,
}

impl<'arena> NodeFactory<'arena> {
    /// Immutable view of the owning arena, for post-creation queries such
    /// as the EA-GAP-FLAGS classifier.
    #[allow(dead_code)] // production consumers arrive with the B-3/B-4 owners
    pub(crate) fn arena(&self) -> &TransformArena {
        self.arena
    }

    /// Mutable view of the owning arena, for publishing classified flags.
    #[allow(dead_code)] // production consumers arrive with the B-3/B-4 owners
    pub(crate) fn arena_mut(&mut self) -> &mut TransformArena {
        self.arena
    }

    pub(crate) fn new(arena: &'arena mut TransformArena) -> Self {
        Self { arena }
    }

    fn node_id(
        &self,
        source: TransformSourceId,
        node: TransformNode,
    ) -> Result<NodeId, TransformError> {
        if node.source != source {
            return Err(TransformError::CrossSourceNode {
                expected: source,
                actual: node.source,
            });
        }
        self.arena.node(node)?;
        Ok(node.node)
    }

    fn optional_node_id(
        &self,
        source: TransformSourceId,
        node: Option<TransformNode>,
    ) -> Result<Option<NodeId>, TransformError> {
        node.map(|node| self.node_id(source, node)).transpose()
    }

    fn array_id(
        &self,
        source: TransformSourceId,
        array: TransformNodeArray,
    ) -> Result<NodeArrayId, TransformError> {
        if array.source != source {
            return Err(TransformError::CrossSourceNode {
                expected: source,
                actual: array.source,
            });
        }
        self.arena.node_array(array)?;
        Ok(array.array)
    }

    fn optional_array_id(
        &self,
        source: TransformSourceId,
        array: Option<TransformNodeArray>,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        array.map(|array| self.array_id(source, array)).transpose()
    }

    fn array_node_handles(
        &self,
        source: TransformSourceId,
        array: Option<NodeArrayId>,
    ) -> Result<Option<Vec<TransformNode>>, TransformError> {
        let Some(array) = array else { return Ok(None) };
        let array =
            self.arena
                .node_array_ref(source, array)
                .ok_or(TransformError::UnknownNodeArray(TransformNodeArray {
                    source,
                    array,
                }))?;
        Ok(Some(
            self.arena
                .node_array(array)?
                .nodes
                .iter()
                .map(|&node| TransformNode::new(source, node))
                .collect(),
        ))
    }

    fn child_flags(&self, child: Option<TransformNode>) -> Result<TransformFlags, TransformError> {
        child
            .map(|child| self.arena.propagate_child_flags(child))
            .transpose()
            .map(Option::unwrap_or_default)
    }

    fn children_flags(
        &self,
        children: Option<TransformNodeArray>,
    ) -> Result<TransformFlags, TransformError> {
        children
            .map(|children| {
                self.arena.node_array(children)?;
                Ok(self.arena.array_transform_flags(children))
            })
            .transpose()
            .map(Option::unwrap_or_default)
    }

    fn name_flags(&self, name: Option<TransformNode>) -> Result<TransformFlags, TransformError> {
        let Some(name) = name else {
            return Ok(TransformFlags::NONE);
        };
        let mut flags = self.arena.propagate_child_flags(name)?;
        if self.arena.node(name)?.kind == SyntaxKind::Identifier {
            flags = flags & !TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT;
        }
        Ok(flags)
    }

    fn modifier_flags(
        &self,
        modifiers: Option<TransformNodeArray>,
    ) -> Result<ModifierFlags, TransformError> {
        let Some(modifiers) = modifiers else {
            return Ok(ModifierFlags::NONE);
        };
        let mut flags = ModifierFlags::NONE;
        for &node in &self.arena.node_array(modifiers)?.nodes {
            let kind = self
                .arena
                .node(TransformNode::new(modifiers.source, node))?
                .kind;
            flags |= match kind {
                SyntaxKind::ExportKeyword => ModifierFlags::EXPORT,
                SyntaxKind::DeclareKeyword => ModifierFlags::AMBIENT,
                SyntaxKind::DefaultKeyword => ModifierFlags::DEFAULT,
                SyntaxKind::ConstKeyword => ModifierFlags::CONST,
                SyntaxKind::PublicKeyword => ModifierFlags::PUBLIC,
                SyntaxKind::PrivateKeyword => ModifierFlags::PRIVATE,
                SyntaxKind::ProtectedKeyword => ModifierFlags::PROTECTED,
                SyntaxKind::AbstractKeyword => ModifierFlags::ABSTRACT,
                SyntaxKind::StaticKeyword => ModifierFlags::STATIC,
                SyntaxKind::OverrideKeyword => ModifierFlags::OVERRIDE,
                SyntaxKind::ReadonlyKeyword => ModifierFlags::READONLY,
                SyntaxKind::AccessorKeyword => ModifierFlags::ACCESSOR,
                SyntaxKind::AsyncKeyword => ModifierFlags::ASYNC,
                SyntaxKind::InKeyword => ModifierFlags::IN,
                SyntaxKind::OutKeyword => ModifierFlags::OUT,
                _ => ModifierFlags::NONE,
            };
        }
        Ok(flags)
    }

    fn finish_update(
        &mut self,
        updated: TransformNode,
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.arena.set_original_node(updated, Some(original))?;
        self.set_text_range(updated, original)
    }

    /// tsc-port: createIdentifier @6.0.3
    /// tsc-hash: dd1baeac5d32597682b2f4f1acf9729f109bc958a82c19a446911a4bc94e709d
    /// tsc-span: _tsc.js:21609-21625
    pub fn create_identifier(
        &mut self,
        source: TransformSourceId,
        text: impl Into<String>,
    ) -> Result<TransformNode, TransformError> {
        let text = text.into();
        let flags = if text == "await" {
            TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT
        } else {
            TransformFlags::NONE
        };
        self.create_node(
            source,
            NodeData::Identifier(IdentifierData {
                escaped_text: tsc_syntax::escape_leading_underscores(&text),
                text,
            }),
            flags,
        )
    }

    /// tsc-port: createPrivateIdentifier @6.0.3
    /// tsc-hash: 095d8d14824ed1e3e193ecbd3c0d5cdf52ba4c6f89d9c5c949d7d65c0d2375e7
    /// tsc-span: _tsc.js:21673-21676
    pub fn create_private_identifier(
        &mut self,
        source: TransformSourceId,
        text: impl Into<String>,
    ) -> Result<TransformNode, TransformError> {
        let text = text.into();
        assert!(
            text.starts_with('#'),
            "private identifier text must start with #"
        );
        self.create_node(
            source,
            NodeData::PrivateIdentifier(PrivateIdentifierData {
                escaped_text: tsc_syntax::escape_leading_underscores(&text),
                text,
            }),
            TransformFlags::CONTAINS_CLASS_FIELDS,
        )
    }

    /// tsc-port: createUniqueName @6.0.3
    /// tsc-hash: 63ce34b71e831906ac09351627344eeb9bf76d71b5685d4715fb1873a61ccec5
    /// tsc-span: _tsc.js:21647-21651
    pub fn create_unique_name(
        &mut self,
        source: TransformSourceId,
        text: impl Into<String>,
        flags: GeneratedIdentifierFlags,
    ) -> Result<TransformNode, TransformError> {
        assert!(
            !flags.contains(GeneratedIdentifierFlags::FILE_LEVEL)
                || flags.contains(GeneratedIdentifierFlags::OPTIMISTIC),
            "file-level generated names must also be optimistic"
        );
        let text = text.into();
        let identifier = self.create_identifier(source, text.clone())?;
        let binding = self.arena.allocate_generated_binding_id();
        let metadata = self.arena.metadata_mut(identifier);
        metadata.set_generated_binding_id(binding);
        if flags.contains(GeneratedIdentifierFlags::OPTIMISTIC) {
            metadata.set_generated_binding_preferred_base(&text);
        } else {
            metadata.set_generated_binding_base(&text);
        }
        if flags.contains(GeneratedIdentifierFlags::FILE_LEVEL) {
            metadata.mark_generated_binding_file_level_optimistic();
        }
        if flags.contains(GeneratedIdentifierFlags::RESERVED_IN_NESTED_SCOPES) {
            metadata.reserve_generated_binding_in_nested_scopes();
        }
        Ok(identifier)
    }

    /// tsc-port: createNumericLiteral @6.0.3
    /// tsc-hash: 980f3b89b56e38a70c01d1c91a7badd1457360373e55352879a6cbec2ec3d68f
    /// tsc-span: _tsc.js:21508-21516
    pub fn create_numeric_literal(
        &mut self,
        source: TransformSourceId,
        text: impl Into<String>,
    ) -> Result<TransformNode, TransformError> {
        let text = text.into();
        assert!(
            !text.starts_with('-'),
            "negative numbers must use createPrefixUnaryExpression"
        );
        let flags = if text.starts_with("0b")
            || text.starts_with("0B")
            || text.starts_with("0o")
            || text.starts_with("0O")
        {
            TransformFlags::CONTAINS_ES_2015
        } else {
            TransformFlags::NONE
        };
        self.create_node(
            source,
            NodeData::NumericLiteral(NumericLiteralData { text }),
            flags,
        )
    }

    /// tsc-port: createBigIntLiteral @6.0.3
    /// tsc-hash: 022cceccdf4a015574b3873f3fa517d8423e591fa13ba1155bfd39588ab996a4
    /// tsc-span: _tsc.js:21517-21522
    pub fn create_big_int_literal(
        &mut self,
        source: TransformSourceId,
        text: impl Into<String>,
    ) -> Result<TransformNode, TransformError> {
        self.create_node(
            source,
            NodeData::BigIntLiteral(BigIntLiteralData { text: text.into() }),
            TransformFlags::CONTAINS_ES_2020,
        )
    }

    /// tsc-port: createStringLiteral @6.0.3
    /// tsc-hash: 2bf21e80bf4e61e4e1af7273cc968a2d4423ba01535d7cedc31a7ed35ebc1c2e
    /// tsc-span: _tsc.js:21529-21534
    pub fn create_string_literal(
        &mut self,
        source: TransformSourceId,
        text: impl Into<String>,
        single_quote: bool,
    ) -> Result<TransformNode, TransformError> {
        let literal = self.create_node(
            source,
            NodeData::StringLiteral(StringLiteralData {
                text: text.into(),
                has_extended_unicode_escape: None,
            }),
            TransformFlags::NONE,
        )?;
        self.arena
            .metadata_mut(literal)
            .set_string_literal_single_quote(single_quote);
        Ok(literal)
    }

    /// Lossless UTF-16 spelling of createStringLiteral @6.0.3.
    /// tsc-port: createStringLiteral @6.0.3
    /// tsc-hash: 2bf21e80bf4e61e4e1af7273cc968a2d4423ba01535d7cedc31a7ed35ebc1c2e
    /// tsc-span: _tsc.js:21529-21534
    pub fn create_string_literal_from_code_units(
        &mut self,
        source: TransformSourceId,
        units: &[u16],
        single_quote: bool,
    ) -> Result<TransformNode, TransformError> {
        let literal =
            self.create_string_literal(source, String::from_utf16_lossy(units), single_quote)?;
        self.arena
            .metadata_mut(literal)
            .set_javascript_string_value(JavaScriptString::from_code_units(units.to_vec()));
        Ok(literal)
    }

    /// Lossless UTF-16 spelling of createTemplateLiteralLikeNode @6.0.3.
    /// tsc-port: createTemplateLiteralLikeNode @6.0.3
    /// tsc-hash: 1511a496596c3ef84c876eb6ea51776d475c854621a500428d83ea351bcdbfcb
    /// tsc-span: _tsc.js:22873-22879
    pub fn create_template_literal_like_from_code_units(
        &mut self,
        source: TransformSourceId,
        kind: SyntaxKind,
        units: &[u16],
        raw: Option<&[u16]>,
    ) -> Result<TransformNode, TransformError> {
        let text = String::from_utf16_lossy(units);
        let raw_text = raw.map(String::from_utf16_lossy);
        let data = match kind {
            SyntaxKind::NoSubstitutionTemplateLiteral => {
                NodeData::NoSubstitutionTemplateLiteral(NoSubstitutionTemplateLiteralData {
                    text,
                    raw_text,
                })
            }
            SyntaxKind::TemplateHead => NodeData::TemplateHead(TemplateHeadData { text, raw_text }),
            SyntaxKind::TemplateMiddle => {
                NodeData::TemplateMiddle(TemplateMiddleData { text, raw_text })
            }
            SyntaxKind::TemplateTail => NodeData::TemplateTail(TemplateTailData { text, raw_text }),
            _ => return Err(TransformError::FactoryTokenKindExpected(kind)),
        };
        let literal = self.create_node(source, data, TransformFlags::CONTAINS_ES_2015)?;
        self.arena
            .metadata_mut(literal)
            .set_javascript_string_value(JavaScriptString::from_code_units(units.to_vec()));
        Ok(literal)
    }

    /// tsc-port: createTemplateHead @6.0.3
    /// tsc-hash: 66009daaca51dc1c16219f047f47bef9667665635db9c7f881892e1937c15f1c
    /// tsc-span: _tsc.js:22891-22894
    pub fn create_template_head(
        &mut self,
        source: TransformSourceId,
        text: impl Into<String>,
        raw_text: Option<String>,
    ) -> Result<TransformNode, TransformError> {
        self.create_node(
            source,
            NodeData::TemplateHead(TemplateHeadData {
                text: text.into(),
                raw_text,
            }),
            TransformFlags::CONTAINS_ES_2015,
        )
    }

    /// tsc-port: createNull @6.0.3
    /// tsc-hash: 6d3657a649f6ef05b2185a7f92da6e9a9e5bccb728d07d7623dd864e3d2527cf
    /// tsc-span: _tsc.js:21773-21775
    pub fn create_null(
        &mut self,
        source: TransformSourceId,
    ) -> Result<TransformNode, TransformError> {
        self.create_token(source, SyntaxKind::NullKeyword, TransformFlags::NONE)
    }

    /// tsc-port: createTrue @6.0.3
    /// tsc-hash: b2355b30279164e3edf79d14065fa95d18ffee0a82480f5f4260cce7ca37e997
    /// tsc-span: _tsc.js:21776-21778
    pub fn create_true(
        &mut self,
        source: TransformSourceId,
    ) -> Result<TransformNode, TransformError> {
        self.create_token(source, SyntaxKind::TrueKeyword, TransformFlags::NONE)
    }

    /// tsc-port: createFalse @6.0.3
    /// tsc-hash: 91e0e60d7a8aa228fa0c3f681265e275234d0bd27275d86e90d30b94faf5beb0
    /// tsc-span: _tsc.js:21779-21781
    pub fn create_false(
        &mut self,
        source: TransformSourceId,
    ) -> Result<TransformNode, TransformError> {
        self.create_token(source, SyntaxKind::FalseKeyword, TransformFlags::NONE)
    }

    /// tsc-port: createModifier @6.0.3
    /// tsc-hash: 16ab8287c8c7901cceb90a8aa4af2e88cc76cfff60cd24601fff5cb43b949240
    /// tsc-span: _tsc.js:21782-21784
    pub fn create_modifier(
        &mut self,
        source: TransformSourceId,
        kind: SyntaxKind,
    ) -> Result<TransformNode, TransformError> {
        self.create_token(source, kind, classify_created_token_flags(kind))
    }

    /// tsc-port: createModifiersFromModifierFlags @6.0.3
    /// tsc-hash: 73ac99ed0d4ef0b813e77fbd2960f0d180815867a31a19c0b88b25437b7afef8
    /// tsc-span: _tsc.js:21785-21803
    pub fn create_modifiers_from_modifier_flags(
        &mut self,
        source: TransformSourceId,
        flags: ModifierFlags,
    ) -> Result<Option<TransformNodeArray>, TransformError> {
        let mut modifiers = Vec::new();
        for (flag, kind) in [
            (ModifierFlags::EXPORT, SyntaxKind::ExportKeyword),
            (ModifierFlags::AMBIENT, SyntaxKind::DeclareKeyword),
            (ModifierFlags::DEFAULT, SyntaxKind::DefaultKeyword),
            (ModifierFlags::CONST, SyntaxKind::ConstKeyword),
            (ModifierFlags::PUBLIC, SyntaxKind::PublicKeyword),
            (ModifierFlags::PRIVATE, SyntaxKind::PrivateKeyword),
            (ModifierFlags::PROTECTED, SyntaxKind::ProtectedKeyword),
            (ModifierFlags::ABSTRACT, SyntaxKind::AbstractKeyword),
            (ModifierFlags::STATIC, SyntaxKind::StaticKeyword),
            (ModifierFlags::OVERRIDE, SyntaxKind::OverrideKeyword),
            (ModifierFlags::READONLY, SyntaxKind::ReadonlyKeyword),
            (ModifierFlags::ACCESSOR, SyntaxKind::AccessorKeyword),
            (ModifierFlags::ASYNC, SyntaxKind::AsyncKeyword),
            (ModifierFlags::IN, SyntaxKind::InKeyword),
            (ModifierFlags::OUT, SyntaxKind::OutKeyword),
        ] {
            if flags.contains(flag) {
                modifiers.push(self.create_modifier(source, kind)?);
            }
        }
        if modifiers.is_empty() {
            Ok(None)
        } else {
            self.create_node_array(source, modifiers).map(Some)
        }
    }

    /// tsc-port: createQualifiedName @6.0.3
    /// tsc-hash: ea9c44a9536fb2859fd4832758c8effcf3d8fc49d914ae9bd1256a774ab6aed6
    /// tsc-span: _tsc.js:21804-21811
    pub fn create_qualified_name(
        &mut self,
        source: TransformSourceId,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.child_flags(Some(left))? | self.name_flags(Some(right))?;
        self.create_node(
            source,
            NodeData::QualifiedName(QualifiedNameData {
                left: Some(self.node_id(source, left)?),
                right: Some(self.node_id(source, right)?),
            }),
            flags,
        )
    }

    /// tsc-port: updateQualifiedName @6.0.3
    /// tsc-hash: 4a54348b38bc483707af9d8fc1247dc025e36da5f3cb5eb3e30508c1c8595281
    /// tsc-span: _tsc.js:21812-21814
    pub fn update_qualified_name(
        &mut self,
        original: TransformNode,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::QualifiedName(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::QualifiedName,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.left == Some(left.node) && data.right == Some(right.node) {
            return Ok(original);
        }
        let updated = self.create_qualified_name(original.source, left, right)?;
        self.finish_update(updated, original)
    }

    /// tsc-port: createComputedPropertyName @6.0.3
    /// tsc-hash: 3b149a7a43988fa82cfdc435da487233991b7592d92d071f6aefb86d7aeddc93
    /// tsc-span: _tsc.js:21815-21820
    pub fn create_computed_property_name(
        &mut self,
        source: TransformSourceId,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.child_flags(Some(expression))?
            | TransformFlags::CONTAINS_ES_2015
            | TransformFlags::CONTAINS_COMPUTED_PROPERTY_NAME;
        self.create_node(
            source,
            NodeData::ComputedPropertyName(ComputedPropertyNameData {
                expression: Some(self.node_id(source, expression)?),
            }),
            flags,
        )
    }

    /// tsc-port: updateComputedPropertyName @6.0.3
    /// tsc-hash: 93e9e44bd9cb5471d6cb4947f7ebfeedb816b2b7ee584b8fa018198b67a1aee5
    /// tsc-span: _tsc.js:21821-21823
    pub fn update_computed_property_name(
        &mut self,
        original: TransformNode,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::ComputedPropertyName(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::ComputedPropertyName,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.expression == Some(expression.node) {
            return Ok(original);
        }
        let updated = self.create_computed_property_name(original.source, expression)?;
        self.finish_update(updated, original)
    }

    /// tsc-port: createKeywordTypeNode @6.0.3
    /// tsc-hash: c535d4a3a91b9d3e92266d2f42920a0ef2e4fe642e0d712898fe93dbebb6117f
    /// tsc-span: _tsc.js:22130-22132
    pub fn create_keyword_type_node(
        &mut self,
        source: TransformSourceId,
        kind: SyntaxKind,
    ) -> Result<TransformNode, TransformError> {
        self.create_token(source, kind, classify_created_token_flags(kind))
    }

    /// tsc-port: createTypePredicateNode @6.0.3
    /// tsc-hash: 4660ec6804e795abb7153113fafde9c7df9490d92ab1bf032565919d2eb3a0d3
    /// tsc-span: _tsc.js:22133-22140
    pub fn create_type_predicate_node(
        &mut self,
        source: TransformSourceId,
        asserts_modifier: Option<TransformNode>,
        parameter_name: TransformNode,
        r#type: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        self.create_node(
            source,
            NodeData::TypePredicate(TypePredicateData {
                asserts_modifier: self.optional_node_id(source, asserts_modifier)?,
                parameter_name: Some(self.node_id(source, parameter_name)?),
                r#type: self.optional_node_id(source, r#type)?,
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: updateTypePredicateNode @6.0.3
    /// tsc-hash: 837e1b51444e9f9b89e4f6a044c55ed13d24ba37987b1efbbd6dd77189e1b81a
    /// tsc-span: _tsc.js:22141-22143
    pub fn update_type_predicate_node(
        &mut self,
        original: TransformNode,
        asserts_modifier: Option<TransformNode>,
        parameter_name: TransformNode,
        r#type: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::TypePredicate(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::TypePredicate,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.asserts_modifier == asserts_modifier.map(TransformNode::node)
            && data.parameter_name == Some(parameter_name.node)
            && data.r#type == r#type.map(TransformNode::node)
        {
            return Ok(original);
        }
        let updated = self.create_type_predicate_node(
            original.source,
            asserts_modifier,
            parameter_name,
            r#type,
        )?;
        self.finish_update(updated, original)
    }

    /// tsc-port: createTypeReferenceNode @6.0.3
    /// tsc-hash: f2446c3e6857b4c9c1415f0e42fde5ce046d7799226aea45f62d086108c0c8a7
    /// tsc-span: _tsc.js:22144-22150
    pub fn create_type_reference_node(
        &mut self,
        source: TransformSourceId,
        type_name: TransformNode,
        type_arguments: Option<TransformNodeArray>,
    ) -> Result<TransformNode, TransformError> {
        self.node_id(source, type_name)?;
        let type_arguments = TypeParenthesizer::parenthesize_type_arguments(self, type_arguments)?;
        self.create_node(
            source,
            NodeData::TypeReference(TypeReferenceData {
                type_arguments: self.optional_array_id(source, type_arguments)?,
                type_name: Some(type_name.node),
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: updateTypeReferenceNode @6.0.3
    /// tsc-hash: 15b433e394bbd78ab1139c81e14eff92cdfad4d0ac10d6a6ff3b70c4e5f5384f
    /// tsc-span: _tsc.js:22151-22153
    pub fn update_type_reference_node(
        &mut self,
        original: TransformNode,
        type_name: TransformNode,
        type_arguments: Option<TransformNodeArray>,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::TypeReference(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::TypeReference,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.type_name == Some(type_name.node)
            && data.type_arguments == type_arguments.map(TransformNodeArray::array)
        {
            return Ok(original);
        }
        let updated =
            self.create_type_reference_node(original.source, type_name, type_arguments)?;
        self.finish_update(updated, original)
    }

    /// tsc-port: createFunctionTypeNode @6.0.3
    /// tsc-hash: 723220f86e25512153be45ff3afe9a5583276e0069782b2959ef9f1960c7d422
    /// tsc-span: _tsc.js:22154-22167
    pub fn create_function_type_node(
        &mut self,
        source: TransformSourceId,
        type_parameters: Option<TransformNodeArray>,
        parameters: TransformNodeArray,
        r#type: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.create_node(
            source,
            NodeData::FunctionType(FunctionTypeData {
                type_parameters: self.optional_array_id(source, type_parameters)?,
                parameters: Some(self.array_id(source, parameters)?),
                r#type: Some(self.node_id(source, r#type)?),
                modifiers: None,
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: createConstructorTypeNode @6.0.3
    /// tsc-hash: 455c2e88b7965d1fd83f44d855aa0b2685511bc10ec253cf8a48fc9d19e231fb
    /// tsc-span: _tsc.js:22176-22196
    pub fn create_constructor_type_node(
        &mut self,
        source: TransformSourceId,
        modifiers: Option<TransformNodeArray>,
        type_parameters: Option<TransformNodeArray>,
        parameters: TransformNodeArray,
        r#type: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.create_node(
            source,
            NodeData::ConstructorType(ConstructorTypeData {
                type_parameters: self.optional_array_id(source, type_parameters)?,
                parameters: Some(self.array_id(source, parameters)?),
                r#type: Some(self.node_id(source, r#type)?),
                modifiers: self.optional_array_id(source, modifiers)?,
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: createTypeQueryNode @6.0.3
    /// tsc-hash: 31e1f54c8ef6f8b36cd8058a3b6d6cab764502928034385310345af0295239d6
    /// tsc-span: _tsc.js:22210-22216
    pub fn create_type_query_node(
        &mut self,
        source: TransformSourceId,
        expr_name: TransformNode,
        type_arguments: Option<TransformNodeArray>,
    ) -> Result<TransformNode, TransformError> {
        self.node_id(source, expr_name)?;
        let type_arguments = TypeParenthesizer::parenthesize_type_arguments(self, type_arguments)?;
        self.create_node(
            source,
            NodeData::TypeQuery(TypeQueryData {
                type_arguments: self.optional_array_id(source, type_arguments)?,
                expr_name: Some(expr_name.node),
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: updateTypeQueryNode @6.0.3
    /// tsc-hash: 62fa5bc36064341e2bf0d7e667c0d2f2d1eaf6852678d8174b92edf53915e6f7
    /// tsc-span: _tsc.js:22217-22219
    pub fn update_type_query_node(
        &mut self,
        original: TransformNode,
        expr_name: TransformNode,
        type_arguments: Option<TransformNodeArray>,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::TypeQuery(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::TypeQuery,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.expr_name == Some(expr_name.node)
            && data.type_arguments == type_arguments.map(TransformNodeArray::array)
        {
            return Ok(original);
        }
        let updated = self.create_type_query_node(original.source, expr_name, type_arguments)?;
        self.finish_update(updated, original)
    }

    /// tsc-port: createTypeLiteralNode @6.0.3
    /// tsc-hash: 3210913fe0c3a8c31e249d34bef8c1e09648e3a0e5c38839f6e2efd8e00617bf
    /// tsc-span: _tsc.js:22220-22225
    pub fn create_type_literal_node(
        &mut self,
        source: TransformSourceId,
        members: TransformNodeArray,
    ) -> Result<TransformNode, TransformError> {
        self.create_node(
            source,
            NodeData::TypeLiteral(TypeLiteralData {
                members: Some(self.array_id(source, members)?),
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: createArrayTypeNode @6.0.3
    /// tsc-hash: 71e29dc77eaa156837ba89b71ffc6b028e29a3da6e605952ea80b7443b0a38aa
    /// tsc-span: _tsc.js:22229-22234
    pub fn create_array_type_node(
        &mut self,
        source: TransformSourceId,
        element_type: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let element_type =
            TypeParenthesizer::parenthesize_non_array_type_of_postfix_type(self, element_type)?;
        self.create_node(
            source,
            NodeData::ArrayType(ArrayTypeData {
                element_type: Some(self.node_id(source, element_type)?),
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: createTupleTypeNode @6.0.3
    /// tsc-hash: 16e1e91fae1c7a14ceabac5a86b34b47dd8ae8c5abb5b75c8bde1924d07993a3
    /// tsc-span: _tsc.js:22238-22243
    pub fn create_tuple_type_node(
        &mut self,
        source: TransformSourceId,
        elements: TransformNodeArray,
    ) -> Result<TransformNode, TransformError> {
        let elements = TypeParenthesizer::parenthesize_element_types_of_tuple_type(self, elements)?;
        self.create_node(
            source,
            NodeData::TupleType(TupleTypeData {
                elements: Some(self.array_id(source, elements)?),
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: createNamedTupleMember @6.0.3
    /// tsc-hash: b9c321f1be3f372a173ff70718534a5faf7f75dc9ca2e85e39acebd17bf51437
    /// tsc-span: _tsc.js:22247-22259
    pub fn create_named_tuple_member(
        &mut self,
        source: TransformSourceId,
        dot_dot_dot_token: Option<TransformNode>,
        name: TransformNode,
        question_token: Option<TransformNode>,
        r#type: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.create_node(
            source,
            NodeData::NamedTupleMember(NamedTupleMemberData {
                dot_dot_dot_token: self.optional_node_id(source, dot_dot_dot_token)?,
                name: Some(self.node_id(source, name)?),
                question_token: self.optional_node_id(source, question_token)?,
                r#type: Some(self.node_id(source, r#type)?),
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: createOptionalTypeNode @6.0.3
    /// tsc-hash: 6f813cfb6a42736b3eac89e49ae158bb1ce05dac582b3f459a1dcc86e17eb47c
    /// tsc-span: _tsc.js:22260-22265
    pub fn create_optional_type_node(
        &mut self,
        source: TransformSourceId,
        r#type: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let r#type = TypeParenthesizer::parenthesize_type_of_optional_type(self, r#type)?;
        self.create_node(
            source,
            NodeData::OptionalType(OptionalTypeData {
                r#type: Some(self.node_id(source, r#type)?),
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: createRestTypeNode @6.0.3
    /// tsc-hash: 95358d06d9e5e4137c4f285d0e0c0e2e6a4d68a668ddbdbce1eef2c8a4c68f9a
    /// tsc-span: _tsc.js:22269-22274
    pub fn create_rest_type_node(
        &mut self,
        source: TransformSourceId,
        r#type: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.create_node(
            source,
            NodeData::RestType(RestTypeData {
                r#type: Some(self.node_id(source, r#type)?),
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: createUnionTypeNode @6.0.3
    /// tsc-hash: 6adb99e1838a49bb83823cfaf97759760116f78c604beb7a8fdd0ca878d3f8db
    /// tsc-span: _tsc.js:22287-22292
    pub fn create_union_type_node(
        &mut self,
        source: TransformSourceId,
        types: TransformNodeArray,
    ) -> Result<TransformNode, TransformError> {
        let types = TypeParenthesizer::parenthesize_constituent_types_of_union_type(self, types)?;
        self.create_node(
            source,
            NodeData::UnionType(UnionTypeData {
                types: Some(self.array_id(source, types)?),
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: createIntersectionTypeNode @6.0.3
    /// tsc-hash: 00d6b64783b04d826bc0ecd7a89e6d69a34eb70a9cb6dd89dec68d9c78aac108
    /// tsc-span: _tsc.js:22293-22298
    pub fn create_intersection_type_node(
        &mut self,
        source: TransformSourceId,
        types: TransformNodeArray,
    ) -> Result<TransformNode, TransformError> {
        let types =
            TypeParenthesizer::parenthesize_constituent_types_of_intersection_type(self, types)?;
        self.create_node(
            source,
            NodeData::IntersectionType(IntersectionTypeData {
                types: Some(self.array_id(source, types)?),
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: createConditionalTypeNode @6.0.3
    /// tsc-hash: 26620ce9ec00d3d4431b1281e07c48a1cce3d4053d80b268e2f47abd4148c49d
    /// tsc-span: _tsc.js:22299-22309
    pub fn create_conditional_type_node(
        &mut self,
        source: TransformSourceId,
        check_type: TransformNode,
        extends_type: TransformNode,
        true_type: TransformNode,
        false_type: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let check_type =
            TypeParenthesizer::parenthesize_check_type_of_conditional_type(self, check_type)?;
        let extends_type =
            TypeParenthesizer::parenthesize_extends_type_of_conditional_type(self, extends_type)?;
        self.create_node(
            source,
            NodeData::ConditionalType(ConditionalTypeData {
                check_type: Some(self.node_id(source, check_type)?),
                extends_type: Some(self.node_id(source, extends_type)?),
                true_type: Some(self.node_id(source, true_type)?),
                false_type: Some(self.node_id(source, false_type)?),
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: updateConditionalTypeNode @6.0.3
    /// tsc-hash: a9b797b561095291b77f07dfaf68b44637cd30b514c376740a5f79217684cfe2
    /// tsc-span: _tsc.js:22310-22312
    pub fn update_conditional_type_node(
        &mut self,
        original: TransformNode,
        check_type: TransformNode,
        extends_type: TransformNode,
        true_type: TransformNode,
        false_type: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::ConditionalType(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::ConditionalType,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.check_type == Some(check_type.node)
            && data.extends_type == Some(extends_type.node)
            && data.true_type == Some(true_type.node)
            && data.false_type == Some(false_type.node)
        {
            return Ok(original);
        }
        let updated = self.create_conditional_type_node(
            original.source,
            check_type,
            extends_type,
            true_type,
            false_type,
        )?;
        self.finish_update(updated, original)
    }

    /// tsc-port: createInferTypeNode @6.0.3
    /// tsc-hash: 3a5a517ad01a212dde9226f06e8915b407030a32ff3d40efae3f8d14760ebb80
    /// tsc-span: _tsc.js:22313-22318
    pub fn create_infer_type_node(
        &mut self,
        source: TransformSourceId,
        type_parameter: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.create_node(
            source,
            NodeData::InferType(InferTypeData {
                type_parameter: Some(self.node_id(source, type_parameter)?),
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: createTemplateLiteralType @6.0.3
    /// tsc-hash: c9d6f05d997bcf9bb33292d79f296358c35a2f787d1b66803c2206ae11d8c65d
    /// tsc-span: _tsc.js:22322-22328
    pub fn create_template_literal_type(
        &mut self,
        source: TransformSourceId,
        head: TransformNode,
        template_spans: TransformNodeArray,
    ) -> Result<TransformNode, TransformError> {
        self.create_node(
            source,
            NodeData::TemplateLiteralType(TemplateLiteralTypeData {
                head: Some(self.node_id(source, head)?),
                template_spans: Some(self.array_id(source, template_spans)?),
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: createImportTypeNode @6.0.3
    /// tsc-hash: a950588693ed3752ed2f76f11c5a4034b8a56d876317d12a39e2d69819a068ae
    /// tsc-span: _tsc.js:22332-22344
    #[allow(clippy::too_many_arguments)]
    pub fn create_import_type_node(
        &mut self,
        source: TransformSourceId,
        argument: TransformNode,
        attributes: Option<TransformNode>,
        qualifier: Option<TransformNode>,
        type_arguments: Option<TransformNodeArray>,
        is_type_of: bool,
    ) -> Result<TransformNode, TransformError> {
        self.node_id(source, argument)?;
        let type_arguments = TypeParenthesizer::parenthesize_type_arguments(self, type_arguments)?;
        self.create_node(
            source,
            NodeData::ImportType(ImportTypeData {
                type_arguments: self.optional_array_id(source, type_arguments)?,
                is_type_of,
                argument: Some(argument.node),
                attributes: self.optional_node_id(source, attributes)?,
                qualifier: self.optional_node_id(source, qualifier)?,
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: updateImportTypeNode @6.0.3
    /// tsc-hash: 8c2532bf69ef911105aef4c352613095f64516e3d9257cdca4372b838362ffb4
    /// tsc-span: _tsc.js:22345-22347
    #[allow(clippy::too_many_arguments)]
    pub fn update_import_type_node(
        &mut self,
        original: TransformNode,
        argument: TransformNode,
        attributes: Option<TransformNode>,
        qualifier: Option<TransformNode>,
        type_arguments: Option<TransformNodeArray>,
        is_type_of: bool,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::ImportType(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::ImportType,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.argument == Some(argument.node)
            && data.attributes == attributes.map(TransformNode::node)
            && data.qualifier == qualifier.map(TransformNode::node)
            && data.type_arguments == type_arguments.map(TransformNodeArray::array)
            && data.is_type_of == is_type_of
        {
            return Ok(original);
        }
        let updated = self.create_import_type_node(
            original.source,
            argument,
            attributes,
            qualifier,
            type_arguments,
            is_type_of,
        )?;
        self.finish_update(updated, original)
    }

    /// tsc-port: createParenthesizedType @6.0.3
    /// tsc-hash: af93078d7ab5922c44cce7268a62547f85cfd5dc932158376679ada4a609b094
    /// tsc-span: _tsc.js:22348-22353
    pub fn create_parenthesized_type(
        &mut self,
        source: TransformSourceId,
        r#type: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.create_node(
            source,
            NodeData::ParenthesizedType(ParenthesizedTypeData {
                r#type: Some(self.node_id(source, r#type)?),
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: createThisTypeNode @6.0.3
    /// tsc-hash: b07b24ff9384331e2fd5677569ae4a4771452581ceb03a89ac23b5e2fb1fb818
    /// tsc-span: _tsc.js:22357-22361
    pub fn create_this_type_node(
        &mut self,
        source: TransformSourceId,
    ) -> Result<TransformNode, TransformError> {
        self.create_token(
            source,
            SyntaxKind::ThisType,
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: createTypeOperatorNode @6.0.3
    /// tsc-hash: 912a60aaa14c05c5820b19ce24cebc78ad8c247acaa23c8430c13a49091ae65b
    /// tsc-span: _tsc.js:22362-22368
    pub fn create_type_operator_node(
        &mut self,
        source: TransformSourceId,
        operator: SyntaxKind,
        r#type: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let r#type = if operator == SyntaxKind::ReadonlyKeyword {
            TypeParenthesizer::parenthesize_operand_of_readonly_type_operator(self, r#type)?
        } else {
            TypeParenthesizer::parenthesize_operand_of_type_operator(self, r#type)?
        };
        self.create_node(
            source,
            NodeData::TypeOperator(TypeOperatorData {
                operator,
                r#type: Some(self.node_id(source, r#type)?),
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: updateTypeOperatorNode @6.0.3
    /// tsc-hash: c0577d9718fb34c1aa3344cc3de64fc38f2addc54f3ae84479a20b67a9a63a3a
    /// tsc-span: _tsc.js:22369-22371
    pub fn update_type_operator_node(
        &mut self,
        original: TransformNode,
        r#type: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::TypeOperator(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::TypeOperator,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.r#type == Some(r#type.node) {
            return Ok(original);
        }
        let operator = data.operator;
        let updated = self.create_type_operator_node(original.source, operator, r#type)?;
        self.finish_update(updated, original)
    }

    /// tsc-port: createIndexedAccessTypeNode @6.0.3
    /// tsc-hash: 5c7144a5ad40365fd112084b684b5bcdf956fb3c4b564f95e3542d3c0a52780f
    /// tsc-span: _tsc.js:22372-22378
    pub fn create_indexed_access_type_node(
        &mut self,
        source: TransformSourceId,
        object_type: TransformNode,
        index_type: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let object_type =
            TypeParenthesizer::parenthesize_non_array_type_of_postfix_type(self, object_type)?;
        self.create_node(
            source,
            NodeData::IndexedAccessType(IndexedAccessTypeData {
                object_type: Some(self.node_id(source, object_type)?),
                index_type: Some(self.node_id(source, index_type)?),
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: updateIndexedAccessTypeNode @6.0.3
    /// tsc-hash: 15970351ab97b36b0be9b6e08d55dcd586bb6a293269133afce2220c0307061f
    /// tsc-span: _tsc.js:22379-22381
    pub fn update_indexed_access_type_node(
        &mut self,
        original: TransformNode,
        object_type: TransformNode,
        index_type: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::IndexedAccessType(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::IndexedAccessType,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.object_type == Some(object_type.node) && data.index_type == Some(index_type.node) {
            return Ok(original);
        }
        let updated =
            self.create_indexed_access_type_node(original.source, object_type, index_type)?;
        self.finish_update(updated, original)
    }

    /// tsc-port: createMappedTypeNode @6.0.3
    /// tsc-hash: 0fce1cfe620cd3abc168e2a6f8472f4771169491d673c70cec1a1caf2ca2c6b8
    /// tsc-span: _tsc.js:22382-22397
    #[allow(clippy::too_many_arguments)]
    pub fn create_mapped_type_node(
        &mut self,
        source: TransformSourceId,
        readonly_token: Option<TransformNode>,
        type_parameter: TransformNode,
        name_type: Option<TransformNode>,
        question_token: Option<TransformNode>,
        r#type: Option<TransformNode>,
        members: Option<TransformNodeArray>,
    ) -> Result<TransformNode, TransformError> {
        self.create_node(
            source,
            NodeData::MappedType(MappedTypeData {
                readonly_token: self.optional_node_id(source, readonly_token)?,
                type_parameter: Some(self.node_id(source, type_parameter)?),
                name_type: self.optional_node_id(source, name_type)?,
                question_token: self.optional_node_id(source, question_token)?,
                r#type: self.optional_node_id(source, r#type)?,
                members: self.optional_array_id(source, members)?,
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: createLiteralTypeNode @6.0.3
    /// tsc-hash: 77717b3b5b2eef35aebafcfbb103733a859cc9af2c44baa3f185a92d35e28d43
    /// tsc-span: _tsc.js:22398-22403
    pub fn create_literal_type_node(
        &mut self,
        source: TransformSourceId,
        literal: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.create_node(
            source,
            NodeData::LiteralType(LiteralTypeData {
                literal: Some(self.node_id(source, literal)?),
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: createTemplateLiteralTypeSpan @6.0.3
    /// tsc-hash: b74c981670edd3edc82d407c9dd7af05e53cfe7118d393b0998405422941e2ee
    /// tsc-span: _tsc.js:22120-22129
    pub fn create_template_literal_type_span(
        &mut self,
        source: TransformSourceId,
        r#type: TransformNode,
        literal: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.create_node(
            source,
            NodeData::TemplateLiteralTypeSpan(TemplateLiteralTypeSpanData {
                r#type: Some(self.node_id(source, r#type)?),
                literal: Some(self.node_id(source, literal)?),
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: createExpressionWithTypeArguments @6.0.3
    /// tsc-hash: 9d82e84ce18b62e683344bdba7f4067959b02e3ba13c9303ea37889e54517ddf
    /// tsc-span: _tsc.js:22944-22954
    pub fn create_expression_with_type_arguments(
        &mut self,
        source: TransformSourceId,
        expression: TransformNode,
        type_arguments: Option<TransformNodeArray>,
    ) -> Result<TransformNode, TransformError> {
        self.node_id(source, expression)?;
        let type_arguments = TypeParenthesizer::parenthesize_type_arguments(self, type_arguments)?;
        let flags = self.child_flags(Some(expression))?
            | self.children_flags(type_arguments)?
            | TransformFlags::CONTAINS_ES_2015;
        self.create_node(
            source,
            NodeData::ExpressionWithTypeArguments(ExpressionWithTypeArgumentsData {
                type_arguments: self.optional_array_id(source, type_arguments)?,
                expression: Some(expression.node),
            }),
            flags,
        )
    }

    /// tsc-port: createTypeParameterDeclaration @6.0.3
    /// tsc-hash: 473b9ac013591e2008c4c697e515baf6ff9ead4bed170531a0cbbf2f2ed7511a
    /// tsc-span: _tsc.js:21824-21834
    pub fn create_type_parameter_declaration(
        &mut self,
        source: TransformSourceId,
        modifiers: Option<TransformNodeArray>,
        name: TransformNode,
        constraint: Option<TransformNode>,
        default_type: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        self.create_node(
            source,
            NodeData::TypeParameter(TypeParameterData {
                name: Some(self.node_id(source, name)?),
                modifiers: self.optional_array_id(source, modifiers)?,
                constraint: self.optional_node_id(source, constraint)?,
                r#default: self.optional_node_id(source, default_type)?,
                expression: None,
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: updateTypeParameterDeclaration @6.0.3
    /// tsc-hash: 788c8788bc75cad86a413051c1d6a3fee2af7ca35556280cf6624dff7910037b
    /// tsc-span: _tsc.js:21835-21837
    pub fn update_type_parameter_declaration(
        &mut self,
        original: TransformNode,
        modifiers: Option<TransformNodeArray>,
        name: TransformNode,
        constraint: Option<TransformNode>,
        default_type: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::TypeParameter(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::TypeParameter,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.modifiers == modifiers.map(TransformNodeArray::array)
            && data.name == Some(name.node)
            && data.constraint == constraint.map(TransformNode::node)
            && data.r#default == default_type.map(TransformNode::node)
        {
            return Ok(original);
        }
        let updated = self.create_type_parameter_declaration(
            original.source,
            modifiers,
            name,
            constraint,
            default_type,
        )?;
        self.finish_update(updated, original)
    }

    /// tsc-port: createParameterDeclaration @6.0.3
    /// tsc-hash: 31cde0f942a7460844092dcd2b95e2e97bafbcd5a2dcc6f4f5e0a1b7f5f11ef5
    /// tsc-span: _tsc.js:21838-21853
    #[allow(clippy::too_many_arguments)]
    pub fn create_parameter_declaration(
        &mut self,
        source: TransformSourceId,
        modifiers: Option<TransformNodeArray>,
        dot_dot_dot_token: Option<TransformNode>,
        name: TransformNode,
        question_token: Option<TransformNode>,
        r#type: Option<TransformNode>,
        initializer: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let is_this = matches!(
            &self.arena.node(name)?.data,
            NodeData::Identifier(data) if data.escaped_text == "this"
        );
        let mut flags = TransformFlags::CONTAINS_TYPE_SCRIPT;
        if !is_this {
            flags = self.children_flags(modifiers)?
                | self.child_flags(dot_dot_dot_token)?
                | self.name_flags(Some(name))?
                | self.child_flags(question_token)?
                | self.child_flags(initializer)?;
            if question_token.is_some() || r#type.is_some() {
                flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
            }
            if dot_dot_dot_token.is_some() || initializer.is_some() {
                flags |= TransformFlags::CONTAINS_ES_2015;
            }
            if self
                .modifier_flags(modifiers)?
                .intersects(ModifierFlags::PARAMETER_PROPERTY_MODIFIER)
            {
                flags |= TransformFlags::CONTAINS_TYPE_SCRIPT_CLASS_SYNTAX;
            }
        }
        self.create_node(
            source,
            NodeData::Parameter(ParameterData {
                name: Some(self.node_id(source, name)?),
                modifiers: self.optional_array_id(source, modifiers)?,
                dot_dot_dot_token: self.optional_node_id(source, dot_dot_dot_token)?,
                question_token: self.optional_node_id(source, question_token)?,
                r#type: self.optional_node_id(source, r#type)?,
                initializer: self.optional_node_id(source, initializer)?,
            }),
            flags,
        )
    }

    /// tsc-port: updateParameterDeclaration @6.0.3
    /// tsc-hash: df52aba274cf57e89061d225869966a790bfd214c0c9a525040cfded629a3637
    /// tsc-span: _tsc.js:21854-21856
    #[allow(clippy::too_many_arguments)]
    pub fn update_parameter_declaration(
        &mut self,
        original: TransformNode,
        modifiers: Option<TransformNodeArray>,
        dot_dot_dot_token: Option<TransformNode>,
        name: TransformNode,
        question_token: Option<TransformNode>,
        r#type: Option<TransformNode>,
        initializer: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::Parameter(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::Parameter,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.modifiers == modifiers.map(TransformNodeArray::array)
            && data.dot_dot_dot_token == dot_dot_dot_token.map(TransformNode::node)
            && data.name == Some(name.node)
            && data.question_token == question_token.map(TransformNode::node)
            && data.r#type == r#type.map(TransformNode::node)
            && data.initializer == initializer.map(TransformNode::node)
        {
            return Ok(original);
        }
        let updated = self.create_parameter_declaration(
            original.source,
            modifiers,
            dot_dot_dot_token,
            name,
            question_token,
            r#type,
            initializer,
        )?;
        self.finish_update(updated, original)
    }

    /// tsc-port: createPropertySignature @6.0.3
    /// tsc-hash: 63cf0f85580adca8cecdc9964ca537b32ffb49c7abb016d9db2996c02d52fa0e
    /// tsc-span: _tsc.js:21870-21882
    pub fn create_property_signature(
        &mut self,
        source: TransformSourceId,
        modifiers: Option<TransformNodeArray>,
        name: TransformNode,
        question_token: Option<TransformNode>,
        r#type: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        self.create_node(
            source,
            NodeData::PropertySignature(PropertySignatureData {
                name: Some(self.node_id(source, name)?),
                question_token: self.optional_node_id(source, question_token)?,
                modifiers: self.optional_array_id(source, modifiers)?,
                r#type: self.optional_node_id(source, r#type)?,
                initializer: None,
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: createPropertyDeclaration @6.0.3
    /// tsc-hash: 2f7df66cb0e988da54fd316bcba05db7e36dc1c9f8472f465001a4007f1dd4bb
    /// tsc-span: _tsc.js:21890-21902
    pub fn create_property_declaration(
        &mut self,
        source: TransformSourceId,
        modifiers: Option<TransformNodeArray>,
        name: TransformNode,
        question_or_exclamation_token: Option<TransformNode>,
        r#type: Option<TransformNode>,
        initializer: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let token_kind = question_or_exclamation_token
            .map(|token| self.arena.node(token).map(|node| node.kind))
            .transpose()?;
        let question_token = (token_kind == Some(SyntaxKind::QuestionToken))
            .then_some(question_or_exclamation_token)
            .flatten();
        let exclamation_token = (token_kind == Some(SyntaxKind::ExclamationToken))
            .then_some(question_or_exclamation_token)
            .flatten();
        let modifier_flags = self.modifier_flags(modifiers)?;
        let name_kind = self.arena.node(name)?.kind;
        let mut flags = self.children_flags(modifiers)?
            | self.name_flags(Some(name))?
            | self.child_flags(initializer)?
            | TransformFlags::CONTAINS_CLASS_FIELDS;
        if modifier_flags.contains(ModifierFlags::AMBIENT)
            || question_token.is_some()
            || exclamation_token.is_some()
            || r#type.is_some()
        {
            flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
        }
        if name_kind == SyntaxKind::ComputedPropertyName
            || modifier_flags.contains(ModifierFlags::STATIC) && initializer.is_some()
        {
            flags |= TransformFlags::CONTAINS_TYPE_SCRIPT_CLASS_SYNTAX;
        }
        self.create_node(
            source,
            NodeData::PropertyDeclaration(PropertyDeclarationData {
                name: Some(self.node_id(source, name)?),
                modifiers: self.optional_array_id(source, modifiers)?,
                question_token: self.optional_node_id(source, question_token)?,
                exclamation_token: self.optional_node_id(source, exclamation_token)?,
                r#type: self.optional_node_id(source, r#type)?,
                initializer: self.optional_node_id(source, initializer)?,
            }),
            flags,
        )
    }

    /// tsc-port: createMethodSignature @6.0.3
    /// tsc-hash: 81cf79b7cd2c1aec90fa92a6932715ca44940f8320269e0dfbe09588eefce280
    /// tsc-span: _tsc.js:21906-21923
    #[allow(clippy::too_many_arguments)]
    pub fn create_method_signature(
        &mut self,
        source: TransformSourceId,
        modifiers: Option<TransformNodeArray>,
        name: TransformNode,
        question_token: Option<TransformNode>,
        type_parameters: Option<TransformNodeArray>,
        parameters: TransformNodeArray,
        r#type: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        self.create_node(
            source,
            NodeData::MethodSignature(MethodSignatureData {
                name: Some(self.node_id(source, name)?),
                type_parameters: self.optional_array_id(source, type_parameters)?,
                parameters: Some(self.array_id(source, parameters)?),
                r#type: self.optional_node_id(source, r#type)?,
                question_token: self.optional_node_id(source, question_token)?,
                modifiers: self.optional_array_id(source, modifiers)?,
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    fn function_facets(is_async: bool, is_generator: bool) -> TransformFlags {
        if is_async && is_generator {
            TransformFlags::CONTAINS_ES_2018
        } else if is_async {
            TransformFlags::CONTAINS_ES_2017
        } else if is_generator {
            TransformFlags::CONTAINS_GENERATOR
        } else {
            TransformFlags::NONE
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn function_like_flags(
        &self,
        modifiers: Option<TransformNodeArray>,
        asterisk_token: Option<TransformNode>,
        name: Option<TransformNode>,
        question_token: Option<TransformNode>,
        type_parameters: Option<TransformNodeArray>,
        parameters: TransformNodeArray,
        r#type: Option<TransformNode>,
        body: TransformNode,
        hoisted: bool,
        method: bool,
    ) -> Result<TransformFlags, TransformError> {
        let is_async = self
            .modifier_flags(modifiers)?
            .contains(ModifierFlags::ASYNC);
        let mut flags = self.children_flags(modifiers)?
            | self.child_flags(asterisk_token)?
            | self.name_flags(name)?
            | self.child_flags(question_token)?
            | self.children_flags(type_parameters)?
            | self.children_flags(Some(parameters))?
            | self.child_flags(r#type)?
            | (self.child_flags(Some(body))? & !TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT)
            | Self::function_facets(is_async, asterisk_token.is_some());
        if type_parameters.is_some() || r#type.is_some() || question_token.is_some() {
            flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
        }
        if hoisted {
            flags |= TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION;
        }
        if method {
            flags |= TransformFlags::CONTAINS_ES_2015;
        }
        Ok(flags)
    }

    /// tsc-port: createMethodDeclaration @6.0.3
    /// tsc-hash: ab2bb8f981f84e6971291e817bb111793868f0254f394570ed3056fc5bc28544
    /// tsc-span: _tsc.js:21924-21951
    #[allow(clippy::too_many_arguments)]
    pub fn create_method_declaration(
        &mut self,
        source: TransformSourceId,
        modifiers: Option<TransformNodeArray>,
        asterisk_token: Option<TransformNode>,
        name: TransformNode,
        question_token: Option<TransformNode>,
        type_parameters: Option<TransformNodeArray>,
        parameters: TransformNodeArray,
        r#type: Option<TransformNode>,
        body: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let flags = match body {
            None => TransformFlags::CONTAINS_TYPE_SCRIPT,
            Some(body) => self.function_like_flags(
                modifiers,
                asterisk_token,
                Some(name),
                question_token,
                type_parameters,
                parameters,
                r#type,
                body,
                false,
                true,
            )?,
        };
        self.create_node(
            source,
            NodeData::MethodDeclaration(MethodDeclarationData {
                name: Some(self.node_id(source, name)?),
                type_parameters: self.optional_array_id(source, type_parameters)?,
                parameters: Some(self.array_id(source, parameters)?),
                r#type: self.optional_node_id(source, r#type)?,
                asterisk_token: self.optional_node_id(source, asterisk_token)?,
                question_token: self.optional_node_id(source, question_token)?,
                exclamation_token: None,
                body: self.optional_node_id(source, body)?,
                modifiers: self.optional_array_id(source, modifiers)?,
            }),
            flags,
        )
    }

    /// tsc-port: createConstructorDeclaration @6.0.3
    /// tsc-hash: c5eefe07225bf0585ce38867bc66134c557c7da79684697fa32e2cc899276163
    /// tsc-span: _tsc.js:21982-22001
    pub fn create_constructor_declaration(
        &mut self,
        source: TransformSourceId,
        modifiers: Option<TransformNodeArray>,
        parameters: TransformNodeArray,
        body: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let flags = if let Some(body) = body {
            self.children_flags(modifiers)?
                | self.children_flags(Some(parameters))?
                | (self.child_flags(Some(body))?
                    & !TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT)
                | TransformFlags::CONTAINS_ES_2015
        } else {
            TransformFlags::CONTAINS_TYPE_SCRIPT
        };
        self.create_node(
            source,
            NodeData::Constructor(ConstructorData {
                name: None,
                type_parameters: None,
                parameters: Some(self.array_id(source, parameters)?),
                r#type: None,
                body: self.optional_node_id(source, body)?,
                modifiers: self.optional_array_id(source, modifiers)?,
            }),
            flags,
        )
    }

    /// tsc-port: createGetAccessorDeclaration @6.0.3
    /// tsc-hash: f5c87298a2abeffe5ad1a6109406c98a7de78dfa72c5bd200e0c7d3e08b614c7
    /// tsc-span: _tsc.js:22012-22042
    pub fn create_get_accessor_declaration(
        &mut self,
        source: TransformSourceId,
        modifiers: Option<TransformNodeArray>,
        name: TransformNode,
        parameters: TransformNodeArray,
        r#type: Option<TransformNode>,
        body: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let flags = if let Some(body) = body {
            self.children_flags(modifiers)?
                | self.name_flags(Some(name))?
                | self.children_flags(Some(parameters))?
                | self.child_flags(r#type)?
                | (self.child_flags(Some(body))?
                    & !TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT)
                | if r#type.is_some() {
                    TransformFlags::CONTAINS_TYPE_SCRIPT
                } else {
                    TransformFlags::NONE
                }
        } else {
            TransformFlags::CONTAINS_TYPE_SCRIPT
        };
        self.create_node(
            source,
            NodeData::GetAccessor(GetAccessorData {
                name: Some(self.node_id(source, name)?),
                type_parameters: None,
                parameters: Some(self.array_id(source, parameters)?),
                r#type: self.optional_node_id(source, r#type)?,
                body: self.optional_node_id(source, body)?,
                modifiers: self.optional_array_id(source, modifiers)?,
            }),
            flags,
        )
    }

    /// tsc-port: createSetAccessorDeclaration @6.0.3
    /// tsc-hash: 3bba80705cdd8bd650d471958328abd977688a800de71f71ca1a55203bad4354
    /// tsc-span: _tsc.js:22043-22064
    pub fn create_set_accessor_declaration(
        &mut self,
        source: TransformSourceId,
        modifiers: Option<TransformNodeArray>,
        name: TransformNode,
        parameters: TransformNodeArray,
        body: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let flags = if let Some(body) = body {
            self.children_flags(modifiers)?
                | self.name_flags(Some(name))?
                | self.children_flags(Some(parameters))?
                | (self.child_flags(Some(body))?
                    & !TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT)
        } else {
            TransformFlags::CONTAINS_TYPE_SCRIPT
        };
        self.create_node(
            source,
            NodeData::SetAccessor(SetAccessorData {
                name: Some(self.node_id(source, name)?),
                type_parameters: None,
                parameters: Some(self.array_id(source, parameters)?),
                r#type: None,
                body: self.optional_node_id(source, body)?,
                modifiers: self.optional_array_id(source, modifiers)?,
            }),
            flags,
        )
    }

    /// tsc-port: createCallSignature @6.0.3
    /// tsc-hash: 32c5319dfe22e038d8d3dc8c218bdd631b95cd3dd11ef733087df755f2c30724
    /// tsc-span: _tsc.js:22075-22089
    pub fn create_call_signature(
        &mut self,
        source: TransformSourceId,
        type_parameters: Option<TransformNodeArray>,
        parameters: TransformNodeArray,
        r#type: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        self.create_node(
            source,
            NodeData::CallSignature(CallSignatureData {
                type_parameters: self.optional_array_id(source, type_parameters)?,
                parameters: Some(self.array_id(source, parameters)?),
                r#type: self.optional_node_id(source, r#type)?,
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: createConstructSignature @6.0.3
    /// tsc-hash: c33ed25cf62032c76f611cd1792312083b28a7be4b41539846b851a25609a261
    /// tsc-span: _tsc.js:22090-22104
    pub fn create_construct_signature(
        &mut self,
        source: TransformSourceId,
        type_parameters: Option<TransformNodeArray>,
        parameters: TransformNodeArray,
        r#type: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        self.create_node(
            source,
            NodeData::ConstructSignature(ConstructSignatureData {
                type_parameters: self.optional_array_id(source, type_parameters)?,
                parameters: Some(self.array_id(source, parameters)?),
                r#type: self.optional_node_id(source, r#type)?,
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: createIndexSignature @6.0.3
    /// tsc-hash: a124930b25a2975d85671d6890a0ccb372e9c5401be007da6e555d28a746b914
    /// tsc-span: _tsc.js:22105-22119
    pub fn create_index_signature(
        &mut self,
        source: TransformSourceId,
        modifiers: Option<TransformNodeArray>,
        parameters: TransformNodeArray,
        r#type: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.create_node(
            source,
            NodeData::IndexSignature(IndexSignatureData {
                type_parameters: None,
                parameters: Some(self.array_id(source, parameters)?),
                r#type: Some(self.node_id(source, r#type)?),
                modifiers: self.optional_array_id(source, modifiers)?,
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: createFunctionDeclaration @6.0.3
    /// tsc-hash: f0b57d5b5e5c6840a8efeaf5ba98282c519b05f1ce0d73ddedfc751e575812ae
    /// tsc-span: _tsc.js:23303-23328
    #[allow(clippy::too_many_arguments)]
    pub fn create_function_declaration(
        &mut self,
        source: TransformSourceId,
        modifiers: Option<TransformNodeArray>,
        asterisk_token: Option<TransformNode>,
        name: Option<TransformNode>,
        type_parameters: Option<TransformNodeArray>,
        parameters: TransformNodeArray,
        r#type: Option<TransformNode>,
        body: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let ambient = self
            .modifier_flags(modifiers)?
            .contains(ModifierFlags::AMBIENT);
        let flags = match body.filter(|_| !ambient) {
            None => TransformFlags::CONTAINS_TYPE_SCRIPT,
            Some(body) => self.function_like_flags(
                modifiers,
                asterisk_token,
                name,
                None,
                type_parameters,
                parameters,
                r#type,
                body,
                true,
                false,
            )?,
        };
        self.create_node(
            source,
            NodeData::FunctionDeclaration(FunctionDeclarationData {
                name: self.optional_node_id(source, name)?,
                type_parameters: self.optional_array_id(source, type_parameters)?,
                parameters: Some(self.array_id(source, parameters)?),
                r#type: self.optional_node_id(source, r#type)?,
                asterisk_token: self.optional_node_id(source, asterisk_token)?,
                body: self.optional_node_id(source, body)?,
                modifiers: self.optional_array_id(source, modifiers)?,
            }),
            flags,
        )
    }

    /// tsc-port: createFunctionExpression @6.0.3
    /// tsc-hash: 83a792bb983da8a130478b3366df8b8342f1363c86d100dd45b374794ed50473
    /// tsc-span: _tsc.js:22676-22700
    #[allow(clippy::too_many_arguments)]
    pub fn create_function_expression(
        &mut self,
        source: TransformSourceId,
        modifiers: Option<TransformNodeArray>,
        asterisk_token: Option<TransformNode>,
        name: Option<TransformNode>,
        type_parameters: Option<TransformNodeArray>,
        parameters: TransformNodeArray,
        r#type: Option<TransformNode>,
        body: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.function_like_flags(
            modifiers,
            asterisk_token,
            name,
            None,
            type_parameters,
            parameters,
            r#type,
            body,
            true,
            false,
        )?;
        self.create_node(
            source,
            NodeData::FunctionExpression(FunctionExpressionData {
                name: self.optional_node_id(source, name)?,
                type_parameters: self.optional_array_id(source, type_parameters)?,
                parameters: Some(self.array_id(source, parameters)?),
                r#type: self.optional_node_id(source, r#type)?,
                asterisk_token: self.optional_node_id(source, asterisk_token)?,
                body: Some(self.node_id(source, body)?),
                modifiers: self.optional_array_id(source, modifiers)?,
            }),
            flags,
        )
    }

    fn parenthesize_concise_body(
        &mut self,
        body: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if self.arena.node(body)?.kind == SyntaxKind::Block {
            return Ok(body);
        }
        let emitted = self.skip_partially_emitted_expressions(body)?;
        let comma = self.arena.node(emitted)?.kind == SyntaxKind::CommaListExpression
            || self.binary_operator(emitted)? == Some(SyntaxKind::CommaToken);
        let object = self
            .arena
            .node(self.leftmost_expression(emitted, false)?)?
            .kind
            == SyntaxKind::ObjectLiteralExpression;
        if !comma && !object {
            return Ok(body);
        }
        let flags = self.arena.propagate_child_flags(body)?;
        let parenthesized = self.create_node(
            body.source,
            NodeData::ParenthesizedExpression(ParenthesizedExpressionData {
                expression: Some(body.node),
            }),
            flags,
        )?;
        self.set_text_range(parenthesized, body)
    }

    /// tsc-port: createArrowFunction @6.0.3
    /// tsc-hash: 86ab9adbb9da5a28bf8a8dad687363b83f315bfa06def479590c1f305a367d72
    /// tsc-span: _tsc.js:22701-22719
    #[allow(clippy::too_many_arguments)]
    pub fn create_arrow_function(
        &mut self,
        source: TransformSourceId,
        modifiers: Option<TransformNodeArray>,
        type_parameters: Option<TransformNodeArray>,
        parameters: TransformNodeArray,
        r#type: Option<TransformNode>,
        equals_greater_than_token: Option<TransformNode>,
        body: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let equals_greater_than_token = match equals_greater_than_token {
            Some(token) => token,
            None => self.create_token(
                source,
                SyntaxKind::EqualsGreaterThanToken,
                TransformFlags::NONE,
            )?,
        };
        let body = self.parenthesize_concise_body(body)?;
        let is_async = self
            .modifier_flags(modifiers)?
            .contains(ModifierFlags::ASYNC);
        let mut flags = self.children_flags(modifiers)?
            | self.children_flags(type_parameters)?
            | self.children_flags(Some(parameters))?
            | self.child_flags(r#type)?
            | self.child_flags(Some(equals_greater_than_token))?
            | (self.child_flags(Some(body))? & !TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT)
            | TransformFlags::CONTAINS_ES_2015;
        if type_parameters.is_some() || r#type.is_some() {
            flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
        }
        if is_async {
            flags |= TransformFlags::CONTAINS_ES_2017 | TransformFlags::CONTAINS_LEXICAL_THIS;
        }
        self.create_node(
            source,
            NodeData::ArrowFunction(ArrowFunctionData {
                type_parameters: self.optional_array_id(source, type_parameters)?,
                parameters: Some(self.array_id(source, parameters)?),
                r#type: self.optional_node_id(source, r#type)?,
                body: Some(self.node_id(source, body)?),
                modifiers: self.optional_array_id(source, modifiers)?,
                equals_greater_than_token: Some(self.node_id(source, equals_greater_than_token)?),
            }),
            flags,
        )
    }

    /// tsc-port: createJSDocFunctionType @6.0.3
    /// tsc-hash: faedb83d3042fa68c12e00798ec7535289532e88b98900d6b25da17a8113b193
    /// tsc-span: _tsc.js:23707-23719
    pub fn create_jsdoc_function_type(
        &mut self,
        source: TransformSourceId,
        parameters: TransformNodeArray,
        r#type: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let mut flags = self.children_flags(Some(parameters))?;
        if r#type.is_some() {
            flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
        }
        self.create_node(
            source,
            NodeData::JSDocFunctionType(JSDocFunctionTypeData {
                name: None,
                type_parameters: None,
                parameters: Some(self.array_id(source, parameters)?),
                r#type: self.optional_node_id(source, r#type)?,
            }),
            flags,
        )
    }

    /// tsc-port: createBlock @6.0.3
    /// tsc-hash: 55f45ae70ca3cf1ff88e796f6f2903492a2c2b0a04c6171196e0954bff771350
    /// tsc-span: _tsc.js:23045-23057
    pub fn create_block(
        &mut self,
        source: TransformSourceId,
        statements: TransformNodeArray,
        multi_line: bool,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.children_flags(Some(statements))?;
        let node = self.create_node(
            source,
            NodeData::Block(BlockData {
                statements: Some(self.array_id(source, statements)?),
            }),
            flags,
        )?;
        self.set_multi_line(node, multi_line)
    }

    fn parenthesize_operand_of_prefix_unary(
        &mut self,
        operand: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let emitted = self.skip_partially_emitted_expressions(operand)?;
        if self.expression_precedence(emitted)? >= PRECEDENCE_UNARY {
            return Ok(operand);
        }
        let flags = self.arena.propagate_child_flags(operand)?;
        let parenthesized = self.create_node(
            operand.source,
            NodeData::ParenthesizedExpression(ParenthesizedExpressionData {
                expression: Some(operand.node),
            }),
            flags,
        )?;
        self.set_text_range(parenthesized, operand)
    }

    /// tsc-port: createPrefixUnaryExpression @6.0.3
    /// tsc-hash: 9340e19524cc302b3a5462effae0ae6072b65466ecc42b2fdfdeaa98253fa818
    /// tsc-span: _tsc.js:22759-22769
    pub fn create_prefix_unary_expression(
        &mut self,
        source: TransformSourceId,
        operator: SyntaxKind,
        operand: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let operand = self.parenthesize_operand_of_prefix_unary(operand)?;
        let mut flags = self.child_flags(Some(operand))?;
        let ordinary_identifier = self.arena.node(operand)?.kind == SyntaxKind::Identifier
            && self.arena.metadata(operand).is_none_or(|metadata| {
                metadata.generated_binding_id().is_none()
                    && !metadata.flags().contains(EmitFlags::LOCAL_NAME)
            });
        if matches!(
            operator,
            SyntaxKind::PlusPlusToken | SyntaxKind::MinusMinusToken
        ) && ordinary_identifier
        {
            flags |= TransformFlags::CONTAINS_UPDATE_EXPRESSION_FOR_IDENTIFIER;
        }
        self.create_node(
            source,
            NodeData::PrefixUnaryExpression(PrefixUnaryExpressionData {
                operator,
                operand: Some(self.node_id(source, operand)?),
            }),
            flags,
        )
    }

    fn parenthesize_left_side_of_access(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let emitted = self.skip_partially_emitted_expressions(expression)?;
        let record = self.arena.node(emitted)?;
        let optional = NodeFlags::from_bits(record.flags).contains(NodeFlags::OPTIONAL_CHAIN);
        let left_hand_side = self.expression_precedence(emitted)? >= PRECEDENCE_LEFT_HAND_SIDE
            && !(record.kind == SyntaxKind::NewExpression
                && matches!(&record.data, NodeData::NewExpression(data) if data.arguments.is_none()));
        if left_hand_side && !optional {
            return Ok(expression);
        }
        let flags = self.arena.propagate_child_flags(expression)?;
        let parenthesized = self.create_node(
            expression.source,
            NodeData::ParenthesizedExpression(ParenthesizedExpressionData {
                expression: Some(expression.node),
            }),
            flags,
        )?;
        self.set_text_range(parenthesized, expression)
    }

    /// tsc-port: createPropertyAccessExpression @6.0.3
    /// tsc-hash: ed73474ca2fac1e817ecac78b28378368a34a5a097d3c81e9f138777a654d05c
    /// tsc-span: _tsc.js:22474-22488
    pub fn create_property_access_expression(
        &mut self,
        source: TransformSourceId,
        expression: TransformNode,
        name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let original_expression = expression;
        let expression = self.parenthesize_left_side_of_access(expression)?;
        let name_flags = if self.arena.node(name)?.kind == SyntaxKind::Identifier {
            self.name_flags(Some(name))?
        } else {
            self.child_flags(Some(name))?
                | TransformFlags::CONTAINS_PRIVATE_IDENTIFIER_IN_EXPRESSION
        };
        let mut flags = self.child_flags(Some(expression))? | name_flags;
        if self.arena.node(original_expression)?.kind == SyntaxKind::SuperKeyword {
            flags |= TransformFlags::CONTAINS_ES_2017 | TransformFlags::CONTAINS_ES_2018;
        }
        self.create_node(
            source,
            NodeData::PropertyAccessExpression(PropertyAccessExpressionData {
                name: Some(self.node_id(source, name)?),
                expression: Some(self.node_id(source, expression)?),
                question_dot_token: None,
            }),
            flags,
        )
    }

    /// tsc-port: createElementAccessExpression @6.0.3
    /// tsc-hash: 803a9754018ad7ca337808e56e5ebfbc2f26dc717082946b1f4b30d9b13f8368
    /// tsc-span: _tsc.js:22524-22538
    pub fn create_element_access_expression(
        &mut self,
        source: TransformSourceId,
        expression: TransformNode,
        index: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let original_expression = expression;
        let expression = self.parenthesize_left_side_of_access(expression)?;
        let mut flags = self.child_flags(Some(expression))? | self.child_flags(Some(index))?;
        if self.arena.node(original_expression)?.kind == SyntaxKind::SuperKeyword {
            flags |= TransformFlags::CONTAINS_ES_2017 | TransformFlags::CONTAINS_ES_2018;
        }
        self.create_node(
            source,
            NodeData::ElementAccessExpression(ElementAccessExpressionData {
                expression: Some(self.node_id(source, expression)?),
                question_dot_token: None,
                argument_expression: Some(self.node_id(source, index)?),
            }),
            flags,
        )
    }

    /// tsc-port: updateBindingElement @6.0.3
    /// tsc-hash: ceb986743d45f7b61e48c74a230ec55ddc7d87cdd373504d1461c786efe5c0e7
    /// tsc-span: _tsc.js:22438-22440
    pub fn update_binding_element(
        &mut self,
        original: TransformNode,
        dot_dot_dot_token: Option<TransformNode>,
        property_name: Option<TransformNode>,
        name: TransformNode,
        initializer: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::BindingElement(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::BindingElement,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.dot_dot_dot_token == dot_dot_dot_token.map(TransformNode::node)
            && data.property_name == property_name.map(TransformNode::node)
            && data.name == Some(name.node)
            && data.initializer == initializer.map(TransformNode::node)
        {
            return Ok(original);
        }
        let mut flags = self.child_flags(dot_dot_dot_token)?
            | self.name_flags(property_name)?
            | self.name_flags(Some(name))?
            | self.child_flags(initializer)?
            | TransformFlags::CONTAINS_ES_2015;
        if dot_dot_dot_token.is_some() {
            flags |= TransformFlags::CONTAINS_REST_OR_SPREAD;
        }
        let updated = self.create_node(
            original.source,
            NodeData::BindingElement(BindingElementData {
                name: Some(self.node_id(original.source, name)?),
                property_name: self.optional_node_id(original.source, property_name)?,
                dot_dot_dot_token: self.optional_node_id(original.source, dot_dot_dot_token)?,
                initializer: self.optional_node_id(original.source, initializer)?,
            }),
            flags,
        )?;
        self.finish_update(updated, original)
    }

    /// tsc-port: createVariableDeclaration @6.0.3
    /// tsc-hash: f69a941184a04a14a893a7f878efe04e38c34c7ef2fd9d0208dfd6b3b1e49585
    /// tsc-span: _tsc.js:23274-23286
    pub fn create_variable_declaration(
        &mut self,
        source: TransformSourceId,
        name: TransformNode,
        exclamation_token: Option<TransformNode>,
        r#type: Option<TransformNode>,
        initializer: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let mut flags = self.name_flags(Some(name))? | self.child_flags(initializer)?;
        if exclamation_token.is_some() || r#type.is_some() {
            flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
        }
        self.create_node(
            source,
            NodeData::VariableDeclaration(VariableDeclarationData {
                name: Some(self.node_id(source, name)?),
                exclamation_token: self.optional_node_id(source, exclamation_token)?,
                r#type: self.optional_node_id(source, r#type)?,
                initializer: self.optional_node_id(source, initializer)?,
            }),
            flags,
        )
    }

    /// tsc-port: createVariableDeclarationList @6.0.3
    /// tsc-hash: 324d5fd5f464f3047fb5d2b3c5761cd3a469334262e88077a32c862a5c1051d8
    /// tsc-span: _tsc.js:23287-23299
    pub fn create_variable_declaration_list(
        &mut self,
        source: TransformSourceId,
        declarations: TransformNodeArray,
        node_flags: NodeFlags,
    ) -> Result<TransformNode, TransformError> {
        let mut flags = self.children_flags(Some(declarations))?
            | TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION;
        if node_flags.intersects(NodeFlags::BLOCK_SCOPED) {
            flags |=
                TransformFlags::CONTAINS_ES_2015 | TransformFlags::CONTAINS_BLOCK_SCOPED_BINDING;
        }
        if node_flags.intersects(NodeFlags::USING) {
            flags |= TransformFlags::CONTAINS_ES_NEXT;
        }
        let node = self.create_node(
            source,
            NodeData::VariableDeclarationList(VariableDeclarationListData {
                declarations: Some(self.array_id(source, declarations)?),
            }),
            flags,
        )?;
        self.set_node_flags(node, node_flags & NodeFlags::BLOCK_SCOPED)
    }

    /// tsc-port: createVariableStatement @6.0.3
    /// tsc-hash: d983b3f4c50c1983954810c3e38b933886c1ea1c0dd82ca010fd172f14f138ac
    /// tsc-span: _tsc.js:23058-23072
    pub fn create_variable_statement(
        &mut self,
        source: TransformSourceId,
        modifiers: Option<TransformNodeArray>,
        declaration_list: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = if self
            .modifier_flags(modifiers)?
            .contains(ModifierFlags::AMBIENT)
        {
            TransformFlags::CONTAINS_TYPE_SCRIPT
        } else {
            self.children_flags(modifiers)? | self.child_flags(Some(declaration_list))?
        };
        self.create_node(
            source,
            NodeData::VariableStatement(VariableStatementData {
                modifiers: self.optional_array_id(source, modifiers)?,
                declaration_list: Some(self.node_id(source, declaration_list)?),
            }),
            flags,
        )
    }

    /// tsc-port: createEmptyStatement @6.0.3
    /// tsc-hash: bd30f754766c0eba7d66bcce0b1bc25b080d95febd09f6d40fccdbbaa9b3a86a
    /// tsc-span: _tsc.js:23073-23077
    pub fn create_empty_statement(
        &mut self,
        source: TransformSourceId,
    ) -> Result<TransformNode, TransformError> {
        self.create_node(
            source,
            NodeData::EmptyStatement(EmptyStatementData {}),
            TransformFlags::NONE,
        )
    }

    /// tsc-port: createExpressionStatement @6.0.3
    /// tsc-hash: 356fde9ecc71857651c8a89b5afd1e5ff130243b093215225c190b4dc1c58c29
    /// tsc-span: _tsc.js:23078-23085
    pub fn create_expression_statement(
        &mut self,
        source: TransformSourceId,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.child_flags(Some(expression))?;
        self.create_node(
            source,
            NodeData::ExpressionStatement(ExpressionStatementData {
                expression: Some(self.node_id(source, expression)?),
            }),
            flags,
        )
    }

    /// tsc-port: createClassDeclaration @6.0.3
    /// tsc-hash: 4116022701afc60714ac17403b6470636b06686462a2951b6a090fab0c87f1c9
    /// tsc-span: _tsc.js:23339-23356
    pub fn create_class_declaration(
        &mut self,
        source: TransformSourceId,
        modifiers: Option<TransformNodeArray>,
        name: Option<TransformNode>,
        type_parameters: Option<TransformNodeArray>,
        heritage_clauses: Option<TransformNodeArray>,
        members: TransformNodeArray,
    ) -> Result<TransformNode, TransformError> {
        let ambient = self
            .modifier_flags(modifiers)?
            .contains(ModifierFlags::AMBIENT);
        let mut flags = if ambient {
            TransformFlags::CONTAINS_TYPE_SCRIPT
        } else {
            self.children_flags(modifiers)?
                | self.name_flags(name)?
                | self.children_flags(type_parameters)?
                | self.children_flags(heritage_clauses)?
                | self.children_flags(Some(members))?
                | TransformFlags::CONTAINS_ES_2015
        };
        if !ambient && type_parameters.is_some() {
            flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
        }
        if flags.contains(TransformFlags::CONTAINS_TYPE_SCRIPT_CLASS_SYNTAX) {
            flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
        }
        self.create_node(
            source,
            NodeData::ClassDeclaration(ClassDeclarationData {
                name: self.optional_node_id(source, name)?,
                type_parameters: self.optional_array_id(source, type_parameters)?,
                heritage_clauses: self.optional_array_id(source, heritage_clauses)?,
                members: Some(self.array_id(source, members)?),
                modifiers: self.optional_array_id(source, modifiers)?,
            }),
            flags,
        )
    }

    /// tsc-port: updateClassDeclaration @6.0.3
    /// tsc-hash: e9a9bbea29832f1ee7b89c669fcbdabf91cf8994cb643fdf51f0953f531859af
    /// tsc-span: _tsc.js:23357-23359
    pub fn update_class_declaration(
        &mut self,
        original: TransformNode,
        modifiers: Option<TransformNodeArray>,
        name: Option<TransformNode>,
        type_parameters: Option<TransformNodeArray>,
        heritage_clauses: Option<TransformNodeArray>,
        members: TransformNodeArray,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::ClassDeclaration(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::ClassDeclaration,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.modifiers == modifiers.map(TransformNodeArray::array)
            && data.name == name.map(TransformNode::node)
            && data.type_parameters == type_parameters.map(TransformNodeArray::array)
            && data.heritage_clauses == heritage_clauses.map(TransformNodeArray::array)
            && data.members == Some(members.array)
        {
            return Ok(original);
        }
        let updated = self.create_class_declaration(
            original.source,
            modifiers,
            name,
            type_parameters,
            heritage_clauses,
            members,
        )?;
        self.finish_update(updated, original)
    }

    /// tsc-port: createInterfaceDeclaration @6.0.3
    /// tsc-hash: fd7593ca57140b4d21704e0086a9a3fe7de50a9995942c7c016bce5b9ce7fb2f
    /// tsc-span: _tsc.js:23360-23373
    pub fn create_interface_declaration(
        &mut self,
        source: TransformSourceId,
        modifiers: Option<TransformNodeArray>,
        name: TransformNode,
        type_parameters: Option<TransformNodeArray>,
        heritage_clauses: Option<TransformNodeArray>,
        members: TransformNodeArray,
    ) -> Result<TransformNode, TransformError> {
        self.create_node(
            source,
            NodeData::InterfaceDeclaration(InterfaceDeclarationData {
                name: Some(self.node_id(source, name)?),
                modifiers: self.optional_array_id(source, modifiers)?,
                type_parameters: self.optional_array_id(source, type_parameters)?,
                heritage_clauses: self.optional_array_id(source, heritage_clauses)?,
                members: Some(self.array_id(source, members)?),
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: createTypeAliasDeclaration @6.0.3
    /// tsc-hash: b3e49567db105fe68eeee9696bc6aaa4bdaf62cdb5d7f886c371ac5fc9f1480b
    /// tsc-span: _tsc.js:23374-23388
    pub fn create_type_alias_declaration(
        &mut self,
        source: TransformSourceId,
        modifiers: Option<TransformNodeArray>,
        name: TransformNode,
        type_parameters: Option<TransformNodeArray>,
        r#type: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.create_node(
            source,
            NodeData::TypeAliasDeclaration(TypeAliasDeclarationData {
                name: Some(self.node_id(source, name)?),
                modifiers: self.optional_array_id(source, modifiers)?,
                type_parameters: self.optional_array_id(source, type_parameters)?,
                r#type: Some(self.node_id(source, r#type)?),
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
    }

    /// tsc-port: createEnumDeclaration @6.0.3
    /// tsc-hash: 0c84f0f447d6956fce64d8966fa7255547aac15f32c1185bd84db88281603e6b
    /// tsc-span: _tsc.js:23389-23401
    pub fn create_enum_declaration(
        &mut self,
        source: TransformSourceId,
        modifiers: Option<TransformNodeArray>,
        name: TransformNode,
        members: TransformNodeArray,
    ) -> Result<TransformNode, TransformError> {
        let flags = (self.children_flags(modifiers)?
            | self.child_flags(Some(name))?
            | self.children_flags(Some(members))?
            | TransformFlags::CONTAINS_TYPE_SCRIPT)
            & !TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT;
        self.create_node(
            source,
            NodeData::EnumDeclaration(EnumDeclarationData {
                name: Some(self.node_id(source, name)?),
                modifiers: self.optional_array_id(source, modifiers)?,
                members: Some(self.array_id(source, members)?),
            }),
            flags,
        )
    }

    /// tsc-port: createModuleDeclaration @6.0.3
    /// tsc-hash: 5c93acf62127508d0a9e52f9662a085401065bf8333ddf36457fee89f385c8b5
    /// tsc-span: _tsc.js:23402-23418
    pub fn create_module_declaration(
        &mut self,
        source: TransformSourceId,
        modifiers: Option<TransformNodeArray>,
        name: TransformNode,
        body: Option<TransformNode>,
        node_flags: NodeFlags,
    ) -> Result<TransformNode, TransformError> {
        let ambient = self
            .modifier_flags(modifiers)?
            .contains(ModifierFlags::AMBIENT);
        let flags = if ambient {
            TransformFlags::CONTAINS_TYPE_SCRIPT
        } else {
            self.children_flags(modifiers)?
                | self.child_flags(Some(name))?
                | self.child_flags(body)?
                | TransformFlags::CONTAINS_TYPE_SCRIPT
        } & !TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT;
        let node = self.create_node(
            source,
            NodeData::ModuleDeclaration(ModuleDeclarationData {
                name: Some(self.node_id(source, name)?),
                modifiers: self.optional_array_id(source, modifiers)?,
                body: self.optional_node_id(source, body)?,
            }),
            flags,
        )?;
        self.set_node_flags(
            node,
            node_flags
                & (NodeFlags::NAMESPACE
                    | NodeFlags::NESTED_NAMESPACE
                    | NodeFlags::GLOBAL_AUGMENTATION),
        )
    }

    /// tsc-port: updateModuleDeclaration @6.0.3
    /// tsc-hash: fec3de9c79133c1b2261e7db8f5b5bed6fc43d035ec2d99f0020a86d22f90d5d
    /// tsc-span: _tsc.js:23419-23421
    pub fn update_module_declaration(
        &mut self,
        original: TransformNode,
        modifiers: Option<TransformNodeArray>,
        name: TransformNode,
        body: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let record = self.arena.node(original)?.clone();
        let NodeData::ModuleDeclaration(data) = &record.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::ModuleDeclaration,
                actual: record.kind,
            });
        };
        if data.modifiers == modifiers.map(TransformNodeArray::array)
            && data.name == Some(name.node)
            && data.body == body.map(TransformNode::node)
        {
            return Ok(original);
        }
        let updated = self.create_module_declaration(
            original.source,
            modifiers,
            name,
            body,
            NodeFlags::from_bits(record.flags),
        )?;
        self.finish_update(updated, original)
    }

    /// tsc-port: createModuleBlock @6.0.3
    /// tsc-hash: 2a85268a91eb2ed412acb4198bbbafee3460abbb477164fa9f7a8cbef67b884a
    /// tsc-span: _tsc.js:23422-23428
    pub fn create_module_block(
        &mut self,
        source: TransformSourceId,
        statements: TransformNodeArray,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.children_flags(Some(statements))?;
        self.create_node(
            source,
            NodeData::ModuleBlock(ModuleBlockData {
                statements: Some(self.array_id(source, statements)?),
            }),
            flags,
        )
    }

    /// tsc-port: updateModuleBlock @6.0.3
    /// tsc-hash: 75600d68fbf2ce6e8b0f74a61686bfafce59dbeb60d37780e75ba3670c539245
    /// tsc-span: _tsc.js:23429-23431
    pub fn update_module_block(
        &mut self,
        original: TransformNode,
        statements: TransformNodeArray,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::ModuleBlock(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::ModuleBlock,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.statements == Some(statements.array) {
            return Ok(original);
        }
        let updated = self.create_module_block(original.source, statements)?;
        self.finish_update(updated, original)
    }

    /// tsc-port: createNamespaceExportDeclaration @6.0.3
    /// tsc-hash: 2afc9f27c4434b1c6144e4b96b2ebf2d6d33b7dfc2294225456814dfeea54eef
    /// tsc-span: _tsc.js:23443-23451
    pub fn create_namespace_export_declaration(
        &mut self,
        source: TransformSourceId,
        name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.name_flags(Some(name))? | TransformFlags::CONTAINS_TYPE_SCRIPT;
        self.create_node(
            source,
            NodeData::NamespaceExportDeclaration(NamespaceExportDeclarationData {
                name: Some(self.node_id(source, name)?),
                modifiers: None,
            }),
            flags,
        )
    }

    /// tsc-port: createImportEqualsDeclaration @6.0.3
    /// tsc-hash: 6af6e2d7c08f4e62fbdc9ba6e516855e34f4ef0b03f82127b378eee9fe8cd8b1
    /// tsc-span: _tsc.js:23460-23476
    pub fn create_import_equals_declaration(
        &mut self,
        source: TransformSourceId,
        modifiers: Option<TransformNodeArray>,
        is_type_only: bool,
        name: TransformNode,
        module_reference: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let mut flags = self.children_flags(modifiers)?
            | self.name_flags(Some(name))?
            | self.child_flags(Some(module_reference))?;
        if self.arena.node(module_reference)?.kind != SyntaxKind::ExternalModuleReference {
            flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
        }
        flags = flags & !TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT;
        self.create_node(
            source,
            NodeData::ImportEqualsDeclaration(ImportEqualsDeclarationData {
                name: Some(self.node_id(source, name)?),
                modifiers: self.optional_array_id(source, modifiers)?,
                is_type_only,
                module_reference: Some(self.node_id(source, module_reference)?),
            }),
            flags,
        )
    }

    /// tsc-port: createImportDeclaration @6.0.3
    /// tsc-hash: e116f2faf2ee4cce4b7326f5583963fd4d6cfee54db85f41ba3d4ad3f64fec2f
    /// tsc-span: _tsc.js:23477-23490
    pub fn create_import_declaration(
        &mut self,
        source: TransformSourceId,
        modifiers: Option<TransformNodeArray>,
        import_clause: Option<TransformNode>,
        module_specifier: TransformNode,
        attributes: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let flags = (self.child_flags(import_clause)?
            | self.child_flags(Some(module_specifier))?)
            & !TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT;
        self.create_node(
            source,
            NodeData::ImportDeclaration(ImportDeclarationData {
                modifiers: self.optional_array_id(source, modifiers)?,
                import_clause: self.optional_node_id(source, import_clause)?,
                module_specifier: Some(self.node_id(source, module_specifier)?),
                attributes: self.optional_node_id(source, attributes)?,
            }),
            flags,
        )
    }

    /// tsc-port: createImportClause @6.0.3
    /// tsc-hash: 60b27f351bbfe4e46fb868ceb2fb98f79b9cbf81ed0c051b43529f496ee7d7c7
    /// tsc-span: _tsc.js:23491-23510
    pub fn create_import_clause(
        &mut self,
        source: TransformSourceId,
        phase_modifier: Option<SyntaxKind>,
        name: Option<TransformNode>,
        named_bindings: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let mut flags = self.child_flags(name)? | self.child_flags(named_bindings)?;
        if phase_modifier == Some(SyntaxKind::TypeKeyword) {
            flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
        }
        flags = flags & !TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT;
        self.create_node(
            source,
            NodeData::ImportClause(ImportClauseData {
                name: self.optional_node_id(source, name)?,
                is_type_only: phase_modifier == Some(SyntaxKind::TypeKeyword),
                phase_modifier,
                named_bindings: self.optional_node_id(source, named_bindings)?,
            }),
            flags,
        )
    }

    /// tsc-port: createImportAttributes @6.0.3
    /// tsc-hash: 50ab88de5743ce92432c926b4efcba8c1cdd887d900be70c148f9e670363ffdd
    /// tsc-span: _tsc.js:23543-23553
    pub fn create_import_attributes(
        &mut self,
        source: TransformSourceId,
        elements: TransformNodeArray,
        multi_line: Option<bool>,
        token: Option<SyntaxKind>,
    ) -> Result<TransformNode, TransformError> {
        self.create_node(
            source,
            NodeData::ImportAttributes(ImportAttributesData {
                token: token.unwrap_or(SyntaxKind::WithKeyword),
                elements: Some(self.array_id(source, elements)?),
                multi_line,
            }),
            TransformFlags::CONTAINS_ES_NEXT,
        )
    }

    /// tsc-port: createImportAttribute @6.0.3
    /// tsc-hash: ca97951e35495e6668087824324d39f42da8fa45dac3986515bf6a742f959abb
    /// tsc-span: _tsc.js:23554-23560
    pub fn create_import_attribute(
        &mut self,
        source: TransformSourceId,
        name: TransformNode,
        value: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.create_node(
            source,
            NodeData::ImportAttribute(ImportAttributeData {
                name: Some(self.node_id(source, name)?),
                value: Some(self.node_id(source, value)?),
            }),
            TransformFlags::CONTAINS_ES_NEXT,
        )
    }

    /// tsc-port: createNamespaceImport @6.0.3
    /// tsc-hash: b79562e8c9aae08811a7d9779a7cf41040904a880e1677019aa3f3f723878c13
    /// tsc-span: _tsc.js:23564-23573
    pub fn create_namespace_import(
        &mut self,
        source: TransformSourceId,
        name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags =
            self.child_flags(Some(name))? & !TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT;
        self.create_node(
            source,
            NodeData::NamespaceImport(NamespaceImportData {
                name: Some(self.node_id(source, name)?),
            }),
            flags,
        )
    }

    /// tsc-port: createNamespaceExport @6.0.3
    /// tsc-hash: 2bef163fa8ccf36f2fcbb0d4583c0fee21ab5607bd7c13a1785ca8440058dd59
    /// tsc-span: _tsc.js:23574-23583
    pub fn create_namespace_export(
        &mut self,
        source: TransformSourceId,
        name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = (self.child_flags(Some(name))? | TransformFlags::CONTAINS_ES_2020)
            & !TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT;
        self.create_node(
            source,
            NodeData::NamespaceExport(NamespaceExportData {
                name: Some(self.node_id(source, name)?),
            }),
            flags,
        )
    }

    /// tsc-port: createNamedImports @6.0.3
    /// tsc-hash: 6859e60e136be0c2291d2340d91b1b81f2f05c18737de28d110ad6b64319bfb3
    /// tsc-span: _tsc.js:23584-23593
    pub fn create_named_imports(
        &mut self,
        source: TransformSourceId,
        elements: TransformNodeArray,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.children_flags(Some(elements))?
            & !TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT;
        self.create_node(
            source,
            NodeData::NamedImports(NamedImportsData {
                elements: Some(self.array_id(source, elements)?),
            }),
            flags,
        )
    }

    /// tsc-port: createImportSpecifier @6.0.3
    /// tsc-hash: b6a75ae8f3acaf31a832a2a9fb939905b848197371dbd4ca84d40e86c5e92b84
    /// tsc-span: _tsc.js:23594-23605
    pub fn create_import_specifier(
        &mut self,
        source: TransformSourceId,
        is_type_only: bool,
        property_name: Option<TransformNode>,
        name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = (self.child_flags(property_name)? | self.child_flags(Some(name))?)
            & !TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT;
        self.create_node(
            source,
            NodeData::ImportSpecifier(ImportSpecifierData {
                name: Some(self.node_id(source, name)?),
                property_name: self.optional_node_id(source, property_name)?,
                is_type_only,
            }),
            flags,
        )
    }

    /// tsc-port: createExportAssignment @6.0.3
    /// tsc-hash: ce4faa5df88ad7ee17d05b67cd4235cf005964aa029f49898c4b40039cbcf351
    /// tsc-span: _tsc.js:23606-23623
    pub fn create_export_assignment(
        &mut self,
        source: TransformSourceId,
        modifiers: Option<TransformNodeArray>,
        is_export_equals: bool,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = (self.children_flags(modifiers)? | self.child_flags(Some(expression))?)
            & !TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT;
        self.create_node(
            source,
            NodeData::ExportAssignment(ExportAssignmentData {
                modifiers: self.optional_array_id(source, modifiers)?,
                is_export_equals: Some(is_export_equals),
                expression: Some(self.node_id(source, expression)?),
            }),
            flags,
        )
    }

    /// tsc-port: createExportDeclaration @6.0.3
    /// tsc-hash: 9a0b5dd2decaa4205a68486280acf4778c851fb338a6c64fd464c3fba40d3174
    /// tsc-span: _tsc.js:23624-23635
    pub fn create_export_declaration(
        &mut self,
        source: TransformSourceId,
        modifiers: Option<TransformNodeArray>,
        is_type_only: bool,
        export_clause: Option<TransformNode>,
        module_specifier: Option<TransformNode>,
        attributes: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let flags = (self.children_flags(modifiers)?
            | self.child_flags(export_clause)?
            | self.child_flags(module_specifier)?)
            & !TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT;
        self.create_node(
            source,
            NodeData::ExportDeclaration(ExportDeclarationData {
                modifiers: self.optional_array_id(source, modifiers)?,
                is_type_only,
                export_clause: self.optional_node_id(source, export_clause)?,
                module_specifier: self.optional_node_id(source, module_specifier)?,
                attributes: self.optional_node_id(source, attributes)?,
            }),
            flags,
        )
    }

    /// tsc-port: updateExportDeclaration @6.0.3
    /// tsc-hash: fc193596760f2607c4177c742263b4ca9f3ac18c4e52ef79fb8ab05c5bd1294d
    /// tsc-span: _tsc.js:23636-23646
    pub fn update_export_declaration(
        &mut self,
        original: TransformNode,
        modifiers: Option<TransformNodeArray>,
        is_type_only: bool,
        export_clause: Option<TransformNode>,
        module_specifier: Option<TransformNode>,
        attributes: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::ExportDeclaration(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::ExportDeclaration,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.modifiers == modifiers.map(TransformNodeArray::array)
            && data.is_type_only == is_type_only
            && data.export_clause == export_clause.map(TransformNode::node)
            && data.module_specifier == module_specifier.map(TransformNode::node)
            && data.attributes == attributes.map(TransformNode::node)
        {
            return Ok(original);
        }
        let updated = self.create_export_declaration(
            original.source,
            modifiers,
            is_type_only,
            export_clause,
            module_specifier,
            attributes,
        )?;
        self.finish_update(updated, original)
    }

    /// tsc-port: createNamedExports @6.0.3
    /// tsc-hash: 460b70c2210a56afe53208e9ca9c35d8d0e307f8136796277e27424d870d8093
    /// tsc-span: _tsc.js:23647-23653
    pub fn create_named_exports(
        &mut self,
        source: TransformSourceId,
        elements: TransformNodeArray,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.children_flags(Some(elements))?
            & !TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT;
        self.create_node(
            source,
            NodeData::NamedExports(NamedExportsData {
                elements: Some(self.array_id(source, elements)?),
            }),
            flags,
        )
    }

    /// tsc-port: updateNamedExports @6.0.3
    /// tsc-hash: 7aeb946bff86ab842ddb34d311d51311e0a27be6fc2b04b6a4ec43f8ae4230c2
    /// tsc-span: _tsc.js:23654-23656
    pub fn update_named_exports(
        &mut self,
        original: TransformNode,
        elements: TransformNodeArray,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::NamedExports(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::NamedExports,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.elements == Some(elements.array) {
            return Ok(original);
        }
        let updated = self.create_named_exports(original.source, elements)?;
        self.finish_update(updated, original)
    }

    /// tsc-port: createExportSpecifier @6.0.3
    /// tsc-hash: f58a66c6503fcb1ff04e93dfe984e9892e0a006efc1d44369110321ac37ed46a
    /// tsc-span: _tsc.js:23657-23668
    pub fn create_export_specifier(
        &mut self,
        source: TransformSourceId,
        is_type_only: bool,
        property_name: Option<TransformNode>,
        name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = (self.child_flags(property_name)? | self.child_flags(Some(name))?)
            & !TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT;
        self.create_node(
            source,
            NodeData::ExportSpecifier(ExportSpecifierData {
                name: Some(self.node_id(source, name)?),
                is_type_only,
                property_name: self.optional_node_id(source, property_name)?,
            }),
            flags,
        )
    }

    /// tsc-port: createExternalModuleReference @6.0.3
    /// tsc-hash: f50a611d50a0ac36401a62a3ff2d1e53a24bfccc11eab10f28bae9cadc0800a7
    /// tsc-span: _tsc.js:23675-23683
    pub fn create_external_module_reference(
        &mut self,
        source: TransformSourceId,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.child_flags(Some(expression))?
            & !TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT;
        self.create_node(
            source,
            NodeData::ExternalModuleReference(ExternalModuleReferenceData {
                expression: Some(self.node_id(source, expression)?),
            }),
            flags,
        )
    }

    /// tsc-port: createHeritageClause @6.0.3
    /// tsc-hash: 709c54e3478568976cdbd05fd9248867e86cc414e67138ce1d7bdf96d912cbc0
    /// tsc-span: _tsc.js:24106-24122
    pub fn create_heritage_clause(
        &mut self,
        source: TransformSourceId,
        token: SyntaxKind,
        types: TransformNodeArray,
    ) -> Result<TransformNode, TransformError> {
        let mut flags = self.children_flags(Some(types))?;
        flags |= match token {
            SyntaxKind::ExtendsKeyword => TransformFlags::CONTAINS_ES_2015,
            SyntaxKind::ImplementsKeyword => TransformFlags::CONTAINS_TYPE_SCRIPT,
            _ => {
                return Err(TransformError::FactoryKindMismatch {
                    expected: SyntaxKind::ExtendsKeyword,
                    actual: token,
                })
            }
        };
        self.create_node(
            source,
            NodeData::HeritageClause(HeritageClauseData {
                token,
                types: Some(self.array_id(source, types)?),
            }),
            flags,
        )
    }

    /// tsc-port: createEnumMember @6.0.3
    /// tsc-hash: 0e79cb062e2688378b68df053d049c0c451f1cab85d9e9b1ddfe201a228d3118
    /// tsc-span: _tsc.js:24194-24202
    pub fn create_enum_member(
        &mut self,
        source: TransformSourceId,
        name: TransformNode,
        initializer: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.child_flags(Some(name))?
            | self.child_flags(initializer)?
            | TransformFlags::CONTAINS_TYPE_SCRIPT;
        self.create_node(
            source,
            NodeData::EnumMember(EnumMemberData {
                name: Some(self.node_id(source, name)?),
                initializer: self.optional_node_id(source, initializer)?,
            }),
            flags,
        )
    }

    /// tsc-port: replaceModifiers @6.0.3
    /// tsc-hash: d4de45449da7572d85b4b83532e6a9ba1d825bb5a84fd6678f65f5476f854221
    /// tsc-span: _tsc.js:24933-24944
    pub fn replace_modifiers(
        &mut self,
        original: TransformNode,
        modifiers: Option<TransformNodeArray>,
    ) -> Result<TransformNode, TransformError> {
        let record = self.arena.node(original)?.clone();
        let modifiers_id = self.optional_array_id(original.source, modifiers)?;
        let current_modifiers = match &record.data {
            NodeData::TypeParameter(data) => data.modifiers,
            NodeData::Parameter(data) => data.modifiers,
            NodeData::ConstructorType(data) => data.modifiers,
            NodeData::PropertySignature(data) => data.modifiers,
            NodeData::PropertyDeclaration(data) => data.modifiers,
            NodeData::MethodSignature(data) => data.modifiers,
            NodeData::MethodDeclaration(data) => data.modifiers,
            NodeData::Constructor(data) => data.modifiers,
            NodeData::GetAccessor(data) => data.modifiers,
            NodeData::SetAccessor(data) => data.modifiers,
            NodeData::IndexSignature(data) => data.modifiers,
            NodeData::FunctionExpression(data) => data.modifiers,
            NodeData::ArrowFunction(data) => data.modifiers,
            NodeData::ClassExpression(data) => data.modifiers,
            NodeData::VariableStatement(data) => data.modifiers,
            NodeData::FunctionDeclaration(data) => data.modifiers,
            NodeData::ClassDeclaration(data) => data.modifiers,
            NodeData::InterfaceDeclaration(data) => data.modifiers,
            NodeData::TypeAliasDeclaration(data) => data.modifiers,
            NodeData::EnumDeclaration(data) => data.modifiers,
            NodeData::ModuleDeclaration(data) => data.modifiers,
            NodeData::ImportEqualsDeclaration(data) => data.modifiers,
            NodeData::ImportDeclaration(data) => data.modifiers,
            NodeData::ExportAssignment(data) => data.modifiers,
            NodeData::ExportDeclaration(data) => data.modifiers,
            _ => {
                return Err(TransformError::FactoryKindMismatch {
                    expected: SyntaxKind::Unknown,
                    actual: record.kind,
                })
            }
        };
        if current_modifiers == modifiers_id {
            return Ok(original);
        }

        let source = original.source;
        let child = |id: Option<NodeId>| id.map(|id| TransformNode::new(source, id));
        let array = |id: Option<NodeArrayId>| id.map(|id| TransformNodeArray::new(source, id));
        let required_child = |id: Option<NodeId>, parent, field| {
            id.map(|id| TransformNode::new(source, id))
                .ok_or(TransformError::RequiredChildRemoved { parent, field })
        };
        let required_array = |id: Option<NodeArrayId>, parent, field| {
            id.map(|id| TransformNodeArray::new(source, id))
                .ok_or(TransformError::RequiredChildRemoved { parent, field })
        };
        let updated = match record.data {
            NodeData::TypeParameter(data) => self.create_type_parameter_declaration(
                source,
                modifiers,
                required_child(data.name, SyntaxKind::TypeParameter, "name")?,
                child(data.constraint),
                child(data.r#default),
            )?,
            NodeData::Parameter(data) => self.create_parameter_declaration(
                source,
                modifiers,
                child(data.dot_dot_dot_token),
                required_child(data.name, SyntaxKind::Parameter, "name")?,
                child(data.question_token),
                child(data.r#type),
                child(data.initializer),
            )?,
            NodeData::ConstructorType(data) => self.create_constructor_type_node(
                source,
                modifiers,
                array(data.type_parameters),
                required_array(data.parameters, SyntaxKind::ConstructorType, "parameters")?,
                required_child(data.r#type, SyntaxKind::ConstructorType, "type")?,
            )?,
            NodeData::PropertySignature(data) => self.create_property_signature(
                source,
                modifiers,
                required_child(data.name, SyntaxKind::PropertySignature, "name")?,
                child(data.question_token),
                child(data.r#type),
            )?,
            NodeData::PropertyDeclaration(data) => self.create_property_declaration(
                source,
                modifiers,
                required_child(data.name, SyntaxKind::PropertyDeclaration, "name")?,
                child(data.question_token.or(data.exclamation_token)),
                child(data.r#type),
                child(data.initializer),
            )?,
            NodeData::MethodSignature(data) => self.create_method_signature(
                source,
                modifiers,
                required_child(data.name, SyntaxKind::MethodSignature, "name")?,
                child(data.question_token),
                array(data.type_parameters),
                required_array(data.parameters, SyntaxKind::MethodSignature, "parameters")?,
                child(data.r#type),
            )?,
            NodeData::MethodDeclaration(data) => self.create_method_declaration(
                source,
                modifiers,
                child(data.asterisk_token),
                required_child(data.name, SyntaxKind::MethodDeclaration, "name")?,
                child(data.question_token),
                array(data.type_parameters),
                required_array(data.parameters, SyntaxKind::MethodDeclaration, "parameters")?,
                child(data.r#type),
                child(data.body),
            )?,
            NodeData::Constructor(data) => {
                let parameters =
                    required_array(data.parameters, SyntaxKind::Constructor, "parameters")?;
                let body = child(data.body);
                let transform_flags = if let Some(body) = body {
                    self.children_flags(modifiers)?
                        | self.children_flags(Some(parameters))?
                        | (self.child_flags(Some(body))?
                            & !TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT)
                        | TransformFlags::CONTAINS_ES_2015
                } else {
                    TransformFlags::CONTAINS_TYPE_SCRIPT
                };
                return self.update_constructor_declaration(
                    original,
                    modifiers_id,
                    Some(parameters.array()),
                    body.map(TransformNode::node),
                    transform_flags,
                );
            }
            NodeData::GetAccessor(data) => {
                let name = required_child(data.name, SyntaxKind::GetAccessor, "name")?;
                let parameters =
                    required_array(data.parameters, SyntaxKind::GetAccessor, "parameters")?;
                let r#type = child(data.r#type);
                let body = child(data.body);
                let transform_flags = if let Some(body) = body {
                    self.children_flags(modifiers)?
                        | self.name_flags(Some(name))?
                        | self.children_flags(Some(parameters))?
                        | self.child_flags(r#type)?
                        | (self.child_flags(Some(body))?
                            & !TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT)
                        | if r#type.is_some() {
                            TransformFlags::CONTAINS_TYPE_SCRIPT
                        } else {
                            TransformFlags::NONE
                        }
                } else {
                    TransformFlags::CONTAINS_TYPE_SCRIPT
                };
                return self.update_get_accessor_declaration(
                    original,
                    modifiers_id,
                    Some(name.node()),
                    Some(parameters.array()),
                    r#type.map(TransformNode::node),
                    body.map(TransformNode::node),
                    transform_flags,
                );
            }
            NodeData::SetAccessor(data) => {
                let name = required_child(data.name, SyntaxKind::SetAccessor, "name")?;
                let parameters =
                    required_array(data.parameters, SyntaxKind::SetAccessor, "parameters")?;
                let body = child(data.body);
                let transform_flags = if let Some(body) = body {
                    self.children_flags(modifiers)?
                        | self.name_flags(Some(name))?
                        | self.children_flags(Some(parameters))?
                        | (self.child_flags(Some(body))?
                            & !TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT)
                } else {
                    TransformFlags::CONTAINS_TYPE_SCRIPT
                };
                return self.update_set_accessor_declaration(
                    original,
                    modifiers_id,
                    Some(name.node()),
                    Some(parameters.array()),
                    body.map(TransformNode::node),
                    transform_flags,
                );
            }
            NodeData::IndexSignature(data) => self.create_index_signature(
                source,
                modifiers,
                required_array(data.parameters, SyntaxKind::IndexSignature, "parameters")?,
                required_child(data.r#type, SyntaxKind::IndexSignature, "type")?,
            )?,
            NodeData::FunctionExpression(data) => self.create_function_expression(
                source,
                modifiers,
                child(data.asterisk_token),
                child(data.name),
                array(data.type_parameters),
                required_array(
                    data.parameters,
                    SyntaxKind::FunctionExpression,
                    "parameters",
                )?,
                child(data.r#type),
                required_child(data.body, SyntaxKind::FunctionExpression, "body")?,
            )?,
            NodeData::ArrowFunction(data) => self.create_arrow_function(
                source,
                modifiers,
                array(data.type_parameters),
                required_array(data.parameters, SyntaxKind::ArrowFunction, "parameters")?,
                child(data.r#type),
                child(data.equals_greater_than_token),
                required_child(data.body, SyntaxKind::ArrowFunction, "body")?,
            )?,
            NodeData::VariableStatement(data) => self.create_variable_statement(
                source,
                modifiers,
                required_child(
                    data.declaration_list,
                    SyntaxKind::VariableStatement,
                    "declarationList",
                )?,
            )?,
            NodeData::FunctionDeclaration(data) => self.create_function_declaration(
                source,
                modifiers,
                child(data.asterisk_token),
                child(data.name),
                array(data.type_parameters),
                required_array(
                    data.parameters,
                    SyntaxKind::FunctionDeclaration,
                    "parameters",
                )?,
                child(data.r#type),
                child(data.body),
            )?,
            NodeData::ClassDeclaration(data) => {
                return self.update_class_declaration(
                    original,
                    modifiers,
                    child(data.name),
                    array(data.type_parameters),
                    array(data.heritage_clauses),
                    required_array(data.members, SyntaxKind::ClassDeclaration, "members")?,
                );
            }
            NodeData::InterfaceDeclaration(data) => self.create_interface_declaration(
                source,
                modifiers,
                required_child(data.name, SyntaxKind::InterfaceDeclaration, "name")?,
                array(data.type_parameters),
                array(data.heritage_clauses),
                required_array(data.members, SyntaxKind::InterfaceDeclaration, "members")?,
            )?,
            NodeData::TypeAliasDeclaration(data) => self.create_type_alias_declaration(
                source,
                modifiers,
                required_child(data.name, SyntaxKind::TypeAliasDeclaration, "name")?,
                array(data.type_parameters),
                required_child(data.r#type, SyntaxKind::TypeAliasDeclaration, "type")?,
            )?,
            NodeData::EnumDeclaration(data) => self.create_enum_declaration(
                source,
                modifiers,
                required_child(data.name, SyntaxKind::EnumDeclaration, "name")?,
                required_array(data.members, SyntaxKind::EnumDeclaration, "members")?,
            )?,
            NodeData::ModuleDeclaration(data) => {
                return self.update_module_declaration(
                    original,
                    modifiers,
                    required_child(data.name, SyntaxKind::ModuleDeclaration, "name")?,
                    child(data.body),
                );
            }
            NodeData::ImportEqualsDeclaration(data) => self.create_import_equals_declaration(
                source,
                modifiers,
                data.is_type_only,
                required_child(data.name, SyntaxKind::ImportEqualsDeclaration, "name")?,
                required_child(
                    data.module_reference,
                    SyntaxKind::ImportEqualsDeclaration,
                    "moduleReference",
                )?,
            )?,
            NodeData::ImportDeclaration(data) => self.create_import_declaration(
                source,
                modifiers,
                child(data.import_clause),
                required_child(
                    data.module_specifier,
                    SyntaxKind::ImportDeclaration,
                    "moduleSpecifier",
                )?,
                child(data.attributes),
            )?,
            NodeData::ExportAssignment(data) => self.create_export_assignment(
                source,
                modifiers,
                data.is_export_equals.unwrap_or(false),
                required_child(data.expression, SyntaxKind::ExportAssignment, "expression")?,
            )?,
            NodeData::ExportDeclaration(data) => {
                return self.update_export_declaration(
                    original,
                    modifiers,
                    data.is_type_only,
                    child(data.export_clause),
                    child(data.module_specifier),
                    child(data.attributes),
                );
            }
            NodeData::ClassExpression(mut data) => {
                data.modifiers = modifiers_id;
                let flags = self.arena.transform_flags(original);
                return self.update_node(original, NodeData::ClassExpression(data), flags);
            }
            _ => unreachable!("modifier-bearing kind was classified above"),
        };
        self.finish_update(updated, original)
    }

    pub fn create_node(
        &mut self,
        source: TransformSourceId,
        mut data: NodeData,
        mut transform_flags: TransformFlags,
    ) -> Result<TransformNode, TransformError> {
        if matches!(data, NodeData::Token) {
            return Err(TransformError::FactoryTokenDataRequiresTokenConstructor);
        }
        self.normalize_embedded_statements(source, &mut data)?;
        self.apply_parenthesizer_rules(source, &mut data)?;
        transform_flags |= private_identifier_expression_flags(self.arena, source, &data)?;
        let syntax = &mut self.arena.source_mut(source)?.source;
        let id = syntax.arena.alloc_node(
            data,
            u32::MAX as usize,
            u32::MAX as usize,
            NodeFlags::SYNTHESIZED,
        );
        let node = TransformNode { source, node: id };
        self.arena.set_transform_flags(node, transform_flags);
        Ok(node)
    }

    /// tsc-port: getUnscopedHelperName @6.0.3
    /// tsc-hash: 4eccb820e726db854c379fb20072e2506d22a8caa82b367dcd88168334c0936e
    /// tsc-span: _tsc.js:25526-25528
    pub(crate) fn create_unscoped_helper_identifier(
        &mut self,
        source: TransformSourceId,
        helper_name: EmitHelperName,
    ) -> Result<TransformNode, TransformError> {
        let text = helper_name.text();
        let identifier = self.create_node(
            source,
            NodeData::Identifier(tsc_syntax::nodes::IdentifierData {
                escaped_text: tsc_syntax::escape_leading_underscores(text),
                text: text.to_owned(),
            }),
            TransformFlags::NONE,
        )?;
        self.arena
            .metadata_mut(identifier)
            .add_flags(EmitFlags::HELPER_NAME | EmitFlags::ADVISE_ON_EMIT_NODE);
        Ok(identifier)
    }

    /// Create a typed source-range anchor for a statement erased by a
    /// transform. The node deliberately has no printable payload: its
    /// original identity and text range let the printer advance comment and
    /// source ownership without pretending that runtime syntax was emitted.
    ///
    /// tsc-port: createNotEmittedStatement @6.0.3
    /// tsc-hash: 507998126dcad7e2261d2fc7547506ccb811baf36c9a527c5de919f5f20cd06e
    /// tsc-span: _tsc.js:24351-24356
    pub fn create_not_emitted_statement(
        &mut self,
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let node = self.create_node(
            original.source,
            NodeData::NotEmittedStatement(tsc_syntax::nodes::NotEmittedStatementData {}),
            TransformFlags::NONE,
        )?;
        self.arena.set_original_node(node, Some(original))?;
        self.set_text_range(node, original)
    }

    pub fn create_token(
        &mut self,
        source: TransformSourceId,
        kind: SyntaxKind,
        transform_flags: TransformFlags,
    ) -> Result<TransformNode, TransformError> {
        // ThisType and NotEmittedTypeElement are the named kind-only nodes
        // admitted above the token range. Keep this an allowlist: all other
        // non-token kinds must use their typed constructor.
        if kind > SyntaxKind::LastToken
            && !matches!(
                kind,
                SyntaxKind::ThisType | SyntaxKind::NotEmittedTypeElement
            )
        {
            return Err(TransformError::FactoryTokenKindExpected(kind));
        }
        let syntax = &mut self.arena.source_mut(source)?.source;
        let id = syntax.arena.alloc_token(
            kind,
            u32::MAX as usize,
            u32::MAX as usize,
            NodeFlags::SYNTHESIZED,
        );
        let node = TransformNode { source, node: id };
        self.arena.set_transform_flags(node, transform_flags);
        Ok(node)
    }

    /// tsc-port: createNotEmittedTypeElement @6.0.3
    /// tsc-hash: 3c0ce7b67743bc6f4e6a2d2810f9537a7d80f0aff7b4c82324f71a9dfd065d52
    /// tsc-span: _tsc.js:24368-24370
    pub fn create_not_emitted_type_element(
        &mut self,
        source: TransformSourceId,
    ) -> Result<TransformNode, TransformError> {
        self.create_token(
            source,
            SyntaxKind::NotEmittedTypeElement,
            TransformFlags::NONE,
        )
    }

    pub fn create_node_array(
        &mut self,
        source: TransformSourceId,
        nodes: Vec<TransformNode>,
    ) -> Result<TransformNodeArray, TransformError> {
        self.create_node_array_with_trailing_comma(source, nodes, false)
    }

    /// tsc's `createNodeArray(elements, hasTrailingComma)` keeps a trailing
    /// comma even for a final omitted expression. That distinction is needed
    /// for recovery trees such as the AMD lowering of `import()`, whose
    /// one-hole dependency list must print as `[,]` rather than `[]`.
    pub(crate) fn create_node_array_with_trailing_comma(
        &mut self,
        source: TransformSourceId,
        nodes: Vec<TransformNode>,
        has_trailing_comma: bool,
    ) -> Result<TransformNodeArray, TransformError> {
        let mut raw = Vec::with_capacity(nodes.len());
        let mut flags = TransformFlags::NONE;
        for mut node in nodes {
            if node.source != source {
                // h2-7a-m-3 §4 seam: single-pool original provenance.
                // A reused annotation can come from another mounted source;
                // remap its complete subtree before storing source-local raw
                // child ids in this node array.
                node = self.clone_node_to_source(node, source)?;
            }
            self.arena.node(node)?;
            flags |= self.arena.propagate_child_flags(node)?;
            raw.push(node.node);
        }
        let syntax = &mut self.arena.source_mut(source)?.source;
        let array_id = syntax.arena.alloc_synthetic_array(raw);
        syntax.arena.node_array_mut(array_id).has_trailing_comma = has_trailing_comma;
        let array = TransformNodeArray {
            source,
            array: array_id,
        };
        if !flags.is_empty() {
            self.arena.array_transform_flags.insert(array, flags);
        }
        Ok(array)
    }

    pub fn update_node_array(
        &mut self,
        original: TransformNodeArray,
        nodes: Vec<TransformNode>,
    ) -> Result<TransformNodeArray, TransformError> {
        let original_record = self.arena.node_array(original)?.clone();
        if original_record.nodes.len() == nodes.len()
            && original_record
                .nodes
                .iter()
                .zip(&nodes)
                .all(|(left, right)| right.source == original.source && *left == right.node)
        {
            return Ok(original);
        }
        let updated = self.create_node_array(original.source, nodes)?;
        let syntax = &mut self.arena.source_mut(original.source)?.source;
        let record = syntax.arena.node_array_mut(updated.array);
        record.pos = original_record.pos;
        record.end = original_record.end;
        record.has_trailing_comma = original_record.has_trailing_comma;
        record.is_missing_list = original_record.is_missing_list;
        Ok(updated)
    }

    /// `setTextRange(nodeArray, location)` for a synthesized statements
    /// array — the es2015 class-body/constructor lanes range the ARRAY to
    /// the original members/body statements while the Block keeps its own
    /// range policy (`_tsc.js:105239-105243`, `105300-105301`,
    /// `105373-105381`).
    pub fn set_node_array_text_range(
        &mut self,
        array: TransformNodeArray,
        pos: u32,
        end: u32,
    ) -> Result<(), TransformError> {
        let syntax = &mut self.arena.source_mut(array.source)?.source;
        let record = syntax.arena.node_array_mut(array.array);
        record.pos = pos;
        record.end = end;
        Ok(())
    }

    /// tsc-port: cloneNode @6.0.3
    /// tsc-hash: d223dcea6ccf14e9212d40d5b8df188197023622ea3e5d624ffb974a25db19d6
    /// tsc-span: _tsc.js:24436-24466
    pub fn clone_node(&mut self, original: TransformNode) -> Result<TransformNode, TransformError> {
        let record = self.arena.node(original)?.clone();
        let transform_flags = self.arena.transform_flags(original);
        let flags = NodeFlags::from_bits(record.flags) | NodeFlags::SYNTHESIZED;
        let syntax = &mut self.arena.source_mut(original.source)?.source;
        let id = match record.data.clone() {
            NodeData::Token => {
                syntax
                    .arena
                    .alloc_token(record.kind, u32::MAX as usize, u32::MAX as usize, flags)
            }
            data => syntax
                .arena
                .alloc_node(data, u32::MAX as usize, u32::MAX as usize, flags),
        };
        let clone = TransformNode {
            source: original.source,
            node: id,
        };
        {
            let copied = self
                .arena
                .source_mut(original.source)?
                .source
                .arena
                .node_mut(id);
            copied.numeric_literal_flags = record.numeric_literal_flags;
            copied.multi_line = record.multi_line;
            copied.js_doc = record.js_doc;
            copied.parent = None;
        }
        self.arena.set_transform_flags(clone, transform_flags);
        self.arena.set_original_node(clone, Some(original))?;
        Ok(clone)
    }

    /// tsrs-native: cross-kind declaration creation must retain the original
    /// node's arena-owned JSDoc array (h2-7a-m-4 §5.12).
    pub(crate) fn set_js_doc_from_original(
        &mut self,
        updated: TransformNode,
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if updated.source != original.source {
            return Err(TransformError::CrossSourceNode {
                expected: original.source,
                actual: updated.source,
            });
        }
        let js_doc = self.arena.node(original)?.js_doc;
        self.arena
            .source_mut(updated.source)?
            .source
            .arena
            .node_mut(updated.node)
            .js_doc = js_doc;
        Ok(updated)
    }

    /// Clone a reused syntax subtree into another mounted source while
    /// preserving its original-chain projection.
    ///
    /// h2-7a-m-3 §4 seam: single-pool original provenance. TypeScript's node
    /// pool has no per-source child-handle restriction; the Rust arena must
    /// therefore remap the complete reused subtree before it is attached to a
    /// target-source node array. Both source handles are still validated in
    /// this arena.
    pub fn clone_node_to_source(
        &mut self,
        original: TransformNode,
        target: TransformSourceId,
    ) -> Result<TransformNode, TransformError> {
        self.arena.node(original)?;
        self.arena.source(target)?;
        if original.source == target {
            return self.clone_node(original);
        }
        CrossSourceReuseClone::new(self.arena, original.source, target).clone_node(original.node)
    }

    pub fn update_node(
        &mut self,
        original: TransformNode,
        mut data: NodeData,
        mut transform_flags: TransformFlags,
    ) -> Result<TransformNode, TransformError> {
        let record = self.arena.node(original)?.clone();
        self.normalize_embedded_statements(original.source, &mut data)?;
        // Tokens and kind-only syntax nodes intentionally share the payload-
        // free `NodeData::Token` representation. An update can still change
        // their transform flags, but their kind is owned by the original node
        // rather than derivable from the payload.
        let kind = match data.kind() {
            Some(kind) => kind,
            None if matches!(data, NodeData::Token) && matches!(record.data, NodeData::Token) => {
                record.kind
            }
            None => {
                return Err(TransformError::FactoryKindMismatch {
                    expected: record.kind,
                    actual: SyntaxKind::Unknown,
                });
            }
        };
        if kind != record.kind {
            return Err(TransformError::FactoryKindMismatch {
                expected: record.kind,
                actual: kind,
            });
        }
        transform_flags |= private_identifier_expression_flags(self.arena, original.source, &data)?;
        if record.data == data && self.arena.transform_flags(original) == transform_flags {
            return Ok(original);
        }
        self.apply_parenthesizer_rules(original.source, &mut data)?;
        let (pos, end) = (record.pos, record.end);
        let updated = self.clone_node(original)?;
        let updated_record = self
            .arena
            .source_mut(updated.source)?
            .source
            .arena
            .node_mut(updated.node);
        updated_record.data = data;
        updated_record.pos = pos;
        updated_record.end = end;
        self.arena.set_transform_flags(updated, transform_flags);
        Ok(updated)
    }

    /// Update the runtime-owned constructor shape while retaining the
    /// signature fields that the parser attaches after factory creation.
    /// `typeParameters` and `type` are deliberately absent from the public
    /// constructor factory in tsc, so an update restores them from the parse
    /// tree instead of accepting transformed replacements.
    ///
    /// tsc-port: updateConstructorDeclaration/finishUpdateConstructorDeclaration @6.0.3
    /// tsc-hash: 458f5a752c894ba21fc18800fe4a10be5fd7f9e837fd38e4c0f20ba1e054072e
    /// tsc-span: _tsc.js:21982-22010
    pub fn update_constructor_declaration(
        &mut self,
        original: TransformNode,
        modifiers: Option<NodeArrayId>,
        parameters: Option<NodeArrayId>,
        body: Option<NodeId>,
        transform_flags: TransformFlags,
    ) -> Result<TransformNode, TransformError> {
        let record = self.arena.node(original)?.clone();
        let NodeData::Constructor(original_data) = record.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::Constructor,
                actual: record.kind,
            });
        };

        if original_data.modifiers == modifiers
            && original_data.parameters == parameters
            && original_data.body == body
        {
            return Ok(original);
        }

        self.update_node(
            original,
            NodeData::Constructor(tsc_syntax::nodes::ConstructorData {
                modifiers,
                name: original_data.name,
                type_parameters: original_data.type_parameters,
                parameters,
                r#type: original_data.r#type,
                body,
            }),
            transform_flags,
        )
    }

    /// Update the factory-owned getter fields and restore only its recovery
    /// type parameters. The return type is an ordinary getter factory field,
    /// so transformTypeScript can erase it by passing `None`.
    ///
    /// tsc-port: updateGetAccessorDeclaration/finishUpdateGetAccessorDeclaration @6.0.3
    /// tsc-hash: c2cee5560b6c2d55d7fc907e6cef6821f93e3bfa32f5bc1e1d0c4c264dfa4ac6
    /// tsc-span: _tsc.js:22012-22043
    #[allow(clippy::too_many_arguments)]
    pub fn update_get_accessor_declaration(
        &mut self,
        original: TransformNode,
        modifiers: Option<NodeArrayId>,
        name: Option<NodeId>,
        parameters: Option<NodeArrayId>,
        r#type: Option<NodeId>,
        body: Option<NodeId>,
        transform_flags: TransformFlags,
    ) -> Result<TransformNode, TransformError> {
        let record = self.arena.node(original)?.clone();
        let NodeData::GetAccessor(original_data) = record.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::GetAccessor,
                actual: record.kind,
            });
        };

        if original_data.modifiers == modifiers
            && original_data.name == name
            && original_data.parameters == parameters
            && original_data.r#type == r#type
            && original_data.body == body
        {
            return Ok(original);
        }

        self.update_node(
            original,
            NodeData::GetAccessor(tsc_syntax::nodes::GetAccessorData {
                modifiers,
                name,
                type_parameters: original_data.type_parameters,
                parameters,
                r#type,
                body,
            }),
            transform_flags,
        )
    }

    /// Update the runtime-owned fields of a setter while retaining invalid
    /// signature syntax captured by parser recovery. TypeScript's public
    /// setter factory intentionally has no `typeParameters` or `type`
    /// arguments; `finishUpdateSetAccessorDeclaration` copies those fields
    /// from the original only when another field forced an update.
    ///
    /// Keeping that rule at the factory boundary prevents the TypeScript
    /// transform from confusing a valid parameter annotation (which must be
    /// erased) with an invalid accessor-level annotation (which must survive
    /// so diagnostics and JavaScript recovery output agree with tsc).
    ///
    /// tsc-port: updateSetAccessorDeclaration/finishUpdateSetAccessorDeclaration @6.0.3
    /// tsc-hash: 183d0138ac0cabe72f5bb019160715f1456edd95dee61c3b1a246b840d4a1191
    /// tsc-span: _tsc.js:22065-22073
    pub fn update_set_accessor_declaration(
        &mut self,
        original: TransformNode,
        modifiers: Option<NodeArrayId>,
        name: Option<NodeId>,
        parameters: Option<NodeArrayId>,
        body: Option<NodeId>,
        transform_flags: TransformFlags,
    ) -> Result<TransformNode, TransformError> {
        let record = self.arena.node(original)?.clone();
        let NodeData::SetAccessor(original_data) = record.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::SetAccessor,
                actual: record.kind,
            });
        };

        // The tsc updater compares only fields exposed by the setter factory.
        // If none changed it returns the parsed node, including its original
        // transform flags and source-preserving printer identity.
        if original_data.modifiers == modifiers
            && original_data.name == name
            && original_data.parameters == parameters
            && original_data.body == body
        {
            return Ok(original);
        }

        self.update_node(
            original,
            NodeData::SetAccessor(tsc_syntax::nodes::SetAccessorData {
                modifiers,
                name,
                type_parameters: original_data.type_parameters,
                parameters,
                r#type: original_data.r#type,
                body,
            }),
            transform_flags,
        )
    }

    /// Keep the NodeFactory boundary responsible for the runtime shape of an
    /// embedded statement. A transform still returns a `NotEmittedStatement`
    /// as the typed source-range anchor for erased syntax; a control-flow
    /// owner, however, must retain a printable statement child.
    ///
    /// tsc-port: asEmbeddedStatement @6.0.3
    /// tsc-hash: 7ef4cec5d94d43fffaab4a42df7fd984af2d0bd4789f719476499e9120f3a1c5
    /// tsc-span: _tsc.js:24978-24980
    fn normalize_embedded_statements(
        &mut self,
        source: TransformSourceId,
        data: &mut NodeData,
    ) -> Result<(), TransformError> {
        match data {
            NodeData::IfStatement(data) => {
                self.normalize_embedded_statement(source, &mut data.then_statement)?;
                self.normalize_embedded_statement(source, &mut data.else_statement)
            }
            NodeData::DoStatement(data) => {
                self.normalize_embedded_statement(source, &mut data.statement)
            }
            NodeData::WhileStatement(data) => {
                self.normalize_embedded_statement(source, &mut data.statement)
            }
            NodeData::ForStatement(data) => {
                self.normalize_embedded_statement(source, &mut data.statement)
            }
            NodeData::ForInStatement(data) => {
                self.normalize_embedded_statement(source, &mut data.statement)
            }
            NodeData::ForOfStatement(data) => {
                self.normalize_embedded_statement(source, &mut data.statement)
            }
            NodeData::WithStatement(data) => {
                self.normalize_embedded_statement(source, &mut data.statement)
            }
            NodeData::LabeledStatement(data) => {
                self.normalize_embedded_statement(source, &mut data.statement)
            }
            _ => Ok(()),
        }
    }

    fn normalize_embedded_statement(
        &mut self,
        source: TransformSourceId,
        statement: &mut Option<NodeId>,
    ) -> Result<(), TransformError> {
        let Some(id) = *statement else {
            return Ok(());
        };
        let original = self
            .arena
            .node_ref(source, id)
            .ok_or(TransformError::UnknownNode(TransformNode {
                source,
                node: id,
            }))?;
        if self.arena.node(original)?.kind != SyntaxKind::NotEmittedStatement {
            return Ok(());
        }

        let empty = self.create_node(
            source,
            NodeData::EmptyStatement(tsc_syntax::nodes::EmptyStatementData {}),
            TransformFlags::NONE,
        )?;
        self.arena.set_original_node(empty, Some(original))?;
        self.set_text_range(empty, original)?;
        *statement = Some(empty.node);
        Ok(())
    }

    /// Apply the grammar-owning NodeFactory rules to every synthetic or
    /// structurally updated node. Parsed nodes can retain their source shape
    /// while unchanged; after a child changes, the factory is responsible for
    /// restoring any parentheses required by the new runtime expression.
    fn apply_parenthesizer_rules(
        &mut self,
        source: TransformSourceId,
        data: &mut NodeData,
    ) -> Result<(), TransformError> {
        self.parenthesize_binary_operands(source, data)?;
        self.parenthesize_conditional_operands(source, data)?;
        self.parenthesize_initializer_for_disallowed_comma(source, data)?;
        self.parenthesize_computed_property_name_expression(source, data)?;
        self.parenthesize_export_assignment_expression(source, data)
    }

    /// A conditional's condition is a logical-OR expression in the grammar,
    /// not an assignment expression. Once a transform replaces it with a
    /// lower-precedence conditional (for example the expansion of `??`), the
    /// NodeFactory must restore parentheses before the printer sees it.
    /// Branches admit assignment expressions and need parentheses only for a
    /// comma sequence.
    ///
    /// tsc-port: parenthesizeConditionOfConditionalExpression/parenthesizeBranchOfConditionalExpression @6.0.3
    /// tsc-hash: b967fe8e42676a1566ef7b7cf3ca1d78b366ceed9da4c1cb554e657a2dd86474
    /// tsc-span: _tsc.js:20423-20435
    fn parenthesize_conditional_operands(
        &mut self,
        source: TransformSourceId,
        data: &mut NodeData,
    ) -> Result<(), TransformError> {
        let NodeData::ConditionalExpression(conditional) = &*data else {
            return Ok(());
        };
        let (Some(condition), Some(when_true), Some(when_false)) = (
            conditional.condition,
            conditional.when_true,
            conditional.when_false,
        ) else {
            return Ok(());
        };
        let condition =
            self.arena
                .node_ref(source, condition)
                .ok_or(TransformError::UnknownNode(TransformNode {
                    source,
                    node: condition,
                }))?;
        let when_true =
            self.arena
                .node_ref(source, when_true)
                .ok_or(TransformError::UnknownNode(TransformNode {
                    source,
                    node: when_true,
                }))?;
        let when_false =
            self.arena
                .node_ref(source, when_false)
                .ok_or(TransformError::UnknownNode(TransformNode {
                    source,
                    node: when_false,
                }))?;
        let condition = self
            .parenthesize_expression_at_or_below_precedence(condition, PRECEDENCE_CONDITIONAL)?
            .node;
        let when_true = self
            .parenthesize_expression_at_or_below_precedence(when_true, PRECEDENCE_COMMA)?
            .node;
        let when_false = self
            .parenthesize_expression_at_or_below_precedence(when_false, PRECEDENCE_COMMA)?
            .node;
        let NodeData::ConditionalExpression(conditional) = data else {
            unreachable!("conditional owner was checked above")
        };
        conditional.condition = Some(condition);
        conditional.when_true = Some(when_true);
        conditional.when_false = Some(when_false);
        Ok(())
    }

    fn parenthesize_expression_at_or_below_precedence(
        &mut self,
        expression: TransformNode,
        maximum_unparenthesized: i8,
    ) -> Result<TransformNode, TransformError> {
        let emitted = self.skip_partially_emitted_expressions(expression)?;
        if self.arena.node(emitted)?.kind == SyntaxKind::ParenthesizedExpression
            || self.expression_precedence(emitted)? > maximum_unparenthesized
        {
            return Ok(expression);
        }
        let flags = self.arena.propagate_child_flags(expression)?;
        self.create_node(
            expression.source,
            NodeData::ParenthesizedExpression(tsc_syntax::nodes::ParenthesizedExpressionData {
                expression: Some(expression.node),
            }),
            flags,
        )
    }

    /// Parsed computed names already have a grammar-valid source shape and
    /// are not rewritten merely because their expression is a comma
    /// sequence. Once a transform creates or updates the name, NodeFactory's
    /// parenthesizer owns the new expression shape and wraps that sequence.
    ///
    /// tsc-port: parenthesizeExpressionOfComputedPropertyName @6.0.3
    /// tsc-hash: 2d1650a26a5b48410cad11ba30c3c5f861ddf333ae754f7ea847582cade0dddc
    /// tsc-span: _tsc.js:20420-20422
    /// tsc-port: createComputedPropertyName @6.0.3
    /// tsc-hash: 3b149a7a43988fa82cfdc435da487233991b7592d92d071f6aefb86d7aeddc93
    /// tsc-span: _tsc.js:21815-21820
    fn parenthesize_computed_property_name_expression(
        &mut self,
        source: TransformSourceId,
        data: &mut NodeData,
    ) -> Result<(), TransformError> {
        let NodeData::ComputedPropertyName(computed) = data else {
            return Ok(());
        };
        let Some(expression) = computed
            .expression
            .and_then(|expression| self.arena.node_ref(source, expression))
        else {
            return Ok(());
        };
        let emitted = self.skip_partially_emitted_expressions(expression)?;
        if self.arena.node(emitted)?.kind == SyntaxKind::ParenthesizedExpression
            || !(self.arena.node(emitted)?.kind == SyntaxKind::CommaListExpression
                || self.binary_operator(emitted)? == Some(SyntaxKind::CommaToken))
        {
            return Ok(());
        }
        let flags = self.arena.propagate_child_flags(expression)?;
        let parenthesized = self.create_node(
            source,
            NodeData::ParenthesizedExpression(tsc_syntax::nodes::ParenthesizedExpressionData {
                expression: Some(expression.node),
            }),
            flags,
        )?;
        computed.expression = Some(parenthesized.node);
        Ok(())
    }

    /// tsc's `asInitializer`, `createPropertyAssignment`, and
    /// `createShorthandPropertyAssignment` route initializer-like expression
    /// fields through `parenthesizeExpressionForDisallowedComma`. Keep that
    /// grammar rule at the shared factory boundary so a transform may return
    /// a comma sequence without knowing which declaration or object-literal
    /// member owns it.
    fn parenthesize_initializer_for_disallowed_comma(
        &mut self,
        source: TransformSourceId,
        data: &mut NodeData,
    ) -> Result<(), TransformError> {
        let initializer = match data {
            NodeData::Parameter(data) => data.initializer,
            NodeData::PropertyDeclaration(data) => data.initializer,
            NodeData::BindingElement(data) => data.initializer,
            NodeData::VariableDeclaration(data) => data.initializer,
            NodeData::PropertyAssignment(data) => data.initializer,
            NodeData::ShorthandPropertyAssignment(data) => data.object_assignment_initializer,
            _ => None,
        };
        let Some(initializer) = initializer else {
            return Ok(());
        };
        let expression =
            self.arena
                .node_ref(source, initializer)
                .ok_or(TransformError::UnknownNode(TransformNode {
                    source,
                    node: initializer,
                }))?;
        let parenthesized = self.parenthesize_expression_for_disallowed_comma(expression)?;
        if parenthesized.node == initializer {
            return Ok(());
        }
        match data {
            NodeData::Parameter(data) => data.initializer = Some(parenthesized.node),
            NodeData::PropertyDeclaration(data) => data.initializer = Some(parenthesized.node),
            NodeData::BindingElement(data) => data.initializer = Some(parenthesized.node),
            NodeData::VariableDeclaration(data) => data.initializer = Some(parenthesized.node),
            NodeData::PropertyAssignment(data) => data.initializer = Some(parenthesized.node),
            NodeData::ShorthandPropertyAssignment(data) => {
                data.object_assignment_initializer = Some(parenthesized.node)
            }
            _ => unreachable!("initializer owner was checked above"),
        }
        Ok(())
    }

    /// tsc-port: parenthesizeExpressionForDisallowedComma @6.0.3
    /// tsc-hash: 26883ad7ea9da9f2b7f019f26b3ce67bd9570c6563b2c9b01ae6dfc7488a527f
    /// tsc-span: _tsc.js:24598-24607
    /// tsc-port: asInitializer @6.0.3
    /// tsc-hash: eb236c3157d20301d43793ee9e3d1e31f849ec18465aed587ade74b3cad220e2
    /// tsc-span: _tsc.js:29102-29104
    fn parenthesize_expression_for_disallowed_comma(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let emitted = self.skip_partially_emitted_expressions(expression)?;
        // Rust keeps `ExpressionWithTypeArguments` as a recovery wrapper after
        // erasing valid instantiation type arguments. For this particular
        // comma check its emitted expression owns the precedence; other
        // contexts still see the wrapper and can retain grammar parentheses
        // around assignment targets.
        let precedence = match &self.arena.node(emitted)?.data {
            NodeData::ExpressionWithTypeArguments(data) => data
                .expression
                .and_then(|expression| self.arena.node_ref(emitted.source, expression))
                .map(|expression| self.expression_precedence(expression))
                .transpose()?
                .unwrap_or(PRECEDENCE_INVALID),
            _ => self.expression_precedence(emitted)?,
        };
        if precedence > PRECEDENCE_COMMA {
            return Ok(expression);
        }
        let flags = self.arena.propagate_child_flags(expression)?;
        let parenthesized = self.create_node(
            expression.source,
            NodeData::ParenthesizedExpression(tsc_syntax::nodes::ParenthesizedExpressionData {
                expression: Some(expression.node),
            }),
            flags,
        )?;
        self.set_text_range(parenthesized, expression)
    }

    /// tsc's export-assignment factory uses assignment-RHS rules for
    /// `export =` and a dedicated left-edge rule for `export default`.
    /// TypeScript erasure can expose a class/function expression beneath a
    /// PartiallyEmittedExpression, so this must run after transformed children
    /// are installed rather than being a printer-only special case.
    ///
    /// tsc-port: parenthesizeExpressionOfExportDefault @6.0.3
    /// tsc-hash: b679ce5fcfe28d204e724e3f74823d351eb0d86503befebc7fbdaa10faf999e5
    /// tsc-span: _tsc.js:20436-20450
    /// tsc-port: createExportAssignment @6.0.3
    /// tsc-hash: 64500f653db9d9252ef2b9ec5a648ba9765966d792d336786188f0010c02dab1
    /// tsc-span: _tsc.js:23606-23622
    fn parenthesize_export_assignment_expression(
        &mut self,
        source: TransformSourceId,
        data: &mut NodeData,
    ) -> Result<(), TransformError> {
        let NodeData::ExportAssignment(assignment) = data else {
            return Ok(());
        };
        let Some(expression) = assignment
            .expression
            .and_then(|expression| self.arena.node_ref(source, expression))
        else {
            return Ok(());
        };
        let needs_parentheses = if assignment.is_export_equals == Some(true) {
            self.binary_operand_needs_parentheses(SyntaxKind::EqualsToken, expression, false, None)?
        } else {
            self.export_default_expression_needs_parentheses(expression)?
        };
        if !needs_parentheses {
            return Ok(());
        }
        let flags = self.arena.propagate_child_flags(expression)?;
        let parenthesized = self.create_node(
            source,
            NodeData::ParenthesizedExpression(tsc_syntax::nodes::ParenthesizedExpressionData {
                expression: Some(expression.node),
            }),
            flags,
        )?;
        assignment.expression = Some(parenthesized.node);
        Ok(())
    }

    fn export_default_expression_needs_parentheses(
        &self,
        expression: TransformNode,
    ) -> Result<bool, TransformError> {
        let emitted = self.skip_partially_emitted_expressions(expression)?;
        if self.arena.node(emitted)?.kind == SyntaxKind::ParenthesizedExpression {
            return Ok(false);
        }
        if self.arena.node(emitted)?.kind == SyntaxKind::CommaListExpression
            || self.binary_operator(emitted)? == Some(SyntaxKind::CommaToken)
        {
            return Ok(true);
        }
        let leftmost = self.leftmost_expression(emitted, false)?;
        Ok(matches!(
            self.arena.node(leftmost)?.kind,
            SyntaxKind::ClassExpression | SyntaxKind::FunctionExpression
        ))
    }

    fn leftmost_expression(
        &self,
        mut expression: TransformNode,
        stop_at_call_expressions: bool,
    ) -> Result<TransformNode, TransformError> {
        loop {
            let next = match &self.arena.node(expression)?.data {
                NodeData::PostfixUnaryExpression(data) => data.operand,
                NodeData::BinaryExpression(data) => data.left,
                NodeData::ConditionalExpression(data) => data.condition,
                NodeData::TaggedTemplateExpression(data) => data.tag,
                NodeData::CallExpression(_) if stop_at_call_expressions => None,
                NodeData::CallExpression(data) => data.expression,
                NodeData::AsExpression(data) => data.expression,
                NodeData::ElementAccessExpression(data) => data.expression,
                NodeData::PropertyAccessExpression(data) => data.expression,
                NodeData::NonNullExpression(data) => data.expression,
                NodeData::PartiallyEmittedExpression(data) => data.expression,
                NodeData::SatisfiesExpression(data) => data.expression,
                _ => None,
            };
            let Some(next) = next.and_then(|next| self.arena.node_ref(expression.source, next))
            else {
                return Ok(expression);
            };
            expression = next;
        }
    }

    /// Keep every created or structurally updated binary expression in the
    /// grammar-safe shape guaranteed by TypeScript's NodeFactory. Parsed
    /// nodes retain their original identity when no child changes; once a
    /// transform replaces a child, precedence and associativity are owned by
    /// this factory boundary rather than by each individual transform.
    ///
    /// tsc-port: createParenthesizerRules @6.0.3 (binary operand rules)
    /// tsc-hash: 7fdf522c085caae040c757af4fb1b4cd79b9b119347a48009c3c8177a316cce5
    /// tsc-span: _tsc.js:20329-20419
    fn parenthesize_binary_operands(
        &mut self,
        source: TransformSourceId,
        data: &mut NodeData,
    ) -> Result<(), TransformError> {
        let NodeData::BinaryExpression(binary) = data else {
            return Ok(());
        };
        let (Some(left), Some(operator), Some(right)) =
            (binary.left, binary.operator_token, binary.right)
        else {
            return Ok(());
        };
        let Some(left) = self.arena.node_ref(source, left) else {
            return Ok(());
        };
        let Some(operator) = self.arena.node_ref(source, operator) else {
            return Ok(());
        };
        let Some(right) = self.arena.node_ref(source, right) else {
            return Ok(());
        };
        let operator = self.arena.node(operator)?.kind;
        let left = self.parenthesize_binary_operand(operator, left, true, None)?;
        let right = self.parenthesize_binary_operand(operator, right, false, Some(left))?;
        binary.left = Some(left.node);
        binary.right = Some(right.node);
        Ok(())
    }

    fn parenthesize_binary_operand(
        &mut self,
        operator: SyntaxKind,
        operand: TransformNode,
        is_left: bool,
        left_operand: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        if !self.binary_operand_needs_parentheses(operator, operand, is_left, left_operand)? {
            return Ok(operand);
        }
        let flags = self.arena.propagate_child_flags(operand)?;
        self.create_node(
            operand.source,
            NodeData::ParenthesizedExpression(tsc_syntax::nodes::ParenthesizedExpressionData {
                expression: Some(operand.node),
            }),
            flags,
        )
    }

    fn binary_operand_needs_parentheses(
        &self,
        operator: SyntaxKind,
        operand: TransformNode,
        is_left: bool,
        left_operand: Option<TransformNode>,
    ) -> Result<bool, TransformError> {
        let emitted_operand = self.skip_partially_emitted_expressions(operand)?;
        if self.arena.node(emitted_operand)?.kind == SyntaxKind::ParenthesizedExpression {
            return Ok(false);
        }
        if self
            .binary_operator(emitted_operand)?
            .is_some_and(|operand_operator| {
                mixing_binary_operators_requires_parentheses(operator, operand_operator)
            })
        {
            return Ok(true);
        }
        let operator_precedence = binary_operator_precedence(operator);
        let operator_associativity = binary_operator_associativity(operator);
        if !is_left
            && self.arena.node(operand)?.kind == SyntaxKind::ArrowFunction
            && operator_precedence > PRECEDENCE_ASSIGNMENT
        {
            return Ok(true);
        }
        let operand_precedence = self.expression_precedence(emitted_operand)?;
        if operand_precedence < operator_precedence {
            return Ok(!(operator_associativity == Associativity::Right
                && !is_left
                && self.arena.node(operand)?.kind == SyntaxKind::YieldExpression));
        }
        if operand_precedence > operator_precedence {
            return Ok(false);
        }
        if is_left {
            return Ok(operator_associativity == Associativity::Right);
        }
        if self.binary_operator(emitted_operand)? == Some(operator) {
            if operator_has_associative_property(operator) {
                return Ok(false);
            }
            if operator == SyntaxKind::PlusToken {
                let left_kind = left_operand
                    .map(|left| self.binary_plus_literal_kind(left))
                    .transpose()?
                    .flatten();
                if left_kind.is_some()
                    && left_kind == self.binary_plus_literal_kind(emitted_operand)?
                {
                    return Ok(false);
                }
            }
        }
        Ok(self.expression_associativity(emitted_operand)? == Associativity::Left)
    }

    fn skip_partially_emitted_expressions(
        &self,
        mut node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        loop {
            let NodeData::PartiallyEmittedExpression(data) = &self.arena.node(node)?.data else {
                return Ok(node);
            };
            let Some(expression) = data.expression else {
                return Ok(node);
            };
            let Some(expression) = self.arena.node_ref(node.source, expression) else {
                return Ok(node);
            };
            node = expression;
        }
    }

    fn binary_operator(&self, node: TransformNode) -> Result<Option<SyntaxKind>, TransformError> {
        let NodeData::BinaryExpression(data) = &self.arena.node(node)?.data else {
            return Ok(None);
        };
        data.operator_token
            .and_then(|operator| self.arena.node_ref(node.source, operator))
            .map(|operator| self.arena.node(operator).map(|operator| operator.kind))
            .transpose()
    }

    fn expression_precedence(&self, node: TransformNode) -> Result<i8, TransformError> {
        let kind = self.arena.node(node)?.kind;
        Ok(match kind {
            SyntaxKind::CommaListExpression => PRECEDENCE_COMMA,
            SyntaxKind::SpreadElement => PRECEDENCE_SPREAD,
            SyntaxKind::YieldExpression => PRECEDENCE_YIELD,
            SyntaxKind::ConditionalExpression => PRECEDENCE_CONDITIONAL,
            SyntaxKind::BinaryExpression => self
                .binary_operator(node)?
                .map(binary_operator_precedence)
                .unwrap_or(PRECEDENCE_INVALID),
            SyntaxKind::TypeAssertionExpression
            | SyntaxKind::NonNullExpression
            | SyntaxKind::PrefixUnaryExpression
            | SyntaxKind::TypeOfExpression
            | SyntaxKind::VoidExpression
            | SyntaxKind::DeleteExpression
            | SyntaxKind::AwaitExpression => PRECEDENCE_UNARY,
            SyntaxKind::PostfixUnaryExpression => PRECEDENCE_UPDATE,
            SyntaxKind::CallExpression => PRECEDENCE_LEFT_HAND_SIDE,
            SyntaxKind::NewExpression => match &self.arena.node(node)?.data {
                NodeData::NewExpression(data) if data.arguments.is_some() => PRECEDENCE_MEMBER,
                _ => PRECEDENCE_LEFT_HAND_SIDE,
            },
            SyntaxKind::TaggedTemplateExpression
            | SyntaxKind::PropertyAccessExpression
            | SyntaxKind::ElementAccessExpression
            | SyntaxKind::MetaProperty => PRECEDENCE_MEMBER,
            SyntaxKind::AsExpression | SyntaxKind::SatisfiesExpression => PRECEDENCE_RELATIONAL,
            SyntaxKind::ThisKeyword
            | SyntaxKind::SuperKeyword
            | SyntaxKind::Identifier
            | SyntaxKind::PrivateIdentifier
            | SyntaxKind::NullKeyword
            | SyntaxKind::TrueKeyword
            | SyntaxKind::FalseKeyword
            | SyntaxKind::NumericLiteral
            | SyntaxKind::BigIntLiteral
            | SyntaxKind::StringLiteral
            | SyntaxKind::ArrayLiteralExpression
            | SyntaxKind::ObjectLiteralExpression
            | SyntaxKind::FunctionExpression
            | SyntaxKind::ArrowFunction
            | SyntaxKind::ClassExpression
            | SyntaxKind::RegularExpressionLiteral
            | SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::TemplateExpression
            | SyntaxKind::ParenthesizedExpression
            | SyntaxKind::OmittedExpression
            | SyntaxKind::JsxElement
            | SyntaxKind::JsxSelfClosingElement
            | SyntaxKind::JsxFragment => PRECEDENCE_PRIMARY,
            _ => PRECEDENCE_INVALID,
        })
    }

    fn expression_associativity(
        &self,
        node: TransformNode,
    ) -> Result<Associativity, TransformError> {
        Ok(match self.arena.node(node)?.kind {
            SyntaxKind::NewExpression => match &self.arena.node(node)?.data {
                NodeData::NewExpression(data) if data.arguments.is_some() => Associativity::Left,
                _ => Associativity::Right,
            },
            SyntaxKind::PrefixUnaryExpression
            | SyntaxKind::TypeOfExpression
            | SyntaxKind::VoidExpression
            | SyntaxKind::DeleteExpression
            | SyntaxKind::AwaitExpression
            | SyntaxKind::ConditionalExpression
            | SyntaxKind::YieldExpression => Associativity::Right,
            SyntaxKind::BinaryExpression => self
                .binary_operator(node)?
                .map(binary_operator_associativity)
                .unwrap_or(Associativity::Left),
            _ => Associativity::Left,
        })
    }

    fn binary_plus_literal_kind(
        &self,
        node: TransformNode,
    ) -> Result<Option<SyntaxKind>, TransformError> {
        let node = self.skip_partially_emitted_expressions(node)?;
        let kind = self.arena.node(node)?.kind;
        if kind >= SyntaxKind::FirstLiteralToken && kind <= SyntaxKind::LastLiteralToken {
            return Ok(Some(kind));
        }
        if self.binary_operator(node)? != Some(SyntaxKind::PlusToken) {
            return Ok(None);
        }
        let NodeData::BinaryExpression(data) = &self.arena.node(node)?.data else {
            return Ok(None);
        };
        let (Some(left), Some(right)) = (data.left, data.right) else {
            return Ok(None);
        };
        let (Some(left), Some(right)) = (
            self.arena.node_ref(node.source, left),
            self.arena.node_ref(node.source, right),
        ) else {
            return Ok(None);
        };
        let left_kind = self.binary_plus_literal_kind(left)?;
        Ok(
            (left_kind.is_some() && left_kind == self.binary_plus_literal_kind(right)?)
                .then_some(left_kind)
                .flatten(),
        )
    }

    pub fn set_text_range(
        &mut self,
        node: TransformNode,
        location: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if node.source != location.source {
            return Err(TransformError::CrossSourceNode {
                expected: node.source,
                actual: location.source,
            });
        }
        let location_record = self.arena.node(location)?;
        let (pos, end) = (location_record.pos, location_record.end);
        let record = self
            .arena
            .source_mut(node.source)?
            .source
            .arena
            .node_mut(node.node);
        record.pos = pos;
        record.end = end;
        Ok(node)
    }

    /// Assign a typed text range without using another syntax node as a
    /// carrier. The source identity is checked independently and original
    /// byte boundaries are revalidated against that source before the raw
    /// node range is updated.
    pub fn set_text_range_from_source_range(
        &mut self,
        node: TransformNode,
        source: TransformSourceId,
        range: SourceRange,
    ) -> Result<TransformNode, TransformError> {
        if node.source != source {
            return Err(TransformError::CrossSourceNode {
                expected: node.source,
                actual: source,
            });
        }
        self.arena.node(node)?;
        let (pos, end) = match range {
            SourceRange::Original(range) => {
                let pos = range.start().value();
                let end = range.end().value();
                let syntax = self.arena.source(source)?.syntax();
                SourceRange::from_raw(pos, end, syntax.positions())
                    .map_err(|error| TransformError::InvalidSourceRange { node, error })?;
                (pos, end)
            }
            SourceRange::Synthesized => (u32::MAX, u32::MAX),
        };
        let record = self
            .arena
            .source_mut(node.source)?
            .source
            .arena
            .node_mut(node.node);
        record.pos = pos;
        record.end = end;
        Ok(node)
    }

    /// tsc-port: getGeneratedNameForNode @6.0.3
    /// tsc-hash: 7aeec7c8966a869665e0b8f01a41cd52e75bc957006170fd446ea819be9a6ea0
    /// tsc-span: _tsc.js:21652-21666
    pub fn get_generated_name_for_node(
        &mut self,
        node: TransformNode,
        flags: GeneratedIdentifierFlags,
        prefix: Option<&str>,
        suffix: Option<&str>,
    ) -> Result<TransformNode, TransformError> {
        let record = self.arena.node(node)?;
        // `isMemberName` in the upstream helper is deliberately narrower than
        // the literal-node family: string/numeric literals use the stable
        // generated@NodeId spelling even when their text happens to look like
        // an identifier.
        let base = match &record.data {
            NodeData::Identifier(data) => data.text.clone(),
            NodeData::PrivateIdentifier(data) => data.text.clone(),
            _ => format!("generated@{}", node.node.0),
        };
        let mut text = String::new();
        if let Some(prefix) = prefix {
            text.push_str(prefix);
        }
        text.push_str(&base);
        if let Some(suffix) = suffix {
            text.push_str(suffix);
        }
        let flags = if prefix.is_some() || suffix.is_some() {
            flags | GeneratedIdentifierFlags::OPTIMISTIC
        } else {
            flags
        };
        let generated = self.create_unique_name(node.source, text, flags)?;
        self.arena.set_original_node(generated, Some(node))?;
        Ok(generated)
    }

    /// Create the automatic temporary spelling used by
    /// `getGeneratedNameForNode` for a non-member source node.
    ///
    /// tsc-port: getGeneratedNameForNode @6.0.3 (non-member default-flag arm)
    /// tsc-hash: 7aeec7c8966a869665e0b8f01a41cd52e75bc957006170fd446ea819be9a6ea0
    /// tsc-span: _tsc.js:21652-21666
    pub fn get_generated_name_for_non_member_node(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let generated = self.create_identifier(node.source, "")?;
        let binding = self.arena.allocate_generated_binding_id();
        self.arena
            .metadata_mut(generated)
            .set_generated_binding_id(binding);
        self.arena.set_original_node(generated, Some(node))?;
        Ok(generated)
    }

    /// tsc-port: createObjectBindingPattern @6.0.3
    /// tsc-hash: 23d7a5579cd4dfaa4635b01de2c532bbaeabc4068a99d8ababa9f708fc61c827
    /// tsc-span: _tsc.js:22407-22415
    ///
    /// The m-3.5 census carried only this face's classifier header, not a
    /// callable typed constructor.
    pub fn create_object_binding_pattern(
        &mut self,
        source: TransformSourceId,
        elements: TransformNodeArray,
    ) -> Result<TransformNode, TransformError> {
        let mut flags = self.children_flags(Some(elements))?
            | TransformFlags::CONTAINS_ES_2015
            | TransformFlags::CONTAINS_BINDING_PATTERN;
        if flags.contains(TransformFlags::CONTAINS_REST_OR_SPREAD) {
            flags |=
                TransformFlags::CONTAINS_ES_2018 | TransformFlags::CONTAINS_OBJECT_REST_OR_SPREAD;
        }
        self.create_node(
            source,
            NodeData::ObjectBindingPattern(ObjectBindingPatternData {
                elements: Some(self.array_id(source, elements)?),
            }),
            flags,
        )
    }

    /// tsc-port: updateObjectBindingPattern @6.0.3
    /// tsc-hash: a1877f7e9c670537afcd95fa37782cc6ceb2f526f24b395c6a82041793550d3f
    /// tsc-span: _tsc.js:22416-22418
    /// tsc-update-helper: update @6.0.3; _tsc.js:24995-25001;
    /// 384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707
    pub fn update_object_binding_pattern(
        &mut self,
        original: TransformNode,
        elements: TransformNodeArray,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::ObjectBindingPattern(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::ObjectBindingPattern,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.elements == Some(elements.array) {
            return Ok(original);
        }
        let updated = self.create_object_binding_pattern(original.source, elements)?;
        self.finish_update(updated, original)
    }

    /// tsc-port: createArrayBindingPattern @6.0.3
    /// tsc-hash: bfaf580f0cf09f3625153d01c95fece6e5bed1ef4870c7b59651a14268e6a21c
    /// tsc-span: _tsc.js:22419-22424
    ///
    /// Binding-pattern creation paired with `create_object_binding_pattern`.
    pub fn create_array_binding_pattern(
        &mut self,
        source: TransformSourceId,
        elements: TransformNodeArray,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.children_flags(Some(elements))?
            | TransformFlags::CONTAINS_ES_2015
            | TransformFlags::CONTAINS_BINDING_PATTERN;
        self.create_node(
            source,
            NodeData::ArrayBindingPattern(ArrayBindingPatternData {
                elements: Some(self.array_id(source, elements)?),
            }),
            flags,
        )
    }

    /// tsc-port: updateArrayBindingPattern @6.0.3
    /// tsc-hash: ae9ab75e1a9b7dcc480dc265373b88b3e065b2ab608c71268602266ff8d74d2e
    /// tsc-span: _tsc.js:22425-22427
    /// tsc-update-helper: update @6.0.3; _tsc.js:24995-25001;
    /// 384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707
    pub fn update_array_binding_pattern(
        &mut self,
        original: TransformNode,
        elements: TransformNodeArray,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::ArrayBindingPattern(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::ArrayBindingPattern,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.elements == Some(elements.array) {
            return Ok(original);
        }
        let updated = self.create_array_binding_pattern(original.source, elements)?;
        self.finish_update(updated, original)
    }

    /// tsc-port: updateCallSignature @6.0.3
    /// tsc-hash: c49efe9cf9993da89238c41a22c7e8c289df6ec7458c3dcd14f401a91df53fee
    /// tsc-span: _tsc.js:22087-22089
    /// tsc-update-helper: update @6.0.3; _tsc.js:24995-25001;
    /// 384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707
    pub fn update_call_signature(
        &mut self,
        original: TransformNode,
        type_parameters: Option<TransformNodeArray>,
        parameters: TransformNodeArray,
        r#type: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::CallSignature(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::CallSignature,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.type_parameters == type_parameters.map(TransformNodeArray::array)
            && data.parameters == Some(parameters.array)
            && data.r#type == r#type.map(TransformNode::node)
        {
            return Ok(original);
        }
        let updated =
            self.create_call_signature(original.source, type_parameters, parameters, r#type)?;
        self.finish_update(updated, original)
    }

    /// tsc-port: updateConstructSignature @6.0.3
    /// tsc-hash: 59110741e7881d179ddb838f6377163dda60c2a66b4c8fdae0dd62c198920817
    /// tsc-span: _tsc.js:22102-22104
    /// tsc-update-helper: update @6.0.3; _tsc.js:24995-25001;
    /// 384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707
    pub fn update_construct_signature(
        &mut self,
        original: TransformNode,
        type_parameters: Option<TransformNodeArray>,
        parameters: TransformNodeArray,
        r#type: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::ConstructSignature(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::ConstructSignature,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.type_parameters == type_parameters.map(TransformNodeArray::array)
            && data.parameters == Some(parameters.array)
            && data.r#type == r#type.map(TransformNode::node)
        {
            return Ok(original);
        }
        let updated =
            self.create_construct_signature(original.source, type_parameters, parameters, r#type)?;
        self.finish_update(updated, original)
    }

    /// tsc-port: updateIndexSignature @6.0.3
    /// tsc-hash: 8e2eedfa4017cbd1f9e101cda70f9ea752b509f0cf6d03733ebc3a20402e3602
    /// tsc-span: _tsc.js:22117-22119
    /// tsc-update-helper: update @6.0.3; _tsc.js:24995-25001;
    /// 384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707
    pub fn update_index_signature(
        &mut self,
        original: TransformNode,
        modifiers: Option<TransformNodeArray>,
        parameters: TransformNodeArray,
        r#type: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::IndexSignature(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::IndexSignature,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.modifiers == modifiers.map(TransformNodeArray::array)
            && data.parameters == Some(parameters.array)
            && data.r#type == Some(r#type.node)
        {
            return Ok(original);
        }
        let updated =
            self.create_index_signature(original.source, modifiers, parameters, r#type)?;
        self.finish_update(updated, original)
    }

    /// tsc-port: updateFunctionTypeNode @6.0.3
    /// tsc-hash: e278aa99e2ebfb90ef9964f8e9bc9856f0061b63a0bfe74fb8048282003060b0
    /// tsc-span: _tsc.js:22167-22169
    /// tsc-update-helper: update @6.0.3; _tsc.js:24995-25001;
    /// 384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707
    pub fn update_function_type_node(
        &mut self,
        original: TransformNode,
        type_parameters: Option<TransformNodeArray>,
        parameters: TransformNodeArray,
        r#type: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::FunctionType(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::FunctionType,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.type_parameters == type_parameters.map(TransformNodeArray::array)
            && data.parameters == Some(parameters.array)
            && data.r#type == Some(r#type.node)
        {
            return Ok(original);
        }
        let modifiers = data.modifiers;
        let updated =
            self.create_function_type_node(original.source, type_parameters, parameters, r#type)?;
        if let NodeData::FunctionType(updated_data) = &mut self
            .arena
            .source_mut(updated.source)?
            .source
            .arena
            .node_mut(updated.node)
            .data
        {
            updated_data.modifiers = modifiers;
        }
        self.finish_update(updated, original)
    }

    /// tsc-port: updateConstructorTypeNode @6.0.3
    /// tsc-hash: 53812dd5f88b1561212ad4eeb0d33c849f9eba8c1cad7b59319dbfeb1a1a64ed
    /// tsc-span: _tsc.js:22201-22203
    /// tsc-update-helper: update @6.0.3; _tsc.js:24995-25001;
    /// 384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707
    pub fn update_constructor_type_node(
        &mut self,
        original: TransformNode,
        modifiers: Option<TransformNodeArray>,
        type_parameters: Option<TransformNodeArray>,
        parameters: TransformNodeArray,
        r#type: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::ConstructorType(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::ConstructorType,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.modifiers == modifiers.map(TransformNodeArray::array)
            && data.type_parameters == type_parameters.map(TransformNodeArray::array)
            && data.parameters == Some(parameters.array)
            && data.r#type == Some(r#type.node)
        {
            return Ok(original);
        }
        let updated = self.create_constructor_type_node(
            original.source,
            modifiers,
            type_parameters,
            parameters,
            r#type,
        )?;
        self.finish_update(updated, original)
    }

    /// tsc-port: updateLiteralTypeNode @6.0.3
    /// tsc-hash: 298b40fa2ea6b8f79d877d670c29066ad61d87e29b4615626bce3ea0c7d1f698
    /// tsc-span: _tsc.js:22404-22406
    /// tsc-update-helper: update @6.0.3; _tsc.js:24995-25001;
    /// 384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707
    pub fn update_literal_type_node(
        &mut self,
        original: TransformNode,
        literal: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::LiteralType(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::LiteralType,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.literal == Some(literal.node) {
            return Ok(original);
        }
        let updated = self.create_literal_type_node(original.source, literal)?;
        self.finish_update(updated, original)
    }

    /// tsc-port: updateExpressionWithTypeArguments @6.0.3
    /// tsc-hash: 419b692ee8529550e84aa5a83ca2ecc21844853e3c24e60ed3efa404125bfb4e
    /// tsc-span: _tsc.js:22955-22957
    /// tsc-update-helper: update @6.0.3; _tsc.js:24995-25001;
    /// 384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707
    pub fn update_expression_with_type_arguments(
        &mut self,
        original: TransformNode,
        expression: TransformNode,
        type_arguments: Option<TransformNodeArray>,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::ExpressionWithTypeArguments(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::ExpressionWithTypeArguments,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.expression == Some(expression.node)
            && data.type_arguments == type_arguments.map(TransformNodeArray::array)
        {
            return Ok(original);
        }
        let updated = self.create_expression_with_type_arguments(
            original.source,
            expression,
            type_arguments,
        )?;
        self.finish_update(updated, original)
    }

    /// tsc-port: updateMethodSignature @6.0.3
    /// tsc-hash: 23759da43eecf0cf1b5b1dc0077def1ad113179fee3d9b2e26abb143b6f45a51
    /// tsc-span: _tsc.js:21921-21923
    /// tsc-update-helper: update @6.0.3; _tsc.js:24995-25001;
    /// 384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707
    #[allow(clippy::too_many_arguments)]
    pub fn update_method_signature(
        &mut self,
        original: TransformNode,
        modifiers: Option<TransformNodeArray>,
        name: TransformNode,
        question_token: Option<TransformNode>,
        type_parameters: Option<TransformNodeArray>,
        parameters: TransformNodeArray,
        r#type: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::MethodSignature(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::MethodSignature,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.modifiers == modifiers.map(TransformNodeArray::array)
            && data.name == Some(name.node)
            && data.question_token == question_token.map(TransformNode::node)
            && data.type_parameters == type_parameters.map(TransformNodeArray::array)
            && data.parameters == Some(parameters.array)
            && data.r#type == r#type.map(TransformNode::node)
        {
            return Ok(original);
        }
        let updated = self.create_method_signature(
            original.source,
            modifiers,
            name,
            question_token,
            type_parameters,
            parameters,
            r#type,
        )?;
        self.finish_update(updated, original)
    }

    /// tsc-port: updatePropertySignature @6.0.3
    /// tsc-hash: f4085b45d8681eb86f901f6b607dc2b6955faf9b0c6a11ae03a8a41e1ebf62b3
    /// tsc-span: _tsc.js:21881-21883
    /// tsc-update-helper: update @6.0.3; _tsc.js:24995-25001;
    /// 384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707
    pub fn update_property_signature(
        &mut self,
        original: TransformNode,
        modifiers: Option<TransformNodeArray>,
        name: TransformNode,
        question_token: Option<TransformNode>,
        r#type: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::PropertySignature(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::PropertySignature,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.modifiers == modifiers.map(TransformNodeArray::array)
            && data.name == Some(name.node)
            && data.question_token == question_token.map(TransformNode::node)
            && data.r#type == r#type.map(TransformNode::node)
        {
            return Ok(original);
        }
        let initializer = data.initializer;
        let updated = self.create_property_signature(
            original.source,
            modifiers,
            name,
            question_token,
            r#type,
        )?;
        if let NodeData::PropertySignature(updated_data) = &mut self
            .arena
            .source_mut(updated.source)?
            .source
            .arena
            .node_mut(updated.node)
            .data
        {
            updated_data.initializer = initializer;
        }
        self.finish_update(updated, original)
    }

    /// tsc-port: updatePropertyDeclaration @6.0.3
    /// tsc-hash: 993e39266d70e68575e3d46b2efaf63e3642f3cf00fecf2087d108c103260d68
    /// tsc-span: _tsc.js:21903-21905
    /// tsc-update-helper: update @6.0.3; _tsc.js:24995-25001;
    /// 384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707
    pub fn update_property_declaration(
        &mut self,
        original: TransformNode,
        modifiers: Option<TransformNodeArray>,
        name: TransformNode,
        question_or_exclamation_token: Option<TransformNode>,
        r#type: Option<TransformNode>,
        initializer: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::PropertyDeclaration(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::PropertyDeclaration,
                actual: self.arena.node(original)?.kind,
            });
        };
        let token_kind = question_or_exclamation_token
            .map(|token| self.arena.node(token).map(|record| record.kind))
            .transpose()?;
        let question_token = (token_kind == Some(SyntaxKind::QuestionToken))
            .then(|| question_or_exclamation_token.map(TransformNode::node))
            .flatten();
        let exclamation_token = (token_kind == Some(SyntaxKind::ExclamationToken))
            .then(|| question_or_exclamation_token.map(TransformNode::node))
            .flatten();
        if data.modifiers == modifiers.map(TransformNodeArray::array)
            && data.name == Some(name.node)
            && data.question_token == question_token
            && data.exclamation_token == exclamation_token
            && data.r#type == r#type.map(TransformNode::node)
            && data.initializer == initializer.map(TransformNode::node)
        {
            return Ok(original);
        }
        let updated = self.create_property_declaration(
            original.source,
            modifiers,
            name,
            question_or_exclamation_token,
            r#type,
            initializer,
        )?;
        self.finish_update(updated, original)
    }

    /// tsc-port: updateVariableDeclaration @6.0.3
    /// tsc-hash: 358c095739f3d61b27920e79d77d32db503831a16900ebbd8c316453ebd4319a
    /// tsc-span: _tsc.js:23284-23286
    /// tsc-update-helper: update @6.0.3; _tsc.js:24995-25001;
    /// 384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707
    pub fn update_variable_declaration(
        &mut self,
        original: TransformNode,
        name: TransformNode,
        exclamation_token: Option<TransformNode>,
        r#type: Option<TransformNode>,
        initializer: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::VariableDeclaration(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::VariableDeclaration,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.name == Some(name.node)
            && data.exclamation_token == exclamation_token.map(TransformNode::node)
            && data.r#type == r#type.map(TransformNode::node)
            && data.initializer == initializer.map(TransformNode::node)
        {
            return Ok(original);
        }
        let updated = self.create_variable_declaration(
            original.source,
            name,
            exclamation_token,
            r#type,
            initializer,
        )?;
        self.finish_update(updated, original)
    }

    /// tsc-port: updateVariableDeclarationList @6.0.3
    /// tsc-hash: b781eb4611be5c3cd53060e1d106f959cd5c77bf804b8b3e17962e5f4f6fee41
    /// tsc-span: _tsc.js:23300-23302
    /// tsc-update-helper: update @6.0.3; _tsc.js:24995-25001;
    /// 384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707
    pub fn update_variable_declaration_list(
        &mut self,
        original: TransformNode,
        declarations: TransformNodeArray,
    ) -> Result<TransformNode, TransformError> {
        let record = self.arena.node(original)?;
        let NodeData::VariableDeclarationList(data) = &record.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::VariableDeclarationList,
                actual: record.kind,
            });
        };
        if data.declarations == Some(declarations.array) {
            return Ok(original);
        }
        let node_flags = NodeFlags::from_bits(record.flags);
        let updated =
            self.create_variable_declaration_list(original.source, declarations, node_flags)?;
        self.finish_update(updated, original)
    }

    /// tsc-port: updateVariableStatement @6.0.3
    /// tsc-hash: 6f9cc9c2a3ca6c613b932a8e29fb14d79a1c7605ba091b31d0379294dabf7584
    /// tsc-span: _tsc.js:23070-23072
    /// tsc-update-helper: update @6.0.3; _tsc.js:24995-25001;
    /// 384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707
    pub fn update_variable_statement(
        &mut self,
        original: TransformNode,
        modifiers: Option<TransformNodeArray>,
        declaration_list: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::VariableStatement(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::VariableStatement,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.modifiers == modifiers.map(TransformNodeArray::array)
            && data.declaration_list == Some(declaration_list.node)
        {
            return Ok(original);
        }
        let updated =
            self.create_variable_statement(original.source, modifiers, declaration_list)?;
        self.finish_update(updated, original)
    }

    /// tsc-port: updateFunctionDeclaration @6.0.3
    /// tsc-hash: 9462b0124a59adde30893b9a303fd35ea26098e55d29b4546f99c96171f9b8b3
    /// tsc-span: _tsc.js:23328-23330
    /// tsc-update-helper: update @6.0.3; _tsc.js:24995-25001;
    /// 384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707
    #[allow(clippy::too_many_arguments)]
    pub fn update_function_declaration(
        &mut self,
        original: TransformNode,
        modifiers: Option<TransformNodeArray>,
        asterisk_token: Option<TransformNode>,
        name: Option<TransformNode>,
        type_parameters: Option<TransformNodeArray>,
        parameters: TransformNodeArray,
        r#type: Option<TransformNode>,
        body: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::FunctionDeclaration(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::FunctionDeclaration,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.modifiers == modifiers.map(TransformNodeArray::array)
            && data.asterisk_token == asterisk_token.map(TransformNode::node)
            && data.name == name.map(TransformNode::node)
            && data.type_parameters == type_parameters.map(TransformNodeArray::array)
            && data.parameters == Some(parameters.array)
            && data.r#type == r#type.map(TransformNode::node)
            && data.body == body.map(TransformNode::node)
        {
            return Ok(original);
        }
        let updated = self.create_function_declaration(
            original.source,
            modifiers,
            asterisk_token,
            name,
            type_parameters,
            parameters,
            r#type,
            body,
        )?;
        self.finish_update(updated, original)
    }

    /// tsc-port: updateInterfaceDeclaration @6.0.3
    /// tsc-hash: 997b1720a05cfc7ee297629d53fb51da0f26514f9c41a8c01cd20ae557bdb7af
    /// tsc-span: _tsc.js:23371-23373
    /// tsc-update-helper: update @6.0.3; _tsc.js:24995-25001;
    /// 384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707
    pub fn update_interface_declaration(
        &mut self,
        original: TransformNode,
        modifiers: Option<TransformNodeArray>,
        name: TransformNode,
        type_parameters: Option<TransformNodeArray>,
        heritage_clauses: Option<TransformNodeArray>,
        members: TransformNodeArray,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::InterfaceDeclaration(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::InterfaceDeclaration,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.modifiers == modifiers.map(TransformNodeArray::array)
            && data.name == Some(name.node)
            && data.type_parameters == type_parameters.map(TransformNodeArray::array)
            && data.heritage_clauses == heritage_clauses.map(TransformNodeArray::array)
            && data.members == Some(members.array)
        {
            return Ok(original);
        }
        let updated = self.create_interface_declaration(
            original.source,
            modifiers,
            name,
            type_parameters,
            heritage_clauses,
            members,
        )?;
        self.finish_update(updated, original)
    }

    /// tsc-port: updateTypeAliasDeclaration @6.0.3
    /// tsc-hash: 18f2a4d1aea8004552afd48aea56ce5c95d9ecfada79d091cb90836b64ae4fe9
    /// tsc-span: _tsc.js:23386-23388
    /// tsc-update-helper: update @6.0.3; _tsc.js:24995-25001;
    /// 384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707
    pub fn update_type_alias_declaration(
        &mut self,
        original: TransformNode,
        modifiers: Option<TransformNodeArray>,
        name: TransformNode,
        type_parameters: Option<TransformNodeArray>,
        r#type: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::TypeAliasDeclaration(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::TypeAliasDeclaration,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.modifiers == modifiers.map(TransformNodeArray::array)
            && data.name == Some(name.node)
            && data.type_parameters == type_parameters.map(TransformNodeArray::array)
            && data.r#type == Some(r#type.node)
        {
            return Ok(original);
        }
        let updated = self.create_type_alias_declaration(
            original.source,
            modifiers,
            name,
            type_parameters,
            r#type,
        )?;
        self.finish_update(updated, original)
    }

    /// tsc-port: updateEnumDeclaration @6.0.3
    /// tsc-hash: 456a159c79f01073a60a0e3026433b233e67d47b869f17f4473bbe84e17381d3
    /// tsc-span: _tsc.js:23399-23401
    /// tsc-update-helper: update @6.0.3; _tsc.js:24995-25001;
    /// 384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707
    pub fn update_enum_declaration(
        &mut self,
        original: TransformNode,
        modifiers: Option<TransformNodeArray>,
        name: TransformNode,
        members: TransformNodeArray,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::EnumDeclaration(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::EnumDeclaration,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.modifiers == modifiers.map(TransformNodeArray::array)
            && data.name == Some(name.node)
            && data.members == Some(members.array)
        {
            return Ok(original);
        }
        let updated = self.create_enum_declaration(original.source, modifiers, name, members)?;
        self.finish_update(updated, original)
    }

    /// tsc-port: updateImportEqualsDeclaration @6.0.3
    /// tsc-hash: 14e52bd20433b9decff3e9461edd6b4574de1e1daa4f24738a5c7b1f80ef9336
    /// tsc-span: _tsc.js:23474-23476
    /// tsc-update-helper: update @6.0.3; _tsc.js:24995-25001;
    /// 384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707
    pub fn update_import_equals_declaration(
        &mut self,
        original: TransformNode,
        modifiers: Option<TransformNodeArray>,
        is_type_only: bool,
        name: TransformNode,
        module_reference: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::ImportEqualsDeclaration(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::ImportEqualsDeclaration,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.modifiers == modifiers.map(TransformNodeArray::array)
            && data.is_type_only == is_type_only
            && data.name == Some(name.node)
            && data.module_reference == Some(module_reference.node)
        {
            return Ok(original);
        }
        let updated = self.create_import_equals_declaration(
            original.source,
            modifiers,
            is_type_only,
            name,
            module_reference,
        )?;
        self.finish_update(updated, original)
    }

    /// tsc-port: updateImportDeclaration @6.0.3
    /// tsc-hash: 564a286760581b87138d31c781afc65cc1e9d92bcab78603fafcd9f63210e034
    /// tsc-span: _tsc.js:23488-23490
    /// tsc-update-helper: update @6.0.3; _tsc.js:24995-25001;
    /// 384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707
    pub fn update_import_declaration(
        &mut self,
        original: TransformNode,
        modifiers: Option<TransformNodeArray>,
        import_clause: Option<TransformNode>,
        module_specifier: TransformNode,
        attributes: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::ImportDeclaration(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::ImportDeclaration,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.modifiers == modifiers.map(TransformNodeArray::array)
            && data.import_clause == import_clause.map(TransformNode::node)
            && data.module_specifier == Some(module_specifier.node)
            && data.attributes == attributes.map(TransformNode::node)
        {
            return Ok(original);
        }
        let updated = self.create_import_declaration(
            original.source,
            modifiers,
            import_clause,
            module_specifier,
            attributes,
        )?;
        self.finish_update(updated, original)
    }

    /// tsc-port: updateImportClause @6.0.3
    /// tsc-hash: 41b6dd8eca7438e60623695dd4bf4f9457b5217a369a5f9401bb095f48c6201c
    /// tsc-span: _tsc.js:23507-23512
    /// tsc-update-helper: update @6.0.3; _tsc.js:24995-25001;
    /// 384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707
    pub fn update_import_clause(
        &mut self,
        original: TransformNode,
        phase_modifier: Option<SyntaxKind>,
        name: Option<TransformNode>,
        named_bindings: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::ImportClause(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::ImportClause,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.phase_modifier == phase_modifier
            && data.name == name.map(TransformNode::node)
            && data.named_bindings == named_bindings.map(TransformNode::node)
        {
            return Ok(original);
        }
        let updated =
            self.create_import_clause(original.source, phase_modifier, name, named_bindings)?;
        self.finish_update(updated, original)
    }

    /// tsc-port: updateNamedImports @6.0.3
    /// tsc-hash: d87b33ca4751d24e4337f8cffa0c153ba744058af100132936582319bc40f149
    /// tsc-span: _tsc.js:23591-23593
    /// tsc-update-helper: update @6.0.3; _tsc.js:24995-25001;
    /// 384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707
    pub fn update_named_imports(
        &mut self,
        original: TransformNode,
        elements: TransformNodeArray,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::NamedImports(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::NamedImports,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.elements == Some(elements.array) {
            return Ok(original);
        }
        let updated = self.create_named_imports(original.source, elements)?;
        self.finish_update(updated, original)
    }

    /// tsc-port: updateExportAssignment @6.0.3
    /// tsc-hash: 58aedbb908b41a70067e8c16957132448269b5cc6e814f4709ba207b77d98858
    /// tsc-span: _tsc.js:23621-23623
    /// tsc-update-helper: update @6.0.3; _tsc.js:24995-25001;
    /// 384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707
    pub fn update_export_assignment(
        &mut self,
        original: TransformNode,
        modifiers: Option<TransformNodeArray>,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::ExportAssignment(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::ExportAssignment,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.modifiers == modifiers.map(TransformNodeArray::array)
            && data.expression == Some(expression.node)
        {
            return Ok(original);
        }
        let updated = self.create_export_assignment(
            original.source,
            modifiers,
            data.is_export_equals.unwrap_or(false),
            expression,
        )?;
        self.finish_update(updated, original)
    }

    /// tsc-port: updateExternalModuleReference @6.0.3
    /// tsc-hash: a1fdfc130d05c893eb25b64974f6b92edcb35f548e502be14b7385ab47cc2c91
    /// tsc-span: _tsc.js:23682-23684
    /// tsc-update-helper: update @6.0.3; _tsc.js:24995-25001;
    /// 384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707
    pub fn update_external_module_reference(
        &mut self,
        original: TransformNode,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::ExternalModuleReference(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::ExternalModuleReference,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.expression == Some(expression.node) {
            return Ok(original);
        }
        let updated = self.create_external_module_reference(original.source, expression)?;
        self.finish_update(updated, original)
    }

    /// tsc-port: updateHeritageClause @6.0.3
    /// tsc-hash: d4e5edbc9ae4d5a75a96e2c8b456b0f39b2613b802b5c329a3eff339ad399006
    /// tsc-span: _tsc.js:24123-24125
    /// tsc-update-helper: update @6.0.3; _tsc.js:24995-25001;
    /// 384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707
    pub fn update_heritage_clause(
        &mut self,
        original: TransformNode,
        types: TransformNodeArray,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::HeritageClause(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::HeritageClause,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.types == Some(types.array) {
            return Ok(original);
        }
        let updated = self.create_heritage_clause(original.source, data.token, types)?;
        self.finish_update(updated, original)
    }

    /// tsc-port: updateEnumMember @6.0.3
    /// tsc-hash: 90d682b2bebda075bce747cd5dcc0c7ccf617d371564be4a72897cbf697a732f
    /// tsc-span: _tsc.js:24202-24204
    /// tsc-update-helper: update @6.0.3; _tsc.js:24995-25001;
    /// 384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707
    pub fn update_enum_member(
        &mut self,
        original: TransformNode,
        name: TransformNode,
        initializer: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::EnumMember(data) = &self.arena.node(original)?.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::EnumMember,
                actual: self.arena.node(original)?.kind,
            });
        };
        if data.name == Some(name.node) && data.initializer == initializer.map(TransformNode::node)
        {
            return Ok(original);
        }
        let updated = self.create_enum_member(original.source, name, initializer)?;
        self.finish_update(updated, original)
    }

    /// tsc-port: updateSourceFile @6.0.3
    /// tsc-hash: ab02b476644b993482e9e449d7808b3c8c00a249a4ba3a94e93a8b01f8c63dc9
    /// tsc-span: _tsc.js:24316-24327
    /// tsc-update-helper: update @6.0.3; _tsc.js:24995-25001;
    /// 384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707
    #[allow(clippy::too_many_arguments)]
    pub fn update_source_file(
        &mut self,
        original: TransformNode,
        statements: TransformNodeArray,
        is_declaration_file: bool,
        referenced_files: Vec<FileReference>,
        type_reference_directives: Vec<TypeReferenceDirective>,
        _has_no_default_lib: bool,
        lib_reference_directives: Vec<FileReference>,
    ) -> Result<TransformNode, TransformError> {
        let record = self.arena.node(original)?.clone();
        let NodeData::SourceFile(data) = record.data else {
            return Err(TransformError::FactoryKindMismatch {
                expected: SyntaxKind::SourceFile,
                actual: record.kind,
            });
        };
        let source = self.arena.source(original.source)?;
        if data.statements == Some(statements.array)
            && source.syntax().is_declaration_file == is_declaration_file
            && source.syntax().referenced_files == referenced_files
            && source.syntax().type_reference_directives == type_reference_directives
            && source.syntax().lib_reference_directives == lib_reference_directives
        {
            return Ok(original);
        }
        let flags = self.children_flags(Some(statements))?
            | self.child_flags(
                data.end_of_file_token
                    .and_then(|node| self.arena.node_ref(original.source, node)),
            )?;
        let updated = self.create_node(
            original.source,
            NodeData::SourceFile(SourceFileData {
                statements: Some(self.array_id(original.source, statements)?),
                end_of_file_token: data.end_of_file_token,
            }),
            flags,
        )?;
        let updated = self.finish_update(updated, original)?;
        let syntax = &mut self.arena.source_mut(original.source)?.source;
        syntax.is_declaration_file = is_declaration_file;
        syntax.referenced_files = referenced_files;
        syntax.type_reference_directives = type_reference_directives;
        syntax.lib_reference_directives = lib_reference_directives;
        Ok(updated)
    }

    pub fn set_multi_line(
        &mut self,
        node: TransformNode,
        multi_line: bool,
    ) -> Result<TransformNode, TransformError> {
        self.arena.node(node)?;
        self.arena
            .source_mut(node.source)?
            .source
            .arena
            .node_mut(node.node)
            .multi_line = Some(multi_line);
        Ok(node)
    }

    pub fn set_node_flags(
        &mut self,
        node: TransformNode,
        flags: NodeFlags,
    ) -> Result<TransformNode, TransformError> {
        self.arena.node(node)?;
        let record = self
            .arena
            .source_mut(node.source)?
            .source
            .arena
            .node_mut(node.node);
        record.flags = (NodeFlags::from_bits(record.flags) | flags | NodeFlags::SYNTHESIZED).bits();
        Ok(node)
    }
}

struct CrossSourceReuseClone<'a> {
    arena: &'a mut TransformArena,
    source: TransformSourceId,
    target: TransformSourceId,
    nodes: BTreeMap<NodeId, NodeId>,
    arrays: BTreeMap<NodeArrayId, NodeArrayId>,
}

impl<'a> CrossSourceReuseClone<'a> {
    fn new(
        arena: &'a mut TransformArena,
        source: TransformSourceId,
        target: TransformSourceId,
    ) -> Self {
        Self {
            arena,
            source,
            target,
            nodes: BTreeMap::new(),
            arrays: BTreeMap::new(),
        }
    }

    fn clone_node(&mut self, node: NodeId) -> Result<TransformNode, TransformError> {
        if let Some(&node) = self.nodes.get(&node) {
            return Ok(TransformNode::new(self.target, node));
        }

        let original = TransformNode::new(self.source, node);
        if self.arena.node(original).is_err() {
            let target = TransformNode::new(self.target, node);
            self.arena.node(target)?;
            return Ok(target);
        }
        let record = self.arena.node(original)?.clone();
        let transform_flags = self.arena.transform_flags(original);
        let mut data = record.data.clone();
        try_visit_each_child(&mut data, self)?;
        let js_doc = record
            .js_doc
            .map(|array| self.clone_array(array))
            .transpose()?;
        let flags = NodeFlags::from_bits(record.flags) | NodeFlags::SYNTHESIZED;
        let target_arena = &mut self.arena.source_mut(self.target)?.source.arena;
        let cloned = match data {
            NodeData::Token => {
                target_arena.alloc_token(record.kind, u32::MAX as usize, u32::MAX as usize, flags)
            }
            data => target_arena.alloc_node(data, u32::MAX as usize, u32::MAX as usize, flags),
        };
        {
            let copied = self
                .arena
                .source_mut(self.target)?
                .source
                .arena
                .node_mut(cloned);
            copied.numeric_literal_flags = record.numeric_literal_flags;
            copied.multi_line = record.multi_line;
            copied.js_doc = js_doc;
            copied.parent = None;
        }
        self.nodes.insert(node, cloned);
        let cloned = TransformNode::new(self.target, cloned);
        self.arena.set_transform_flags(cloned, transform_flags);
        self.arena.set_original_node(cloned, Some(original))?;
        Ok(cloned)
    }

    fn clone_array(&mut self, array: NodeArrayId) -> Result<NodeArrayId, TransformError> {
        if let Some(&array) = self.arrays.get(&array) {
            return Ok(array);
        }
        let original = TransformNodeArray::new(self.source, array);
        if self.arena.node_array(original).is_err() {
            let target = TransformNodeArray::new(self.target, array);
            self.arena.node_array(target)?;
            return Ok(array);
        }
        let record = self.arena.node_array(original)?.clone();
        let transform_flags = self.arena.array_transform_flags(original);
        let mut nodes = Vec::with_capacity(record.nodes.len());
        for node in record.nodes {
            nodes.push(self.clone_node(node)?.node());
        }
        let cloned = self
            .arena
            .source_mut(self.target)?
            .source
            .arena
            .alloc_synthetic_array(nodes);
        {
            let copied = self
                .arena
                .source_mut(self.target)?
                .source
                .arena
                .node_array_mut(cloned);
            copied.has_trailing_comma = record.has_trailing_comma;
            copied.is_missing_list = record.is_missing_list;
        }
        self.arrays.insert(array, cloned);
        self.arena.set_array_transform_flags(
            TransformNodeArray::new(self.target, cloned),
            transform_flags,
        );
        Ok(cloned)
    }
}

impl NodeDataChildVisitor for CrossSourceReuseClone<'_> {
    type Error = TransformError;

    fn node_kind(&self, id: NodeId) -> SyntaxKind {
        self.arena
            .node(TransformNode::new(self.source, id))
            .or_else(|_| self.arena.node(TransformNode::new(self.target, id)))
            .expect("reuse-clone child belongs to its mounted source")
            .kind
    }

    fn visit_node(&mut self, id: NodeId) -> Result<Option<NodeId>, Self::Error> {
        self.clone_node(id).map(|node| Some(node.node()))
    }

    fn visit_nodes(&mut self, id: NodeArrayId) -> Result<Option<NodeArrayId>, Self::Error> {
        self.clone_array(id).map(Some)
    }

    fn required_child_removed(&mut self, parent: SyntaxKind, field: &'static str) -> Self::Error {
        TransformError::RequiredChildRemoved { parent, field }
    }
}

fn mixing_binary_operators_requires_parentheses(left: SyntaxKind, right: SyntaxKind) -> bool {
    (left == SyntaxKind::QuestionQuestionToken
        && matches!(
            right,
            SyntaxKind::AmpersandAmpersandToken | SyntaxKind::BarBarToken
        ))
        || (right == SyntaxKind::QuestionQuestionToken
            && matches!(
                left,
                SyntaxKind::AmpersandAmpersandToken | SyntaxKind::BarBarToken
            ))
}

const fn operator_has_associative_property(operator: SyntaxKind) -> bool {
    matches!(
        operator,
        SyntaxKind::AsteriskToken
            | SyntaxKind::BarToken
            | SyntaxKind::AmpersandToken
            | SyntaxKind::CaretToken
            | SyntaxKind::CommaToken
    )
}

const fn binary_operator_associativity(operator: SyntaxKind) -> Associativity {
    if matches!(
        operator,
        SyntaxKind::AsteriskAsteriskToken
            | SyntaxKind::EqualsToken
            | SyntaxKind::PlusEqualsToken
            | SyntaxKind::MinusEqualsToken
            | SyntaxKind::AsteriskEqualsToken
            | SyntaxKind::AsteriskAsteriskEqualsToken
            | SyntaxKind::SlashEqualsToken
            | SyntaxKind::PercentEqualsToken
            | SyntaxKind::LessThanLessThanEqualsToken
            | SyntaxKind::GreaterThanGreaterThanEqualsToken
            | SyntaxKind::GreaterThanGreaterThanGreaterThanEqualsToken
            | SyntaxKind::AmpersandEqualsToken
            | SyntaxKind::CaretEqualsToken
            | SyntaxKind::BarEqualsToken
            | SyntaxKind::BarBarEqualsToken
            | SyntaxKind::AmpersandAmpersandEqualsToken
            | SyntaxKind::QuestionQuestionEqualsToken
    ) {
        Associativity::Right
    } else {
        Associativity::Left
    }
}

const fn binary_operator_precedence(operator: SyntaxKind) -> i8 {
    match operator {
        SyntaxKind::CommaToken => PRECEDENCE_COMMA,
        SyntaxKind::EqualsToken
        | SyntaxKind::PlusEqualsToken
        | SyntaxKind::MinusEqualsToken
        | SyntaxKind::AsteriskEqualsToken
        | SyntaxKind::AsteriskAsteriskEqualsToken
        | SyntaxKind::SlashEqualsToken
        | SyntaxKind::PercentEqualsToken
        | SyntaxKind::LessThanLessThanEqualsToken
        | SyntaxKind::GreaterThanGreaterThanEqualsToken
        | SyntaxKind::GreaterThanGreaterThanGreaterThanEqualsToken
        | SyntaxKind::AmpersandEqualsToken
        | SyntaxKind::CaretEqualsToken
        | SyntaxKind::BarEqualsToken
        | SyntaxKind::BarBarEqualsToken
        | SyntaxKind::AmpersandAmpersandEqualsToken
        | SyntaxKind::QuestionQuestionEqualsToken => PRECEDENCE_ASSIGNMENT,
        SyntaxKind::QuestionQuestionToken | SyntaxKind::BarBarToken => 5,
        SyntaxKind::AmpersandAmpersandToken => 6,
        SyntaxKind::BarToken => 7,
        SyntaxKind::CaretToken => 8,
        SyntaxKind::AmpersandToken => 9,
        SyntaxKind::EqualsEqualsToken
        | SyntaxKind::ExclamationEqualsToken
        | SyntaxKind::EqualsEqualsEqualsToken
        | SyntaxKind::ExclamationEqualsEqualsToken => 10,
        SyntaxKind::LessThanToken
        | SyntaxKind::GreaterThanToken
        | SyntaxKind::LessThanEqualsToken
        | SyntaxKind::GreaterThanEqualsToken
        | SyntaxKind::InstanceOfKeyword
        | SyntaxKind::InKeyword
        | SyntaxKind::AsKeyword
        | SyntaxKind::SatisfiesKeyword => PRECEDENCE_RELATIONAL,
        SyntaxKind::LessThanLessThanToken
        | SyntaxKind::GreaterThanGreaterThanToken
        | SyntaxKind::GreaterThanGreaterThanGreaterThanToken => 12,
        SyntaxKind::PlusToken | SyntaxKind::MinusToken => 13,
        SyntaxKind::AsteriskToken | SyntaxKind::SlashToken | SyntaxKind::PercentToken => 14,
        SyntaxKind::AsteriskAsteriskToken => 15,
        _ => PRECEDENCE_INVALID,
    }
}

#[cfg(test)]
#[path = "../tests/unit/factory_classifier/tests.rs"]
mod factory_classifier_tests;

#[cfg(test)]
#[path = "../tests/unit/factory_seams/tests.rs"]
mod original_provenance_tests;

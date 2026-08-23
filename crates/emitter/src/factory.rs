use std::collections::BTreeMap;

use tsc_program::SourceFileId;
use tsc_syntax::{
    for_each_observable_field, Node, NodeArray, NodeArrayId, NodeData, NodeId, ObservableField,
    SourceFile, SyntaxKind,
};
use tsc_types::NodeFlags;

use crate::{
    EmitFlags, EmitMetadata, EmitResolverNode, SourceRange, TransformError, TransformFlags,
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
    pub(crate) const fn new(source: TransformSourceId, node: NodeId) -> Self {
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
    pub(crate) const fn new(source: TransformSourceId, array: NodeArrayId) -> Self {
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
}

impl TransformArena {
    pub const fn new() -> Self {
        Self {
            sources: Vec::new(),
            node_transform_flags: BTreeMap::new(),
            array_transform_flags: BTreeMap::new(),
            metadata: BTreeMap::new(),
        }
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
            if node.source != original.source {
                return Err(TransformError::CrossSourceNode {
                    expected: node.source,
                    actual: original.source,
                });
            }
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
        if kind > SyntaxKind::LastToken {
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
        for node in nodes {
            if node.source != source {
                return Err(TransformError::CrossSourceNode {
                    expected: source,
                    actual: node.source,
                });
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

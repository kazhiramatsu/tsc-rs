use std::collections::BTreeMap;

use tsc_program::SourceFileId;
use tsc_syntax::{
    for_each_observable_field, Node, NodeArray, NodeArrayId, NodeData, NodeId, ObservableField,
    SourceFile, SyntaxKind,
};
use tsc_types::NodeFlags;

use crate::{EmitMetadata, TransformError, TransformFlags};

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
    source: SourceFile,
}

impl TransformSource {
    pub const fn program_source(&self) -> Option<SourceFileId> {
        self.program_source
    }

    pub const fn syntax(&self) -> &SourceFile {
        &self.source
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
        let mut detached = source.clone();
        detached.arena = std::mem::take(&mut detached.arena).into_detached();
        self.sources.push(TransformSource {
            program_source,
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

/// Synthetic-node constructor scoped to one mutable transform arena.
pub struct NodeFactory<'arena> {
    arena: &'arena mut TransformArena,
}

impl<'arena> NodeFactory<'arena> {
    pub(crate) fn new(arena: &'arena mut TransformArena) -> Self {
        Self { arena }
    }

    pub fn create_node(
        &mut self,
        source: TransformSourceId,
        data: NodeData,
        transform_flags: TransformFlags,
    ) -> Result<TransformNode, TransformError> {
        if matches!(data, NodeData::Token) {
            return Err(TransformError::FactoryTokenDataRequiresTokenConstructor);
        }
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
        let array_id = self
            .arena
            .source_mut(source)?
            .source
            .arena
            .alloc_synthetic_array(raw);
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
        data: NodeData,
        transform_flags: TransformFlags,
    ) -> Result<TransformNode, TransformError> {
        let record = self.arena.node(original)?;
        let kind = data.kind().ok_or(TransformError::FactoryKindMismatch {
            expected: record.kind,
            actual: SyntaxKind::Unknown,
        })?;
        if kind != record.kind {
            return Err(TransformError::FactoryKindMismatch {
                expected: record.kind,
                actual: kind,
            });
        }
        if record.data == data && self.arena.transform_flags(original) == transform_flags {
            return Ok(original);
        }
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

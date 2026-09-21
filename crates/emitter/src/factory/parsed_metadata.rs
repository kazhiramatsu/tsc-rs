use std::collections::BTreeMap;
use std::sync::Arc;

use tsc_diagnostics::TextSnapshot;
use tsc_program::SourceFileId;
use tsc_syntax::{NodeId, SourceFile};
use tsc_types::IdentityLease;

use super::{TransformArena, TransformNode, TransformSourceId};
use crate::metadata::{CommentSourceRange, InternalEmitFlags};
use crate::{CommentRange, EmitConstantValue, EmitFlags, EmitHost, EmitMetadata, TransformError};

/// Metadata actually attached to original parse nodes by a completed
/// JavaScript transform/print. It is passed only to the declaration transform
/// of that same ordinary Bundle emit. SourceFile roots dispose their annotated
/// parse nodes before declaration emission; fresh getter/forced operations also
/// start empty.
///
/// Synthetic nodes and their `original` chains are never projected into this
/// snapshot. The portable packet covers the observed flags / internal flags /
/// typeNode / constantValue / commentRange mutations (tsc keeps a bundle's
/// parse-node `emitNode`s, including
/// `InternalEmitFlags.TransformPrivateStaticElements` stamped on a decorated
/// class's static private members and the end-only `commentRange` the
/// class-fields transform stamps on a lowered private access's parsed
/// receiver, because a Bundle root has no parse SourceFile to dispose:
/// `disposeEmitNodes(getSourceFileOfNode(getParseTreeNode(bundle)))` is a
/// no-op, `_tsc.js:25302-25310` / `116050-116052` / `116261-116275`); other
/// metadata is a typed refusal, never silently discarded.
///
/// A comment range is a tsc `TextRange` whose endpoints index the text of the
/// source that produced it; Rust keeps that source explicitly
/// (`CommentRange::source`). The packet records it as the Program identity of
/// that source, independently of the annotated node's own source, and remaps
/// both through the declaration arena's mount table.
#[derive(Debug, Default)]
pub struct ParsedEmitMetadata {
    sources: BTreeMap<SourceFileId, ParsedSourceIdentity>,
    nodes: BTreeMap<(SourceFileId, NodeId), ParsedNodeMetadata>,
}

#[derive(Debug)]
struct ParsedSourceIdentity {
    snapshot: Arc<TextSnapshot>,
    lease: Option<IdentityLease>,
    node_base: u32,
    node_end: u32,
}

impl ParsedSourceIdentity {
    fn new(source: &SourceFile) -> Self {
        Self {
            snapshot: Arc::clone(source.snapshot()),
            lease: source.node_identity_lease().cloned(),
            node_base: source.arena.node_base(),
            node_end: source.arena.node_end(),
        }
    }

    fn matches(&self, source: &SourceFile) -> bool {
        Arc::ptr_eq(&self.snapshot, source.snapshot())
            && self.lease.as_ref() == source.node_identity_lease()
            && self.node_base == source.arena.node_base()
            && self.node_end == source.arena.node_end()
    }
}

#[derive(Debug)]
struct ParsedNodeMetadata {
    flags: EmitFlags,
    internal_flags: InternalEmitFlags,
    type_node: Option<(SourceFileId, NodeId)>,
    constant_value: Option<EmitConstantValue>,
    comment_range: Option<ParsedCommentRange>,
}

/// tsc `emitNode.commentRange` on a parse node (`setCommentRange`,
/// `_tsc.js:25362-25365`; read by `getCommentRange`, `25358-25361`). The
/// endpoints stay the validated byte positions of the producing source's
/// text: the snapshot identity below requires that exact text snapshot, so
/// no position is re-derived or converted at the boundary.
#[derive(Debug)]
struct ParsedCommentRange {
    source: SourceFileId,
    range: CommentSourceRange,
}

impl TransformArena {
    /// Capture direct parse-node metadata before disposing a JavaScript
    /// transformation. The immutable host supplies source identity authority.
    pub fn snapshot_parsed_emit_metadata(
        &self,
        host: &dyn EmitHost,
    ) -> Result<ParsedEmitMetadata, TransformError> {
        let mut snapshot = ParsedEmitMetadata::default();
        for (&node, metadata) in &self.metadata {
            if !self.is_parsed_node(node)? || metadata == &EmitMetadata::default() {
                continue;
            }
            // An explicit whitelist ensures new fields and synthetic identity
            // channels cannot quietly disappear at this boundary.
            let mut remainder = metadata.clone();
            remainder.flags = EmitFlags::NONE;
            remainder.internal_flags = InternalEmitFlags::NONE;
            remainder.type_node = None;
            remainder.constant_value = None;
            remainder.comment_range = None;
            if remainder != EmitMetadata::default() {
                return Err(TransformError::ParsedEmitMetadataNotPortable(node));
            }
            let identity = snapshot.remember_node(self, host, node)?;
            let type_node = metadata
                .type_node()
                .map(|node| snapshot.remember_node(self, host, node))
                .transpose()?;
            let comment_range = metadata
                .comment_range()
                .map(|range| snapshot.remember_comment_range(self, host, range))
                .transpose()?;
            if snapshot
                .nodes
                .insert(
                    identity,
                    ParsedNodeMetadata {
                        flags: metadata.flags(),
                        internal_flags: metadata.internal_flags(),
                        type_node,
                        constant_value: metadata.constant_value().cloned(),
                        comment_range,
                    },
                )
                .is_some()
            {
                return Err(TransformError::ParsedEmitMetadataSourceMismatch(identity.0));
            }
        }
        Ok(snapshot)
    }

    /// Restore into a freshly mounted declaration arena. Program tokens, the
    /// original snapshot and parse leases must all agree; mounting order may
    /// differ. Validation is atomic and does not overwrite local metadata.
    pub fn restore_parsed_emit_metadata(
        &mut self,
        snapshot: &ParsedEmitMetadata,
        host: &dyn EmitHost,
    ) -> Result<(), TransformError> {
        let mut mounted = BTreeMap::new();
        for (&program, identity) in &snapshot.sources {
            let original = host
                .source_file(program)
                .and_then(|source| source.syntax())
                .ok_or(TransformError::ParsedEmitMetadataSourceMismatch(program))?;
            if !identity.matches(original) {
                return Err(TransformError::ParsedEmitMetadataSourceMismatch(program));
            }
            for (index, source) in self.sources.iter().enumerate() {
                if source.program_source() == Some(program) {
                    let source_id = TransformSourceId(index as u32);
                    self.validate_parsed_metadata_source(source_id, host)?;
                    if mounted.insert(program, source_id).is_some() {
                        return Err(TransformError::ParsedEmitMetadataSourceMismatch(program));
                    }
                }
            }
            if !mounted.contains_key(&program) {
                return Err(TransformError::ParsedEmitMetadataSourceMismatch(program));
            }
        }
        // Every identity below was registered in `sources` by the snapshot,
        // so each lookup succeeds; a typed failure keeps the atomic contract
        // (no write before the whole packet resolved) instead of a panic.
        let mounted_source = |program: SourceFileId| {
            mounted
                .get(&program)
                .copied()
                .ok_or(TransformError::ParsedEmitMetadataSourceMismatch(program))
        };
        let mut restored = Vec::with_capacity(snapshot.nodes.len());
        for (&(program, id), metadata) in &snapshot.nodes {
            let node = TransformNode::new(mounted_source(program)?, id);
            if self
                .metadata(node)
                .is_some_and(|value| value != &EmitMetadata::default())
            {
                return Err(TransformError::ParsedEmitMetadataRestoreConflict(node));
            }
            let value = EmitMetadata {
                flags: metadata.flags,
                internal_flags: metadata.internal_flags,
                constant_value: metadata.constant_value.clone(),
                type_node: metadata
                    .type_node
                    .map(|(program, id)| -> Result<TransformNode, TransformError> {
                        Ok(TransformNode::new(mounted_source(program)?, id))
                    })
                    .transpose()?,
                comment_range: metadata
                    .comment_range
                    .as_ref()
                    .map(|range| -> Result<CommentRange, TransformError> {
                        Ok(CommentRange::from_parts(
                            mounted_source(range.source)?,
                            range.range,
                        ))
                    })
                    .transpose()?,
                ..EmitMetadata::default()
            };
            restored.push((node, value));
        }
        self.metadata.extend(restored);
        Ok(())
    }

    fn validate_parsed_metadata_source(
        &self,
        source: TransformSourceId,
        host: &dyn EmitHost,
    ) -> Result<SourceFileId, TransformError> {
        let mounted = self.source(source)?;
        let program = mounted.program_source().ok_or_else(|| {
            TransformError::MissingProgramSource(TransformNode::new(source, mounted.syntax().root))
        })?;
        let original = host
            .source_file(program)
            .and_then(|source| source.syntax())
            .ok_or(TransformError::ParsedEmitMetadataSourceMismatch(program))?;
        if !Arc::ptr_eq(mounted.syntax().snapshot(), original.snapshot())
            || mounted.parsed_node_identity_lease.as_ref() != original.node_identity_lease()
            || mounted.parsed_node_base != original.arena.node_base()
            || mounted.parsed_node_end != original.arena.node_end()
        {
            return Err(TransformError::ParsedEmitMetadataSourceMismatch(program));
        }
        Ok(program)
    }
}

impl ParsedEmitMetadata {
    fn remember_node(
        &mut self,
        arena: &TransformArena,
        host: &dyn EmitHost,
        node: TransformNode,
    ) -> Result<(SourceFileId, NodeId), TransformError> {
        if !arena.is_parsed_node(node)? {
            return Err(TransformError::ParsedEmitMetadataNotPortable(node));
        }
        Ok((
            self.remember_source(arena, host, node.source())?,
            node.node(),
        ))
    }

    /// A comment range's source is validated and remembered on its own: the
    /// range may index a different source than the annotated node, and its
    /// positions are only meaningful against that source's exact text.
    fn remember_comment_range(
        &mut self,
        arena: &TransformArena,
        host: &dyn EmitHost,
        range: CommentRange,
    ) -> Result<ParsedCommentRange, TransformError> {
        Ok(ParsedCommentRange {
            source: self.remember_source(arena, host, range.source())?,
            range: range.range(),
        })
    }

    fn remember_source(
        &mut self,
        arena: &TransformArena,
        host: &dyn EmitHost,
        source: TransformSourceId,
    ) -> Result<SourceFileId, TransformError> {
        let program = arena.validate_parsed_metadata_source(source, host)?;
        let original = host
            .source_file(program)
            .and_then(|source| source.syntax())
            .ok_or(TransformError::ParsedEmitMetadataSourceMismatch(program))?;
        self.sources
            .entry(program)
            .or_insert_with(|| ParsedSourceIdentity::new(original));
        Ok(program)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/factory/parsed_metadata/tests.rs"]
mod tests;

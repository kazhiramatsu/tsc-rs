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
mod tests {
    use super::*;
    use crate::EmitSource;
    use tsc_diagnostics::JsStr;
    use tsc_types::CompilerOptions;

    struct Host {
        options: CompilerOptions,
        ids: [SourceFileId; 2],
        sources: [SourceFile; 2],
    }

    impl Host {
        fn new() -> Self {
            Self {
                options: CompilerOptions::default(),
                ids: [SourceFileId::from_raw(7), SourceFileId::from_raw(19)],
                sources: ["a", "b"].map(|name| {
                    tsc_syntax::parse_source_file(
                        format!("/src/{name}.ts"),
                        format!("let {name}: number;"),
                        Default::default(),
                        None,
                    )
                }),
            }
        }

        fn mount(&self, reverse: bool) -> (TransformArena, [TransformNode; 2]) {
            let mut arena = TransformArena::new();
            let mut nodes = Vec::new();
            for index in if reverse { [1, 0] } else { [0, 1] } {
                let source = arena.add_source(&self.sources[index], Some(self.ids[index]));
                nodes.push(arena.root(source).unwrap());
            }
            if reverse {
                nodes.reverse();
            }
            (arena, nodes.try_into().unwrap())
        }
    }

    impl EmitHost for Host {
        fn compiler_options(&self) -> &CompilerOptions {
            &self.options
        }
        fn current_directory(&self) -> JsStr<'_> {
            JsStr::from("/")
        }
        fn common_source_directory(&self) -> JsStr<'_> {
            JsStr::from("/src/")
        }
        fn config_file_path(&self) -> Option<JsStr<'_>> {
            None
        }
        fn use_case_sensitive_file_names(&self) -> bool {
            true
        }
        fn source_file_ids(&self) -> &[SourceFileId] {
            &self.ids
        }
        fn source_file(&self, id: SourceFileId) -> Option<EmitSource<'_>> {
            let index = self.ids.iter().position(|&candidate| candidate == id)?;
            let source = &self.sources[index];
            let path = JsStr::from(&source.file_name);
            Some(EmitSource::new(id, path, path, true, None, Some(source)))
        }
    }

    #[test]
    fn parsed_metadata_survives_reordered_mounts_without_projecting_synthetic_originals() {
        let host = Host::new();
        let (mut javascript, [a, b]) = host.mount(false);
        javascript
            .metadata_mut(a)
            .set_flags(EmitFlags::NO_TRAILING_SOURCE_MAP);
        javascript.metadata_mut(a).set_type_node(b);
        let synthetic = javascript.factory().clone_node(a).unwrap();
        javascript
            .metadata_mut(synthetic)
            .set_flags(EmitFlags::NO_LEADING_SOURCE_MAP);
        let snapshot = javascript.snapshot_parsed_emit_metadata(&host).unwrap();
        drop(javascript);
        let (mut declaration, [a, b]) = host.mount(true);
        declaration
            .restore_parsed_emit_metadata(&snapshot, &host)
            .unwrap();
        assert_eq!(
            declaration.metadata(a).unwrap().flags(),
            EmitFlags::NO_TRAILING_SOURCE_MAP
        );
        assert_eq!(declaration.metadata(a).unwrap().type_node(), Some(b));
        assert!(declaration.metadata(b).is_none());
        let (fresh_forced, _) = host.mount(false);
        assert!(fresh_forced.metadata.is_empty());
    }

    #[test]
    fn parsed_metadata_rejects_foreign_program_authority_and_conflicting_targets_atomically() {
        let host = Host::new();
        let (mut javascript, [a, b]) = host.mount(false);
        javascript
            .metadata_mut(a)
            .set_flags(EmitFlags::NO_TRAILING_SOURCE_MAP);
        javascript
            .metadata_mut(b)
            .set_flags(EmitFlags::NO_TRAILING_SOURCE_MAP);
        let snapshot = javascript.snapshot_parsed_emit_metadata(&host).unwrap();
        let foreign = Host::new();
        let (mut declaration, _) = foreign.mount(false);
        let before = declaration.clone();
        assert!(matches!(
            declaration.restore_parsed_emit_metadata(&snapshot, &foreign),
            Err(TransformError::ParsedEmitMetadataSourceMismatch(_))
        ));
        assert_eq!(declaration, before);
        assert!(matches!(
            declaration.restore_parsed_emit_metadata(&snapshot, &host),
            Err(TransformError::ParsedEmitMetadataSourceMismatch(_))
        ));
        assert_eq!(declaration, before);

        let (mut declaration, [_, b]) = host.mount(true);
        declaration
            .metadata_mut(b)
            .set_flags(EmitFlags::NO_LEADING_SOURCE_MAP);
        let before = declaration.clone();
        assert_eq!(
            declaration.restore_parsed_emit_metadata(&snapshot, &host),
            Err(TransformError::ParsedEmitMetadataRestoreConflict(b))
        );
        assert_eq!(declaration, before);
    }

    #[test]
    fn parsed_metadata_rejects_unimplemented_fields_and_synthetic_type_references() {
        let host = Host::new();
        let (mut arena, [a, _]) = host.mount(false);
        arena.metadata_mut(a).set_starts_on_new_line(true);
        assert!(matches!(arena.snapshot_parsed_emit_metadata(&host),
            Err(TransformError::ParsedEmitMetadataNotPortable(node)) if node == a));
        // Synthetic comments and source-map ranges stay typed refusals: the
        // comment-range admission does not widen the packet to them.
        arena.metadata.clear();
        arena
            .metadata_mut(a)
            .add_leading_comment(crate::SyntheticComment::new(
                crate::SyntheticCommentKind::SingleLine,
                " synthetic",
                false,
                true,
            ));
        assert!(matches!(arena.snapshot_parsed_emit_metadata(&host),
            Err(TransformError::ParsedEmitMetadataNotPortable(node)) if node == a));
        arena.metadata.clear();
        arena
            .metadata_mut(a)
            .set_source_map_range(crate::SourceMapRange::new(
                a.source(),
                crate::SourceRange::Synthesized,
            ));
        assert!(matches!(arena.snapshot_parsed_emit_metadata(&host),
            Err(TransformError::ParsedEmitMetadataNotPortable(node)) if node == a));
        arena.metadata.clear();
        let synthetic = arena.factory().clone_node(a).unwrap();
        arena.metadata_mut(a).set_type_node(synthetic);
        assert!(matches!(arena.snapshot_parsed_emit_metadata(&host),
            Err(TransformError::ParsedEmitMetadataNotPortable(node)) if node == synthetic));
    }

    #[test]
    fn parsed_metadata_rejects_same_snapshot_and_numeric_nodes_from_another_parse_domain() {
        let mut host = Host::new();
        let mount = |source: &SourceFile| {
            tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
                source.file_name.clone(),
                Arc::clone(source.snapshot()),
                Default::default(),
                None,
                &tsc_types::IdentityDomain::ephemeral(),
            )
            .unwrap()
        };
        host.sources[0] = mount(&host.sources[0]);
        let foreign = mount(&host.sources[0]);
        assert!(Arc::ptr_eq(host.sources[0].snapshot(), foreign.snapshot()));
        assert_eq!(host.sources[0].arena.node_base(), foreign.arena.node_base());
        let mut arena = TransformArena::new();
        let source = arena.add_source(&foreign, Some(host.ids[0]));
        let node = arena.root(source).unwrap();
        arena
            .metadata_mut(node)
            .set_flags(EmitFlags::NO_TRAILING_SOURCE_MAP);
        assert!(matches!(arena.snapshot_parsed_emit_metadata(&host),
            Err(TransformError::ParsedEmitMetadataSourceMismatch(id)) if id == host.ids[0]));
    }

    #[test]
    fn parsed_constants_preserve_number_bits_and_string_code_units() {
        let host = Host::new();
        let (mut javascript, [a, b]) = host.mount(false);
        let number =
            EmitConstantValue::Number(crate::JavaScriptNumber::from_bits(0x8000_0000_0000_0000));
        let string = EmitConstantValue::String(crate::JavaScriptString::from_code_units(vec![
            0x6587, 0xd83d, 0xde00, 0xd800,
        ]));
        javascript
            .metadata_mut(a)
            .set_constant_value(number.clone());
        javascript
            .metadata_mut(b)
            .set_constant_value(string.clone());
        let snapshot = javascript.snapshot_parsed_emit_metadata(&host).unwrap();
        let (mut declaration, [a, b]) = host.mount(true);
        declaration
            .restore_parsed_emit_metadata(&snapshot, &host)
            .unwrap();
        assert_eq!(
            declaration.metadata(a).unwrap().constant_value(),
            Some(&number)
        );
        assert_eq!(
            declaration.metadata(b).unwrap().constant_value(),
            Some(&string)
        );
    }

    /// A parsed node of the source's arena other than its root.
    fn parsed_sibling(host: &Host, index: usize, root: TransformNode) -> TransformNode {
        let arena = &host.sources[index].arena;
        (arena.node_base()..arena.node_end())
            .map(|id| TransformNode::new(root.source(), NodeId(id)))
            .find(|&node| node != root)
            .expect("a parsed source holds more than its root")
    }

    #[test]
    fn parsed_metadata_comment_ranges_survive_reordered_mounts_with_their_own_source_identity() {
        let host = Host::new();
        let (mut javascript, [a, b]) = host.mount(false);
        let (a_child, b_child) = (parsed_sibling(&host, 0, a), parsed_sibling(&host, 1, b));
        let positions = |index: usize| host.sources[index].positions();
        // The upstream producer shape (`moveRangePos(receiver, -1)`): an
        // end-only range; here it indexes the OTHER source's text.
        let end_only = CommentRange::from_raw(b.source(), u32::MAX, 5, positions(1)).unwrap();
        let start_only = CommentRange::from_raw(a.source(), 4, u32::MAX, positions(0)).unwrap();
        let paired = CommentRange::from_raw(b.source(), 0, 5, positions(1)).unwrap();
        let synthesized = CommentRange::new(a.source(), crate::SourceRange::Synthesized);
        javascript.metadata_mut(a).set_comment_range(end_only);
        javascript
            .metadata_mut(a_child)
            .set_comment_range(start_only);
        javascript.metadata_mut(b).set_comment_range(paired);
        javascript
            .metadata_mut(b_child)
            .set_comment_range(synthesized);
        javascript
            .metadata_mut(b_child)
            .set_flags(EmitFlags::NO_COMMENTS);
        let snapshot = javascript.snapshot_parsed_emit_metadata(&host).unwrap();
        drop(javascript);

        let (mut declaration, [a2, b2]) = host.mount(true);
        assert_ne!(a2.source(), a.source());
        declaration
            .restore_parsed_emit_metadata(&snapshot, &host)
            .unwrap();
        let restored = |node: TransformNode| declaration.metadata(node).unwrap().comment_range();
        assert_eq!(
            restored(a2),
            Some(CommentRange::from_raw(b2.source(), u32::MAX, 5, positions(1)).unwrap())
        );
        assert_eq!(
            restored(TransformNode::new(a2.source(), a_child.node())),
            Some(CommentRange::from_raw(a2.source(), 4, u32::MAX, positions(0)).unwrap())
        );
        assert_eq!(
            restored(b2),
            Some(CommentRange::from_raw(b2.source(), 0, 5, positions(1)).unwrap())
        );
        // The synthesized range was stamped with source `a`: it remaps to
        // `a`'s new index even though it decorates a node of `b`.
        let b2_child = TransformNode::new(b2.source(), b_child.node());
        assert_eq!(
            restored(b2_child),
            Some(CommentRange::new(
                a2.source(),
                crate::SourceRange::Synthesized
            ))
        );
        assert_eq!(
            restored(b2_child).unwrap().range(),
            CommentSourceRange::Synthesized
        );
        assert_eq!(
            declaration.metadata(b2_child).unwrap().flags(),
            EmitFlags::NO_COMMENTS
        );
        assert_eq!(declaration.metadata.len(), 4);
    }

    #[test]
    fn parsed_metadata_comment_range_sources_are_validated_and_restore_stays_atomic() {
        let host = Host::new();
        let (mut javascript, [a, b]) = host.mount(false);
        javascript.metadata_mut(a).set_comment_range(
            CommentRange::from_raw(b.source(), u32::MAX, 5, host.sources[1].positions()).unwrap(),
        );
        let snapshot = javascript.snapshot_parsed_emit_metadata(&host).unwrap();

        // The range's source (b) is absent from the target arena: nothing is
        // written even though the annotated node's source (a) is mounted.
        let mut declaration = TransformArena::new();
        declaration.add_source(&host.sources[0], Some(host.ids[0]));
        let before = declaration.clone();
        assert!(matches!(
            declaration.restore_parsed_emit_metadata(&snapshot, &host),
            Err(TransformError::ParsedEmitMetadataSourceMismatch(id)) if id == host.ids[1]
        ));
        assert_eq!(declaration, before);

        // A range over a source mounted without Program identity cannot be
        // remembered: the boundary refuses instead of guessing an owner.
        let mut javascript = TransformArena::new();
        let source = javascript.add_source(&host.sources[0], Some(host.ids[0]));
        let detached = javascript.add_source(&host.sources[1], None);
        let node = javascript.root(source).unwrap();
        javascript.metadata_mut(node).set_comment_range(
            CommentRange::from_raw(detached, u32::MAX, 5, host.sources[1].positions()).unwrap(),
        );
        assert!(matches!(
            javascript.snapshot_parsed_emit_metadata(&host),
            Err(TransformError::MissingProgramSource(_))
        ));

        // A foreign Program with identical text and numeric identities is
        // still refused through the comment range's source.
        let foreign = Host::new();
        let (mut declaration, _) = foreign.mount(false);
        let before = declaration.clone();
        assert!(matches!(
            declaration.restore_parsed_emit_metadata(&snapshot, &foreign),
            Err(TransformError::ParsedEmitMetadataSourceMismatch(_))
        ));
        assert_eq!(declaration, before);
    }
}

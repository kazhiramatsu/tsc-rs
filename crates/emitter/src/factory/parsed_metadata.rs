use std::collections::BTreeMap;
use std::sync::Arc;

use tsc_diagnostics::TextSnapshot;
use tsc_program::SourceFileId;
use tsc_syntax::{NodeId, SourceFile};
use tsc_types::IdentityLease;

use super::{TransformArena, TransformNode, TransformSourceId};
use crate::{EmitFlags, EmitHost, EmitMetadata, TransformError};

/// Metadata actually attached to original parse nodes by a completed
/// JavaScript transform/print. It is passed only to the declaration transform
/// of that same ordinary emit. Fresh getter/forced operations start empty.
///
/// Synthetic nodes and their `original` chains are never projected into this
/// snapshot. The first portable packet covers the observed flags/typeNode
/// mutations; other metadata is a typed refusal, never silently discarded.
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
    type_node: Option<(SourceFileId, NodeId)>,
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
            remainder.type_node = None;
            if remainder != EmitMetadata::default() {
                return Err(TransformError::ParsedEmitMetadataNotPortable(node));
            }
            let identity = snapshot.remember_node(self, host, node)?;
            let type_node = metadata
                .type_node()
                .map(|node| snapshot.remember_node(self, host, node))
                .transpose()?;
            if snapshot
                .nodes
                .insert(
                    identity,
                    ParsedNodeMetadata {
                        flags: metadata.flags(),
                        type_node,
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
        let mut restored = Vec::with_capacity(snapshot.nodes.len());
        for (&(program, id), metadata) in &snapshot.nodes {
            let node = TransformNode::new(mounted[&program], id);
            if self
                .metadata(node)
                .is_some_and(|value| value != &EmitMetadata::default())
            {
                return Err(TransformError::ParsedEmitMetadataRestoreConflict(node));
            }
            let value = EmitMetadata {
                flags: metadata.flags,
                type_node: metadata
                    .type_node
                    .map(|(program, id)| TransformNode::new(mounted[&program], id)),
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
        let program = arena.validate_parsed_metadata_source(node.source(), host)?;
        let original = host
            .source_file(program)
            .and_then(|source| source.syntax())
            .ok_or(TransformError::ParsedEmitMetadataSourceMismatch(program))?;
        self.sources
            .entry(program)
            .or_insert_with(|| ParsedSourceIdentity::new(original));
        Ok((program, node.node()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EmitSource;
    use std::path::Path;
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
        fn current_directory(&self) -> &Path {
            Path::new("/")
        }
        fn common_source_directory(&self) -> &Path {
            Path::new("/src/")
        }
        fn config_file_path(&self) -> Option<&Path> {
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
            let path = Path::new(&source.file_name);
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
}

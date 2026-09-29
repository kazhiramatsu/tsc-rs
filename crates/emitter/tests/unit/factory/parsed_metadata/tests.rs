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
        .map(|id| TransformNode::new(root.source(), NodeId::new(id)))
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

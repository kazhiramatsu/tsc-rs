//! Session facts must follow emitted identities without entering syntax copies.

use tsc_emitter::{
    prepare_emit_source, EmitFlags, TransformArena, TransformFlags, TransformNode,
    TransformNodeArray,
};
use tsc_syntax::{parse_source_file, NodeData, NodeId};

#[test]
fn session_facts_are_source_scoped_and_independent_of_syntax_and_clones() {
    let parsed = parse_source_file("a.ts", "const value: number = 1;", Default::default(), None);
    let mut arena = TransformArena::new();
    let first = arena.add_source(&parsed, None);
    let second = arena.add_source(&parsed, None);
    let root = arena.root(first).unwrap();
    let other_root = arena.root(second).unwrap();
    assert_eq!(root.node(), other_root.node());
    let NodeData::SourceFile(data) = arena.node(root).unwrap().data() else {
        panic!("source file");
    };
    let array = TransformNodeArray::new(first, data.statements.unwrap());
    let other_array = TransformNodeArray::new(second, array.array());
    let before = arena.source(first).unwrap().syntax().clone();
    arena.set_transform_flags(root, TransformFlags::CONTAINS_TYPE_SCRIPT);
    arena.set_array_transform_flags(array, TransformFlags::CONTAINS_ES_2015);
    arena.metadata_mut(root).add_flags(EmitFlags::NO_COMMENTS);
    arena.factory().set_multi_line(root, true).unwrap();

    assert_eq!(arena.transform_flags(other_root), TransformFlags::NONE);
    assert_eq!(
        arena.array_transform_flags(other_array),
        TransformFlags::NONE
    );
    assert!(arena.metadata(other_root).is_none());
    assert_eq!(arena.source(first).unwrap().syntax(), &before);
    assert_eq!(arena.multi_line(root).unwrap(), Some(true));
    assert_eq!(arena.node(root).unwrap().multi_line(), None);

    let mut cloned = arena.clone();
    assert_ne!(arena.id(), cloned.id());
    assert_eq!(cloned.transform_flags(root), arena.transform_flags(root));
    assert_eq!(
        cloned.array_transform_flags(array),
        arena.array_transform_flags(array)
    );
    cloned.factory().set_multi_line(root, false).unwrap();
    cloned.clear_session_metadata();
    assert_eq!(cloned.transform_flags(root), TransformFlags::NONE);
    assert_eq!(cloned.array_transform_flags(array), TransformFlags::NONE);
    assert!(cloned.metadata(root).is_none());
    // Clearing metadata has always retained own literal/format properties.
    assert_eq!(cloned.multi_line(root).unwrap(), Some(false));
    assert_eq!(arena.multi_line(root).unwrap(), Some(true));
    assert!(arena
        .transform_flags(root)
        .contains(TransformFlags::CONTAINS_TYPE_SCRIPT));

    let unknown = TransformNode::new(first, NodeId::new(u32::MAX - 1));
    arena.set_transform_flags(unknown, TransformFlags::CONTAINS_TYPE_SCRIPT);
    assert_eq!(arena.transform_flags(unknown), TransformFlags::NONE);
}

#[test]
fn prepared_flags_follow_the_source_when_mounted_in_a_different_slot() {
    let parsed = parse_source_file(
        "prepared.ts",
        "export const value: number = 1;",
        Default::default(),
        None,
    );
    let mut preparation = TransformArena::new();
    let original_slot = preparation.add_source(&parsed, None);
    let original_root = preparation.root(original_slot).unwrap();
    preparation.set_transform_flags(original_root, TransformFlags::CONTAINS_TYPE_SCRIPT);
    let prepared = preparation.into_prepared_source(original_slot);
    assert!(prepared.matches(&parsed));

    let mut arena = TransformArena::new();
    let untouched = arena.add_source(&parsed, None);
    let mounted = arena.add_prepared_source(prepared);
    assert_ne!(mounted, original_slot);
    let mounted_root = arena.root(mounted).unwrap();
    assert_eq!(
        arena.transform_flags(mounted_root),
        TransformFlags::CONTAINS_TYPE_SCRIPT
    );
    assert_eq!(
        arena.transform_flags(arena.root(untouched).unwrap()),
        TransformFlags::NONE
    );

    // Also exercise the real preparation path (classification plus censuses).
    let prepared = prepare_emit_source(&parsed, None).unwrap();
    let mounted = arena.add_prepared_source(prepared);
    assert!(arena
        .transform_flags(arena.root(mounted).unwrap())
        .contains(TransformFlags::CONTAINS_TYPE_SCRIPT));
}

#[test]
fn parsed_format_overrides_become_cloned_syntax_without_synthesized_table_entries() {
    use tsc_syntax::SyntaxKind;

    let parsed = parse_source_file("block.ts", "{ const value = 1; }", Default::default(), None);
    let block = parsed
        .arena
        .nodes()
        .iter()
        .enumerate()
        .find(|(_, node)| node.kind == SyntaxKind::Block)
        .map(|(index, _)| NodeId::new(parsed.arena.node_base() + index as u32))
        .unwrap();
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let target = arena.add_source(&parsed, None);
    let original = TransformNode::new(source, block);
    assert_eq!(arena.node(original).unwrap().multi_line(), Some(false));
    arena.factory().set_multi_line(original, true).unwrap();
    // The parsed record's false bit must not mask its session override.
    assert_eq!(arena.multi_line(original).unwrap(), Some(true));
    assert_eq!(arena.node(original).unwrap().multi_line(), Some(false));
    assert_eq!(parsed.arena.node(block).multi_line(), Some(false));
    assert_eq!(
        arena.literal_properties(original).unwrap().multi_line(),
        Some(true)
    );

    let cloned = arena.factory().clone_node(original).unwrap();
    let recloned = arena.factory().clone_node(cloned).unwrap();
    let deep = arena.factory().deep_clone_node(original).unwrap();
    let cross_source = arena
        .factory()
        .clone_node_to_source(original, target)
        .unwrap();
    let data = arena.node(original).unwrap().data();
    let updated = arena
        .factory()
        .update_node(original, data, TransformFlags::CONTAINS_ES_2015)
        .unwrap();
    assert_ne!(updated, original);
    for node in [cloned, recloned, deep, cross_source, updated] {
        assert!(!arena.is_parsed_node(node).unwrap());
        assert_eq!(arena.node(node).unwrap().multi_line(), Some(true));
        assert_eq!(arena.multi_line(node).unwrap(), Some(true));
        assert!(arena.literal_properties(node).is_none());
    }
    arena.factory().set_multi_line(updated, false).unwrap();
    assert_eq!(arena.node(updated).unwrap().multi_line(), Some(false));
    assert_eq!(arena.multi_line(updated).unwrap(), Some(false));
    assert!(arena.literal_properties(updated).is_none());
    assert_eq!(arena.multi_line(original).unwrap(), Some(true));

    let statements = arena
        .factory()
        .create_node_array(source, Vec::new())
        .unwrap();
    let created = arena
        .factory()
        .create_block(source, statements, true)
        .unwrap();
    assert_eq!(arena.node(created).unwrap().multi_line(), Some(true));
    assert!(arena.literal_properties(created).is_none());
    let mut cloned_arena = arena.clone();
    cloned_arena
        .factory()
        .set_multi_line(created, false)
        .unwrap();
    assert_eq!(cloned_arena.multi_line(created).unwrap(), Some(false));
    assert_eq!(arena.multi_line(created).unwrap(), Some(true));
}

#[test]
fn flag_tables_remain_outside_syntax_equality_and_reject_unknown_handles() {
    use tsc_emitter::TransformError;
    use tsc_syntax::NodeArrayId;

    let parsed = parse_source_file("flags.ts", "const value = 1;", Default::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let root = arena.root(source).unwrap();
    let mut cloned = arena.clone();
    assert_eq!(arena, cloned);
    cloned.set_transform_flags(root, TransformFlags::CONTAINS_ES_2015);
    assert_eq!(arena, cloned);
    let NodeData::SourceFile(data) = arena.node(root).unwrap().data() else {
        panic!("source");
    };
    let array = TransformNodeArray::new(source, data.statements.unwrap());
    arena.set_transform_flags(root, TransformFlags::CONTAINS_ES_2015);
    assert_eq!(arena, cloned);
    cloned.set_array_transform_flags(array, TransformFlags::CONTAINS_ES_2015);
    assert_eq!(arena, cloned);

    let mut other_arena = TransformArena::new();
    other_arena.add_source(&parsed, None);
    let missing_source = other_arena.add_source(&parsed, None);
    let missing = TransformNode::new(missing_source, root.node());
    let missing_node = TransformNode::new(source, NodeId::new(u32::MAX - 1));
    let missing_array = TransformNodeArray::new(source, NodeArrayId::new(u32::MAX - 1));
    arena.set_transform_flags(missing, TransformFlags::CONTAINS_ES_2015);
    arena.set_array_transform_flags(missing_array, TransformFlags::CONTAINS_ES_2015);
    assert_eq!(arena.transform_flags(missing), TransformFlags::NONE);
    assert_eq!(
        arena.array_transform_flags(missing_array),
        TransformFlags::NONE
    );
    assert!(matches!(
        arena.multi_line(missing),
        Err(TransformError::UnknownSource(_))
    ));
    assert!(matches!(
        arena.multi_line(missing_node),
        Err(TransformError::UnknownNode(_))
    ));
    assert!(matches!(
        arena.factory().set_multi_line(missing_node, true),
        Err(TransformError::UnknownNode(_))
    ));
}

#[test]
fn printer_prefers_parsed_multiline_override_and_clones_keep_the_same_output() {
    use tsc_emitter::{
        create_printer, transform_nodes, NewLineKind, PrintRequest, PrinterOptions,
        SourceFileTextMode, StandaloneWriter, TransformRoot,
    };
    use tsc_syntax::SyntaxKind;

    let parsed = parse_source_file("format.json", "[1, 2]", Default::default(), None);
    let array_id = parsed
        .arena
        .nodes()
        .iter()
        .enumerate()
        .find(|(_, node)| node.kind == SyntaxKind::ArrayLiteralExpression)
        .map(|(index, _)| NodeId::new(parsed.arena.node_base() + index as u32))
        .unwrap();
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let array = TransformNode::new(source, array_id);
    assert_eq!(arena.node(array).unwrap().multi_line(), Some(false));
    let print = |arena, node| {
        let mut result = transform_nodes(
            arena,
            vec![TransformRoot::SourceFile(source)],
            Vec::new(),
            false,
        )
        .unwrap();
        create_printer(
            PrinterOptions::new(NewLineKind::LineFeed)
                .with_source_file_text_mode(SourceFileTextMode::Canonical),
        )
        .print(
            &mut result,
            PrintRequest::StandaloneNode {
                node,
                writer: StandaloneWriter::MultiLine,
            },
            None,
        )
        .unwrap()
        .text()
        .to_owned()
    };
    let single_line = print(arena.clone(), array);
    arena.factory().set_multi_line(array, true).unwrap();
    let cloned = arena.factory().clone_node(array).unwrap();
    let overridden = print(arena.clone(), array);
    assert_ne!(single_line, overridden);
    assert!(overridden.contains('\n'));
    assert_eq!(overridden, print(arena, cloned));
}

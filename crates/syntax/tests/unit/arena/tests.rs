use super::*;
use crate::nodes::{
    IdentifierData, JSDocComment, JSDocParameterTagData, JSDocTextData, JSDocTypeExpressionData,
    JSDocTypeLiteralData, JSDocTypedefTagData, SourceFileData, StringLiteralData,
};

#[test]
fn finalizes_parent_links_and_error_aggregation() {
    let mut arena = NodeArena::new();
    let text = arena.push_text(b"x");
    let stmt = arena.alloc_node(
        NodeData::StringLiteral(StringLiteralData { text }),
        0,
        1,
        NodeFlags::THIS_NODE_HAS_ERROR,
    );
    let statements = arena.alloc_array(&[stmt], 0, 1, false, crate::NodeArrayRole::Children);
    let eof = arena.alloc_token(SyntaxKind::EndOfFileToken, 1, 1, NodeFlags::NONE);
    let root = arena.alloc_node(
        NodeData::SourceFile(SourceFileData {
            statements: Some(statements),
            end_of_file_token: Some(eof),
        }),
        0,
        1,
        NodeFlags::NONE,
    );

    arena.finalize_tree(root, &[stmt]);

    assert_eq!(arena.node(stmt).parent, Some(root));
    assert_eq!(arena.node(eof).parent, Some(root));
    assert!(NodeFlags::from_bits(arena.node(root).flags)
        .contains(NodeFlags::THIS_NODE_OR_ANY_SUB_NODES_HAS_ERROR));
}

#[test]
fn jsdoc_child_order_and_comment_union_follow_tsc() {
    let mut arena = NodeArena::new();
    let identifier = |arena: &mut NodeArena, text: &str, pos: usize| {
        arena.alloc_node(
            NodeData::Identifier(IdentifierData {
                escaped_text: tsc_types::EscapedName::from_identifier_escaped_text(text),
            }),
            pos,
            pos + text.len(),
            NodeFlags::JS_DOC,
        )
    };
    let tag_name = identifier(&mut arena, "param", 1);
    let name = identifier(&mut arena, "value", 7);
    let type_node = identifier(&mut arena, "T", 14);
    let type_expression = arena.alloc_node(
        NodeData::JSDocTypeExpression(JSDocTypeExpressionData {
            r#type: Some(type_node),
        }),
        13,
        16,
        NodeFlags::JS_DOC,
    );
    let description = arena.push_text(b"description");
    let comment_text = arena.alloc_node(
        NodeData::JSDocText(JSDocTextData { text: description }),
        17,
        28,
        NodeFlags::JS_DOC,
    );
    let comments = arena.alloc_array(
        &[comment_text],
        17,
        28,
        false,
        crate::NodeArrayRole::Children,
    );

    for (is_name_first, expected) in [
        (true, vec![tag_name, name, type_expression, comment_text]),
        (false, vec![tag_name, type_expression, name, comment_text]),
    ] {
        let parameter = arena.alloc_node(
            NodeData::JSDocParameterTag(JSDocParameterTagData {
                tag_name: Some(tag_name),
                comment: Some(JSDocComment::Nodes(comments)),
                name: Some(name),
                type_expression: Some(type_expression),
                is_name_first,
                is_bracketed: false,
            }),
            0,
            28,
            NodeFlags::JS_DOC,
        );
        let mut actual = Vec::new();
        for_each_child(&arena, arena.node(parameter), |child| {
            actual.push(child);
            false
        });
        assert_eq!(actual, expected);
    }

    let full_name = identifier(&mut arena, "Alias", 29);
    let plain = arena.push_text(b"plain");
    let typedef = arena.alloc_node(
        NodeData::JSDocTypedefTag(JSDocTypedefTagData {
            tag_name: Some(tag_name),
            comment: Some(JSDocComment::Text {
                text: plain,
                pos: 0,
                end: 0,
                text_end: 0,
            }),
            name: Some(full_name),
            full_name: Some(full_name),
            type_expression: Some(type_expression),
        }),
        29,
        40,
        NodeFlags::JS_DOC,
    );
    let mut actual = Vec::new();
    for_each_child(&arena, arena.node(typedef), |child| {
        actual.push(child);
        false
    });
    assert_eq!(actual, [tag_name, type_expression, full_name]);

    let property_tags = arena.empty_array(41);
    let type_literal = arena.alloc_node(
        NodeData::JSDocTypeLiteral(JSDocTypeLiteralData {
            js_doc_property_tags: Some(property_tags),
            is_array_type: false,
        }),
        41,
        41,
        NodeFlags::JS_DOC,
    );
    let typedef = arena.alloc_node(
        NodeData::JSDocTypedefTag(JSDocTypedefTagData {
            tag_name: Some(tag_name),
            comment: None,
            name: Some(full_name),
            full_name: Some(full_name),
            type_expression: Some(type_literal),
        }),
        41,
        42,
        NodeFlags::JS_DOC,
    );
    let mut actual = Vec::new();
    for_each_child(&arena, arena.node(typedef), |child| {
        actual.push(child);
        false
    });
    assert_eq!(actual, [tag_name, full_name, type_literal]);
}

#[test]
fn synthetic_node_array_preserves_tsc_negative_span() {
    let mut arena = NodeArena::new();
    let array = arena.alloc_synthetic_array(&Vec::new());
    assert_eq!(arena.node_array(array).pos, u32::MAX);
    assert_eq!(arena.node_array(array).end, u32::MAX);
    assert!(!arena.node_array(array).has_trailing_comma);
}

#[test]
fn only_modifier_lists_derive_modifier_metadata() {
    let mut arena = NodeArena::new();
    let modifier = arena.alloc_token(SyntaxKind::ExportKeyword, 0, 6, NodeFlags::NONE);
    let children = arena.alloc_array(&[modifier], 0, 6, false, crate::NodeArrayRole::Children);
    let modifiers = arena.alloc_array(&[modifier], 0, 6, false, crate::NodeArrayRole::Modifiers);
    assert_eq!(
        arena.node_array(children).modifier_flags,
        ModifierFlags::NONE
    );
    assert_eq!(
        arena.node_array(modifiers).modifier_flags,
        ModifierFlags::EXPORT
    );
    // A factory/decoder can know a generic list's role only on attachment.
    let statement = arena.alloc_node(
        NodeData::VariableStatement(crate::nodes::VariableStatementData {
            modifiers: Some(children),
            declaration_list: None,
        }),
        0,
        6,
        NodeFlags::NONE,
    );
    assert_eq!(arena.node(statement).modifiers_field(), Some(children));
    assert_eq!(
        arena.node_array(children).modifier_flags,
        ModifierFlags::EXPORT
    );
}

#[test]
fn record_identities_preserve_exhaustion_without_appending() {
    let mut arena = NodeArena::with_bases(u32::MAX - 1, u32::MAX - 1);
    let node = arena.alloc_token(SyntaxKind::EndOfFileToken, 0, 0, NodeFlags::NONE);
    let array = arena.alloc_array(&[node], 0, 0, false, NodeArrayRole::Children);
    assert_eq!(node.index(), u32::MAX - 1);
    assert_eq!(array.index(), u32::MAX - 1);
    assert_eq!(arena.node_end(), u32::MAX);
    assert_eq!(arena.array_end(), u32::MAX);
    assert!(arena.contains_node(node));
    assert!(arena.contains_array(array));

    let nodes = arena.len();
    let arrays = arena.node_arrays().len();
    let items = arena.array_item_count();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        arena.alloc_missing(SyntaxKind::Identifier, 0);
    }))
    .is_err());
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        arena.alloc_array(&[node], 0, 0, false, NodeArrayRole::Children);
    }))
    .is_err());
    assert_eq!(arena.len(), nodes);
    assert_eq!(arena.node_arrays().len(), arrays);
    assert_eq!(arena.array_item_count(), items);
    assert_eq!(arena.node_end(), u32::MAX);
    assert_eq!(arena.array_end(), u32::MAX);

    let empty = NodeArena::with_bases(u32::MAX, u32::MAX);
    assert!(empty.is_empty());
    assert_eq!(empty.node_end(), u32::MAX);
    assert_eq!(empty.array_end(), u32::MAX);
}

fn append_after_detaching_preserves_record_identities(arena: &NodeArena) {
    let nodes = arena.nodes().len();
    let arrays = arena.node_arrays().len();
    let node_end = arena.node_base() + u32::try_from(nodes).unwrap();
    let array_end = arena.array_base() + u32::try_from(arrays).unwrap();
    let mut copy = arena.detached_clone();
    let parent = copy.next_node_id();
    assert_eq!(parent.index(), node_end);
    let first = copy.alloc_missing(SyntaxKind::Identifier, 3);
    assert_eq!(first, parent);
    let second = copy.alloc_node(
        NodeData::Identifier(IdentifierData {
            escaped_text: tsc_types::EscapedName::from_identifier_escaped_text("next"),
        }),
        4,
        8,
        NodeFlags::NONE,
    );
    let list = copy.alloc_array(&[first, second], 3, 8, false, NodeArrayRole::Children);
    let empty = copy.empty_array(8);
    assert_eq!(second.index(), node_end + 1);
    assert_eq!(list.index(), array_end);
    assert_eq!(empty.index(), array_end + 1);
    assert_eq!(copy.node_array(list).nodes, [first, second]);
    assert!(copy.node_array(empty).nodes.is_empty());
    assert_eq!(copy.node_end(), node_end + 2);
    assert_eq!(copy.array_end(), array_end + 2);
    assert_eq!(arena.len(), nodes);
    assert_eq!(arena.node_arrays().len(), arrays);
    assert_eq!(arena.node_end(), node_end);
    assert_eq!(arena.array_end(), array_end);
}

#[test]
fn cloned_and_relocated_arenas_append_after_their_existing_records() {
    let mut source = crate::parse_source_file(
        "identities.ts",
        "export class C { method<T>(value: T) { return [value]; } }",
        crate::ParseOptions {
            node_id_base: 37,
            node_array_id_base: 19,
            ..crate::ParseOptions::default()
        },
        None,
    );
    append_after_detaching_preserves_record_identities(&source.arena);
    let domain = tsc_types::IdentityDomain::new();
    let _node_gap = domain.lease(IdentitySpace::Node, 11).unwrap();
    let _array_gap = domain.lease(IdentitySpace::NodeArray, 7).unwrap();
    source.relocate_into_identity_domain(&domain).unwrap();
    append_after_detaching_preserves_record_identities(&source.arena);
    let cloned = source.arena.clone();
    append_after_detaching_preserves_record_identities(&cloned);
}

#[test]
fn direct_rooms_and_overflow_fallback_keep_append_identities() {
    use crate::placement::{SyntaxStripe, SPACES};
    for room_size in [1, 1024] {
        let domain = tsc_types::IdentityDomain::new();
        let room = domain
            .lease_rooms(&[SPACES.into_iter().map(|space| (space, room_size)).collect()])
            .unwrap()
            .pop()
            .unwrap();
        let mut stripe = SyntaxStripe::new(room);
        let mut sources = Vec::new();
        for _ in 0..2 {
            let snapshot = tsc_diagnostics::TextSnapshot::new(
                "const value = { name: 'text' };".to_owned(),
                tsc_diagnostics::DocumentVersion::default(),
            );
            let source = stripe.parse("room.ts".into(), snapshot, crate::ParseOptions::default());
            assert_eq!(source.arena.is_published(), room_size == 1024);
            append_after_detaching_preserves_record_identities(&source.arena);
            sources.push(source);
        }
        let statistics = stripe.seal().unwrap();
        assert_eq!(statistics.fallbacks, if room_size == 1 { 2 } else { 0 });
        for source in &mut sources {
            source.relocate_into_identity_domain(&domain).unwrap();
            append_after_detaching_preserves_record_identities(&source.arena);
        }
    }
}

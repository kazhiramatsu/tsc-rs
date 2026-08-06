use super::*;
use crate::nodes::{
    IdentifierData, JSDocComment, JSDocParameterTagData, JSDocTextData, JSDocTypeExpressionData,
    JSDocTypeLiteralData, JSDocTypedefTagData, SourceFileData, StringLiteralData,
};

#[test]
fn finalizes_parent_links_and_error_aggregation() {
    let mut arena = NodeArena::new();
    let stmt = arena.alloc_node(
        NodeData::StringLiteral(StringLiteralData {
            text: "x".to_owned(),
            has_extended_unicode_escape: None,
        }),
        0,
        1,
        NodeFlags::THIS_NODE_HAS_ERROR,
    );
    let statements = arena.alloc_array(vec![stmt], 0, 1, false);
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

    arena.finalize_tree(root);

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
                escaped_text: text.to_owned(),
                text: text.to_owned(),
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
    let comment_text = arena.alloc_node(
        NodeData::JSDocText(JSDocTextData {
            text: "description".to_owned(),
        }),
        17,
        28,
        NodeFlags::JS_DOC,
    );
    let comments = arena.alloc_array(vec![comment_text], 17, 28, false);

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
    let typedef = arena.alloc_node(
        NodeData::JSDocTypedefTag(JSDocTypedefTagData {
            tag_name: Some(tag_name),
            comment: Some(JSDocComment::Text("plain".to_owned())),
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
    let array = arena.alloc_synthetic_array(Vec::new());
    assert_eq!(arena.node_array(array).pos, u32::MAX);
    assert_eq!(arena.node_array(array).end, u32::MAX);
    assert!(!arena.node_array(array).has_trailing_comma);
}

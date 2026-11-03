use super::*;

#[test]
fn generated_node_schema_has_core_nodes() {
    assert_eq!(NodeData::Token.kind(), None);
    let _ = IdentifierData {
        escaped_text: tsc_types::EscapedName::from_identifier_escaped_text(""),
    };
    assert_eq!(
        NodeData::missing(SyntaxKind::Identifier).kind(),
        Some(SyntaxKind::Identifier)
    );
    assert_eq!(NodeData::missing(SyntaxKind::SemicolonToken).kind(), None);
}

fn node(index: u32) -> Option<NodeId> {
    Some(NodeId::new(index))
}

fn array(index: u32) -> Option<NodeArrayId> {
    Some(NodeArrayId::new(index))
}

/// A record of `data`'s kind holding `data` and `js_doc`, its tail (if
/// any) in `tails`.
fn record(
    data: &NodeData,
    js_doc: Option<NodeArrayId>,
    tails: &mut tsc_store::tails::Tails,
) -> Node {
    let kind = data.kind().expect("a payload kind");
    let mut node = Node::new(kind, 1, 2, 3);
    node.encode(data, js_doc, tails, false);
    node
}

#[test]
fn every_kind_reads_back_its_missing_payload_with_and_without_jsdoc() {
    let mut tails = tsc_store::tails::Tails::new();
    for value in 0..=u16::MAX {
        let Some(kind) = SyntaxKind::from_u16(value) else {
            continue;
        };
        let data = NodeData::missing(kind);
        if data.kind().is_none() {
            continue;
        }
        for js_doc in [None, array(7)] {
            let node = record(&data, js_doc, &mut tails);
            assert_eq!(node.data(), data, "{kind:?}");
            assert_eq!(node.js_doc(), js_doc, "{kind:?}");
            assert_eq!((node.kind, node.pos, node.end, node.flags), (kind, 1, 2, 3));
        }
    }
}

#[test]
fn a_payload_past_three_slots_spills_its_cold_fields_to_a_tail() {
    let mut tails = tsc_store::tails::Tails::new();
    // Hot slots only: no tail.
    let hot = NodeData::MethodDeclaration(MethodDeclarationData {
        name: node(1),
        parameters: array(2),
        body: node(3),
        ..match NodeData::missing(SyntaxKind::MethodDeclaration) {
            NodeData::MethodDeclaration(data) => data,
            _ => unreachable!(),
        }
    });
    let inline = record(&hot, None, &mut tails);
    assert_eq!(inline.data(), hot);
    assert_eq!(tails.chunk_words(), 0, "no tail for the hot slots alone");
    // Every slot, a JSDoc list: the tail holds the rest.
    let full = NodeData::MethodDeclaration(MethodDeclarationData {
        name: node(1),
        type_parameters: array(4),
        parameters: array(2),
        r#type: node(5),
        asterisk_token: node(6),
        question_token: node(7),
        exclamation_token: node(8),
        body: node(3),
        modifiers: array(9),
    });
    let spilled = record(&full, array(10), &mut tails);
    assert_eq!(spilled.data(), full);
    assert_eq!(spilled.js_doc(), array(10));
    assert!(tails.chunk_words() > 0);
    // A copy of the record reads the same payload; a copy's own tail too.
    let mut copy = spilled;
    let mut other = tsc_store::tails::Tails::new();
    copy.copy_tail(&mut other);
    drop(tails);
    assert_eq!(copy.data(), full);
    assert_eq!(copy.js_doc(), array(10));
}

#[test]
fn scalars_comments_and_texts_read_back() {
    let mut tails = tsc_store::tails::Tails::new();
    let tag = NodeData::JSDocParameterTag(JSDocParameterTagData {
        tag_name: node(1),
        comment: Some(JSDocComment::Text {
            text: TextId::new(41),
            pos: 5,
            end: 9,
            text_end: 8,
        }),
        name: node(2),
        type_expression: node(3),
        is_name_first: true,
        is_bracketed: false,
    });
    assert_eq!(record(&tag, None, &mut tails).data(), tag);
    let nodes = NodeData::JSDoc(JSDocData {
        comment: Some(JSDocComment::Nodes(NodeArrayId::new(3))),
        tags: array(4),
    });
    assert_eq!(record(&nodes, None, &mut tails).data(), nodes);
    let clause = NodeData::ImportClause(ImportClauseData {
        name: None,
        is_type_only: true,
        phase_modifier: Some(SyntaxKind::DeferKeyword),
        named_bindings: node(2),
    });
    assert_eq!(record(&clause, None, &mut tails).data(), clause);
    let assignment = NodeData::ExportAssignment(ExportAssignmentData {
        modifiers: None,
        is_export_equals: Some(false),
        expression: node(1),
    });
    assert_eq!(record(&assignment, None, &mut tails).data(), assignment);
    let template = NodeData::TemplateHead(TemplateHeadData {
        text: TextId::EMPTY,
        raw_text: Some(TextId::new(9)),
    });
    assert_eq!(record(&template, array(2), &mut tails).data(), template);
}

#[test]
fn the_tail_bit_is_not_a_literal_flag() {
    let mut tails = tsc_store::tails::Tails::new();
    let data = NodeData::ConditionalExpression(ConditionalExpressionData {
        condition: node(1),
        question_token: node(2),
        when_true: node(3),
        colon_token: node(4),
        when_false: node(5),
    });
    let mut node = record(&data, None, &mut tails);
    assert_eq!(node.literal_flags(), 0);
    node.set_literal_flags(u16::MAX);
    assert_eq!(node.literal_flags(), u16::MAX >> 1);
    assert_eq!(node.data(), data);
}

#[test]
fn an_empty_payload_kind_built_as_a_token_reads_back_a_token() {
    let mut tails = tsc_store::tails::Tails::new();
    let mut node = Node::new(SyntaxKind::JSDocAllType, 0, 1, 0);
    node.encode(&NodeData::Token, array(3), &mut tails, false);
    assert_eq!(node.data(), NodeData::Token);
    assert_eq!(node.js_doc(), array(3));
    let empty = NodeData::missing(SyntaxKind::JSDocAllType);
    node.encode(&empty, None, &mut tails, false);
    assert_eq!(node.data(), empty);
    assert_eq!(node.js_doc(), None);
}

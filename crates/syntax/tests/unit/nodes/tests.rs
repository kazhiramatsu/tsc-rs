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

fn legacy_map_identity_words<E>(
    node: &mut Node,
    tails: &mut tsc_store::tails::Tails,
    mut map: impl FnMut(u8, u32) -> Result<u32, E>,
) -> Result<(), E> {
    let spaces = slot_spaces(node.kind);
    let (count, has_comment) = payload_shape(node.kind);
    let mut each = |space: u8, word: &mut u32| -> Result<(), E> {
        if space != SPACE_NONE && *word != 0 {
            *word = map(space, *word)?;
        }
        Ok(())
    };
    if node.literal_flags & HAS_TAIL == 0 {
        for (index, &space) in spaces.iter().enumerate().take(3) {
            each(space, &mut node.words[index])?;
        }
        if count < 3 {
            each(SPACE_ARRAY, &mut node.words[count])?;
        }
        return Ok(());
    }
    for (index, &space) in spaces.iter().enumerate().take(2) {
        each(space, &mut node.words[index + 1])?;
    }
    let at = count.saturating_sub(2);
    let len = at + 1 + if has_comment { 4 } else { 0 };
    let tail = tails.words_mut(node.words[0], len as u32);
    for (index, &space) in spaces.iter().enumerate().skip(2) {
        each(space, &mut tail[index - 2])?;
    }
    each(SPACE_ARRAY, &mut tail[at])?;
    if has_comment {
        // A text's id, or an array's after a zero word (`JSDocComment::words`).
        if tail[at + 1] != 0 {
            each(SPACE_TEXT, &mut tail[at + 1])?;
        } else {
            each(SPACE_ARRAY, &mut tail[at + 2])?;
        }
    }
    Ok(())
}

// Compare raw records and the exact callback sequence, including partial
// writes on errors. Do not decode arbitrary test words as typed payloads.
#[test]
fn relocation_layout_matches_legacy_for_every_kind_and_storage_form() {
    fn mapped_words(node: &Node, tails: &mut tsc_store::tails::Tails) -> Vec<u32> {
        let mut words = node.words.to_vec();
        if node.literal_flags & HAS_TAIL != 0 {
            let (count, comment) = payload_shape(node.kind);
            let len = count.saturating_sub(2) + 1 + if comment { 4 } else { 0 };
            words[0] = 0; // Each independent tail has its own store position.
            words.extend_from_slice(tails.words_mut(node.words[0], len as u32));
        }
        words
    }
    for value in 0..=u16::MAX {
        let Some(kind) = SyntaxKind::from_u16(value) else {
            continue;
        };
        let (count, comment) = payload_shape(kind);
        let layout = identity_layout(kind);
        assert_eq!(
            (layout.count(), layout.has_comment()),
            (count, comment),
            "{kind:?}"
        );
        for index in 0..count {
            assert_eq!(
                layout.space(index),
                slot_spaces(kind).get(index).copied().unwrap_or(0)
            );
        }
        for seed in [0u32, 1, 71, u32::MAX] {
            for form in 0..4 {
                let mut old_tails = tsc_store::tails::Tails::new();
                let mut new_tails = tsc_store::tails::Tails::new();
                let mut input = Node::new(kind, 17, 29, 3);
                input.words = [seed, seed.wrapping_add(1), seed.wrapping_add(2)];
                if form != 0 {
                    let at = count.saturating_sub(2);
                    let len = at + 1 + if comment { 4 } else { 0 };
                    let mut tail: Vec<_> = (0..len).map(|i| seed.wrapping_add(i as u32)).collect();
                    if comment && form == 2 {
                        tail[at + 1] = 0;
                    }
                    if comment && form == 3 {
                        tail[at + 1] = TextId::EMPTY.position();
                    }
                    input.words[0] = old_tails.push(&tail);
                    input.literal_flags |= HAS_TAIL;
                }
                for fail_at in [None, Some(0), Some(1), Some(2), Some(5)] {
                    let mut old = input;
                    let mut new = input;
                    // Each run must own distinct, unmodified tails.
                    old.copy_tail(&mut old_tails);
                    new.copy_tail(&mut new_tails);
                    let mut expected_calls = Vec::new();
                    let mut actual_calls = Vec::new();
                    let apply = |calls: &mut Vec<(u8, u32)>, space, word| {
                        calls.push((space, word));
                        if fail_at == Some(calls.len() - 1) {
                            return Err(calls.len());
                        }
                        Ok(if space == SPACE_TEXT && word == TextId::EMPTY.position() {
                            word
                        } else {
                            word.wrapping_add(u32::from(space) * 100)
                        })
                    };
                    let expected = legacy_map_identity_words(&mut old, &mut old_tails, |s, w| {
                        apply(&mut expected_calls, s, w)
                    });
                    let actual = new
                        .map_identity_words(&mut new_tails, |s, w| apply(&mut actual_calls, s, w));
                    assert_eq!(actual, expected, "{kind:?}, form {form}, seed {seed}");
                    assert_eq!(
                        actual_calls, expected_calls,
                        "{kind:?}, form {form}, seed {seed}"
                    );
                    assert_eq!(
                        mapped_words(&new, &mut new_tails),
                        mapped_words(&old, &mut old_tails),
                        "{kind:?}, form {form}, seed {seed}"
                    );
                    assert_eq!(
                        (
                            new.kind,
                            new.pos,
                            new.end,
                            new.flags,
                            new.parent,
                            new.literal_flags
                        ),
                        (
                            old.kind,
                            old.pos,
                            old.end,
                            old.flags,
                            old.parent,
                            old.literal_flags
                        )
                    );
                }
            }
        }
    }
}

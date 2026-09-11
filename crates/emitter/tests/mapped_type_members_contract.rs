//! Source-derived mapped member-list formatting and factory provenance controls.
use tsc_emitter::{
    base64_encode, create_printer, transform_nodes, EmitFlags, NewLineKind, PrintRequest,
    PrinterOptions, SourceFileTextMode, StandaloneWriter, TransformArena, TransformFlags,
    TransformNodeArray, TransformRoot,
};
use tsc_syntax::{parse_source_file, NodeData};

#[test]
fn mapped_type_members_matches_typescript() {
    let artifact: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/mapped-type-members.json")).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["route"], "direct-factory-and-printer");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 328);
    let mut failures = Vec::new();
    for case in cases {
        let id = case["case_id"].as_str().unwrap();
        for repetition in 0..2 {
            let outcome = std::panic::catch_unwind(|| {
                let parsed = parse_source_file(
                    "main.ts",
                    case["text"].as_str().unwrap(),
                    Default::default(),
                    None,
                );
                let mut arena = TransformArena::new();
                let source = arena.add_source(&parsed, None);
                let NodeData::SourceFile(data) =
                    &arena.node(arena.root(source).unwrap()).unwrap().data
                else {
                    panic!("source")
                };
                let statement = arena
                    .node_array(TransformNodeArray::new(source, data.statements.unwrap()))
                    .unwrap()
                    .nodes[0];
                let NodeData::TypeAliasDeclaration(data) = &arena
                    .node(arena.node_ref(source, statement).unwrap())
                    .unwrap()
                    .data
                else {
                    panic!("type alias")
                };
                let original = arena.node_ref(source, data.r#type.unwrap()).unwrap();
                let NodeData::MappedType(mut data) = arena.node(original).unwrap().data.clone()
                else {
                    panic!("mapped type")
                };
                let original_array = data
                    .members
                    .map(|array| TransformNodeArray::new(source, array));
                let mode = case["parent_mode"].as_str().unwrap();
                let node = if mode == "parsed" {
                    original
                } else {
                    let members = if case["recipe"] == "absent" {
                        None
                    } else {
                        let mut children = if case["recipe"] == "empty" {
                            Vec::new()
                        } else {
                            arena
                                .node_array(original_array.unwrap())
                                .unwrap()
                                .nodes
                                .iter()
                                .map(|id| arena.node_ref(source, *id).unwrap())
                                .collect::<Vec<_>>()
                        };
                        if case["recipe"] == "reverse" {
                            children.reverse();
                        }
                        if case["recipe"] == "clones" {
                            children = children
                                .into_iter()
                                .map(|child| arena.factory().clone_node(child).unwrap())
                                .collect();
                        }
                        let array = arena.factory().create_node_array(source, children).unwrap();
                        if case["ranged_list"].as_bool().unwrap() {
                            let record = arena.node_array(original_array.unwrap()).unwrap();
                            let (pos, end) = (record.pos, record.end);
                            arena
                                .factory()
                                .set_node_array_text_range(array, pos, end)
                                .unwrap();
                        }
                        Some(array)
                    };
                    data.members = members.map(|array| array.array());
                    if mode == "updated" {
                        arena
                            .factory()
                            .update_node(
                                original,
                                NodeData::MappedType(data),
                                TransformFlags::CONTAINS_TYPE_SCRIPT,
                            )
                            .unwrap()
                    } else {
                        assert!(matches!(mode, "created" | "ranged-created"));
                        let readonly = data
                            .readonly_token
                            .map(|id| arena.node_ref(source, id).unwrap());
                        let parameter = arena
                            .node_ref(source, data.type_parameter.unwrap())
                            .unwrap();
                        let name = data.name_type.map(|id| arena.node_ref(source, id).unwrap());
                        let question = data
                            .question_token
                            .map(|id| arena.node_ref(source, id).unwrap());
                        let ty = data.r#type.map(|id| arena.node_ref(source, id).unwrap());
                        let node = arena
                            .factory()
                            .create_mapped_type_node(
                                source, readonly, parameter, name, question, ty, members,
                            )
                            .unwrap();
                        if mode == "ranged-created" {
                            arena.factory().set_text_range(node, original).unwrap();
                        }
                        node
                    }
                };
                let flags =
                    EmitFlags::from_bits(u32::try_from(case["flags"].as_u64().unwrap()).unwrap());
                if !flags.is_empty() {
                    arena.metadata_mut(node).set_flags(flags);
                }
                let raw_position = |position| {
                    if position == u32::MAX {
                        -1
                    } else {
                        i64::from(parsed.positions().byte_to_utf16(position).unwrap())
                    }
                };
                let node_state = |node| {
                    let record = arena.node(node).unwrap();
                    let metadata = arena.metadata(node);
                    serde_json::json!({"kind":format!("{:?}", record.kind),
                        "pos":raw_position(record.pos), "end":raw_position(record.end),
                        "flags":record.flags, "emit_flags":metadata.map_or(EmitFlags::NONE, |m| m.flags()).bits(),
                        "original_present":metadata.and_then(|m| m.original()).is_some()})
                };
                let NodeData::MappedType(data) = &arena.node(node).unwrap().data else {
                    unreachable!()
                };
                let members = data.members.map(|array| {
                    let array = TransformNodeArray::new(source, array);
                    let record = arena.node_array(array).unwrap();
                    let elements = record
                        .nodes
                        .iter()
                        .map(|id| node_state(arena.node_ref(source, *id).unwrap()))
                        .collect::<Vec<_>>();
                    serde_json::json!({"same_original":Some(array)==original_array,
                        "pos":raw_position(record.pos), "end":raw_position(record.end),
                        "has_trailing_comma":record.has_trailing_comma, "elements":elements})
                });
                let tree_state = serde_json::json!({"parent":node_state(node), "members":members});
                let mut transformation = transform_nodes(
                    arena,
                    vec![TransformRoot::SourceFile(source)],
                    Vec::new(),
                    false,
                )
                .unwrap();
                let printed = create_printer(
                    PrinterOptions::new(NewLineKind::CarriageReturnLineFeed)
                        .with_source_file_text_mode(SourceFileTextMode::Canonical)
                        .with_declaration_syntax(true)
                        .with_only_print_js_doc_style(
                            case["only_print_js_doc_style"].as_bool().unwrap_or(false),
                        )
                        .with_remove_comments(case["remove_comments"].as_bool().unwrap_or(false)),
                )
                .print(
                    &mut transformation,
                    PrintRequest::StandaloneNode {
                        node,
                        writer: StandaloneWriter::MultiLine,
                    },
                    None,
                )
                .unwrap();
                let actual = serde_json::json!({"tree_state":tree_state, "text":printed.text(),
                    "utf8_base64":base64_encode(printed.text().as_bytes()), "utf8_bytes":printed.text().len(),
                    "end_utf16":{"position":printed.end().position().value(), "line":printed.end().line(), "column":printed.end().column()}});
                assert_eq!(
                    actual, case["typescript_observation"],
                    "{id} repetition {repetition}"
                );
            });
            if outcome.is_err() {
                failures.push(format!("{id} repetition {repetition}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "mapped type members failures: {failures:?}"
    );
}

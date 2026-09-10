//! Source-derived list wrapper, format and parent flag controls.
use tsc_emitter::{
    base64_encode, create_printer, transform_nodes, EmitFlags, NewLineKind, PrintRequest,
    PrinterOptions, SourceFileTextMode, StandaloneWriter, TransformArena, TransformFlags,
    TransformNodeArray, TransformRoot,
};
use tsc_syntax::{nodes::*, parse_source_file, NodeData, SyntaxKind};

fn typescript_kind_name(kind: SyntaxKind) -> String {
    // typescript.js:5828-5831 assigns these reverse-enum names twice.
    // Native kind identity is already the same301/302; match its observable
    // final alias spelling without changing any factory or printer state.
    match kind {
        SyntaxKind::ImportAttributes => {
            assert_eq!(kind as u16, 301);
            "AssertClause".into()
        }
        SyntaxKind::ImportAttribute => {
            assert_eq!(kind as u16, 302);
            "AssertEntry".into()
        }
        _ => format!("{kind:?}"),
    }
}

#[test]
fn list_format_flags_matches_typescript() {
    let artifact: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/list-format-flags.json")).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["route"], "direct-factory-and-printer");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 160);
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
                let container = case["container"].as_str().unwrap();
                let NodeData::SourceFile(data) =
                    &arena.node(arena.root(source).unwrap()).unwrap().data
                else {
                    panic!("source")
                };
                let statement = arena
                    .node_array(TransformNodeArray::new(source, data.statements.unwrap()))
                    .unwrap()
                    .nodes[0];
                let statement = arena.node_ref(source, statement).unwrap();
                let original = if container == "attributes" {
                    let NodeData::ImportDeclaration(data) = &arena.node(statement).unwrap().data
                    else {
                        panic!("import")
                    };
                    arena.node_ref(source, data.attributes.unwrap()).unwrap()
                } else {
                    let NodeData::VariableStatement(data) = &arena.node(statement).unwrap().data
                    else {
                        panic!("variable statement")
                    };
                    let list = arena
                        .node_ref(source, data.declaration_list.unwrap())
                        .unwrap();
                    let NodeData::VariableDeclarationList(data) = &arena.node(list).unwrap().data
                    else {
                        panic!("declarations")
                    };
                    let declaration = arena
                        .node_array(TransformNodeArray::new(source, data.declarations.unwrap()))
                        .unwrap()
                        .nodes[0];
                    let declaration = arena.node_ref(source, declaration).unwrap();
                    let NodeData::VariableDeclaration(data) =
                        &arena.node(declaration).unwrap().data
                    else {
                        panic!("declaration")
                    };
                    arena
                        .node_ref(
                            source,
                            if container.ends_with("binding") {
                                data.name
                            } else {
                                data.initializer
                            }
                            .unwrap(),
                        )
                        .unwrap()
                };
                let array = match &arena.node(original).unwrap().data {
                    NodeData::ArrayLiteralExpression(data) => data.elements,
                    NodeData::ObjectLiteralExpression(data) => data.properties,
                    NodeData::ArrayBindingPattern(data) => data.elements,
                    NodeData::ObjectBindingPattern(data) => data.elements,
                    NodeData::ImportAttributes(data) => data.elements,
                    _ => panic!("list owner"),
                }
                .unwrap();
                let children = arena
                    .node_array(TransformNodeArray::new(source, array))
                    .unwrap()
                    .nodes
                    .iter()
                    .map(|id| arena.node_ref(source, *id).unwrap())
                    .collect::<Vec<_>>();
                if case["last_starts_on_new_line"].as_bool().unwrap() {
                    arena
                        .metadata_mut(*children.last().unwrap())
                        .set_starts_on_new_line(true);
                }
                let supplied = arena.factory().create_node_array(source, children).unwrap();
                let multi_line = case["multi_line"].as_bool().unwrap();
                let node = match container {
                    "array" => arena
                        .factory()
                        .create_node(
                            source,
                            NodeData::ArrayLiteralExpression(ArrayLiteralExpressionData {
                                elements: Some(supplied.array()),
                            }),
                            TransformFlags::NONE,
                        )
                        .unwrap(),
                    "object" => arena
                        .factory()
                        .create_node(
                            source,
                            NodeData::ObjectLiteralExpression(ObjectLiteralExpressionData {
                                properties: Some(supplied.array()),
                            }),
                            TransformFlags::NONE,
                        )
                        .unwrap(),
                    "array-binding" => arena
                        .factory()
                        .create_array_binding_pattern(source, supplied)
                        .unwrap(),
                    "object-binding" => arena
                        .factory()
                        .create_object_binding_pattern(source, supplied)
                        .unwrap(),
                    "attributes" => arena
                        .factory()
                        .create_import_attributes(source, supplied, Some(multi_line), None)
                        .unwrap(),
                    _ => unreachable!(),
                };
                if container != "attributes" {
                    arena.factory().set_multi_line(node, multi_line).unwrap();
                }
                let flags =
                    EmitFlags::from_bits(u32::try_from(case["flags"].as_u64().unwrap()).unwrap());
                if !flags.is_empty() {
                    arena.metadata_mut(node).set_flags(flags);
                }
                let raw_position = |value| {
                    if value == u32::MAX {
                        -1
                    } else {
                        i64::from(parsed.positions().byte_to_utf16(value).unwrap())
                    }
                };
                let node_state = |node| {
                    let record = arena.node(node).unwrap();
                    let metadata = arena.metadata(node);
                    serde_json::json!({"kind":typescript_kind_name(record.kind), "pos":raw_position(record.pos), "end":raw_position(record.end),
                        "flags":record.flags, "emit_flags":metadata.map_or(EmitFlags::NONE, |m| m.flags()).bits(),
                        "original_present":metadata.and_then(|m| m.original()).is_some(),
                        "starts_on_new_line":metadata.and_then(|m| m.starts_on_new_line())})
                };
                let record = arena.node(node).unwrap();
                let (array, multi_line) = match &record.data {
                    NodeData::ArrayLiteralExpression(data) => (data.elements, record.multi_line),
                    NodeData::ObjectLiteralExpression(data) => (data.properties, record.multi_line),
                    NodeData::ArrayBindingPattern(data) => (data.elements, record.multi_line),
                    NodeData::ObjectBindingPattern(data) => (data.elements, record.multi_line),
                    NodeData::ImportAttributes(data) => (data.elements, data.multi_line),
                    _ => unreachable!(),
                };
                let array = TransformNodeArray::new(source, array.unwrap());
                let record = arena.node_array(array).unwrap();
                let elements = record
                    .nodes
                    .iter()
                    .map(|id| node_state(arena.node_ref(source, *id).unwrap()))
                    .collect::<Vec<_>>();
                let mut parent = node_state(node);
                parent["multi_line"] = serde_json::json!(multi_line);
                let tree_state = serde_json::json!({"parent":parent, "members":{"same_input":array==supplied,
                    "pos":raw_position(record.pos), "end":raw_position(record.end), "has_trailing_comma":record.has_trailing_comma, "elements":elements}});
                let mut transformation = transform_nodes(
                    arena,
                    vec![TransformRoot::SourceFile(source)],
                    Vec::new(),
                    false,
                )
                .unwrap();
                let printed = create_printer(
                    PrinterOptions::new(NewLineKind::CarriageReturnLineFeed)
                        .with_source_file_text_mode(SourceFileTextMode::Canonical),
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
                let actual = serde_json::json!({"tree_state":tree_state, "text":printed.text(), "utf8_base64":base64_encode(printed.text().as_bytes()), "utf8_bytes":printed.text().len(),
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
        "list format flags failures: {failures:?}"
    );
}

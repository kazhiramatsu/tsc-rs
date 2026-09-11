//! Source-produced direct controls for CommaListElements (format 528).
//! These do not admit custom transforms through the compiler Program API.
use tsc_emitter::{
    base64_encode, create_printer, transform_nodes, CommentRange, EmitFlags, NewLineKind,
    PrintRequest, PrinterOptions, SourceFileTextMode, SourceRange, TransformArena, TransformFlags,
    TransformNodeArray, TransformRoot,
};
use tsc_syntax::{nodes::CommaListExpressionData, parse_source_file, NodeData};

#[test]
fn comma_list_printer_matches_typescript() {
    let artifact: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/comma-list-printer.json")).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["route"], "direct-printer-metadata");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 50);
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
                let root = arena.root(source).unwrap();
                let NodeData::SourceFile(mut source_data) = arena.node(root).unwrap().data.clone()
                else {
                    panic!("source file")
                };
                let statement_id = arena
                    .node_array(TransformNodeArray::new(
                        source,
                        source_data.statements.unwrap(),
                    ))
                    .unwrap()
                    .nodes[0];
                let statement = arena.node_ref(source, statement_id).unwrap();
                let NodeData::ExpressionStatement(mut statement_data) =
                    arena.node(statement).unwrap().data.clone()
                else {
                    panic!("expression statement")
                };
                let call = arena
                    .node_ref(source, statement_data.expression.unwrap())
                    .unwrap();
                let NodeData::CallExpression(mut call_data) =
                    arena.node(call).unwrap().data.clone()
                else {
                    panic!("call")
                };
                let original_array = TransformNodeArray::new(source, call_data.arguments.unwrap());
                let original_ids = arena.node_array(original_array).unwrap().nodes.clone();
                let original = original_ids
                    .iter()
                    .map(|id| arena.node_ref(source, *id).unwrap())
                    .collect::<Vec<_>>();
                let mut elements = Vec::new();
                for element in &original {
                    let current = if case["synthetic"].as_bool() == Some(true) {
                        let record = arena.node(*element).unwrap().clone();
                        assert!(matches!(record.data, NodeData::Identifier(_)));
                        let created = arena
                            .factory()
                            .create_node(source, record.data, TransformFlags::NONE)
                            .unwrap();
                        if case["comment_range"].as_bool() == Some(true) {
                            let range =
                                SourceRange::from_raw(record.pos, record.end, parsed.positions())
                                    .unwrap();
                            arena
                                .metadata_mut(created)
                                .set_comment_range(CommentRange::new(source, range));
                        }
                        created
                    } else {
                        *element
                    };
                    if let Some(flags) = case["child_flags"].as_u64() {
                        arena
                            .metadata_mut(current)
                            .set_flags(EmitFlags::from_bits(u32::try_from(flags).unwrap()));
                    }
                    elements.push(current);
                }
                if let Some(index) = case["new_line"].as_u64() {
                    arena
                        .metadata_mut(elements[usize::try_from(index).unwrap()])
                        .set_starts_on_new_line(true);
                }
                let elements_array = if case["synthetic"].as_bool() == Some(true) {
                    arena.factory().create_node_array(source, elements).unwrap()
                } else {
                    original_array
                };
                let comma = arena
                    .factory()
                    .create_node(
                        source,
                        NodeData::CommaListExpression(CommaListExpressionData {
                            elements: Some(elements_array.array()),
                        }),
                        TransformFlags::NONE,
                    )
                    .unwrap();
                if case["parent_range"].as_bool() == Some(true) {
                    let start = arena.node(original[0]).unwrap().pos;
                    let end = arena.node(*original.last().unwrap()).unwrap().end;
                    let range = SourceRange::from_raw(start, end, parsed.positions()).unwrap();
                    arena
                        .factory()
                        .set_text_range_from_source_range(comma, source, range)
                        .unwrap();
                }
                let mut flags = EmitFlags::from_bits(
                    u32::try_from(case["parent_flags"].as_u64().unwrap_or(0)).unwrap(),
                );
                if case["multi_line"].as_bool() == Some(true) {
                    flags |= EmitFlags::MULTI_LINE;
                }
                if !flags.is_empty() {
                    arena.metadata_mut(comma).set_flags(flags);
                }
                call_data.arguments = Some(
                    arena
                        .factory()
                        .create_node_array(source, vec![comma])
                        .unwrap()
                        .array(),
                );
                let call = arena
                    .factory()
                    .update_node(
                        call,
                        NodeData::CallExpression(call_data),
                        TransformFlags::NONE,
                    )
                    .unwrap();
                statement_data.expression = Some(call.node());
                let statement = arena
                    .factory()
                    .update_node(
                        statement,
                        NodeData::ExpressionStatement(statement_data),
                        TransformFlags::NONE,
                    )
                    .unwrap();
                source_data.statements = Some(
                    arena
                        .factory()
                        .create_node_array(source, vec![statement])
                        .unwrap()
                        .array(),
                );
                let updated = arena
                    .factory()
                    .update_node(
                        root,
                        NodeData::SourceFile(source_data),
                        TransformFlags::NONE,
                    )
                    .unwrap();
                arena.replace_root(source, updated).unwrap();
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
                        .with_remove_comments(case["remove_comments"].as_bool().unwrap()),
                )
                .print(&mut transformation, PrintRequest::SourceFile(source), None)
                .unwrap();
                let actual = serde_json::json!({ "text": printed.text(), "utf8_base64": base64_encode(printed.text().as_bytes()),
                    "utf8_bytes": printed.text().len(), "end_utf16": { "position": printed.end().position().value(), "line": printed.end().line(), "column": printed.end().column() } });
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
    assert!(failures.is_empty(), "comma printer failures: {failures:?}");
}

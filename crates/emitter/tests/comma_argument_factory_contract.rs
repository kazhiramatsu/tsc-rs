//! Direct source-derived factory identity/range and printer controls.
use tsc_emitter::{
    base64_encode, create_printer, transform_nodes, EmitFlags, NewLineKind, PrintRequest,
    PrinterOptions, SourceFileTextMode, SourceRange, TransformArena, TransformFlags,
    TransformNodeArray, TransformRoot,
};
use tsc_syntax::{nodes::*, parse_source_file, NodeData, SyntaxKind};

fn raw_position(value: u32) -> i64 {
    if value == u32::MAX {
        -1
    } else {
        i64::from(value)
    }
}

#[test]
fn comma_argument_factory_matches_typescript() {
    let artifact: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/comma-argument-factory.json")).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["route"], "direct-factory-and-printer");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 44);
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
                    panic!("source")
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
                    panic!("statement")
                };
                let call = arena
                    .node_ref(source, statement_data.expression.unwrap())
                    .unwrap();
                let NodeData::CallExpression(call_data) = arena.node(call).unwrap().data.clone()
                else {
                    panic!("call")
                };
                let original_array = TransformNodeArray::new(source, call_data.arguments.unwrap());
                let original_record = arena.node_array(original_array).unwrap().clone();
                let original = original_record
                    .nodes
                    .iter()
                    .map(|id| arena.node_ref(source, *id).unwrap())
                    .collect::<Vec<_>>();
                let supplied = match case["list_mode"].as_str().unwrap() {
                    "plain" => Some(original_array),
                    "absent" => None,
                    "hole" => {
                        let hole = arena
                            .factory()
                            .create_node(
                                source,
                                NodeData::OmittedExpression(OmittedExpressionData {}),
                                TransformFlags::NONE,
                            )
                            .unwrap();
                        let array = arena
                            .factory()
                            .create_node_array(source, vec![original[0], hole])
                            .unwrap();
                        arena
                            .factory()
                            .set_node_array_text_range(
                                array,
                                original_record.pos,
                                original_record.end,
                            )
                            .unwrap();
                        Some(array)
                    }
                    "comma" => {
                        let expression = match case["expression"].as_str().unwrap() {
                            "comma-list" => arena
                                .factory()
                                .create_node(
                                    source,
                                    NodeData::CommaListExpression(CommaListExpressionData {
                                        elements: Some(original_array.array()),
                                    }),
                                    TransformFlags::NONE,
                                )
                                .unwrap(),
                            "binary-comma" => {
                                let mut left = original[0];
                                for right in &original[1..] {
                                    let operator = arena
                                        .factory()
                                        .create_token(
                                            source,
                                            SyntaxKind::CommaToken,
                                            TransformFlags::NONE,
                                        )
                                        .unwrap();
                                    left = arena
                                        .factory()
                                        .create_node(
                                            source,
                                            NodeData::BinaryExpression(BinaryExpressionData {
                                                left: Some(left.node()),
                                                operator_token: Some(operator.node()),
                                                right: Some(right.node()),
                                            }),
                                            TransformFlags::NONE,
                                        )
                                        .unwrap();
                                }
                                left
                            }
                            other => panic!("unknown expression {other}"),
                        };
                        if case["ranged"].as_bool().unwrap() {
                            let pos = arena.node(original[0]).unwrap().pos;
                            let end = arena.node(*original.last().unwrap()).unwrap().end;
                            let range =
                                SourceRange::from_raw(pos, end, parsed.positions()).unwrap();
                            arena
                                .factory()
                                .set_text_range_from_source_range(expression, source, range)
                                .unwrap();
                        }
                        let flags = EmitFlags::from_bits(
                            u32::try_from(case["flags"].as_u64().unwrap()).unwrap(),
                        );
                        if !flags.is_empty() {
                            arena.metadata_mut(expression).set_flags(flags);
                        }
                        Some(
                            arena
                                .factory()
                                .create_node_array(source, vec![expression])
                                .unwrap(),
                        )
                    }
                    other => panic!("unknown list mode {other}"),
                };
                let container = case["container"].as_str().unwrap();
                let data = match container {
                    "call" | "optional-call" => NodeData::CallExpression(CallExpressionData {
                        expression: call_data.expression,
                        question_dot_token: if container == "optional-call" {
                            Some(
                                arena
                                    .factory()
                                    .create_token(
                                        source,
                                        SyntaxKind::QuestionDotToken,
                                        TransformFlags::NONE,
                                    )
                                    .unwrap()
                                    .node(),
                            )
                        } else {
                            None
                        },
                        type_arguments: None,
                        arguments: supplied.map(|array| array.array()),
                    }),
                    "new" => NodeData::NewExpression(NewExpressionData {
                        expression: call_data.expression,
                        type_arguments: None,
                        arguments: supplied.map(|array| array.array()),
                        question_dot_token: None,
                    }),
                    "array" => NodeData::ArrayLiteralExpression(ArrayLiteralExpressionData {
                        elements: supplied.map(|array| array.array()),
                    }),
                    other => panic!("unknown container {other}"),
                };
                let expression = arena
                    .factory()
                    .create_node(source, data, TransformFlags::NONE)
                    .unwrap();
                if container == "array" {
                    arena.factory().set_multi_line(expression, false).unwrap();
                }
                let array_id = match &arena.node(expression).unwrap().data {
                    NodeData::CallExpression(data) => data.arguments,
                    NodeData::NewExpression(data) => data.arguments,
                    NodeData::ArrayLiteralExpression(data) => data.elements,
                    _ => unreachable!(),
                };
                let array_state = array_id.map(|array_id| {
                    let array = TransformNodeArray::new(source, array_id);
                    let record = arena.node_array(array).unwrap();
                    let elements = record.nodes.iter().map(|id| {
                        let node = arena.node_ref(source, *id).unwrap();
                        let record = arena.node(node).unwrap();
                        let metadata = arena.metadata(node);
                        let inner_is_supplied = match &record.data {
                            NodeData::ParenthesizedExpression(data) => Some(arena.node_array(supplied.unwrap()).unwrap().nodes.contains(&data.expression.unwrap())),
                            _ => None,
                        };
                        serde_json::json!({ "kind": format!("{:?}", record.kind), "pos": raw_position(record.pos), "end": raw_position(record.end),
                            "flags": record.flags, "emit_flags": metadata.map_or(EmitFlags::NONE, |m| m.flags()).bits(),
                            "original_present": metadata.and_then(|m| m.original()).is_some(), "inner_is_supplied": inner_is_supplied })
                    }).collect::<Vec<_>>();
                    serde_json::json!({ "same_input": Some(array) == supplied, "pos": raw_position(record.pos), "end": raw_position(record.end), "has_trailing_comma": record.has_trailing_comma, "elements": elements })
                });
                statement_data.expression = Some(expression.node());
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
                        .with_source_file_text_mode(SourceFileTextMode::Canonical),
                )
                .print(&mut transformation, PrintRequest::SourceFile(source), None)
                .unwrap();
                let actual = serde_json::json!({ "array_state": array_state, "text": printed.text(), "utf8_base64": base64_encode(printed.text().as_bytes()),
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
    assert!(
        failures.is_empty(),
        "comma argument factory failures: {failures:?}"
    );
}

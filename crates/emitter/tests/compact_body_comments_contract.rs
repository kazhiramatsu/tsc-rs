//! Pinned list ownership, including upstream's repeated same-line comments.
use serde_json::{json, Value};
use tsc_emitter::{
    create_printer, transform_nodes, CommentRange, EmitFlags, NewLineKind, PrintRequest,
    PrinterOptions, SourceByteRange, SourceFileTextMode, SourceRange, StandaloneWriter,
    TransformArena, TransformNode, TransformRoot,
};
use tsc_syntax::{parse_source_file, NodeData};

fn observe(case: &Value) -> Value {
    let parsed = parse_source_file(
        "compact.ts",
        case["source"].as_str().unwrap(),
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let NodeData::SourceFile(file) = &arena.node(arena.root(source).unwrap()).unwrap().data else {
        unreachable!()
    };
    let top = arena
        .node_array_ref(source, file.statements.unwrap())
        .unwrap();
    let mut function = TransformNode::new(source, arena.node_array(top).unwrap().nodes[0]);
    let NodeData::FunctionDeclaration(data) = arena.node(function).unwrap().data.clone() else {
        unreachable!()
    };
    let mut body = TransformNode::new(source, data.body.unwrap());
    let NodeData::Block(block) = &arena.node(body).unwrap().data else {
        unreachable!()
    };
    let original_array = arena
        .node_array_ref(source, block.statements.unwrap())
        .unwrap();
    let mut statements: Vec<_> = arena
        .node_array(original_array)
        .unwrap()
        .nodes
        .iter()
        .map(|&id| TransformNode::new(source, id))
        .collect();
    let target = *statements.last().unwrap();
    match case["variant"].as_str().unwrap() {
        "TargetNoLeading" => arena
            .metadata_mut(target)
            .add_flags(EmitFlags::NO_LEADING_COMMENTS),
        "TargetNoComments" => arena.metadata_mut(target).add_flags(EmitFlags::NO_COMMENTS),
        "None" | "PreviousNoTrailing" | "BodyNoNested" => {}
        _ => unreachable!(),
    }
    let prefix = case["prefix"].as_str().unwrap();
    if prefix != "none" {
        let mut nodes = Vec::new();
        for i in 0..2 {
            let identifier = arena
                .factory()
                .create_identifier(source, format!("prefix{i}"))
                .unwrap();
            nodes.push(
                arena
                    .factory()
                    .create_expression_statement(source, identifier)
                    .unwrap(),
            );
        }
        if prefix.ends_with("comment-range") {
            let record = arena.node(target).unwrap();
            let range = SourceByteRange::new(
                record.pos,
                record.end,
                arena.source(source).unwrap().syntax().positions(),
            )
            .unwrap();
            assert_eq!(
                (
                    arena.node(nodes[1]).unwrap().pos,
                    arena.node(nodes[1]).unwrap().end
                ),
                (u32::MAX, u32::MAX)
            );
            arena
                .metadata_mut(nodes[1])
                .set_comment_range(CommentRange::new(source, SourceRange::Original(range)));
            arena
                .metadata_mut(nodes[1])
                .add_flags(EmitFlags::NO_COMMENTS);
            if prefix == "break-comment-range" {
                arena.metadata_mut(nodes[1]).set_starts_on_new_line(true);
            }
        }
        nodes.extend(statements);
        statements = nodes;
    }
    if case["variant"] == "PreviousNoTrailing" {
        let index = statements.iter().position(|&node| node == target).unwrap();
        if index > 0 {
            arena
                .metadata_mut(statements[index - 1])
                .add_flags(EmitFlags::NO_TRAILING_COMMENTS);
        }
    }
    if prefix != "none" {
        let array = arena
            .factory()
            .update_node_array(original_array, statements)
            .unwrap();
        // updateBlock is createBlock + update (original identity and text range).
        let updated = arena.factory().create_block(source, array, false).unwrap();
        arena.set_original_node(updated, Some(body)).unwrap();
        arena.factory().set_text_range(updated, body).unwrap();
        body = updated;
        let parameters = arena
            .node_array_ref(source, data.parameters.unwrap())
            .unwrap();
        function = arena
            .factory()
            .update_function_declaration(
                function,
                None,
                None,
                data.name.map(|id| TransformNode::new(source, id)),
                None,
                parameters,
                None,
                Some(body),
            )
            .unwrap();
    }
    if case["variant"] == "BodyNoNested" {
        arena
            .metadata_mut(body)
            .add_flags(EmitFlags::NO_NESTED_COMMENTS);
    }
    if prefix == "break-comment-range" {
        arena.metadata_mut(body).add_flags(EmitFlags::SINGLE_LINE);
    }
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![],
        false,
    )
    .unwrap();
    let newline = match case["newline"].as_str().unwrap() {
        "LineFeed" => NewLineKind::LineFeed,
        "CarriageReturnLineFeed" => NewLineKind::CarriageReturnLineFeed,
        _ => unreachable!(),
    };
    let output = create_printer(
        PrinterOptions::new(newline)
            .with_remove_comments(case["remove_comments"].as_bool().unwrap())
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
    .print(
        &mut result,
        PrintRequest::StandaloneNode {
            node: function,
            writer: StandaloneWriter::MultiLine,
        },
        None,
    )
    .unwrap();
    json!({"text":output.text(), "utf8":output.text().as_bytes(), "utf16_length":output.text_utf16().len()})
}

#[test]
fn compact_body_comments_match_typescript() {
    let artifact: Value =
        serde_json::from_slice(include_bytes!("fixtures/compact-body-comments.json")).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 240);
    let mut failures = Vec::new();
    let mut captures = Vec::new();
    for case in cases {
        let first = observe(case);
        let second = observe(case);
        assert_eq!(first, second, "repeat differs: {}", case["case_id"]);
        if first != case["output"] {
            eprintln!(
                "FAIL {}: actual {}, expected {}",
                case["case_id"], first["text"], case["output"]["text"]
            );
            failures.push(case["case_id"].clone());
        }
        captures.push(json!({"case_id":case["case_id"], "repetitions":[first, second]}));
    }
    if let Some(path) = std::env::var_os("TSC_RS_COMPACT_BODY_CAPTURE") {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        file.write_all(&serde_json::to_vec_pretty(&captures).unwrap())
            .unwrap();
    }
    eprintln!(
        "Compact body comments: {} / {} complete exact twice",
        cases.len() - failures.len(),
        cases.len()
    );
    assert!(
        failures.is_empty(),
        "compact body comment failures: {failures:?}"
    );
}

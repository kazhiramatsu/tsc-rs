//! List and ordinary node comments retain distinct flag ownership.
//! The complete printer outputs are frozen from the pinned TypeScript printer.
use tsc_emitter::{
    create_printer, transform_nodes, EmitFlags, NewLineKind, PrintRequest, PrinterOptions,
    SourceFileTextMode, TransformArena, TransformError, TransformRoot, TransformationContext,
    Transformer,
};
use tsc_syntax::{parse_source_file, NodeData};

struct FlagTransformer {
    flags: EmitFlags,
    parent: bool,
}

impl Transformer for FlagTransformer {
    fn name(&self) -> &'static str {
        "list-comment-flags"
    }

    fn transform_root(
        &mut self,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        let TransformRoot::SourceFile(source) = root else {
            return Ok(root);
        };
        let arena = context.arena();
        let root_node = arena.root(source)?;
        let NodeData::SourceFile(file) = &arena.node(root_node)?.data else {
            unreachable!()
        };
        let statements = arena
            .node_array_ref(source, file.statements.unwrap())
            .unwrap();
        let statement = arena
            .node_ref(source, arena.node_array(statements)?.nodes[0])
            .unwrap();
        let NodeData::ExpressionStatement(statement) = &arena.node(statement)?.data else {
            unreachable!()
        };
        let parent = arena
            .node_ref(source, statement.expression.unwrap())
            .unwrap();
        let array = match &arena.node(parent)?.data {
            NodeData::CallExpression(call) => call.arguments.unwrap(),
            NodeData::ArrayLiteralExpression(array) => array.elements.unwrap(),
            _ => unreachable!(),
        };
        let nodes = if self.parent {
            vec![parent]
        } else {
            arena
                .node_array(arena.node_array_ref(source, array).unwrap())?
                .nodes
                .iter()
                .map(|&node| arena.node_ref(source, node).unwrap())
                .collect()
        };
        for node in nodes {
            context
                .arena_mut()?
                .metadata_mut(node)
                .add_flags(self.flags);
        }
        Ok(root)
    }
}

#[test]
fn list_comment_flags_match_typescript_printer() {
    let artifact: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/list-comment-flags.json")).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 48);
    let mut failures = Vec::new();
    for case in cases {
        let id = case["case_id"].as_str().unwrap();
        let mut exact = true;
        for repetition in 0..2 {
            let flags = match case["variant"].as_str().unwrap() {
                "None" => EmitFlags::NONE,
                "NoLeadingComments" => EmitFlags::NO_LEADING_COMMENTS,
                "NoTrailingComments" => EmitFlags::NO_TRAILING_COMMENTS,
                "NoComments" => EmitFlags::NO_COMMENTS,
                "NoNestedComments" | "ParentNoNestedComments" => EmitFlags::NO_NESTED_COMMENTS,
                _ => unreachable!(),
            };
            let parsed = parse_source_file(
                "list-comments.ts",
                case["source"].as_str().unwrap(),
                Default::default(),
                None,
            );
            let mut arena = TransformArena::new();
            let source = arena.add_source(&parsed, None);
            let mut result = transform_nodes(
                arena,
                vec![TransformRoot::SourceFile(source)],
                vec![Box::new(FlagTransformer {
                    flags,
                    parent: case["variant"] == "ParentNoNestedComments",
                })],
                false,
            )
            .unwrap();
            let output = create_printer(
                PrinterOptions::new(NewLineKind::LineFeed)
                    .with_remove_comments(case["remove_comments"].as_bool().unwrap())
                    .with_source_file_text_mode(SourceFileTextMode::Canonical),
            )
            .print(&mut result, PrintRequest::SourceFile(source), None)
            .unwrap();
            let expected = case["output"].as_str().unwrap();
            if output.text() != expected {
                exact = false;
                eprintln!("List comment flags FAIL {id} repetition {repetition}: actual {:?}, expected {expected:?}", output.text());
            }
        }
        if exact {
            eprintln!("List comment flags EXACT x2 {id}");
        } else {
            failures.push(id);
        }
    }
    assert!(
        failures.is_empty(),
        "list comment flag failures: {failures:?}"
    );
}

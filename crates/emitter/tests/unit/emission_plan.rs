//! Shared and later-mutated nodes retain the initial-root plan snapshot.
use super::*;
use crate::{create_printer, transform_nodes, TransformArena, TransformFlags, TransformRoot};
use tsc_syntax::{parse_source_file, SourceFile};

fn completed(file: &SourceFile) -> (TransformationResult<'static>, TransformNode) {
    let prefix = parse_source_file("prefix.ts", "prefix;", Default::default(), None);
    let mut arena = TransformArena::new();
    arena.add_source(&prefix, None);
    let source = arena.add_source(file, None);
    let root = arena.root(source).unwrap();
    let result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![],
        false,
    )
    .unwrap();
    (result, root)
}

fn printer(target: ScriptTarget) -> Printer {
    create_printer(PrinterOptions::new(NewLineKind::LineFeed).with_target(target))
}

#[test]
fn shared_bodies_and_later_updates_keep_the_initial_reachable_snapshot() {
    let mut file = parse_source_file(
        "bodies.ts",
        "function a() { return 1; } function b() { return 2n; }",
        Default::default(),
        None,
    );
    let statements = file
        .arena
        .node(file.root)
        .as_source_file()
        .unwrap()
        .statements
        .unwrap();
    let functions = file.arena.node_array(statements).nodes.to_vec();
    let first = file
        .arena
        .node(functions[0])
        .as_function_declaration()
        .unwrap();
    let mut second = file
        .arena
        .node(functions[1])
        .as_function_declaration()
        .unwrap();
    let first_body = first.body.unwrap();
    let off_tree_body = second.body.unwrap();
    second.body = Some(first_body);
    file.arena
        .set_data(functions[1], NodeData::FunctionDeclaration(second));
    let (mut result, root) = completed(&file);
    let source = root.source();
    let first_body = TransformNode::new(source, first_body);
    let off_tree_body = TransformNode::new(source, off_tree_body);
    let mut printer = printer(ScriptTarget::ES_NEXT);
    printer.prepare_emission_plan(&result, root).unwrap();
    assert_eq!(printer.emission_plan.function_body_blocks.len(), 1);
    assert!(printer
        .emission_plan
        .function_body_blocks
        .contains(&first_body));
    assert!(!printer
        .emission_plan
        .function_body_blocks
        .contains(&off_tree_body));
    assert!(printer.emission_plan.structured_nodes.is_empty());

    let snapshot = printer.emission_plan.clone();
    second.body = Some(off_tree_body.node());
    let updated = result
        .arena_mut()
        .unwrap()
        .factory()
        .update_node(
            TransformNode::new(source, functions[1]),
            NodeData::FunctionDeclaration(second),
            TransformFlags::NONE,
        )
        .unwrap();
    assert_eq!(printer.emission_plan, snapshot);
    printer.prepare_emission_plan(&result, updated).unwrap();
    assert_eq!(printer.emission_plan.function_body_blocks.len(), 1);
    assert!(printer
        .emission_plan
        .function_body_blocks
        .contains(&off_tree_body));
    assert!(!printer
        .emission_plan
        .function_body_blocks
        .contains(&first_body));
    assert!(printer.emission_plan.structured_nodes.contains(&updated));
    printer.prepare_emission_plan(&result, root).unwrap();
    assert_eq!(printer.emission_plan, snapshot);
}

#[test]
fn literal_tokens_keep_their_target_sensitive_plan_facts() {
    let file = parse_source_file(
        "literals.ts",
        "1_000; 1n; 'text';",
        Default::default(),
        None,
    );
    let numeric = file
        .arena
        .nodes()
        .iter()
        .position(|node| node.kind == SyntaxKind::NumericLiteral)
        .unwrap();
    let bigint = file
        .arena
        .nodes()
        .iter()
        .position(|node| node.kind == SyntaxKind::BigIntLiteral)
        .unwrap();
    let string = file
        .arena
        .nodes()
        .iter()
        .position(|node| node.kind == SyntaxKind::StringLiteral)
        .unwrap();
    let (result, root) = completed(&file);
    let handle = |index: usize| {
        TransformNode::new(
            root.source(),
            NodeId::new(file.arena.node_base() + index as u32),
        )
    };
    let numeric = handle(numeric);
    let bigint = handle(bigint);
    let string = handle(string);
    for target in [
        ScriptTarget::ES2018,
        ScriptTarget::ES2021,
        ScriptTarget::ES_NEXT,
    ] {
        let mut printer = printer(target);
        printer.prepare_emission_plan(&result, numeric).unwrap();
        assert_eq!(
            printer.emission_plan.structured_nodes.contains(&numeric),
            target < ScriptTarget::ES2021
        );
        printer.prepare_emission_plan(&result, bigint).unwrap();
        assert!(printer.emission_plan.structured_nodes.contains(&bigint));
        printer.prepare_emission_plan(&result, string).unwrap();
        assert!(printer.emission_plan.structured_nodes.is_empty());
    }
}

#[test]
fn invalid_local_children_keep_dfs_error_order_and_the_previous_plan() {
    let mut file = parse_source_file(
        "invalid.ts",
        "1_000; first; second;",
        Default::default(),
        None,
    );
    let statements = file
        .arena
        .node(file.root)
        .as_source_file()
        .unwrap()
        .statements
        .unwrap();
    let statements = file.arena.node_array(statements).nodes.to_vec();
    let numeric = file
        .arena
        .node(statements[0])
        .as_expression_statement()
        .unwrap()
        .expression
        .unwrap();
    let first_invalid = NodeId::new(u32::MAX - 2);
    let second_invalid = NodeId::new(u32::MAX - 1);
    for (statement, invalid) in [
        (statements[1], first_invalid),
        (statements[2], second_invalid),
    ] {
        file.arena.set_data(
            statement,
            NodeData::ExpressionStatement(tsc_syntax::nodes::ExpressionStatementData {
                expression: Some(invalid),
            }),
        );
    }
    let (result, root) = completed(&file);
    let mut printer = printer(ScriptTarget::ES2018);
    printer
        .prepare_emission_plan(&result, TransformNode::new(root.source(), numeric))
        .unwrap();
    let previous = printer.emission_plan.clone();
    assert!(
        matches!(printer.prepare_emission_plan(&result, root), Err(PrinterError::UnknownStatement(index)) if index == first_invalid.index())
    );
    assert_eq!(printer.emission_plan, previous);
    let invalid_root = TransformNode::new(root.source(), first_invalid);
    assert!(
        matches!(printer.prepare_emission_plan(&result, invalid_root), Err(PrinterError::Transform(TransformError::UnknownNode(node))) if node == invalid_root)
    );
    assert_eq!(printer.emission_plan, previous);
}

#[test]
fn a_shared_rewritten_body_propagates_to_each_reachable_function() {
    let mut file = parse_source_file(
        "shared.ts",
        "function a() { return 1n; } function b() { return 2; }",
        Default::default(),
        None,
    );
    let statements = file
        .arena
        .node(file.root)
        .as_source_file()
        .unwrap()
        .statements
        .unwrap();
    let functions = file.arena.node_array(statements).nodes.to_vec();
    let first = file
        .arena
        .node(functions[0])
        .as_function_declaration()
        .unwrap();
    let mut second = file
        .arena
        .node(functions[1])
        .as_function_declaration()
        .unwrap();
    second.body = first.body;
    file.arena
        .set_data(functions[1], NodeData::FunctionDeclaration(second));
    let (result, root) = completed(&file);
    let mut printer = printer(ScriptTarget::ES_NEXT);
    printer.prepare_emission_plan(&result, root).unwrap();
    for node in [root.node(), functions[0], functions[1], first.body.unwrap()] {
        assert!(printer
            .emission_plan
            .structured_nodes
            .contains(&TransformNode::new(root.source(), node)));
    }
    // Source file, two functions, shared block, return statement and BigInt.
    assert_eq!(printer.emission_plan.structured_nodes.len(), 6);
    assert_eq!(printer.emission_plan.function_body_blocks.len(), 1);
}

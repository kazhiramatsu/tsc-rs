//! tsc parseArgumentExpression: call and `new` arguments are parsed outside
//! the decorator and `in`-disallowing contexts (TypeScript 7.1's
//! parseArgumentExpression does the same).
use tsc_syntax::{for_each_child, NodeData, NodeId, ParseOptions, SourceFile, SyntaxKind};
use tsc_types::NodeFlags;

fn parse(text: &str) -> SourceFile {
    tsc_syntax::parse_source_file("/a.ts", text, ParseOptions::default(), None)
}

/// The first node of `kind` in source order.
fn find(file: &SourceFile, kind: SyntaxKind) -> NodeId {
    fn visit(file: &SourceFile, node: NodeId, kind: SyntaxKind) -> Option<NodeId> {
        if file.arena.node(node).kind == kind {
            return Some(node);
        }
        let mut found = None;
        for_each_child(&file.arena, file.arena.node(node), |child| {
            found = visit(file, child, kind);
            found.is_some()
        });
        found
    }
    visit(file, file.root, kind).unwrap_or_else(|| panic!("no {kind:?}"))
}

#[test]
fn decorator_call_arguments_are_outside_the_decorator_context() {
    let file = parse("declare function dec(x: unknown): any;\ndeclare const a: number[];\n@dec(a[0]) class C {}\n");
    assert!(
        file.parse_diagnostics.is_empty(),
        "{:?}",
        file.parse_diagnostics
    );
    let access = find(&file, SyntaxKind::ElementAccessExpression);
    let flags = NodeFlags::from_bits(file.arena.node(access).flags);
    assert!(!flags.intersects(NodeFlags::DECORATOR_CONTEXT));
    assert!(matches!(
        file.arena
            .node(file.arena.node(access).parent.unwrap())
            .data,
        NodeData::CallExpression(_)
    ));
}

#[test]
fn arguments_in_a_for_initializer_allow_in() {
    let file = parse("declare function f(x: boolean): number;\ndeclare const o: object;\nfor (var x = f(\"a\" in o); ;) {}\n");
    assert!(
        file.parse_diagnostics.is_empty(),
        "{:?}",
        file.parse_diagnostics
    );
    let binary = find(&file, SyntaxKind::BinaryExpression);
    let NodeData::BinaryExpression(data) = &file.arena.node(binary).data else {
        unreachable!()
    };
    assert_eq!(
        file.arena.node(data.operator_token.unwrap()).kind,
        SyntaxKind::InKeyword
    );
    let flags = NodeFlags::from_bits(file.arena.node(binary).flags);
    assert!(!flags.intersects(NodeFlags::DISALLOW_IN_CONTEXT));
}

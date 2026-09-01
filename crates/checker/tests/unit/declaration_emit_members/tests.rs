use super::*;
use crate::state::test_support::with_program_state;
use tsc_emitter::{
    EmitInternalNodeBuilderFlags, EmitNodeBuilderFlags, EmitSymbolTracker, SourceFileId,
    TransformArena,
};
use tsc_types::CompilerOptions;

struct NoopTracker;
impl EmitSymbolTracker for NoopTracker {}

fn with_member_state(
    source: &str,
    run: impl FnOnce(&mut CheckerState<'_>, &mut TransformArena, tsc_emitter::TransformSourceId),
) {
    with_program_state(
        &[("/main.ts", source)],
        &CompilerOptions::default(),
        |checker| {
            let mut arena = TransformArena::new();
            let target =
                arena.add_source(checker.binder.source(0), Some(SourceFileId::from_raw(0)));
            run(checker, &mut arena, target);
        },
    );
}

fn node_kind(
    arena: &TransformArena,
    target: tsc_emitter::TransformSourceId,
    node: tsc_emitter::TransformNode,
) -> SyntaxKind {
    let _ = target;
    arena
        .source(node.source())
        .expect("transform source")
        .syntax()
        .arena
        .node(node.node())
        .kind
}

fn statement_at(checker: &CheckerState<'_>, index: usize) -> NodeId {
    let source = checker.binder.source(0);
    let NodeData::SourceFile(data) = &source.arena.node(source.root).data else {
        panic!("source file expected")
    };
    let statements = source
        .arena
        .node_array(data.statements.expect("statements"));
    statements.nodes[index]
}

fn first_variable_declaration(checker: &CheckerState<'_>, statement: usize) -> NodeId {
    let statement = statement_at(checker, statement);
    let source = checker.binder.source(0);
    let NodeData::VariableStatement(data) = &source.arena.node(statement).data else {
        panic!("variable statement expected")
    };
    let NodeData::VariableDeclarationList(list) = &source
        .arena
        .node(data.declaration_list.expect("declaration list"))
        .data
    else {
        panic!("declaration list expected")
    };
    source
        .arena
        .node_array(list.declarations.expect("declarations"))
        .nodes[0]
}

#[test]
fn dm_serialize_type_of_declaration() {
    with_member_state(
        "export const named: string = \"a\";",
        |checker, arena, target| {
            let declaration = first_variable_declaration(checker, 0);
            let root = checker.binder.source(0).root;
            let mut tracker = NoopTracker;
            let node = checker
                .emit_create_type_of_declaration(
                    arena,
                    target,
                    declaration,
                    root,
                    EmitNodeBuilderFlags::DECLARATION_EMIT,
                    EmitInternalNodeBuilderFlags::DECLARATION_EMIT,
                    &mut tracker,
                )
                .expect("serialization succeeds")
                .expect("annotated declaration serializes");
            assert_eq!(node_kind(arena, target, node), SyntaxKind::StringKeyword);
        },
    );
    // Non-inferred-type kinds take the AnyKeyword token fallback.
    with_member_state("export function f(): void {}", |checker, arena, target| {
        let function = statement_at(checker, 0);
        let root = checker.binder.source(0).root;
        let mut tracker = NoopTracker;
        let node = checker
            .emit_create_type_of_declaration(
                arena,
                target,
                function,
                root,
                EmitNodeBuilderFlags::DECLARATION_EMIT,
                EmitInternalNodeBuilderFlags::DECLARATION_EMIT,
                &mut tracker,
            )
            .expect("fallback succeeds")
            .expect("fallback token");
        assert_eq!(node_kind(arena, target, node), SyntaxKind::AnyKeyword);
    });
}

#[test]
fn dm_serialize_return_type() {
    with_member_state(
        "export function f(): number { return 1; }",
        |checker, arena, target| {
            let function = statement_at(checker, 0);
            let root = checker.binder.source(0).root;
            let mut tracker = NoopTracker;
            let node = checker
                .emit_create_return_type_of_signature_declaration(
                    arena,
                    target,
                    function,
                    root,
                    EmitNodeBuilderFlags::DECLARATION_EMIT,
                    EmitInternalNodeBuilderFlags::DECLARATION_EMIT,
                    &mut tracker,
                )
                .expect("serialization succeeds")
                .expect("annotated return serializes");
            assert_eq!(node_kind(arena, target, node), SyntaxKind::NumberKeyword);
        },
    );
}

#[test]
fn dm_literal_const_value() {
    let source = "export const s = \"hi\";\n\
                  export const n = 3;\n\
                  export const m = -4;\n\
                  export const b = true;\n\
                  export const g = 5n;";
    with_member_state(source, |checker, arena, target| {
        let mut tracker = NoopTracker;
        let expectations = [
            (0usize, SyntaxKind::StringLiteral),
            (1, SyntaxKind::NumericLiteral),
            (2, SyntaxKind::PrefixUnaryExpression),
            (3, SyntaxKind::TrueKeyword),
            (4, SyntaxKind::BigIntLiteral),
        ];
        for (statement, expected) in expectations {
            let declaration = first_variable_declaration(checker, statement);
            let node = checker
                .emit_create_literal_const_value(arena, target, declaration, &mut tracker)
                .expect("literal serializes");
            assert_eq!(
                node_kind(arena, target, node),
                expected,
                "statement {statement}"
            );
        }
    });
}

#[test]
fn dm_late_bound_index_signatures() {
    // A source-declared index signature is skipped (info.declaration
    // set) after the present-empty result materializes.
    with_member_state(
        "export class Boxes { [key: string]: number; }",
        |checker, arena, target| {
            let class = statement_at(checker, 0);
            let root = checker.binder.source(0).root;
            let mut tracker = NoopTracker;
            let result = checker
                .emit_create_late_bound_index_signatures(
                    arena,
                    target,
                    class,
                    root,
                    EmitNodeBuilderFlags::DECLARATION_EMIT,
                    EmitInternalNodeBuilderFlags::DECLARATION_EMIT,
                    &mut tracker,
                )
                .expect("member succeeds");
            // Upstream returns a PRESENT-EMPTY array here: the instance
            // info list is non-empty (result ||= [] fires) and its one
            // declared info is then skipped (:88632-88633).
            assert_eq!(result, Some(Vec::new()));
        },
    );
}

#[test]
fn dm_declaration_statements_for_source_file() {
    // Pre-lane-F this asserted the typed pending error; the statements
    // cluster now serializes the module's export table.
    with_member_state("export const x = 1;", |checker, arena, target| {
        let root = checker.binder.source(0).root;
        let mut tracker = NoopTracker;
        let statements = checker
            .emit_get_declaration_statements_for_source_file(
                arena,
                target,
                root,
                EmitNodeBuilderFlags::DECLARATION_EMIT,
                EmitInternalNodeBuilderFlags::DECLARATION_EMIT,
                &mut tracker,
            )
            .expect("statements serialize")
            .expect("module exports produce statements");
        assert!(!statements.is_empty());
        assert_eq!(
            node_kind(arena, target, statements[0]),
            SyntaxKind::VariableStatement
        );
    });
}

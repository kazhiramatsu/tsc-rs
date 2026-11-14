use super::*;
use crate::{transform_nodes, TransformArena, TransformRoot, TransformationContext, Transformer};
use std::{cell::RefCell, rc::Rc};
use tsc_syntax::parse_source_file;

fn plain_result(text: &str) -> (TransformationResult<'static>, TransformSourceId) {
    let file = parse_source_file("event.ts", text, Default::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&file, None);
    let result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![],
        false,
    )
    .unwrap();
    (result, source)
}

fn canonical_printer() -> Printer {
    create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
}

struct MutatingHooks {
    events: Rc<RefCell<Vec<(&'static str, String)>>>,
}

impl Transformer for MutatingHooks {
    fn name(&self) -> &'static str {
        "eventMutatingHooks"
    }

    fn initialize(&mut self, context: &mut TransformationContext) -> Result<(), TransformError> {
        context.enable_substitution(SyntaxKind::Identifier)?;
        context.enable_emit_notification(SyntaxKind::Identifier)
    }

    fn substitute_node(
        &mut self,
        context: &mut TransformationContext,
        _: EmitHint,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if context
            .arena()
            .node(node)?
            .as_identifier()
            .is_some_and(|data| data.text() == "seed")
        {
            let arena = context.arena_mut()?;
            arena.set_generated_identifier_text(node, "current")?;
            arena
                .factory()
                .set_node_flags(node, NodeFlags::SYNTHESIZED)?;
        }
        Ok(node)
    }

    fn before_emit_node(
        &mut self,
        context: &TransformationContext,
        _: EmitHint,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        if let Some(data) = context.arena().node(node)?.as_identifier() {
            self.events
                .borrow_mut()
                .push(("before", data.text().to_owned()));
        }
        Ok(())
    }

    fn after_emit_node(
        &mut self,
        context: &TransformationContext,
        _: EmitHint,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        if let Some(data) = context.arena().node(node)?.as_identifier() {
            self.events
                .borrow_mut()
                .push(("after", data.text().to_owned()));
        }
        Ok(())
    }
}

#[test]
fn completed_hook_facts_do_not_survive_into_a_different_mutating_print() {
    let mut printer = canonical_printer();
    let (mut first, source) = plain_result("const old = seed;");
    assert_eq!(
        printer
            .print(&mut first, PrintRequest::SourceFile(source), None)
            .unwrap()
            .text(),
        "const old = seed;\n"
    );

    let file = parse_source_file("active.ts", "const old = seed;", Default::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&file, None);
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut active = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![Box::new(MutatingHooks {
            events: Rc::clone(&events),
        })],
        false,
    )
    .unwrap();
    assert_eq!(
        printer
            .print(&mut active, PrintRequest::SourceFile(source), None)
            .unwrap()
            .text(),
        "const old = current;\n"
    );
    assert_eq!(
        &*events.borrow(),
        &[
            ("before", "old".to_owned()),
            ("after", "old".to_owned()),
            ("before", "current".to_owned()),
            ("after", "current".to_owned())
        ]
    );
    let (mut last, source) = plain_result("const last = 1;");
    assert_eq!(
        printer
            .print(&mut last, PrintRequest::SourceFile(source), None)
            .unwrap()
            .text(),
        "const last = 1;\n"
    );
}

#[test]
fn readonly_grammar_walks_keep_missing_and_unknown_child_boundaries() {
    use tsc_syntax::nodes::{PartiallyEmittedExpressionData, PostfixUnaryExpressionData};
    let mut file = parse_source_file("walk.ts", "seed;", Default::default(), None);
    let unknown = NodeId::new(u32::MAX - 1);
    let partial_missing = file.arena.alloc_node(
        NodeData::PartiallyEmittedExpression(PartiallyEmittedExpressionData { expression: None }),
        0,
        0,
        NodeFlags::SYNTHESIZED,
    );
    let partial_unknown = file.arena.alloc_node(
        NodeData::PartiallyEmittedExpression(PartiallyEmittedExpressionData {
            expression: Some(unknown),
        }),
        0,
        0,
        NodeFlags::SYNTHESIZED,
    );
    let postfix_missing = file.arena.alloc_node(
        NodeData::PostfixUnaryExpression(PostfixUnaryExpressionData {
            operand: None,
            operator: SyntaxKind::PlusPlusToken,
        }),
        0,
        0,
        NodeFlags::SYNTHESIZED,
    );
    let postfix_unknown = file.arena.alloc_node(
        NodeData::PostfixUnaryExpression(PostfixUnaryExpressionData {
            operand: Some(unknown),
            operator: SyntaxKind::PlusPlusToken,
        }),
        0,
        0,
        NodeFlags::SYNTHESIZED,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&file, None);
    let result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![],
        false,
    )
    .unwrap();
    let printer = canonical_printer();
    for id in [partial_missing, partial_unknown] {
        let node = TransformNode::new(source, id);
        let read = ResolvedNode::resolve(result.arena(), node).unwrap();
        assert_eq!(
            printer
                .skip_partially_emitted_expressions(&result, read)
                .unwrap()
                .handle(),
            node
        );
    }
    let node = TransformNode::new(source, postfix_missing);
    assert_eq!(
        printer
            .leftmost_expression(
                &result,
                ResolvedNode::resolve(result.arena(), node).unwrap(),
                false
            )
            .unwrap()
            .handle(),
        node
    );
    let node = TransformNode::new(source, postfix_unknown);
    let error = printer
        .leftmost_expression(
            &result,
            ResolvedNode::resolve(result.arena(), node).unwrap(),
            false,
        )
        .err()
        .unwrap();
    assert!(matches!(error, PrinterError::UnknownStatement(index) if index == unknown.index()));
}

#[test]
fn known_scope_kinds_still_read_reuse_metadata_at_each_query() {
    let (mut result, source) = plain_result("function named() {}");
    let function = result
        .arena()
        .source(source)
        .unwrap()
        .syntax()
        .arena
        .node_ids()
        .find(|&id| {
            result
                .arena()
                .node(TransformNode::new(source, id))
                .unwrap()
                .kind
                == SyntaxKind::FunctionDeclaration
        })
        .map(|id| TransformNode::new(source, id))
        .unwrap();
    let kind = result.arena().node(function).unwrap().kind;
    let mut printer = canonical_printer();
    printer.start_print(&result);
    assert!(printer.push_name_generation_scope(result.arena(), function, kind));
    printer.pop_name_generation_scope();
    result
        .arena_mut()
        .unwrap()
        .metadata_mut(function)
        .add_flags(EmitFlags::REUSE_TEMP_VARIABLE_SCOPE);
    assert!(!printer.push_name_generation_scope(result.arena(), function, kind));
}

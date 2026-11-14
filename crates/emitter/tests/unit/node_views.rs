use super::*;
use crate::{transform_nodes, TransformArena, TransformRoot, TransformationContext, Transformer};
use std::{cell::RefCell, rc::Rc};
use tsc_syntax::parse_source_file;

struct RenameSameNode {
    target: TransformNode,
    before: Rc<RefCell<Vec<String>>>,
}
impl Transformer for RenameSameNode {
    fn name(&self) -> &'static str {
        "renameSameNode"
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
        if node == self.target {
            let arena = context.arena_mut()?;
            arena.set_generated_identifier_text(node, "renamed")?;
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
            self.before.borrow_mut().push(data.text().to_owned());
        }
        Ok(())
    }
}

struct ReplaceRenamed;
impl Transformer for ReplaceRenamed {
    fn name(&self) -> &'static str {
        "replaceRenamed"
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
            .is_some_and(|data| data.text() == "renamed")
        {
            return context
                .substitution_factory()?
                .create_identifier(node.source(), "final");
        }
        Ok(node)
    }
}

fn fixture() -> (TransformArena, TransformSourceId, TransformNode) {
    let file = parse_source_file("record.ts", "const old = seed;", Default::default(), None);
    let target = file
        .arena
        .node_ids()
        .find(|&id| {
            file.arena
                .node(id)
                .as_identifier()
                .is_some_and(|data| data.text() == "seed")
        })
        .unwrap();
    let prefix = parse_source_file("prefix.ts", "prefix;", Default::default(), None);
    let mut arena = TransformArena::new();
    arena.add_source(&prefix, None);
    let source = arena.add_source(&file, None);
    (arena, source, TransformNode::new(source, target))
}

fn canonical_printer() -> Printer {
    create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
}

#[test]
fn identity_returning_substitution_refreshes_the_record_before_notifications_and_text() {
    let (arena, source, target) = fixture();
    let before = Rc::new(RefCell::new(Vec::new()));
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![Box::new(RenameSameNode {
            target,
            before: Rc::clone(&before),
        })],
        false,
    )
    .unwrap();
    let printed = canonical_printer()
        .print(&mut result, PrintRequest::SourceFile(source), None)
        .unwrap();
    assert_eq!(printed.text(), "const old = renamed;\n");
    assert_eq!(&*before.borrow(), &["old", "renamed"]);
}

#[test]
fn the_last_callback_record_and_original_notification_identity_are_both_current() {
    let (arena, source, target) = fixture();
    let before = Rc::new(RefCell::new(Vec::new()));
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            Box::new(RenameSameNode {
                target,
                before: Rc::clone(&before),
            }),
            Box::new(ReplaceRenamed),
        ],
        false,
    )
    .unwrap();
    let printed = canonical_printer()
        .print(&mut result, PrintRequest::SourceFile(source), None)
        .unwrap();
    assert_eq!(printed.text(), "const old = final;\n");
    assert_eq!(&*before.borrow(), &["old", "renamed"]);
}

struct ReplaceArena {
    target: TransformNode,
}
impl Transformer for ReplaceArena {
    fn name(&self) -> &'static str {
        "replaceArena"
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
        if node != self.target {
            return Ok(node);
        }
        let file = parse_source_file("replacement.ts", "replacement;", Default::default(), None);
        let id = file
            .arena
            .node_ids()
            .find(|&id| file.arena.node(id).kind == SyntaxKind::Identifier)
            .unwrap();
        let mut replacement = TransformArena::new();
        let source = replacement.add_source(&file, None);
        *context.arena_mut()? = replacement;
        Ok(TransformNode::new(source, id))
    }
}

#[test]
fn a_changed_identity_still_validates_the_original_notification_and_keeps_its_prefix() {
    let (arena, source, target) = fixture();
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![Box::new(ReplaceArena { target })],
        false,
    )
    .unwrap();
    let mut printer = canonical_printer();
    let error = printer
        .print(&mut result, PrintRequest::SourceFile(source), None)
        .unwrap_err();
    assert!(
        matches!(error, PrinterError::Transform(TransformError::UnknownSource(missing)) if missing == source)
    );
    assert_eq!(printer.own_writer.as_ref().unwrap().text(), "const old = ");
}

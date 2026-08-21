//! H2.5h-b B-1 hook-chaining order contracts (packet §7.5).
//!
//! The frozen `substitution-chain` composition edge pins: both owners
//! save `previousOnSubstituteNode` and delegate (previous runs first),
//! and ES2015 additionally chains `previousOnEmitNode` while Generators
//! registers substitution only. With the frozen registration order
//! `[transformES2015, transformGenerators]`, previous-first delegation
//! is exactly this pipeline's forward substitution walk, and the
//! notification wrap is the forward-before / reverse-after pair with
//! the first-registered transformer outermost. These contracts pin that
//! order and the per-kind enablement split so the B-5 owners inherit a
//! proven chain.

use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use tsc_syntax::SyntaxKind;

struct HookStub {
    label: &'static str,
    log: Rc<RefCell<Vec<String>>>,
    enable_notification: bool,
}

impl Transformer for HookStub {
    fn name(&self) -> &'static str {
        self.label
    }

    fn initialize(&mut self, context: &mut TransformationContext) -> Result<(), TransformError> {
        context.enable_substitution(SyntaxKind::Identifier)?;
        if self.enable_notification {
            context.enable_emit_notification(SyntaxKind::Identifier)?;
        }
        Ok(())
    }

    fn substitute_node(
        &mut self,
        _context: &mut TransformationContext,
        _hint: EmitHint,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.log
            .borrow_mut()
            .push(format!("{}:substitute", self.label));
        Ok(node)
    }

    fn before_emit_node(
        &mut self,
        _context: &TransformationContext,
        _hint: EmitHint,
        _node: TransformNode,
    ) -> Result<(), TransformError> {
        self.log.borrow_mut().push(format!("{}:before", self.label));
        Ok(())
    }

    fn after_emit_node(
        &mut self,
        _context: &TransformationContext,
        _hint: EmitHint,
        _node: TransformNode,
    ) -> Result<(), TransformError> {
        self.log.borrow_mut().push(format!("{}:after", self.label));
        Ok(())
    }
}

fn chained_session(
    log: &Rc<RefCell<Vec<String>>>,
) -> (TransformationResult<'static>, TransformNode, TransformNode) {
    let parsed = tsc_syntax::parse_source_file(
        "hooks.ts".to_owned(),
        "value;\n\"text\";\n".to_owned(),
        Default::default(),
        None,
    );
    let identifier = parsed
        .arena
        .node_ids()
        .find(|id| parsed.arena.node(*id).kind == SyntaxKind::Identifier)
        .expect("identifier");
    let string_literal = parsed
        .arena
        .node_ids()
        .find(|id| parsed.arena.node(*id).kind == SyntaxKind::StringLiteral)
        .expect("string literal");
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let transformers: Vec<Box<dyn Transformer>> = vec![
        Box::new(HookStub {
            label: "es2015",
            log: Rc::clone(log),
            enable_notification: true,
        }),
        Box::new(HookStub {
            label: "generators",
            log: Rc::clone(log),
            enable_notification: false,
        }),
    ];
    let result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        transformers,
        false,
    )
    .expect("identity transform with hook stubs");
    (
        result,
        TransformNode::new(source, identifier),
        TransformNode::new(source, string_literal),
    )
}

#[test]
fn substitution_delegates_previous_first_in_registration_order() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let (mut session, identifier, _) = chained_session(&log);
    log.borrow_mut().clear();
    session
        .substitute_node(EmitHint::Unspecified, identifier)
        .expect("substitute");
    assert_eq!(
        *log.borrow(),
        vec![
            "es2015:substitute".to_owned(),
            "generators:substitute".to_owned()
        ],
    );
}

#[test]
fn notification_wraps_with_the_first_registered_transformer_outermost() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let (mut session, identifier, _) = chained_session(&log);
    log.borrow_mut().clear();
    session
        .before_emit_node(EmitHint::Unspecified, identifier)
        .expect("before");
    session
        .after_emit_node(EmitHint::Unspecified, identifier)
        .expect("after");
    assert_eq!(
        *log.borrow(),
        vec![
            "es2015:before".to_owned(),
            "generators:before".to_owned(),
            "generators:after".to_owned(),
            "es2015:after".to_owned(),
        ],
    );
}

#[test]
fn substitution_only_registration_never_fires_notification_hooks() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let (mut session, _, string_literal) = chained_session(&log);
    log.borrow_mut().clear();
    // StringLiteral has neither enablement: nothing fires.
    session
        .substitute_node(EmitHint::Unspecified, string_literal)
        .expect("substitute");
    session
        .before_emit_node(EmitHint::Unspecified, string_literal)
        .expect("before");
    session
        .after_emit_node(EmitHint::Unspecified, string_literal)
        .expect("after");
    assert!(log.borrow().is_empty());
}

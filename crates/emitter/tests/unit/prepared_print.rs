use super::target_bindings::TargetBinding;
use crate::{
    create_printer, transform_nodes, EmitHint, NewLineKind, PrintRequest, PrinterOptions,
    TransformArena, TransformError, TransformFlags, TransformNode, TransformRoot,
    TransformationContext, TransformationResult, Transformer,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use tsc_syntax::{parse_source_file, NodeData, SyntaxKind};

struct InertNames {
    disposed: Rc<Cell<bool>>,
}

impl Transformer for InertNames {
    fn name(&self) -> &'static str {
        "inertNamesFixture"
    }
    fn has_inert_print_hooks(&self) -> bool {
        true
    }
    fn transform_root(
        &mut self,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        let TransformRoot::SourceFile(source) = root;
        let name =
            TargetBinding::allocate_numbered(context, "generated".into(), "generated".into())?;
        let generated = context.factory()?.create_node(
            source,
            NodeData::Identifier(tsc_syntax::nodes::IdentifierData {
                escaped_text: tsc_types::EscapedName::from_identifier_escaped_text("generated"),
            }),
            TransformFlags::NONE,
        )?;
        name.write_generated_metadata(context.arena_mut()?, generated);
        let statement = context.factory()?.create_node(
            source,
            NodeData::ExpressionStatement(tsc_syntax::nodes::ExpressionStatementData {
                expression: Some(generated.node()),
            }),
            TransformFlags::NONE,
        )?;
        let statements = context
            .factory()?
            .create_node_array(source, vec![statement])?;
        let root_node = context.arena().root(source)?;
        let NodeData::SourceFile(mut data) = context.arena().node(root_node)?.data() else {
            unreachable!("source root")
        };
        data.statements = Some(statements.array());
        let updated = context.factory()?.update_node(
            root_node,
            NodeData::SourceFile(data),
            TransformFlags::NONE,
        )?;
        context.arena_mut()?.replace_root(source, updated)?;
        Ok(root)
    }
    fn dispose(&mut self) {
        assert!(!self.disposed.replace(true));
    }
}

fn fixture() -> (
    TransformationResult<'static>,
    crate::TransformSourceId,
    Rc<Cell<bool>>,
) {
    let file = parse_source_file("fixture.ts", "const value = 1;", Default::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&file, None);
    let disposed = Rc::new(Cell::new(false));
    let result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![Box::new(InertNames {
            disposed: Rc::clone(&disposed),
        })],
        false,
    )
    .unwrap();
    (result, source, disposed)
}

struct OwnerNames {
    owner: std::thread::ThreadId,
    queries: RefCell<Vec<String>>,
}

impl crate::GlobalNameOracle for OwnerNames {
    fn has_global_name(&self, name: &str) -> Result<bool, crate::EmitResolverError> {
        assert_eq!(std::thread::current().id(), self.owner);
        self.queries.borrow_mut().push(name.to_owned());
        Ok(name == "generated_1")
    }
}

#[test]
fn prepared_names_and_non_send_hooks_stay_on_the_owner() {
    let oracle = OwnerNames {
        owner: std::thread::current().id(),
        queries: RefCell::new(Vec::new()),
    };
    let (mut result, source, disposed) = fixture();
    let mut printer = create_printer(PrinterOptions::new(NewLineKind::LineFeed));
    let prepared = printer
        .prepare_source_file_print(&mut result, source, Some(&oracle))
        .unwrap();
    assert!(!oracle.queries.borrow().is_empty());
    let owned = result
        .take_owned_print()
        .expect("explicit inert-hook contract");
    assert!(
        disposed.get(),
        "the Rc-owned hook was disposed on the owner"
    );
    let options = printer.options();
    let actual = std::thread::spawn(move || {
        let mut result = owned.into_result();
        create_printer(options)
            .print_prepared_source_file(&mut result, prepared, None)
            .unwrap()
    })
    .join()
    .unwrap();
    let (mut inline, source, inline_disposed) = fixture();
    let expected = create_printer(options)
        .print_javascript_with_global_names(
            &mut inline,
            PrintRequest::SourceFile(source),
            None,
            &oracle,
        )
        .unwrap();
    assert_eq!(actual, expected);
    assert!(!inline_disposed.get());
    inline.dispose();
    assert!(inline_disposed.get());
}

struct UnknownHooks(Rc<Cell<usize>>);

impl Transformer for UnknownHooks {
    fn name(&self) -> &'static str {
        "unknownHooks"
    }
    fn is_identity_substitution(&self) -> bool {
        true
    }
    fn initialize(&mut self, context: &mut TransformationContext) -> Result<(), TransformError> {
        context.enable_emit_notification(SyntaxKind::SourceFile)
    }
    fn before_emit_node(
        &mut self,
        _: &TransformationContext,
        _: EmitHint,
        _: TransformNode,
    ) -> Result<(), TransformError> {
        self.0.set(self.0.get() + 1);
        Ok(())
    }
}

#[test]
fn identity_substitution_does_not_discard_unknown_notifications() {
    let file = parse_source_file("fixture.ts", "const value = 1;", Default::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&file, None);
    let notifications = Rc::new(Cell::new(0));
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![Box::new(UnknownHooks(Rc::clone(&notifications)))],
        false,
    )
    .unwrap();
    assert!(result.take_owned_print().is_none());
    create_printer(PrinterOptions::new(NewLineKind::LineFeed))
        .print(&mut result, PrintRequest::SourceFile(source), None)
        .unwrap();
    assert!(notifications.get() > 0);
}

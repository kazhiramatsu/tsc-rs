use std::cell::RefCell;
use std::rc::Rc;

use tsc_emitter::{
    transform_nodes, EmitFlags, EmitHelper, EmitHint, JavaScriptString, TransformArena,
    TransformError, TransformFlags, TransformNode, TransformRoot, TransformationContext,
    TransformationState, Transformer,
};
use tsc_syntax::{parse_source_file, NodeData, SyntaxKind};

#[derive(Debug, Default)]
struct ProbeState {
    initialized: usize,
    transformed: usize,
    notified_before: usize,
    notified_after: usize,
    disposed: usize,
    clone: Option<TransformNode>,
}

struct ProbeTransformer {
    state: Rc<RefCell<ProbeState>>,
}

impl Transformer for ProbeTransformer {
    fn name(&self) -> &'static str {
        "probe"
    }

    fn initialize(&mut self, context: &mut TransformationContext) -> Result<(), TransformError> {
        assert_eq!(context.state(), TransformationState::Uninitialized);
        context.enable_emit_notification(SyntaxKind::VariableStatement)?;
        context.enable_substitution(SyntaxKind::VariableStatement)?;
        self.state.borrow_mut().initialized += 1;
        Ok(())
    }

    fn transform_root(
        &mut self,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        assert_eq!(context.state(), TransformationState::Initialized);
        let TransformRoot::SourceFile(source) = root else {
            unreachable!()
        };
        let root_node = context.arena().root(source)?;
        let statements = match &context.arena().node(root_node)?.data {
            NodeData::SourceFile(data) => data
                .statements
                .map(|array| context.arena().node_array_ref(source, array).unwrap())
                .unwrap(),
            _ => unreachable!(),
        };
        let first = context.arena().node_array(statements)?.nodes[0];
        let first = context.arena().node_ref(source, first).unwrap();

        context.start_lexical_environment()?;
        {
            let mut factory = context.factory()?;
            assert_eq!(
                factory.create_node(source, NodeData::Token, TransformFlags::NONE),
                Err(TransformError::FactoryTokenDataRequiresTokenConstructor)
            );
            assert_eq!(
                factory.create_token(source, SyntaxKind::SourceFile, TransformFlags::NONE),
                Err(TransformError::FactoryTokenKindExpected(
                    SyntaxKind::SourceFile
                ))
            );
        }
        let clone = context.factory()?.clone_node(first)?;
        context
            .arena_mut()?
            .metadata_mut(first)
            .set_flags(EmitFlags::NO_TRAILING_COMMENTS);
        context
            .arena_mut()?
            .metadata_mut(first)
            .set_javascript_string_value(JavaScriptString::from_code_units(vec![
                0xd800, 0x0061, 0xdc00,
            ]));
        let second_clone = context.factory()?.clone_node(first)?;
        assert_eq!(
            context.arena().metadata(second_clone).unwrap().flags(),
            EmitFlags::NO_TRAILING_COMMENTS
        );
        assert_eq!(context.arena().get_original_node(second_clone), first);
        assert_eq!(
            context
                .arena()
                .metadata(second_clone)
                .unwrap()
                .javascript_string_value()
                .unwrap()
                .code_units(),
            [0xd800, 0x0061, 0xdc00]
        );
        context.hoist_function_declaration(clone)?;
        let environment = context.end_lexical_environment()?;
        assert_eq!(environment.function_declarations(), [clone]);

        context.start_block_scope()?;
        context.add_block_scoped_variable(clone)?;
        assert_eq!(context.end_block_scope()?, [clone]);
        context.request_emit_helper(EmitHelper::new(
            "outer",
            false,
            vec![EmitHelper::new("dependency", false, Vec::new())],
        ))?;
        let helpers = context.read_emit_helpers()?;
        assert_eq!(
            helpers.iter().map(EmitHelper::name).collect::<Vec<_>>(),
            ["dependency", "outer"]
        );
        self.state.borrow_mut().clone = Some(clone);
        self.state.borrow_mut().transformed += 1;
        Ok(TransformRoot::SourceFile(source))
    }

    fn substitute_node(
        &mut self,
        _context: &TransformationContext,
        _hint: EmitHint,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        Ok(node)
    }

    fn before_emit_node(
        &mut self,
        _context: &TransformationContext,
        _hint: EmitHint,
        _node: TransformNode,
    ) -> Result<(), TransformError> {
        self.state.borrow_mut().notified_before += 1;
        Ok(())
    }

    fn after_emit_node(
        &mut self,
        _context: &TransformationContext,
        _hint: EmitHint,
        _node: TransformNode,
    ) -> Result<(), TransformError> {
        self.state.borrow_mut().notified_after += 1;
        Ok(())
    }

    fn dispose(&mut self) {
        self.state.borrow_mut().disposed += 1;
    }
}

#[test]
fn factory_and_transform_lifecycle_are_session_owned_and_disposed() {
    let parsed = parse_source_file(
        "input.ts",
        "const value: number = 1;\n",
        Default::default(),
        None,
    );
    let original_count = parsed.node_count();
    let original_root = parsed.root;
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let state = Rc::new(RefCell::new(ProbeState::default()));
    let transformer = ProbeTransformer {
        state: Rc::clone(&state),
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![Box::new(transformer)],
        false,
    )
    .expect("identity transform");

    assert_eq!(result.state(), TransformationState::Completed);
    assert_eq!(state.borrow().initialized, 1);
    assert_eq!(state.borrow().transformed, 1);
    assert!(result.arena().source(source).unwrap().syntax().node_count() > original_count);
    assert_eq!(parsed.node_count(), original_count);
    assert_eq!(parsed.root, original_root);

    let root = result.arena().root(source).unwrap();
    let statements = match &result.arena().node(root).unwrap().data {
        NodeData::SourceFile(data) => data.statements.unwrap(),
        _ => unreachable!(),
    };
    let first = result
        .arena()
        .node_ref(
            source,
            result
                .arena()
                .source(source)
                .unwrap()
                .syntax()
                .arena
                .node_array(statements)
                .nodes[0],
        )
        .unwrap();
    assert_eq!(
        result
            .substitute_node(EmitHint::Unspecified, first)
            .unwrap(),
        first
    );
    result
        .before_emit_node(EmitHint::Unspecified, first)
        .unwrap();
    result
        .after_emit_node(EmitHint::Unspecified, first)
        .unwrap();
    assert_eq!(state.borrow().notified_before, 1);
    assert_eq!(state.borrow().notified_after, 1);

    let clone = state.borrow().clone.unwrap();
    assert!(
        result.arena().node(clone).unwrap().flags & tsc_types::NodeFlags::SYNTHESIZED.bits() != 0
    );
    assert_eq!(result.arena().get_original_node(clone), first);
    assert_eq!(result.arena().node(clone).unwrap().pos, u32::MAX);
    assert_eq!(result.arena().node(clone).unwrap().end, u32::MAX);
    assert_eq!(
        TransformFlags::subtree_exclusions(SyntaxKind::FunctionDeclaration),
        TransformFlags::FUNCTION_EXCLUDES
    );

    result.dispose();
    assert_eq!(result.state(), TransformationState::Disposed);
    assert_eq!(state.borrow().disposed, 1);
    assert!(result.arena().metadata(clone).is_none());
}

struct FailingTransformer {
    disposed: Rc<RefCell<usize>>,
}

impl Transformer for FailingTransformer {
    fn name(&self) -> &'static str {
        "failing-probe"
    }

    fn transform_root(
        &mut self,
        _context: &mut TransformationContext,
        _root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        Err(TransformError::BlockScopeRequired)
    }

    fn dispose(&mut self) {
        *self.disposed.borrow_mut() += 1;
    }
}

#[test]
fn failed_transformation_disposes_initialized_transformers() {
    let parsed = parse_source_file("failure.ts", "const value = 1;\n", Default::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let disposed = Rc::new(RefCell::new(0));
    let error = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![Box::new(FailingTransformer {
            disposed: Rc::clone(&disposed),
        })],
        false,
    )
    .err()
    .expect("probe transformer fails");
    assert_eq!(error, TransformError::BlockScopeRequired);
    assert_eq!(*disposed.borrow(), 1);
}

#[test]
fn property_name_flags_survive_named_declaration_subtree_exclusions() {
    let parsed = parse_source_file(
        "computed-name.ts",
        "class Example { [this.key]() {} }\n",
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let root = arena.root(source).unwrap();
    let statements = match &arena.node(root).unwrap().data {
        NodeData::SourceFile(data) => data.statements.unwrap(),
        _ => unreachable!(),
    };
    let class = arena
        .node_array(arena.node_array_ref(source, statements).unwrap())
        .unwrap()
        .nodes[0];
    let class = arena.node_ref(source, class).unwrap();
    let members = match &arena.node(class).unwrap().data {
        NodeData::ClassDeclaration(data) => data.members.unwrap(),
        _ => unreachable!(),
    };
    let method = arena
        .node_array(arena.node_array_ref(source, members).unwrap())
        .unwrap()
        .nodes[0];
    let method = arena.node_ref(source, method).unwrap();
    let name = match &arena.node(method).unwrap().data {
        NodeData::MethodDeclaration(data) => data.name.unwrap(),
        _ => unreachable!(),
    };
    let name = arena.node_ref(source, name).unwrap();

    arena.set_transform_flags(method, TransformFlags::HAS_COMPUTED_FLAGS);
    arena.set_transform_flags(name, TransformFlags::CONTAINS_LEXICAL_THIS);

    assert_eq!(
        arena.propagate_child_flags(method).unwrap(),
        TransformFlags::CONTAINS_LEXICAL_THIS
    );
    assert_eq!(
        TransformFlags::CONTAINS_LEXICAL_THIS_OR_SUPER,
        TransformFlags::PROPERTY_NAME_PROPAGATING_FLAGS
    );
}

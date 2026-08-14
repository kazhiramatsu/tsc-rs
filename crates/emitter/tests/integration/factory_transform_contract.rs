use std::cell::RefCell;
use std::rc::Rc;

use tsc_diagnostics::{DocumentVersion, TextSnapshot};
use tsc_emitter::{
    create_printer, transform_nodes, CommentRange, DisabledSourceMapRecorder, EmitFlags,
    EmitHelper, EmitHint, EmitResolverNode, JavaScriptString, NewLineKind, PrintRequest,
    PrinterError, PrinterOptions, SourceByteRange, SourceFileId, SourceFileTextMode, SourceRange,
    SyntheticComment, SyntheticCommentKind, TransformArena, TransformError, TransformFlags,
    TransformNode, TransformRoot, TransformSourceId, TransformationContext, TransformationState,
    Transformer,
};
use tsc_syntax::{
    for_each_child,
    nodes::{
        ArrowFunctionData, ExportAssignmentData, ExpressionStatementData, IdentifierData,
        PartiallyEmittedExpressionData, ReturnStatementData, SourceFileData, ThrowStatementData,
    },
    parse_source_file, parse_source_file_from_snapshot_in_identity_domain, NodeData, ParseOptions,
    SyntaxKind,
};
use tsc_types::IdentityDomain;

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
        _context: &mut TransformationContext,
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

fn print_canonical_source(text: &str) -> String {
    let parsed = parse_source_file("export-assignment.ts", text, Default::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        Vec::new(),
        false,
    )
    .expect("identity transform");
    create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("canonical export-assignment print")
    .text()
    .to_owned()
}

#[test]
fn parsed_export_assignment_tokens_own_their_internal_comments() {
    let source = concat!(
        "export /*after export*/ = /*before value*/ M /*before semicolon*/;\n",
        "export /*after export*/ default /*before value*/ D /*before semicolon*/;\n",
    );

    assert_eq!(print_canonical_source(source), source);
}

#[test]
fn parsed_export_assignments_emit_zero_width_recovery_expressions() {
    assert_eq!(
        print_canonical_source(concat!(
            "export = /*missing equals*/;\n",
            "export default /*missing default*/;\n",
        )),
        concat!(
            "export = /*missing equals*/ ;\n",
            "export default /*missing default*/ ;\n",
        ),
    );
}

#[test]
fn parsed_throw_line_break_recovery_preserves_statement_comment_ownership() {
    assert_eq!(print_canonical_source("throw\na;"), "throw ;\na;\n");
    assert_eq!(
        print_canonical_source("throw /*after throw*/\n/*before next*/ a;"),
        "throw ; /*after throw*/\n/*before next*/ a;\n",
    );
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SyntheticArrowTokenCommentPlan {
    SemanticOriginalOnly,
    ExplicitSourceAndSynthetic,
}

struct SyntheticArrowTokenOriginalTransformer {
    comments: SyntheticArrowTokenCommentPlan,
}

impl Transformer for SyntheticArrowTokenOriginalTransformer {
    fn name(&self) -> &'static str {
        "synthetic-arrow-token-original"
    }

    fn transform_root(
        &mut self,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        let source = match root {
            TransformRoot::SourceFile(source) => source,
            other => return Ok(other),
        };
        let root_node = context.arena().root(source)?;
        let (end_of_file_token, original_arrow_token, explicit_comment_range) = {
            let syntax = context.arena().source(source)?.syntax();
            let end_of_file_token = match &syntax.arena.node(syntax.root).data {
                NodeData::SourceFile(data) => data.end_of_file_token,
                _ => unreachable!("transform root is a source file"),
            };
            let mut pending = vec![syntax.root];
            let mut original_arrow_token = None;
            while let Some(node) = pending.pop() {
                let record = syntax.arena.node(node);
                if record.kind == SyntaxKind::EqualsGreaterThanToken {
                    original_arrow_token = Some(node);
                    break;
                }
                for_each_child(&syntax.arena, record, |child| {
                    pending.push(child);
                    false
                });
            }
            let original_arrow_token =
                original_arrow_token.expect("fixture contains an arrow token");
            let arrow_record = syntax.arena.node(original_arrow_token);
            let explicit_start = match self.comments {
                SyntheticArrowTokenCommentPlan::SemanticOriginalOnly => arrow_record.pos,
                SyntheticArrowTokenCommentPlan::ExplicitSourceAndSynthetic => {
                    let marker = syntax
                        .text()
                        .find("/* source leading */")
                        .expect("explicit comment fixture contains a source-leading marker");
                    let start = marker
                        .checked_sub(1)
                        .filter(|start| syntax.text().as_bytes().get(*start) == Some(&b'\n'))
                        .expect("source-leading marker follows the owned line boundary");
                    u32::try_from(start).expect("fixture source position fits u32")
                }
            };
            let explicit_comment_range =
                SourceByteRange::new(explicit_start, arrow_record.end, syntax.positions())
                    .expect("parsed arrow comment range");
            (
                end_of_file_token,
                original_arrow_token,
                explicit_comment_range,
            )
        };
        let original_arrow_token = context
            .arena()
            .node_ref(source, original_arrow_token)
            .expect("parsed arrow token belongs to the transform source");

        let arrow_token = context.factory()?.create_token(
            source,
            SyntaxKind::EqualsGreaterThanToken,
            TransformFlags::NONE,
        )?;
        context
            .arena_mut()?
            .set_original_node(arrow_token, Some(original_arrow_token))?;
        if self.comments == SyntheticArrowTokenCommentPlan::ExplicitSourceAndSynthetic {
            let metadata = context.arena_mut()?.metadata_mut(arrow_token);
            metadata.set_comment_range(CommentRange::new(
                source,
                SourceRange::Original(explicit_comment_range),
            ));
            metadata.add_leading_comment(SyntheticComment::new(
                SyntheticCommentKind::MultiLine,
                " synthetic leading ",
                false,
                false,
            ));
            metadata.add_trailing_comment(SyntheticComment::new(
                SyntheticCommentKind::MultiLine,
                " synthetic trailing ",
                false,
                false,
            ));
        }
        let parameters = context.factory()?.create_node_array(source, Vec::new())?;
        let body = context.factory()?.create_node(
            source,
            NodeData::Identifier(IdentifierData {
                escaped_text: "value".to_owned(),
                text: "value".to_owned(),
            }),
            TransformFlags::NONE,
        )?;
        let arrow = context.factory()?.create_node(
            source,
            NodeData::ArrowFunction(ArrowFunctionData {
                type_parameters: None,
                parameters: Some(parameters.array()),
                r#type: None,
                body: Some(body.node()),
                modifiers: None,
                equals_greater_than_token: Some(arrow_token.node()),
            }),
            TransformFlags::NONE,
        )?;
        let statement = context.factory()?.create_node(
            source,
            NodeData::ExpressionStatement(ExpressionStatementData {
                expression: Some(arrow.node()),
            }),
            TransformFlags::NONE,
        )?;
        let statements = context
            .factory()?
            .create_node_array(source, vec![statement])?;
        let flags = context.arena().transform_flags(root_node);
        let updated_root = context.factory()?.update_node(
            root_node,
            NodeData::SourceFile(SourceFileData {
                statements: Some(statements.array()),
                end_of_file_token,
            }),
            flags,
        )?;
        context.arena_mut()?.replace_root(source, updated_root)?;
        Ok(TransformRoot::SourceFile(source))
    }
}

fn print_with_synthetic_arrow_token(
    source_text: &str,
    comments: SyntheticArrowTokenCommentPlan,
    remove_comments: bool,
) -> String {
    let parsed = parse_source_file("arrow.ts", source_text, Default::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![Box::new(SyntheticArrowTokenOriginalTransformer {
            comments,
        })],
        false,
    )
    .expect("synthetic arrow transform");
    create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_remove_comments(remove_comments)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("synthetic arrow print")
    .text()
    .to_owned()
}

#[test]
fn synthetic_arrow_token_original_does_not_borrow_source_comments() {
    let text = print_with_synthetic_arrow_token(
        "const ignored = () => /* borrowed */ value;\n",
        SyntheticArrowTokenCommentPlan::SemanticOriginalOnly,
        false,
    );

    assert_eq!(text, "() => value;\n");
    assert!(!text.contains("borrowed"), "{text}");
}

#[test]
fn explicit_arrow_comment_range_orders_source_and_synthetic_comments() {
    let source_text = concat!(
        "const ignored = ()\n/* source leading */ => ",
        "/* source trailing */ value;\n",
    );
    let text = print_with_synthetic_arrow_token(
        source_text,
        SyntheticArrowTokenCommentPlan::ExplicitSourceAndSynthetic,
        false,
    );

    let source_leading = text.find("source leading").expect("source leading comment");
    let synthetic_leading = text
        .find("synthetic leading")
        .expect("synthetic leading comment");
    let arrow = text.find("=>").expect("arrow token");
    let synthetic_trailing = text
        .find("synthetic trailing")
        .expect("synthetic trailing comment");
    let source_trailing = text
        .find("source trailing")
        .expect("source trailing comment");
    let body = text.rfind("value").expect("generated arrow body");

    assert!(
        source_leading < synthetic_leading
            && synthetic_leading < arrow
            && arrow < synthetic_trailing
            && synthetic_trailing < source_trailing
            && source_trailing < body,
        "{text}"
    );
    for marker in [
        "source leading",
        "synthetic leading",
        "synthetic trailing",
        "source trailing",
    ] {
        assert_eq!(text.matches(marker).count(), 1, "{text}");
    }
    assert_eq!(
        print_with_synthetic_arrow_token(
            source_text,
            SyntheticArrowTokenCommentPlan::ExplicitSourceAndSynthetic,
            true,
        ),
        "() => value;\n",
    );
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NoAsiPartialOriginal {
    ParsedParentheses,
    WholeExpression,
}

struct NoAsiPartialExpressionTransformer {
    original: NoAsiPartialOriginal,
}

impl Transformer for NoAsiPartialExpressionTransformer {
    fn name(&self) -> &'static str {
        "no-asi-partial-expression"
    }

    fn transform_root(
        &mut self,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        let source = match root {
            TransformRoot::SourceFile(source) => source,
            other => return Ok(other),
        };
        let root_node = context.arena().root(source)?;
        let (statements, end_of_file_token) = match &context.arena().node(root_node)?.data {
            NodeData::SourceFile(data) => (
                data.statements
                    .and_then(|statements| context.arena().node_array_ref(source, statements))
                    .expect("fixture source file has a statement list"),
                data.end_of_file_token,
            ),
            _ => unreachable!("transform root is a source file"),
        };
        let statement = context
            .arena()
            .node_array(statements)?
            .nodes
            .first()
            .and_then(|statement| context.arena().node_ref(source, *statement))
            .expect("fixture starts with a return statement");
        let expression = match &context.arena().node(statement)?.data {
            NodeData::ReturnStatement(data) => data
                .expression
                .and_then(|expression| context.arena().node_ref(source, expression))
                .expect("fixture return statement has an expression"),
            _ => panic!("fixture starts with a return statement"),
        };
        let inner = match self.original {
            NoAsiPartialOriginal::ParsedParentheses => {
                match &context.arena().node(expression)?.data {
                    NodeData::ParenthesizedExpression(data) => data
                        .expression
                        .and_then(|inner| context.arena().node_ref(source, inner))
                        .expect("fixture parentheses have an expression"),
                    _ => panic!("fixture return expression is parenthesized"),
                }
            }
            NoAsiPartialOriginal::WholeExpression => expression,
        };

        let wrapper_flags =
            context.arena().propagate_child_flags(inner)? | TransformFlags::CONTAINS_TYPE_SCRIPT;
        let wrapper = context.factory()?.create_node(
            source,
            NodeData::PartiallyEmittedExpression(PartiallyEmittedExpressionData {
                expression: Some(inner.node()),
            }),
            wrapper_flags,
        )?;
        context.factory()?.set_text_range(wrapper, expression)?;
        context
            .arena_mut()?
            .set_original_node(wrapper, Some(expression))?;
        {
            let metadata = context.arena_mut()?.metadata_mut(wrapper);
            metadata.add_leading_comment(SyntheticComment::new(
                SyntheticCommentKind::MultiLine,
                "wrapper-leading",
                false,
                true,
            ));
            metadata.add_trailing_comment(SyntheticComment::new(
                SyntheticCommentKind::MultiLine,
                "wrapper-trailing",
                false,
                false,
            ));
        }

        let statement_flags = context.arena().transform_flags(statement);
        let updated_statement = context.factory()?.update_node(
            statement,
            NodeData::ReturnStatement(ReturnStatementData {
                expression: Some(wrapper.node()),
            }),
            statement_flags,
        )?;
        let statements = context
            .factory()?
            .update_node_array(statements, vec![updated_statement])?;
        let root_flags = context.arena().transform_flags(root_node);
        let updated_root = context.factory()?.update_node(
            root_node,
            NodeData::SourceFile(SourceFileData {
                statements: Some(statements.array()),
                end_of_file_token,
            }),
            root_flags,
        )?;
        context.arena_mut()?.replace_root(source, updated_root)?;
        Ok(TransformRoot::SourceFile(source))
    }
}

fn print_no_asi_partial_expression(
    source_text: &str,
    original: NoAsiPartialOriginal,
    remove_comments: bool,
) -> String {
    let parsed = parse_source_file("no-asi-partial.ts", source_text, Default::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![Box::new(NoAsiPartialExpressionTransformer { original })],
        false,
    )
    .expect("no-ASI partial-expression transform");
    create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_remove_comments(remove_comments)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("no-ASI partial-expression print")
    .text()
    .to_owned()
}

#[test]
fn no_asi_parsed_parentheses_place_wrapper_comments_outside_source_delimiters() {
    assert_eq!(
        print_no_asi_partial_expression(
            "return (/*open*/ value /*close*/);\n",
            NoAsiPartialOriginal::ParsedParentheses,
            false,
        ),
        concat!(
            "return /*wrapper-leading*/\n",
            "( /*open*/value /*close*/) /*wrapper-trailing*/;\n",
        ),
    );
}

#[test]
fn no_asi_whole_partial_expression_keeps_wrapper_comments_inside_synthetic_delimiters() {
    assert_eq!(
        print_no_asi_partial_expression(
            "return value;\n",
            NoAsiPartialOriginal::WholeExpression,
            false,
        ),
        concat!(
            "return (/*wrapper-leading*/\n",
            "value /*wrapper-trailing*/);\n",
        ),
    );
}

#[test]
fn no_asi_partial_parenthesization_is_disabled_when_comments_are_removed() {
    for (source, original) in [
        (
            "return (/*open*/ value /*close*/);\n",
            NoAsiPartialOriginal::ParsedParentheses,
        ),
        ("return value;\n", NoAsiPartialOriginal::WholeExpression),
    ] {
        assert_eq!(
            print_no_asi_partial_expression(source, original, true),
            "return value;\n",
        );
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ArrowTokenHookMode {
    Both,
    SubstitutionOnly,
    NotificationOnly,
    SuppressedSubstitution,
    AdvisedNotification,
}

impl ArrowTokenHookMode {
    const fn emit_flags(self) -> Option<EmitFlags> {
        match self {
            Self::SuppressedSubstitution => Some(EmitFlags::NO_SUBSTITUTION),
            Self::AdvisedNotification => Some(EmitFlags::ADVISE_ON_EMIT_NODE),
            Self::Both | Self::SubstitutionOnly | Self::NotificationOnly => None,
        }
    }
}

struct ArrowTokenHookTransformer {
    mode: ArrowTokenHookMode,
}

fn first_parsed_arrow_token(
    context: &TransformationContext,
    source: TransformSourceId,
) -> Result<TransformNode, TransformError> {
    let arrow = {
        let syntax = context.arena().source(source)?.syntax();
        let mut pending = vec![syntax.root];
        let mut arrow = None;
        while let Some(node) = pending.pop() {
            let record = syntax.arena.node(node);
            if record.kind == SyntaxKind::EqualsGreaterThanToken {
                arrow = Some(node);
                break;
            }
            for_each_child(&syntax.arena, record, |child| {
                pending.push(child);
                false
            });
        }
        arrow.expect("fixture contains an arrow token")
    };
    Ok(context
        .arena()
        .node_ref(source, arrow)
        .expect("parsed arrow token belongs to the transform source"))
}

impl Transformer for ArrowTokenHookTransformer {
    fn name(&self) -> &'static str {
        "arrow-token-hook"
    }

    fn initialize(&mut self, context: &mut TransformationContext) -> Result<(), TransformError> {
        if matches!(
            self.mode,
            ArrowTokenHookMode::Both
                | ArrowTokenHookMode::SubstitutionOnly
                | ArrowTokenHookMode::SuppressedSubstitution
        ) {
            context.enable_substitution(SyntaxKind::EqualsGreaterThanToken)?;
        }
        if matches!(
            self.mode,
            ArrowTokenHookMode::Both | ArrowTokenHookMode::NotificationOnly
        ) {
            context.enable_emit_notification(SyntaxKind::EqualsGreaterThanToken)?;
        }
        Ok(())
    }

    fn transform_root(
        &mut self,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        if let (Some(flags), TransformRoot::SourceFile(source)) = (self.mode.emit_flags(), &root) {
            let arrow = first_parsed_arrow_token(context, *source)?;
            context.arena_mut()?.metadata_mut(arrow).add_flags(flags);
        }
        Ok(root)
    }
}

fn print_with_arrow_token_hook(mode: ArrowTokenHookMode) -> Result<String, PrinterError> {
    let parsed = parse_source_file(
        "arrow.ts",
        "const f = a => /* after */ a;\n",
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![Box::new(ArrowTokenHookTransformer { mode })],
        false,
    )
    .expect("arrow hook transform");
    let printed = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )?;
    Ok(printed.text().to_owned())
}

#[test]
fn retained_arrow_comment_adapter_rejects_emit_pipeline_hooks() {
    let error = print_with_arrow_token_hook(ArrowTokenHookMode::Both)
        .expect_err("retained arrow adapter must reject reordered pipeline phases");

    assert!(matches!(
        error,
        PrinterError::RetainedArrowTokenPipelineHooks {
            substitution: true,
            notification: true,
            ..
        }
    ));
}

#[test]
fn retained_arrow_comment_adapter_rejects_substitution_only() {
    let error = print_with_arrow_token_hook(ArrowTokenHookMode::SubstitutionOnly)
        .expect_err("retained arrow adapter must reject substitution");

    assert!(matches!(
        error,
        PrinterError::RetainedArrowTokenPipelineHooks {
            substitution: true,
            notification: false,
            ..
        }
    ));
}

#[test]
fn retained_arrow_comment_adapter_rejects_notification_only() {
    let error = print_with_arrow_token_hook(ArrowTokenHookMode::NotificationOnly)
        .expect_err("retained arrow adapter must reject notification");

    assert!(matches!(
        error,
        PrinterError::RetainedArrowTokenPipelineHooks {
            substitution: false,
            notification: true,
            ..
        }
    ));
}

#[test]
fn retained_arrow_comment_adapter_honors_no_substitution() {
    assert_eq!(
        print_with_arrow_token_hook(ArrowTokenHookMode::SuppressedSubstitution)
            .expect("NoSubstitution disables the effective token hook"),
        "const f = a => /* after */ a;\n",
    );
}

#[test]
fn retained_arrow_comment_adapter_honors_advise_on_emit_node() {
    let error = print_with_arrow_token_hook(ArrowTokenHookMode::AdvisedNotification)
        .expect_err("AdviseOnEmitNode enables the effective token notification");

    assert!(matches!(
        error,
        PrinterError::RetainedArrowTokenPipelineHooks {
            substitution: false,
            notification: true,
            ..
        }
    ));
}

struct SyntheticExportAssignmentTransformer;

impl SyntheticExportAssignmentTransformer {
    fn create_assignment(
        context: &mut TransformationContext,
        source: TransformSourceId,
        is_export_equals: Option<bool>,
        expression_text: Option<&str>,
    ) -> Result<TransformNode, TransformError> {
        let expression = expression_text
            .map(|text| {
                context.factory()?.create_node(
                    source,
                    NodeData::Identifier(IdentifierData {
                        escaped_text: text.to_owned(),
                        text: text.to_owned(),
                    }),
                    TransformFlags::NONE,
                )
            })
            .transpose()?;
        context.factory()?.create_node(
            source,
            NodeData::ExportAssignment(ExportAssignmentData {
                modifiers: None,
                is_export_equals,
                expression: expression.map(TransformNode::node),
            }),
            TransformFlags::NONE,
        )
    }
}

impl Transformer for SyntheticExportAssignmentTransformer {
    fn name(&self) -> &'static str {
        "synthetic-export-assignment"
    }

    fn transform_root(
        &mut self,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        let source = match root {
            TransformRoot::SourceFile(source) => source,
            other => return Ok(other),
        };
        let root_node = context.arena().root(source)?;
        let end_of_file_token = match &context.arena().node(root_node)?.data {
            NodeData::SourceFile(data) => data.end_of_file_token,
            _ => unreachable!("transform root is a source file"),
        };
        let statements = vec![
            Self::create_assignment(context, source, Some(true), Some("equalsValue"))?,
            Self::create_assignment(context, source, None, Some("defaultValue"))?,
            Self::create_assignment(context, source, Some(true), None)?,
            Self::create_assignment(context, source, None, None)?,
        ];
        let statements = context.factory()?.create_node_array(source, statements)?;
        let flags = context.arena().transform_flags(root_node);
        let updated_root = context.factory()?.update_node(
            root_node,
            NodeData::SourceFile(SourceFileData {
                statements: Some(statements.array()),
                end_of_file_token,
            }),
            flags,
        )?;
        context.arena_mut()?.replace_root(source, updated_root)?;
        Ok(TransformRoot::SourceFile(source))
    }
}

#[test]
fn synthetic_export_assignments_cover_both_branches_and_an_absent_child() {
    let parsed = parse_source_file("synthetic.ts", "", Default::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![Box::new(SyntheticExportAssignmentTransformer)],
        false,
    )
    .expect("synthetic export-assignment transform");
    let text = create_printer(PrinterOptions::new(NewLineKind::LineFeed))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("synthetic export-assignment print")
        .text()
        .to_owned();

    assert_eq!(
        text,
        concat!(
            "export = equalsValue;\n",
            "export default defaultValue;\n",
            "export = ;\n",
            "export default ;\n",
        ),
    );
}

struct SyntheticThrowRecoveryTransformer;

impl Transformer for SyntheticThrowRecoveryTransformer {
    fn name(&self) -> &'static str {
        "synthetic-throw-recovery"
    }

    fn transform_root(
        &mut self,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        let source = match root {
            TransformRoot::SourceFile(source) => source,
            other => return Ok(other),
        };
        let root_node = context.arena().root(source)?;
        let end_of_file_token = match &context.arena().node(root_node)?.data {
            NodeData::SourceFile(data) => data.end_of_file_token,
            _ => unreachable!("transform root is a source file"),
        };
        let expression = context.factory()?.create_node(
            source,
            NodeData::Identifier(IdentifierData {
                escaped_text: String::new(),
                text: String::new(),
            }),
            TransformFlags::NONE,
        )?;
        let statement = context.factory()?.create_node(
            source,
            NodeData::ThrowStatement(ThrowStatementData {
                expression: Some(expression.node()),
            }),
            TransformFlags::NONE,
        )?;
        let statements = context
            .factory()?
            .create_node_array(source, vec![statement])?;
        let flags = context.arena().transform_flags(root_node);
        let updated_root = context.factory()?.update_node(
            root_node,
            NodeData::SourceFile(SourceFileData {
                statements: Some(statements.array()),
                end_of_file_token,
            }),
            flags,
        )?;
        context.arena_mut()?.replace_root(source, updated_root)?;
        Ok(TransformRoot::SourceFile(source))
    }
}

#[test]
fn synthetic_throw_recovery_emits_an_empty_expression_slot() {
    let parsed = parse_source_file("synthetic.ts", "", Default::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![Box::new(SyntheticThrowRecoveryTransformer)],
        false,
    )
    .expect("synthetic throw recovery transform");
    let text = create_printer(PrinterOptions::new(NewLineKind::LineFeed))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            &mut DisabledSourceMapRecorder,
        )
        .expect("synthetic throw recovery print")
        .text()
        .to_owned();

    assert_eq!(text, "throw ;\n");
}

#[derive(Clone, Copy, Debug)]
struct ResolverProjectionNodes {
    synthetic: TransformNode,
    ranged_synthetic: TransformNode,
    parsed_clone: TransformNode,
    synthetic_terminal: TransformNode,
}

struct ResolverProjectionTransformer {
    nodes: Rc<RefCell<Option<ResolverProjectionNodes>>>,
}

impl Transformer for ResolverProjectionTransformer {
    fn name(&self) -> &'static str {
        "resolver-projection"
    }

    fn transform_root(
        &mut self,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        let TransformRoot::SourceFile(source) = root else {
            return Ok(root);
        };
        let parsed_root = context.arena().root(source)?;
        let synthetic = context.factory()?.create_node(
            source,
            NodeData::Identifier(IdentifierData {
                escaped_text: "synthetic".to_owned(),
                text: "synthetic".to_owned(),
            }),
            TransformFlags::NONE,
        )?;
        let ranged_synthetic = context.factory()?.create_node(
            source,
            NodeData::Identifier(IdentifierData {
                escaped_text: "ranged".to_owned(),
                text: "ranged".to_owned(),
            }),
            TransformFlags::NONE,
        )?;
        context
            .factory()?
            .set_text_range(ranged_synthetic, parsed_root)?;
        let parsed_clone = context.factory()?.clone_node(parsed_root)?;
        let synthetic_terminal = context.factory()?.create_node(
            source,
            NodeData::Identifier(IdentifierData {
                escaped_text: "terminal".to_owned(),
                text: "terminal".to_owned(),
            }),
            TransformFlags::NONE,
        )?;
        context
            .arena_mut()?
            .set_original_node(parsed_root, Some(synthetic_terminal))?;
        *self.nodes.borrow_mut() = Some(ResolverProjectionNodes {
            synthetic,
            ranged_synthetic,
            parsed_clone,
            synthetic_terminal,
        });
        Ok(root)
    }
}

#[test]
fn resolver_projection_rejects_synthetic_ids_that_alias_the_next_program_source() {
    let identity_domain = IdentityDomain::reclaiming();
    let parsed = parse_source_file_from_snapshot_in_identity_domain(
        "current.ts",
        TextSnapshot::new("export const current = 1;\n", DocumentVersion::new("1")),
        ParseOptions::default(),
        None,
        &identity_domain,
    )
    .expect("current source identity lease");
    let next = parse_source_file_from_snapshot_in_identity_domain(
        "next.ts",
        TextSnapshot::new("export const next = 2;\n", DocumentVersion::new("1")),
        ParseOptions::default(),
        None,
        &identity_domain,
    )
    .expect("next source identity lease");
    assert_eq!(parsed.arena.node_end(), next.arena.node_base());

    let program_source = SourceFileId::from_raw(19);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(program_source));
    let nodes = Rc::new(RefCell::new(None));
    let result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![Box::new(ResolverProjectionTransformer {
            nodes: Rc::clone(&nodes),
        })],
        false,
    )
    .expect("resolver projection transform");
    let nodes = nodes
        .borrow()
        .as_ref()
        .copied()
        .expect("projection nodes were recorded");
    let parsed_root = result
        .arena()
        .node_ref(source, parsed.root)
        .expect("mounted parsed root");

    assert_eq!(nodes.synthetic.node().0, next.arena.node_base());
    assert_eq!(
        result
            .arena()
            .parse_tree_resolver_node(nodes.synthetic)
            .unwrap(),
        None
    );
    assert_eq!(
        result
            .arena()
            .parse_tree_resolver_node(nodes.ranged_synthetic)
            .unwrap(),
        None
    );
    assert_eq!(
        result
            .arena()
            .require_parse_tree_resolver_node(nodes.synthetic),
        Err(TransformError::ResolverNodeNotInParseTree(nodes.synthetic))
    );
    assert_eq!(
        result.arena().get_original_node(nodes.parsed_clone),
        nodes.synthetic_terminal
    );
    assert_eq!(
        result.arena().get_original_node(parsed_root),
        nodes.synthetic_terminal
    );
    assert_eq!(
        result
            .arena()
            .parse_tree_resolver_node(nodes.parsed_clone)
            .unwrap(),
        Some(EmitResolverNode::new(program_source, parsed.root))
    );
    assert_eq!(
        result
            .arena()
            .parse_tree_resolver_node(parsed_root)
            .unwrap(),
        Some(EmitResolverNode::new(program_source, parsed.root))
    );
    assert_eq!(
        result
            .arena()
            .parse_tree_resolver_node(nodes.synthetic_terminal)
            .unwrap(),
        None
    );
}

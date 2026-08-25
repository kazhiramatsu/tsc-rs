//! Source-comment ownership contracts for expression parentheses.
//!
//! The expected strings in this file were captured from the vendored
//! TypeScript 6.0.3 printer with the same factory graph. The relevant tsc
//! boundaries are `createPartiallyEmittedExpression` (`_tsc.js:24357`), the
//! ranged prefix parenthesizer (`_tsc.js:20472`), the unranged `new` callee
//! parenthesizer (`_tsc.js:20452`), `parenthesizeExpressionForNoAsi`
//! (`_tsc.js:118787`), and the distinct return/yield emitters
//! (`_tsc.js:118876` and `_tsc.js:118523`). This is deliberately a separate
//! integration target so the topology can be checked without running every
//! emitter contract. The composite fixtures also mirror tsc's no-ASI
//! left-edge recursion through call/property and conditional/binary nodes;
//! only the deepest partially-emitted owner receives the synthetic parens.

use tsc_emitter::{
    create_printer, transform_nodes, CommentRange, NewLineKind, PrintRequest, PrinterOptions,
    SourceByteRange, SourceFileTextMode, SourceRange, SyntheticComment, SyntheticCommentKind,
    TransformArena, TransformError, TransformFlags, TransformNode, TransformRoot,
    TransformSourceId, TransformationContext, Transformer,
};
use tsc_syntax::{
    nodes::{
        ArrayLiteralExpressionData, BinaryExpressionData, CallExpressionData,
        ConditionalExpressionData, ExpressionStatementData, IdentifierData, NewExpressionData,
        ParenthesizedExpressionData, PartiallyEmittedExpressionData, PrefixUnaryExpressionData,
        PropertyAccessExpressionData, ReturnStatementData, SourceFileData, SpreadElementData,
        ThrowStatementData,
    },
    parse_source_file, NodeData, SyntaxKind,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TopologyFixture {
    NormalReturn,
    NormalThrow,
    RangedGrammar,
    SyntheticGrammar,
    NoAsiParsed,
    NoAsiSyntheticWhole,
    NoAsiCallPropertyLeftEdge,
    NoAsiConditionalBinaryLeftEdge,
    NextSiblingAfterSyntheticReturn,
    SingleLinePartialReturn,
    ExplicitCommentRangePrefix,
    AsiOuterContainerInheritance,
    ZeroWidthReturnCommentRange,
    ChildZeroWidthCommentRange,
    AmbientParenthesizedContainer,
    AmbientBinaryPrefixFinalChild,
    AmbientCallPrefixArgument,
    AmbientArrayPrefixElement,
    AmbientSpreadPrefixOperand,
    YieldParentRetained,
    YieldOwnerConsumed,
}

impl TopologyFixture {
    const fn source(self) -> &'static str {
        match self {
            Self::NormalReturn | Self::NormalThrow => "/*SRC*/ x /*TAIL*/;\n",
            Self::RangedGrammar => "/*SRC*/ x + y /*TAIL*/;\n",
            Self::SyntheticGrammar => "/*SRC*/ call() /*TAIL*/;\n",
            Self::NoAsiParsed => "return /*PRE*/ (/*OPEN*/ x /*CLOSE*/) /*POST*/;\n",
            Self::NoAsiSyntheticWhole => "/*SRC*/\nx /*TAIL*/;\n",
            Self::NoAsiCallPropertyLeftEdge | Self::NoAsiConditionalBinaryLeftEdge => {
                "//SRC\nx /*TAIL*/;\n"
            }
            Self::NextSiblingAfterSyntheticReturn => "x\n//NEXT\ny;\n",
            Self::SingleLinePartialReturn => "x;\n",
            Self::ExplicitCommentRangePrefix => "/*SRC*/ x+y /*TAIL*/;\n",
            Self::AsiOuterContainerInheritance => "/*SRC*/ call() /*TAIL*/\n",
            Self::ZeroWidthReturnCommentRange => "x /*TAIL*/;\n",
            Self::ChildZeroWidthCommentRange => "x /*TAIL*/;\n",
            Self::AmbientParenthesizedContainer => "/*SRC*/ x /*TAIL*/\n",
            Self::AmbientBinaryPrefixFinalChild
            | Self::AmbientCallPrefixArgument
            | Self::AmbientArrayPrefixElement
            | Self::AmbientSpreadPrefixOperand => "x /*TAIL*/\n",
            Self::YieldParentRetained => "function* g(){ yield <any>//YLEAD\n x /*YTAIL*/; }\n",
            Self::YieldOwnerConsumed => "//SRC\nx /*TAIL*/;\nfunction* f(){yield z /*PARENT*/;}\n",
        }
    }
}

struct TopologyTransformer {
    fixture: TopologyFixture,
}

impl Transformer for TopologyTransformer {
    fn name(&self) -> &'static str {
        "source-comment-topology"
    }

    fn transform_root(
        &mut self,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        let TransformRoot::SourceFile(source) = root else {
            return Ok(root);
        };
        let root_node = context.arena().root(source)?;
        let original_statements = source_file_statements(context, root_node);
        let first = first_statement(context, source, original_statements);

        let (statements, preserve_statement_range) = match self.fixture {
            TopologyFixture::NormalReturn | TopologyFixture::NormalThrow => {
                let expression = expression_statement_expression(context, first);
                let wrapper = create_partial_wrapper(
                    context, expression, expression, "META-L", "META-T", false,
                )?;
                let flags = context.arena().propagate_child_flags(wrapper)?;
                let data = match self.fixture {
                    TopologyFixture::NormalReturn => {
                        NodeData::ReturnStatement(ReturnStatementData {
                            expression: Some(wrapper.node()),
                        })
                    }
                    TopologyFixture::NormalThrow => NodeData::ThrowStatement(ThrowStatementData {
                        expression: Some(wrapper.node()),
                    }),
                    _ => unreachable!(),
                };
                let statement = context.factory()?.create_node(source, data, flags)?;
                (vec![statement], false)
            }
            TopologyFixture::RangedGrammar => {
                let expression = expression_statement_expression(context, first);
                let wrapper = create_partial_wrapper(
                    context, expression, expression, "META-L", "META-T", false,
                )?;
                let prefix_flags = context.arena().propagate_child_flags(wrapper)?;
                let prefix = context.factory()?.create_node(
                    source,
                    NodeData::PrefixUnaryExpression(PrefixUnaryExpressionData {
                        operator: SyntaxKind::MinusToken,
                        operand: Some(wrapper.node()),
                    }),
                    prefix_flags,
                )?;
                let statement_flags = context.arena().propagate_child_flags(prefix)?;
                let statement = context.factory()?.create_node(
                    source,
                    NodeData::ExpressionStatement(ExpressionStatementData {
                        expression: Some(prefix.node()),
                    }),
                    statement_flags,
                )?;
                (vec![statement], false)
            }
            TopologyFixture::SyntheticGrammar => {
                let expression = expression_statement_expression(context, first);
                let wrapper = create_partial_wrapper(
                    context, expression, expression, "META-L", "META-T", false,
                )?;
                let arguments = context.factory()?.create_node_array(source, Vec::new())?;
                let new_flags = context.arena().propagate_child_flags(wrapper)?;
                let new_expression = context.factory()?.create_node(
                    source,
                    NodeData::NewExpression(NewExpressionData {
                        expression: Some(wrapper.node()),
                        type_arguments: None,
                        arguments: Some(arguments.array()),
                        question_dot_token: None,
                    }),
                    new_flags,
                )?;
                let statement_flags = context.arena().propagate_child_flags(new_expression)?;
                let statement = context.factory()?.create_node(
                    source,
                    NodeData::ExpressionStatement(ExpressionStatementData {
                        expression: Some(new_expression.node()),
                    }),
                    statement_flags,
                )?;
                (vec![statement], false)
            }
            TopologyFixture::NoAsiParsed => {
                let parsed_parentheses = match &context.arena().node(first)?.data {
                    NodeData::ReturnStatement(data) => data
                        .expression
                        .and_then(|expression| context.arena().node_ref(source, expression))
                        .expect("parsed return has an expression"),
                    _ => panic!("parsed no-ASI fixture starts with return"),
                };
                let inner = match &context.arena().node(parsed_parentheses)?.data {
                    NodeData::ParenthesizedExpression(data) => data
                        .expression
                        .and_then(|expression| context.arena().node_ref(source, expression))
                        .expect("parsed parentheses have an expression"),
                    _ => panic!("parsed no-ASI fixture retains source parentheses"),
                };
                let wrapper = create_partial_wrapper(
                    context,
                    inner,
                    parsed_parentheses,
                    "WRAP-L",
                    "WRAP-T",
                    true,
                )?;
                let flags = context.arena().transform_flags(first);
                let statement = context.factory()?.update_node(
                    first,
                    NodeData::ReturnStatement(ReturnStatementData {
                        expression: Some(wrapper.node()),
                    }),
                    flags,
                )?;
                (vec![statement], true)
            }
            TopologyFixture::NoAsiSyntheticWhole => {
                let expression = expression_statement_expression(context, first);
                let wrapper = create_partial_wrapper(
                    context, expression, expression, "WRAP-L", "WRAP-T", true,
                )?;
                let flags = context.arena().propagate_child_flags(wrapper)?;
                let statement = context.factory()?.create_node(
                    source,
                    NodeData::ReturnStatement(ReturnStatementData {
                        expression: Some(wrapper.node()),
                    }),
                    flags,
                )?;
                (vec![statement], false)
            }
            TopologyFixture::NoAsiCallPropertyLeftEdge => {
                let expression = expression_statement_expression(context, first);
                let wrapper = create_partial_wrapper(
                    context, expression, expression, "SYN-L", "SYN-T", false,
                )?;
                let member = create_identifier(context, source, "member")?;
                let property_flags = context.arena().propagate_child_flags(wrapper)?
                    | context.arena().propagate_child_flags(member)?;
                let property = context.factory()?.create_node(
                    source,
                    NodeData::PropertyAccessExpression(PropertyAccessExpressionData {
                        name: Some(member.node()),
                        expression: Some(wrapper.node()),
                        question_dot_token: None,
                    }),
                    property_flags,
                )?;
                let arguments = context.factory()?.create_node_array(source, Vec::new())?;
                let call_flags = context.arena().propagate_child_flags(property)?;
                let call = context.factory()?.create_node(
                    source,
                    NodeData::CallExpression(CallExpressionData {
                        expression: Some(property.node()),
                        question_dot_token: None,
                        type_arguments: None,
                        arguments: Some(arguments.array()),
                    }),
                    call_flags,
                )?;
                let statement = create_return_statement(context, source, call)?;
                (vec![statement], false)
            }
            TopologyFixture::NoAsiConditionalBinaryLeftEdge => {
                let expression = expression_statement_expression(context, first);
                let wrapper = create_partial_wrapper(
                    context, expression, expression, "SYN-L", "SYN-T", false,
                )?;
                let right = create_identifier(context, source, "right")?;
                let yes = create_identifier(context, source, "yes")?;
                let no = create_identifier(context, source, "no")?;
                let plus = context.factory()?.create_token(
                    source,
                    SyntaxKind::PlusToken,
                    TransformFlags::NONE,
                )?;
                let binary_flags = context.arena().propagate_child_flags(wrapper)?
                    | context.arena().propagate_child_flags(right)?;
                let binary = context.factory()?.create_node(
                    source,
                    NodeData::BinaryExpression(BinaryExpressionData {
                        left: Some(wrapper.node()),
                        operator_token: Some(plus.node()),
                        right: Some(right.node()),
                    }),
                    binary_flags,
                )?;
                let question = context.factory()?.create_token(
                    source,
                    SyntaxKind::QuestionToken,
                    TransformFlags::NONE,
                )?;
                let colon = context.factory()?.create_token(
                    source,
                    SyntaxKind::ColonToken,
                    TransformFlags::NONE,
                )?;
                let conditional_flags = context.arena().propagate_child_flags(binary)?
                    | context.arena().propagate_child_flags(yes)?
                    | context.arena().propagate_child_flags(no)?;
                let conditional = context.factory()?.create_node(
                    source,
                    NodeData::ConditionalExpression(ConditionalExpressionData {
                        condition: Some(binary.node()),
                        question_token: Some(question.node()),
                        when_true: Some(yes.node()),
                        colon_token: Some(colon.node()),
                        when_false: Some(no.node()),
                    }),
                    conditional_flags,
                )?;
                let statement = create_return_statement(context, source, conditional)?;
                (vec![statement], false)
            }
            TopologyFixture::NextSiblingAfterSyntheticReturn => {
                let expression = expression_statement_expression(context, first);
                let wrapper = create_partial_expression(context, expression, expression)?;
                let statement = create_return_statement(context, source, wrapper)?;
                let next = nth_statement(context, source, original_statements, 1);
                (vec![statement, next], false)
            }
            TopologyFixture::SingleLinePartialReturn => {
                let expression = expression_statement_expression(context, first);
                let wrapper = create_partial_expression(context, expression, expression)?;
                context
                    .arena_mut()?
                    .metadata_mut(wrapper)
                    .add_leading_comment(SyntheticComment::new(
                        SyntheticCommentKind::SingleLine,
                        "LEAD",
                        false,
                        false,
                    ));
                let statement = create_return_statement(context, source, wrapper)?;
                (vec![statement], false)
            }
            TopologyFixture::ExplicitCommentRangePrefix => {
                let donor = expression_statement_expression(context, first);
                let donor_comment_range = {
                    let syntax = context.arena().source(source)?.syntax();
                    let donor_record = context.arena().node(donor)?;
                    SourceByteRange::new(donor_record.pos, donor_record.end, syntax.positions())
                        .expect("parsed binary donor range")
                };
                let left = create_identifier(context, source, "x")?;
                let right = create_identifier(context, source, "y")?;
                let plus = context.factory()?.create_token(
                    source,
                    SyntaxKind::PlusToken,
                    TransformFlags::NONE,
                )?;
                let binary_flags = context.arena().propagate_child_flags(left)?
                    | context.arena().propagate_child_flags(right)?;
                let binary = context.factory()?.create_node(
                    source,
                    NodeData::BinaryExpression(BinaryExpressionData {
                        left: Some(left.node()),
                        operator_token: Some(plus.node()),
                        right: Some(right.node()),
                    }),
                    binary_flags,
                )?;
                let binary_record = context.arena().node(binary)?;
                assert_eq!((binary_record.pos, binary_record.end), (u32::MAX, u32::MAX));
                context
                    .arena_mut()?
                    .metadata_mut(binary)
                    .set_comment_range(CommentRange::new(
                        source,
                        SourceRange::Original(donor_comment_range),
                    ));
                let prefix_flags = context.arena().propagate_child_flags(binary)?;
                let prefix = context.factory()?.create_node(
                    source,
                    NodeData::PrefixUnaryExpression(PrefixUnaryExpressionData {
                        operator: SyntaxKind::MinusToken,
                        operand: Some(binary.node()),
                    }),
                    prefix_flags,
                )?;
                let statement_flags = context.arena().propagate_child_flags(prefix)?;
                let statement = context.factory()?.create_node(
                    source,
                    NodeData::ExpressionStatement(ExpressionStatementData {
                        expression: Some(prefix.node()),
                    }),
                    statement_flags,
                )?;
                (vec![statement], false)
            }
            TopologyFixture::AsiOuterContainerInheritance => {
                let parsed_call = expression_statement_expression(context, first);
                let (call_data, call_flags, call_range) = {
                    let outer = context.arena().node(first)?;
                    let call = context.arena().node(parsed_call)?;
                    let NodeData::CallExpression(data) = &call.data else {
                        panic!("ASI container fixture starts with a call expression")
                    };
                    assert_eq!((outer.pos, outer.end), (call.pos, call.end));
                    (
                        data.clone(),
                        context.arena().transform_flags(parsed_call),
                        (call.pos, call.end),
                    )
                };
                let ranged_call = context.factory()?.create_node(
                    source,
                    NodeData::CallExpression(call_data),
                    call_flags,
                )?;
                context
                    .factory()?
                    .set_text_range(ranged_call, parsed_call)?;
                context
                    .arena_mut()?
                    .set_original_node(ranged_call, Some(parsed_call))?;
                let ranged_record = context.arena().node(ranged_call)?;
                assert_eq!((ranged_record.pos, ranged_record.end), call_range);

                let arguments = context.factory()?.create_node_array(source, Vec::new())?;
                let new_flags = context.arena().propagate_child_flags(ranged_call)?;
                let new_expression = context.factory()?.create_node(
                    source,
                    NodeData::NewExpression(NewExpressionData {
                        expression: Some(ranged_call.node()),
                        type_arguments: None,
                        arguments: Some(arguments.array()),
                        question_dot_token: None,
                    }),
                    new_flags,
                )?;
                let statement_flags = context.arena().transform_flags(first);
                let statement = context.factory()?.update_node(
                    first,
                    NodeData::ExpressionStatement(ExpressionStatementData {
                        expression: Some(new_expression.node()),
                    }),
                    statement_flags,
                )?;
                // Match factory.updateSourceFile(source, [updatedStatement]):
                // the parsed outer statement keeps its range, while the new
                // statement list itself is synthesized.
                (vec![statement], false)
            }
            TopologyFixture::ZeroWidthReturnCommentRange => {
                let expression = expression_statement_expression(context, first);
                let zero_width_range = {
                    let syntax = context.arena().source(source)?.syntax();
                    let end = context.arena().node(expression)?.end;
                    SourceByteRange::new(end, end, syntax.positions())
                        .expect("parsed expression end is a valid zero-width range")
                };
                let statement = create_return_statement(context, source, expression)?;
                context
                    .arena_mut()?
                    .metadata_mut(statement)
                    .set_comment_range(CommentRange::new(
                        source,
                        SourceRange::Original(zero_width_range),
                    ));
                (vec![statement], false)
            }
            TopologyFixture::ChildZeroWidthCommentRange => {
                let expression = expression_statement_expression(context, first);
                let zero_width_range = {
                    let syntax = context.arena().source(source)?.syntax();
                    let end = context.arena().node(expression)?.end;
                    SourceByteRange::new(end, end, syntax.positions())
                        .expect("parsed expression end is a valid zero-width range")
                };
                let statement = create_return_statement(context, source, expression)?;
                context.factory()?.set_text_range(statement, first)?;
                context
                    .arena_mut()?
                    .set_original_node(statement, Some(first))?;
                context
                    .arena_mut()?
                    .metadata_mut(expression)
                    .set_comment_range(CommentRange::new(
                        source,
                        SourceRange::Original(zero_width_range),
                    ));
                (vec![statement], false)
            }
            TopologyFixture::AmbientParenthesizedContainer => {
                let expression = expression_statement_expression(context, first);
                let parenthesized_flags = context.arena().propagate_child_flags(expression)?;
                let parenthesized = context.factory()?.create_node(
                    source,
                    NodeData::ParenthesizedExpression(ParenthesizedExpressionData {
                        expression: Some(expression.node()),
                    }),
                    parenthesized_flags,
                )?;
                let statement_flags = context.arena().transform_flags(first);
                let statement = context.factory()?.update_node(
                    first,
                    NodeData::ExpressionStatement(ExpressionStatementData {
                        expression: Some(parenthesized.node()),
                    }),
                    statement_flags,
                )?;
                (vec![statement], false)
            }
            TopologyFixture::AmbientBinaryPrefixFinalChild => {
                let expression = expression_statement_expression(context, first);
                let prefix_flags = context.arena().propagate_child_flags(expression)?;
                let right = context.factory()?.create_node(
                    source,
                    NodeData::PrefixUnaryExpression(PrefixUnaryExpressionData {
                        operator: SyntaxKind::MinusToken,
                        operand: Some(expression.node()),
                    }),
                    prefix_flags,
                )?;
                let left = create_identifier(context, source, "a")?;
                let plus = context.factory()?.create_token(
                    source,
                    SyntaxKind::PlusToken,
                    TransformFlags::NONE,
                )?;
                let binary_flags = context.arena().propagate_child_flags(left)?
                    | context.arena().propagate_child_flags(right)?;
                let binary = context.factory()?.create_node(
                    source,
                    NodeData::BinaryExpression(BinaryExpressionData {
                        left: Some(left.node()),
                        operator_token: Some(plus.node()),
                        right: Some(right.node()),
                    }),
                    binary_flags,
                )?;
                let statement_flags = context.arena().transform_flags(first);
                let statement = context.factory()?.update_node(
                    first,
                    NodeData::ExpressionStatement(ExpressionStatementData {
                        expression: Some(binary.node()),
                    }),
                    statement_flags,
                )?;
                (vec![statement], false)
            }
            TopologyFixture::AmbientCallPrefixArgument
            | TopologyFixture::AmbientArrayPrefixElement
            | TopologyFixture::AmbientSpreadPrefixOperand => {
                let expression = expression_statement_expression(context, first);
                let prefix_flags = context.arena().propagate_child_flags(expression)?;
                let prefix = context.factory()?.create_node(
                    source,
                    NodeData::PrefixUnaryExpression(PrefixUnaryExpressionData {
                        operator: SyntaxKind::MinusToken,
                        operand: Some(expression.node()),
                    }),
                    prefix_flags,
                )?;
                let replacement = match self.fixture {
                    TopologyFixture::AmbientCallPrefixArgument => {
                        let callee = create_identifier(context, source, "f")?;
                        let arguments =
                            context.factory()?.create_node_array(source, vec![prefix])?;
                        let flags = context.arena().propagate_child_flags(callee)?
                            | context.arena().propagate_child_flags(prefix)?;
                        context.factory()?.create_node(
                            source,
                            NodeData::CallExpression(CallExpressionData {
                                expression: Some(callee.node()),
                                question_dot_token: None,
                                type_arguments: None,
                                arguments: Some(arguments.array()),
                            }),
                            flags,
                        )?
                    }
                    TopologyFixture::AmbientArrayPrefixElement => {
                        let elements =
                            context.factory()?.create_node_array(source, vec![prefix])?;
                        let flags = context.arena().propagate_child_flags(prefix)?;
                        context.factory()?.create_node(
                            source,
                            NodeData::ArrayLiteralExpression(ArrayLiteralExpressionData {
                                elements: Some(elements.array()),
                            }),
                            flags,
                        )?
                    }
                    TopologyFixture::AmbientSpreadPrefixOperand => {
                        let flags = context.arena().propagate_child_flags(prefix)?
                            | TransformFlags::CONTAINS_REST_OR_SPREAD;
                        context.factory()?.create_node(
                            source,
                            NodeData::SpreadElement(SpreadElementData {
                                expression: Some(prefix.node()),
                            }),
                            flags,
                        )?
                    }
                    _ => unreachable!(),
                };
                let statement_flags = context.arena().transform_flags(first);
                let statement = context.factory()?.update_node(
                    first,
                    NodeData::ExpressionStatement(ExpressionStatementData {
                        expression: Some(replacement.node()),
                    }),
                    statement_flags,
                )?;
                (vec![statement], false)
            }
            TopologyFixture::YieldParentRetained => {
                let function = first;
                let yield_expression = generator_yield_expression(context, source, function);
                let type_assertion = match &context.arena().node(yield_expression)?.data {
                    NodeData::YieldExpression(data) => data
                        .expression
                        .and_then(|expression| context.arena().node_ref(source, expression))
                        .expect("yield fixture has an operand"),
                    _ => panic!("generator statement is a yield expression"),
                };
                let inner = match &context.arena().node(type_assertion)?.data {
                    NodeData::TypeAssertionExpression(data) => data
                        .expression
                        .and_then(|expression| context.arena().node_ref(source, expression))
                        .expect("type assertion has an expression"),
                    _ => panic!("yield operand starts with a type assertion"),
                };
                let wrapper = create_partial_wrapper(
                    context,
                    inner,
                    type_assertion,
                    "YMETA-L",
                    "YMETA-T",
                    false,
                )?;
                let updated_function = update_generator_yield_operand(
                    context,
                    source,
                    function,
                    yield_expression,
                    wrapper,
                )?;
                (vec![updated_function], true)
            }
            TopologyFixture::YieldOwnerConsumed => {
                let donor = expression_statement_expression(context, first);
                let function = nth_statement(context, source, original_statements, 1);
                let yield_expression = generator_yield_expression(context, source, function);
                let wrapper =
                    create_partial_wrapper(context, donor, donor, "SYN-L", "SYN-T", true)?;
                let updated_function = update_generator_yield_operand(
                    context,
                    source,
                    function,
                    yield_expression,
                    wrapper,
                )?;
                (vec![updated_function], false)
            }
        };

        replace_source_statements(
            context,
            source,
            root_node,
            original_statements,
            statements,
            preserve_statement_range,
        )?;
        Ok(TransformRoot::SourceFile(source))
    }
}

fn source_file_statements(
    context: &TransformationContext,
    root: TransformNode,
) -> tsc_emitter::TransformNodeArray {
    match &context.arena().node(root).expect("source-file root").data {
        NodeData::SourceFile(data) => data
            .statements
            .and_then(|statements| context.arena().node_array_ref(root.source(), statements))
            .expect("source file has a statement list"),
        _ => panic!("transform root is a source file"),
    }
}

fn first_statement(
    context: &TransformationContext,
    source: TransformSourceId,
    statements: tsc_emitter::TransformNodeArray,
) -> TransformNode {
    nth_statement(context, source, statements, 0)
}

fn nth_statement(
    context: &TransformationContext,
    source: TransformSourceId,
    statements: tsc_emitter::TransformNodeArray,
    index: usize,
) -> TransformNode {
    context
        .arena()
        .node_array(statements)
        .expect("statement array")
        .nodes
        .get(index)
        .and_then(|statement| context.arena().node_ref(source, *statement))
        .expect("fixture has a statement")
}

fn expression_statement_expression(
    context: &TransformationContext,
    statement: TransformNode,
) -> TransformNode {
    match &context
        .arena()
        .node(statement)
        .expect("fixture statement")
        .data
    {
        NodeData::ExpressionStatement(data) => data
            .expression
            .and_then(|expression| context.arena().node_ref(statement.source(), expression))
            .expect("expression statement has an expression"),
        _ => panic!("fixture statement is an expression statement"),
    }
}

fn create_identifier(
    context: &mut TransformationContext,
    source: TransformSourceId,
    text: &str,
) -> Result<TransformNode, TransformError> {
    context.factory()?.create_node(
        source,
        NodeData::Identifier(IdentifierData {
            escaped_text: text.to_owned(),
            text: text.to_owned(),
        }),
        TransformFlags::NONE,
    )
}

fn create_return_statement(
    context: &mut TransformationContext,
    source: TransformSourceId,
    expression: TransformNode,
) -> Result<TransformNode, TransformError> {
    let flags = context.arena().propagate_child_flags(expression)?;
    context.factory()?.create_node(
        source,
        NodeData::ReturnStatement(ReturnStatementData {
            expression: Some(expression.node()),
        }),
        flags,
    )
}

fn generator_yield_expression(
    context: &TransformationContext,
    source: TransformSourceId,
    function: TransformNode,
) -> TransformNode {
    let body = match &context
        .arena()
        .node(function)
        .expect("generator declaration")
        .data
    {
        NodeData::FunctionDeclaration(data) => data
            .body
            .and_then(|body| context.arena().node_ref(source, body))
            .expect("generator fixture has a body"),
        _ => panic!("yield fixture contains a generator declaration"),
    };
    let statements = match &context.arena().node(body).expect("generator body").data {
        NodeData::Block(data) => data
            .statements
            .and_then(|statements| context.arena().node_array_ref(source, statements))
            .expect("generator body has statements"),
        _ => panic!("generator body is a block"),
    };
    expression_statement_expression(context, first_statement(context, source, statements))
}

fn update_generator_yield_operand(
    context: &mut TransformationContext,
    source: TransformSourceId,
    function: TransformNode,
    yield_expression: TransformNode,
    operand: TransformNode,
) -> Result<TransformNode, TransformError> {
    let body = match &context.arena().node(function)?.data {
        NodeData::FunctionDeclaration(data) => data
            .body
            .and_then(|body| context.arena().node_ref(source, body))
            .expect("generator fixture has a body"),
        _ => panic!("yield fixture contains a generator declaration"),
    };
    let body_statements = match &context.arena().node(body)?.data {
        NodeData::Block(data) => data
            .statements
            .and_then(|statements| context.arena().node_array_ref(source, statements))
            .expect("generator body has statements"),
        _ => panic!("generator body is a block"),
    };
    let statement = first_statement(context, source, body_statements);

    let mut yield_data = context.arena().node(yield_expression)?.data.clone();
    match &mut yield_data {
        NodeData::YieldExpression(data) => data.expression = Some(operand.node()),
        _ => panic!("generator statement is a yield expression"),
    }
    let yield_flags = context.arena().transform_flags(yield_expression);
    let updated_yield =
        context
            .factory()?
            .update_node(yield_expression, yield_data, yield_flags)?;

    let statement_flags = context.arena().transform_flags(statement);
    let updated_statement = context.factory()?.update_node(
        statement,
        NodeData::ExpressionStatement(ExpressionStatementData {
            expression: Some(updated_yield.node()),
        }),
        statement_flags,
    )?;
    let updated_body_statements = context
        .factory()?
        .update_node_array(body_statements, vec![updated_statement])?;
    let mut body_data = context.arena().node(body)?.data.clone();
    match &mut body_data {
        NodeData::Block(data) => data.statements = Some(updated_body_statements.array()),
        _ => unreachable!(),
    }
    let body_flags = context.arena().transform_flags(body);
    let updated_body = context
        .factory()?
        .update_node(body, body_data, body_flags)?;

    let mut function_data = context.arena().node(function)?.data.clone();
    match &mut function_data {
        NodeData::FunctionDeclaration(data) => data.body = Some(updated_body.node()),
        _ => unreachable!(),
    }
    let function_flags = context.arena().transform_flags(function);
    context
        .factory()?
        .update_node(function, function_data, function_flags)
}

fn create_partial_expression(
    context: &mut TransformationContext,
    expression: TransformNode,
    original: TransformNode,
) -> Result<TransformNode, TransformError> {
    let flags =
        context.arena().propagate_child_flags(expression)? | TransformFlags::CONTAINS_TYPE_SCRIPT;
    let wrapper = context.factory()?.create_node(
        expression.source(),
        NodeData::PartiallyEmittedExpression(PartiallyEmittedExpressionData {
            expression: Some(expression.node()),
        }),
        flags,
    )?;
    context.factory()?.set_text_range(wrapper, original)?;
    context
        .arena_mut()?
        .set_original_node(wrapper, Some(original))?;
    Ok(wrapper)
}

fn create_partial_wrapper(
    context: &mut TransformationContext,
    expression: TransformNode,
    original: TransformNode,
    leading: &'static str,
    trailing: &'static str,
    leading_has_trailing_new_line: bool,
) -> Result<TransformNode, TransformError> {
    let wrapper = create_partial_expression(context, expression, original)?;
    let metadata = context.arena_mut()?.metadata_mut(wrapper);
    metadata.add_leading_comment(SyntheticComment::new(
        SyntheticCommentKind::MultiLine,
        leading,
        false,
        leading_has_trailing_new_line,
    ));
    metadata.add_trailing_comment(SyntheticComment::new(
        SyntheticCommentKind::MultiLine,
        trailing,
        false,
        false,
    ));
    Ok(wrapper)
}

fn replace_source_statements(
    context: &mut TransformationContext,
    source: TransformSourceId,
    root: TransformNode,
    original_statements: tsc_emitter::TransformNodeArray,
    statements: Vec<TransformNode>,
    preserve_statement_range: bool,
) -> Result<(), TransformError> {
    let statements = if preserve_statement_range {
        context
            .factory()?
            .update_node_array(original_statements, statements)?
    } else {
        context.factory()?.create_node_array(source, statements)?
    };
    let end_of_file_token = match &context.arena().node(root)?.data {
        NodeData::SourceFile(data) => data.end_of_file_token,
        _ => unreachable!(),
    };
    let flags = context.arena().transform_flags(root);
    let updated_root = context.factory()?.update_node(
        root,
        NodeData::SourceFile(SourceFileData {
            statements: Some(statements.array()),
            end_of_file_token,
        }),
        flags,
    )?;
    context.arena_mut()?.replace_root(source, updated_root)?;
    Ok(())
}

fn print_fixture(fixture: TopologyFixture, remove_comments: bool) -> String {
    let parsed = parse_source_file(
        "source-comment-topology.ts",
        fixture.source(),
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![Box::new(TopologyTransformer { fixture })],
        false,
    )
    .expect("source-comment topology transform");
    create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_remove_comments(remove_comments)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
    .print(&mut result, PrintRequest::SourceFile(source), None)
    .expect("source-comment topology print")
    .text()
    .to_owned()
}

#[test]
fn normal_return_and_throw_consume_source_comments_around_wrapper_metadata() {
    for (fixture, keyword) in [
        (TopologyFixture::NormalReturn, "return"),
        (TopologyFixture::NormalThrow, "throw"),
    ] {
        assert_eq!(
            print_fixture(fixture, false),
            format!("{keyword} /*SRC*/ /*META-L*/ x /*META-T*/ /*TAIL*/;\n"),
        );
    }
}

#[test]
fn ranged_grammar_parentheses_leave_source_comments_outside() {
    // tsc's prefix-unary parenthesizer uses setTextRange(createParen(x), x).
    assert_eq!(
        print_fixture(TopologyFixture::RangedGrammar, false),
        "-/*SRC*/ (/*META-L*/ x + y /*META-T*/) /*TAIL*/;\n",
    );
}

#[test]
fn synthetic_grammar_parentheses_move_source_comments_inside() {
    // A call-expression `new` callee uses createParen(x) without setTextRange.
    assert_eq!(
        print_fixture(TopologyFixture::SyntheticGrammar, false),
        "new (/*SRC*/ /*META-L*/ call() /*META-T*/ /*TAIL*/)();\n",
    );
}

#[test]
fn parsed_no_asi_parentheses_split_wrapper_and_token_comment_ownership() {
    assert_eq!(
        print_fixture(TopologyFixture::NoAsiParsed, false),
        concat!(
            "return /*PRE*/ /*WRAP-L*/\n",
            "( /*OPEN*/x /*CLOSE*/) /*WRAP-T*/ /*POST*/;\n",
        ),
    );
}

#[test]
fn synthetic_whole_no_asi_parentheses_consume_source_and_wrapper_comments() {
    assert_eq!(
        print_fixture(TopologyFixture::NoAsiSyntheticWhole, false),
        concat!(
            "return (/*SRC*/\n",
            "/*WRAP-L*/\n",
            "x /*WRAP-T*/ /*TAIL*/);\n",
        ),
    );
}

#[test]
fn no_asi_obligation_reaches_partial_wrapper_through_composite_left_edges() {
    for (fixture, expected) in [
        (
            TopologyFixture::NoAsiCallPropertyLeftEdge,
            concat!(
                "return (//SRC\n",
                "/*SYN-L*/ x /*SYN-T*/ /*TAIL*/).member();\n",
            ),
        ),
        (
            TopologyFixture::NoAsiConditionalBinaryLeftEdge,
            concat!(
                "return (//SRC\n",
                "/*SYN-L*/ x /*SYN-T*/ /*TAIL*/) + right ? yes : no;\n",
            ),
        ),
    ] {
        let output = print_fixture(fixture, false);
        assert_eq!(output, expected, "{fixture:?}");
        assert!(
            output.starts_with("return (//SRC\n"),
            "{fixture:?}: {output}"
        );
    }
}

#[test]
fn trailing_phase_does_not_consume_the_next_statements_leading_comment() {
    // Vendored tsc 6.0.3 exact: the partial owner's trailing phase stops at
    // the end of `x`; `//NEXT` remains owned by the retained `y` statement.
    let output = print_fixture(TopologyFixture::NextSiblingAfterSyntheticReturn, false);
    assert_eq!(output, "return x;\n//NEXT\ny;\n");
    assert_eq!(output.matches("//NEXT").count(), 1, "{output}");
    assert!(
        output.find(';').expect("return semicolon")
            < output.find("//NEXT").expect("next-statement comment"),
        "{output}",
    );
}

#[test]
fn synthetic_single_line_leading_comment_always_terminates_its_line() {
    // Vendored tsc 6.0.3 exact for SingleLine(false, false). The comment kind
    // itself forces both line boundaries; its boolean flags cannot allow the
    // following expression to be commented out.
    assert_eq!(
        print_fixture(TopologyFixture::SingleLinePartialReturn, false),
        "return (\n//LEAD\nx);\n",
    );
}

#[test]
fn source_ranged_grammar_parent_keeps_child_explicit_comment_range() {
    // Vendored tsc 6.0.3 exact: the Binary's raw range stays synthesized,
    // while setCommentRange(binary, donor) owns SRC/TAIL inside the ranged
    // prefix-operand parentheses.
    assert_eq!(
        print_fixture(TopologyFixture::ExplicitCommentRangePrefix, false),
        "-(/*SRC*/ x + y /*TAIL*/);\n",
    );
}

#[test]
fn asi_outer_comment_container_is_inherited_through_synthetic_new() {
    // Vendored tsc 6.0.3 exact for a parsed ASI ExpressionStatement updated
    // to New(synthetic Call wrapper ranged to the original parsed Call).
    // containerPos keeps SRC outside; containerEnd leaves TAIL after the
    // outer statement's generated semicolon.
    let output = print_fixture(TopologyFixture::AsiOuterContainerInheritance, false);
    assert_eq!(output, "/*SRC*/ new (call())(); /*TAIL*/\n");
    assert_eq!(output.matches("SRC").count(), 1, "{output}");
    assert_eq!(output.matches("TAIL").count(), 1, "{output}");
}

#[test]
fn zero_width_parent_comment_range_does_not_claim_child_trailing_comments() {
    // Vendored tsc 6.0.3 exact for setCommentRange(return, { pos: x.end,
    // end: x.end }). A zero-width parent range establishes no comment
    // container, so the parsed child's same-line TAIL remains before `;`.
    let output = print_fixture(TopologyFixture::ZeroWidthReturnCommentRange, false);
    assert_eq!(output, "return x /*TAIL*/;\n");
    assert_eq!(output.matches("TAIL").count(), 1, "{output}");
}

#[test]
fn zero_width_child_comment_range_does_not_fall_back_to_parent_range() {
    // Vendored tsc 6.0.3 exact: Return inherits the parsed statement's text
    // range/original, but x has explicit { pos: x.end, end: x.end }. The
    // explicit empty child owner suppresses TAIL instead of borrowing the
    // wider parent container.
    let output = print_fixture(TopologyFixture::ChildZeroWidthCommentRange, false);
    assert_eq!(output, "return x;\n");
    assert!(!output.contains("TAIL"), "{output}");
}

#[test]
fn ambient_comment_container_flows_through_synthetic_parentheses() {
    // Vendored tsc 6.0.3 exact for updateExpressionStatement(parsedStatement,
    // createParenthesizedExpression(parsedX)). The synthetic parens inherit
    // the ambient container instead of reopening x's source comment phase.
    let output = print_fixture(TopologyFixture::AmbientParenthesizedContainer, false);
    assert_eq!(output, "/*SRC*/ (x); /*TAIL*/\n");
    assert_eq!(output.matches("SRC").count(), 1, "{output}");
    assert_eq!(output.matches("TAIL").count(), 1, "{output}");
    assert!(
        output.find(");").expect("generated close and semicolon")
            < output.find("/*TAIL*/").expect("outer trailing comment"),
        "{output}",
    );
}

#[test]
fn ambient_container_end_reaches_binary_prefix_final_child() {
    // Vendored tsc 6.0.3 exact for a parsed ASI ExpressionStatement updated
    // to Binary(a, +, Prefix(-, parsedX)). Both synthetic levels preserve the
    // ambient containerEnd, so x cannot consume the outer TAIL early.
    let output = print_fixture(TopologyFixture::AmbientBinaryPrefixFinalChild, false);
    assert_eq!(output, "a + -x; /*TAIL*/\n");
    assert_eq!(output.matches("TAIL").count(), 1, "{output}");
    assert!(
        output.find(';').expect("generated outer semicolon")
            < output.find("/*TAIL*/").expect("outer trailing comment"),
        "{output}",
    );
}

#[test]
fn ambient_container_end_reaches_list_and_wrapper_final_children() {
    // Vendored tsc 6.0.3 exact for a parsed ASI ExpressionStatement updated
    // with a retained x under a synthetic Prefix. List/wrapper-local comment
    // phases must keep the outer statement's containerEnd, leaving TAIL after
    // the generated semicolon exactly once.
    for (fixture, expected) in [
        (
            TopologyFixture::AmbientCallPrefixArgument,
            "f(-x); /*TAIL*/\n",
        ),
        (
            TopologyFixture::AmbientArrayPrefixElement,
            "[-x]; /*TAIL*/\n",
        ),
        (
            TopologyFixture::AmbientSpreadPrefixOperand,
            "...-x; /*TAIL*/\n",
        ),
    ] {
        let output = print_fixture(fixture, false);
        assert_eq!(output, expected, "{fixture:?}");
        assert_eq!(output.matches("TAIL").count(), 1, "{fixture:?}: {output}");
        assert!(
            output.find(';').expect("generated outer semicolon")
                < output.find("/*TAIL*/").expect("outer trailing comment"),
            "{fixture:?}: {output}",
        );
    }
}

#[test]
fn yield_parent_retains_trailing_comment_after_operand_consumes_leading_comment() {
    assert_eq!(
        print_fixture(TopologyFixture::YieldParentRetained, false),
        concat!(
            "function* g() {\n",
            "    yield (/*YMETA-L*/ //YLEAD\n",
            "    x /*YMETA-T*/) /*YTAIL*/;\n",
            "}\n",
        ),
    );
}

#[test]
fn yield_consumes_a_different_operand_owner_before_parent_trailing_comments() {
    assert_eq!(
        print_fixture(TopologyFixture::YieldOwnerConsumed, false),
        concat!(
            "function* f() { yield (//SRC\n",
            "/*SYN-L*/\n",
            "x /*SYN-T*/ /*TAIL*/) /*PARENT*/; }\n",
        ),
    );
}

#[test]
fn remove_comments_drops_comment_driven_parentheses_but_keeps_grammar_parentheses() {
    for (fixture, expected) in [
        (TopologyFixture::NormalReturn, "return x;\n"),
        (TopologyFixture::NormalThrow, "throw x;\n"),
        (TopologyFixture::RangedGrammar, "-(x + y);\n"),
        (TopologyFixture::SyntheticGrammar, "new (call())();\n"),
        (TopologyFixture::NoAsiParsed, "return x;\n"),
        (TopologyFixture::NoAsiSyntheticWhole, "return x;\n"),
        (
            TopologyFixture::NoAsiCallPropertyLeftEdge,
            "return x.member();\n",
        ),
        (
            TopologyFixture::NoAsiConditionalBinaryLeftEdge,
            "return x + right ? yes : no;\n",
        ),
        (
            TopologyFixture::NextSiblingAfterSyntheticReturn,
            "return x;\ny;\n",
        ),
        (TopologyFixture::SingleLinePartialReturn, "return x;\n"),
        (TopologyFixture::ExplicitCommentRangePrefix, "-(x + y);\n"),
        (
            TopologyFixture::AsiOuterContainerInheritance,
            "new (call())();\n",
        ),
        (TopologyFixture::ZeroWidthReturnCommentRange, "return x;\n"),
        (TopologyFixture::ChildZeroWidthCommentRange, "return x;\n"),
        (TopologyFixture::AmbientParenthesizedContainer, "(x);\n"),
        (TopologyFixture::AmbientBinaryPrefixFinalChild, "a + -x;\n"),
        (TopologyFixture::AmbientCallPrefixArgument, "f(-x);\n"),
        (TopologyFixture::AmbientArrayPrefixElement, "[-x];\n"),
        (TopologyFixture::AmbientSpreadPrefixOperand, "...-x;\n"),
        (
            TopologyFixture::YieldParentRetained,
            "function* g() {\n    yield x;\n}\n",
        ),
        (
            TopologyFixture::YieldOwnerConsumed,
            "function* f() { yield x; }\n",
        ),
    ] {
        let output = print_fixture(fixture, true);
        assert_eq!(output, expected, "{fixture:?}");
        for marker in [
            "SRC", "TAIL", "PRE", "POST", "OPEN", "CLOSE", "WRAP", "YLEAD", "YTAIL", "META", "SYN",
            "PARENT", "NEXT", "LEAD",
        ] {
            assert!(!output.contains(marker), "{fixture:?}: {output}");
        }
    }
}

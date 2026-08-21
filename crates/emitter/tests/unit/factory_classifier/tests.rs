//! EA-GAP-FLAGS table contracts (packet §12.4): the classifier's output
//! over freshly synthesized nodes, asserted facet-by-facet on the
//! owner-consulted qualification surface. Eight of the nine facets are
//! TransformFlags; the ninth (`ContainsCapturedBlockScopeBinding`) is a
//! NodeCheckFlags fact served by the emit resolver and covered by the
//! B-1 resolver replay contract instead.

use std::collections::BTreeSet;

use super::*;
use tsc_syntax::{nodes as syntax_nodes, NodeData, SyntaxKind};

const FACETS: &[(&str, TransformFlags)] = &[
    ("ES2015", TransformFlags::CONTAINS_ES_2015),
    ("Generator", TransformFlags::CONTAINS_GENERATOR),
    ("Yield", TransformFlags::CONTAINS_YIELD),
    (
        "HoistedDeclarationOrCompletion",
        TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION,
    ),
    ("LexicalThis", TransformFlags::CONTAINS_LEXICAL_THIS),
    ("LexicalSuper", TransformFlags::CONTAINS_LEXICAL_SUPER),
    ("BindingPattern", TransformFlags::CONTAINS_BINDING_PATTERN),
    ("RestOrSpread", TransformFlags::CONTAINS_REST_OR_SPREAD),
];

fn facet_row(flags: TransformFlags) -> BTreeSet<&'static str> {
    FACETS
        .iter()
        .filter(|(_, facet)| flags.contains(*facet))
        .map(|(name, _)| *name)
        .collect()
}

fn row(names: &[&'static str]) -> BTreeSet<&'static str> {
    names.iter().copied().collect()
}

fn setup() -> (TransformArena, TransformSourceId) {
    let parsed = tsc_syntax::parse_source_file(
        "classifier.ts".to_owned(),
        String::new(),
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    (arena, source)
}

fn identifier(text: &str) -> NodeData {
    NodeData::Identifier(syntax_nodes::IdentifierData {
        escaped_text: text.to_owned(),
        text: text.to_owned(),
    })
}

#[test]
fn token_facets_match_the_creation_table() {
    assert_eq!(
        facet_row(classify_created_token_flags(SyntaxKind::SuperKeyword)),
        row(&["ES2015", "LexicalSuper"]),
    );
    assert_eq!(
        facet_row(classify_created_token_flags(SyntaxKind::ThisKeyword)),
        row(&["LexicalThis"]),
    );
    assert_eq!(
        facet_row(classify_created_token_flags(SyntaxKind::StaticKeyword)),
        row(&["ES2015"]),
    );
    assert_eq!(
        facet_row(classify_created_token_flags(SyntaxKind::AsteriskToken)),
        row(&[]),
    );
}

#[test]
fn yield_and_generator_rows_classify_and_exclude_at_the_function_boundary() {
    let (mut arena, source) = setup();
    let mut factory = NodeFactory::new(&mut arena);
    let one = factory
        .create_node(
            source,
            NodeData::NumericLiteral(syntax_nodes::NumericLiteralData {
                text: "1".to_owned(),
            }),
            TransformFlags::NONE,
        )
        .expect("literal");
    let yield_expression = factory
        .create_node(
            source,
            NodeData::YieldExpression(syntax_nodes::YieldExpressionData {
                asterisk_token: None,
                expression: Some(one.node()),
            }),
            TransformFlags::NONE,
        )
        .expect("yield");
    let yield_flags = factory
        .arena()
        .classify_created_node_flags(yield_expression, NodeFlags::SYNTHESIZED)
        .expect("classify yield");
    assert_eq!(facet_row(yield_flags), row(&["ES2015", "Yield"]));
    factory
        .arena_mut()
        .set_transform_flags(yield_expression, yield_flags);

    let statement = factory
        .create_node(
            source,
            NodeData::ExpressionStatement(syntax_nodes::ExpressionStatementData {
                expression: Some(yield_expression.node()),
            }),
            TransformFlags::NONE,
        )
        .expect("statement");
    let statement_flags = factory
        .arena()
        .classify_created_node_flags(statement, NodeFlags::SYNTHESIZED)
        .expect("classify statement");
    factory
        .arena_mut()
        .set_transform_flags(statement, statement_flags);
    let statements = factory
        .create_node_array(source, vec![statement])
        .expect("statements");
    let body = factory
        .create_node(
            source,
            NodeData::Block(syntax_nodes::BlockData {
                statements: Some(statements.array()),
            }),
            TransformFlags::NONE,
        )
        .expect("body");
    let body_flags = factory
        .arena()
        .classify_created_node_flags(body, NodeFlags::SYNTHESIZED)
        .expect("classify body");
    assert_eq!(facet_row(body_flags), row(&["ES2015", "Yield"]));
    factory.arena_mut().set_transform_flags(body, body_flags);

    let asterisk = factory
        .create_token(
            source,
            SyntaxKind::AsteriskToken,
            classify_created_token_flags(SyntaxKind::AsteriskToken),
        )
        .expect("asterisk");
    let function = factory
        .create_node(
            source,
            NodeData::FunctionExpression(syntax_nodes::FunctionExpressionData {
                modifiers: None,
                asterisk_token: Some(asterisk.node()),
                name: None,
                type_parameters: None,
                parameters: None,
                r#type: None,
                body: Some(body.node()),
            }),
            TransformFlags::NONE,
        )
        .expect("function");
    let function_flags = factory
        .arena()
        .classify_created_node_flags(function, NodeFlags::SYNTHESIZED)
        .expect("classify function");
    // The generator function itself carries its body's yield plus its own
    // generator/hoisted facets.
    assert_eq!(
        facet_row(function_flags),
        row(&[
            "ES2015",
            "Yield",
            "Generator",
            "HoistedDeclarationOrCompletion"
        ]),
    );
    factory
        .arena_mut()
        .set_transform_flags(function, function_flags);

    // Consumed as a child, FunctionExcludes strips what a function contains
    // (yield/hoisted/this/super/binding-pattern); the generator facet and
    // ES2015 survive into the parent, exactly like the upstream constant.
    let wrapper = factory
        .create_node(
            source,
            NodeData::ExpressionStatement(syntax_nodes::ExpressionStatementData {
                expression: Some(function.node()),
            }),
            TransformFlags::NONE,
        )
        .expect("wrapper");
    let wrapper_flags = factory
        .arena()
        .classify_created_node_flags(wrapper, NodeFlags::SYNTHESIZED)
        .expect("classify wrapper");
    assert_eq!(facet_row(wrapper_flags), row(&["ES2015", "Generator"]));
}

#[test]
fn binding_pattern_and_spread_rows_classify() {
    let (mut arena, source) = setup();
    let mut factory = NodeFactory::new(&mut arena);
    let dots = factory
        .create_token(
            source,
            SyntaxKind::DotDotDotToken,
            classify_created_token_flags(SyntaxKind::DotDotDotToken),
        )
        .expect("dots");
    let rest_name = factory
        .create_node(source, identifier("rest"), TransformFlags::NONE)
        .expect("rest name");
    let element = factory
        .create_node(
            source,
            NodeData::BindingElement(syntax_nodes::BindingElementData {
                dot_dot_dot_token: Some(dots.node()),
                property_name: None,
                name: Some(rest_name.node()),
                initializer: None,
            }),
            TransformFlags::NONE,
        )
        .expect("element");
    let element_flags = factory
        .arena()
        .classify_created_node_flags(element, NodeFlags::SYNTHESIZED)
        .expect("classify element");
    assert_eq!(facet_row(element_flags), row(&["ES2015", "RestOrSpread"]));
    factory
        .arena_mut()
        .set_transform_flags(element, element_flags);

    let elements = factory
        .create_node_array(source, vec![element])
        .expect("elements");
    let pattern = factory
        .create_node(
            source,
            NodeData::ObjectBindingPattern(syntax_nodes::ObjectBindingPatternData {
                elements: Some(elements.array()),
            }),
            TransformFlags::NONE,
        )
        .expect("pattern");
    let pattern_flags = factory
        .arena()
        .classify_created_node_flags(pattern, NodeFlags::SYNTHESIZED)
        .expect("classify pattern");
    assert_eq!(
        facet_row(pattern_flags),
        row(&["ES2015", "BindingPattern", "RestOrSpread"]),
    );

    let spread_target = factory
        .create_node(source, identifier("items"), TransformFlags::NONE)
        .expect("spread target");
    let spread = factory
        .create_node(
            source,
            NodeData::SpreadElement(syntax_nodes::SpreadElementData {
                expression: Some(spread_target.node()),
            }),
            TransformFlags::NONE,
        )
        .expect("spread");
    let spread_flags = factory
        .arena()
        .classify_created_node_flags(spread, NodeFlags::SYNTHESIZED)
        .expect("classify spread");
    assert_eq!(facet_row(spread_flags), row(&["ES2015", "RestOrSpread"]));
}

#[test]
fn declaration_and_completion_rows_classify() {
    let (mut arena, source) = setup();
    let mut factory = NodeFactory::new(&mut arena);
    let name = factory
        .create_node(source, identifier("value"), TransformFlags::NONE)
        .expect("name");
    let declaration = factory
        .create_node(
            source,
            NodeData::VariableDeclaration(syntax_nodes::VariableDeclarationData {
                name: Some(name.node()),
                exclamation_token: None,
                r#type: None,
                initializer: None,
            }),
            TransformFlags::NONE,
        )
        .expect("declaration");
    let declaration_flags = factory
        .arena()
        .classify_created_node_flags(declaration, NodeFlags::SYNTHESIZED)
        .expect("classify declaration");
    factory
        .arena_mut()
        .set_transform_flags(declaration, declaration_flags);
    let declarations = factory
        .create_node_array(source, vec![declaration])
        .expect("declarations");
    let list = factory
        .create_node(
            source,
            NodeData::VariableDeclarationList(syntax_nodes::VariableDeclarationListData {
                declarations: Some(declarations.array()),
            }),
            TransformFlags::NONE,
        )
        .expect("list");
    let let_flags = factory
        .arena()
        .classify_created_node_flags(list, NodeFlags::LET)
        .expect("classify let list");
    assert_eq!(
        facet_row(let_flags),
        row(&["ES2015", "HoistedDeclarationOrCompletion"]),
    );
    let var_flags = factory
        .arena()
        .classify_created_node_flags(list, NodeFlags::SYNTHESIZED)
        .expect("classify var list");
    assert_eq!(
        facet_row(var_flags),
        row(&["HoistedDeclarationOrCompletion"])
    );

    let return_statement = factory
        .create_node(
            source,
            NodeData::ReturnStatement(syntax_nodes::ReturnStatementData { expression: None }),
            TransformFlags::NONE,
        )
        .expect("return");
    let return_flags = factory
        .arena()
        .classify_created_node_flags(return_statement, NodeFlags::SYNTHESIZED)
        .expect("classify return");
    assert_eq!(
        facet_row(return_flags),
        row(&["HoistedDeclarationOrCompletion"]),
    );
}

#[test]
fn lexical_this_and_super_rows_classify_through_super_calls() {
    let (mut arena, source) = setup();
    let mut factory = NodeFactory::new(&mut arena);
    let super_token = factory
        .create_token(
            source,
            SyntaxKind::SuperKeyword,
            classify_created_token_flags(SyntaxKind::SuperKeyword),
        )
        .expect("super");
    let member = factory
        .create_node(source, identifier("toString"), TransformFlags::NONE)
        .expect("member");
    let access = factory
        .create_node(
            source,
            NodeData::PropertyAccessExpression(syntax_nodes::PropertyAccessExpressionData {
                expression: Some(super_token.node()),
                question_dot_token: None,
                name: Some(member.node()),
            }),
            TransformFlags::NONE,
        )
        .expect("access");
    let access_flags = factory
        .arena()
        .classify_created_node_flags(access, NodeFlags::SYNTHESIZED)
        .expect("classify access");
    assert_eq!(facet_row(access_flags), row(&["ES2015", "LexicalSuper"]));
    factory
        .arena_mut()
        .set_transform_flags(access, access_flags);

    let arguments = factory
        .create_node_array(source, Vec::new())
        .expect("arguments");
    let call = factory
        .create_node(
            source,
            NodeData::CallExpression(syntax_nodes::CallExpressionData {
                expression: Some(access.node()),
                question_dot_token: None,
                type_arguments: None,
                arguments: Some(arguments.array()),
            }),
            TransformFlags::NONE,
        )
        .expect("call");
    let call_flags = factory
        .arena()
        .classify_created_node_flags(call, NodeFlags::SYNTHESIZED)
        .expect("classify call");
    assert_eq!(
        facet_row(call_flags),
        row(&["ES2015", "LexicalSuper", "LexicalThis"]),
    );
}

#[test]
fn identifier_name_position_strips_possible_top_level_await() {
    let (mut arena, source) = setup();
    let mut factory = NodeFactory::new(&mut arena);
    let awaited = factory
        .create_node(source, identifier("await"), TransformFlags::NONE)
        .expect("await identifier");
    let identifier_flags = factory
        .arena()
        .classify_created_node_flags(awaited, NodeFlags::SYNTHESIZED)
        .expect("classify identifier");
    assert!(identifier_flags.contains(TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT));
    factory
        .arena_mut()
        .set_transform_flags(awaited, identifier_flags);

    let declaration = factory
        .create_node(
            source,
            NodeData::VariableDeclaration(syntax_nodes::VariableDeclarationData {
                name: Some(awaited.node()),
                exclamation_token: None,
                r#type: None,
                initializer: None,
            }),
            TransformFlags::NONE,
        )
        .expect("declaration");
    let declaration_flags = factory
        .arena()
        .classify_created_node_flags(declaration, NodeFlags::SYNTHESIZED)
        .expect("classify declaration");
    assert!(
        !declaration_flags.contains(TransformFlags::CONTAINS_POSSIBLE_TOP_LEVEL_AWAIT),
        "a name-position identifier must not leak possible-top-level-await",
    );
}

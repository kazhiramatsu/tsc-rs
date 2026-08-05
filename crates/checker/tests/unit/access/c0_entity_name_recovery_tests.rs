use tsc_binder::bind_source_file;
use tsc_syntax::nodes::{
    ElementAccessExpressionData, EmptyStatementData, JsxNamespacedNameData, NonNullExpressionData,
    PropertyAccessExpressionData, QualifiedNameData,
};
use tsc_syntax::{parse_source_file, LanguageVariant, NodeData, ParseOptions, SyntaxKind};
use tsc_types::{CheckMode, CompilerOptions, NodeFlags};

use crate::state::CheckerState;

#[test]
fn recovered_entity_names_keep_source_text_and_valid_siblings_keep_entity_rendering() {
    let mut source = parse_source_file(
        "entity-recovery.ts".to_owned(),
        "alpha".to_owned(),
        ParseOptions {
            language_variant: LanguageVariant::Standard,
            javascript_file: false,
            ..ParseOptions::default()
        },
        None,
    );
    assert!(source.parse_diagnostics.is_empty());
    let identifier = source
        .arena
        .node_ids()
        .find(|&node| source.arena.node(node).kind == SyntaxKind::Identifier)
        .expect("valid identifier sibling");
    let missing_jsx_name = source.arena.alloc_node(
        NodeData::JsxNamespacedName(JsxNamespacedNameData {
            namespace: Some(identifier),
            name: None,
        }),
        0,
        5,
        NodeFlags::NONE,
    );
    let non_entity = source.arena.alloc_node(
        NodeData::EmptyStatement(EmptyStatementData {}),
        0,
        5,
        NodeFlags::NONE,
    );
    let missing_qualified_name = source.arena.alloc_node(
        NodeData::QualifiedName(QualifiedNameData {
            left: Some(identifier),
            right: None,
        }),
        0,
        5,
        NodeFlags::NONE,
    );
    let missing_property_name = source.arena.alloc_node(
        NodeData::PropertyAccessExpression(PropertyAccessExpressionData {
            expression: Some(identifier),
            name: None,
            question_dot_token: None,
        }),
        0,
        5,
        NodeFlags::NONE,
    );
    let missing_property_chain_name = source.arena.alloc_node(
        NodeData::PropertyAccessExpression(PropertyAccessExpressionData {
            expression: Some(identifier),
            name: None,
            question_dot_token: None,
        }),
        0,
        5,
        NodeFlags::OPTIONAL_CHAIN,
    );
    let missing_non_null_operand = source.arena.alloc_node(
        NodeData::NonNullExpression(NonNullExpressionData { expression: None }),
        0,
        5,
        NodeFlags::NONE,
    );
    let missing_element_receiver = source.arena.alloc_node(
        NodeData::ElementAccessExpression(ElementAccessExpressionData {
            expression: None,
            question_dot_token: None,
            argument_expression: Some(identifier),
        }),
        0,
        5,
        NodeFlags::NONE,
    );
    let missing_element_argument = source.arena.alloc_node(
        NodeData::ElementAccessExpression(ElementAccessExpressionData {
            expression: Some(identifier),
            question_dot_token: None,
            argument_expression: None,
        }),
        0,
        5,
        NodeFlags::NONE,
    );
    let missing_element_chain_receiver = source.arena.alloc_node(
        NodeData::ElementAccessExpression(ElementAccessExpressionData {
            expression: None,
            question_dot_token: None,
            argument_expression: Some(identifier),
        }),
        0,
        5,
        NodeFlags::OPTIONAL_CHAIN,
    );
    let options = CompilerOptions::default();
    let binder = bind_source_file(&source, &options);
    let mut state = CheckerState::new(&source, &binder, &options);

    assert_eq!(
        state
            .entity_name_to_string(identifier)
            .expect("valid entity"),
        "alpha"
    );
    assert_eq!(
        state
            .entity_name_to_string(missing_jsx_name)
            .expect("missing JSX child recovers"),
        "alpha:"
    );
    assert_eq!(
        state
            .entity_name_to_string(missing_qualified_name)
            .expect("missing qualified child recovers"),
        "alpha."
    );
    assert_eq!(
        state
            .entity_name_to_string(missing_property_name)
            .expect("missing property child recovers"),
        "alpha."
    );
    assert_eq!(
        state
            .entity_name_to_string(non_entity)
            .expect("unexpected entity kind retains source text"),
        "alpha"
    );
    for recovered in [
        state
            .check_non_null_assertion(missing_non_null_operand)
            .expect("missing non-null operand"),
        state
            .check_property_access_expression(missing_property_name, CheckMode::NORMAL, false)
            .expect("missing property name"),
        state
            .check_property_access_expression(missing_property_chain_name, CheckMode::NORMAL, false)
            .expect("missing property chain name"),
        state
            .check_qualified_name(missing_qualified_name, CheckMode::NORMAL)
            .expect("missing qualified name"),
        state
            .check_indexed_access(missing_element_receiver, CheckMode::NORMAL)
            .expect("missing element receiver"),
        state
            .check_indexed_access(missing_element_argument, CheckMode::NORMAL)
            .expect("missing element argument"),
        state
            .check_indexed_access(missing_element_chain_receiver, CheckMode::NORMAL)
            .expect("missing element-chain receiver"),
    ] {
        assert!(state.tables.is_error_type(recovered));
    }
}

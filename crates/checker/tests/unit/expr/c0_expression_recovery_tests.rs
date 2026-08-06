use tsc_binder::bind_source_file;
use tsc_syntax::nodes::{
    DeleteExpressionData, EmptyStatementData, ParenthesizedExpressionData, PropertyAssignmentData,
    SpreadElementData, TypeOfExpressionData,
};
use tsc_syntax::{parse_source_file, LanguageVariant, NodeData, ParseOptions, SyntaxKind};
use tsc_types::{CheckMode, CompilerOptions, NodeFlags};

use crate::state::CheckerState;

#[test]
fn missing_expression_slots_use_operator_specific_recovery_values() {
    let mut source = parse_source_file(
        "expression-recovery.ts".to_owned(),
        "const live = 1;\n".to_owned(),
        ParseOptions {
            language_variant: LanguageVariant::Standard,
            javascript_file: false,
            ..ParseOptions::default()
        },
        None,
    );
    let parenthesized = source.arena.alloc_node(
        NodeData::ParenthesizedExpression(ParenthesizedExpressionData { expression: None }),
        0,
        0,
        NodeFlags::NONE,
    );
    let spread = source.arena.alloc_node(
        NodeData::SpreadElement(SpreadElementData { expression: None }),
        0,
        0,
        NodeFlags::NONE,
    );
    let property = source.arena.alloc_node(
        NodeData::PropertyAssignment(PropertyAssignmentData {
            name: None,
            initializer: None,
            modifiers: None,
            question_token: None,
            exclamation_token: None,
        }),
        0,
        0,
        NodeFlags::NONE,
    );
    let type_of = source.arena.alloc_node(
        NodeData::TypeOfExpression(TypeOfExpressionData { expression: None }),
        0,
        0,
        NodeFlags::NONE,
    );
    let delete = source.arena.alloc_node(
        NodeData::DeleteExpression(DeleteExpressionData { expression: None }),
        0,
        0,
        NodeFlags::NONE,
    );
    let detached_this = source
        .arena
        .alloc_token(SyntaxKind::ThisKeyword, 0, 0, NodeFlags::NONE);
    let unbound_class = source.arena.alloc_node(
        NodeData::EmptyStatement(EmptyStatementData {}),
        0,
        0,
        NodeFlags::NONE,
    );

    let options = CompilerOptions::default();
    let binder = bind_source_file(&source, &options);
    let mut state = CheckerState::new(&source, &binder, &options);

    for recovered in [
        state
            .check_parenthesized_expression(parenthesized, CheckMode::NORMAL)
            .expect("parenthesized recovery"),
        state
            .check_spread_expression(spread, CheckMode::NORMAL)
            .expect("spread recovery"),
        state
            .check_property_assignment(property, CheckMode::NORMAL)
            .expect("property recovery"),
        state
            .check_this_expression(detached_this)
            .expect("detached this recovery"),
    ] {
        assert!(state.tables.is_error_type(recovered));
    }
    assert_eq!(
        state
            .check_type_of_expression(type_of)
            .expect("typeof recovery"),
        state.typeof_type
    );
    assert_eq!(
        state
            .check_delete_expression(delete)
            .expect("delete recovery"),
        state.tables.intrinsics.boolean
    );
    assert!(!state
        .class_declaration_extends_null(unbound_class)
        .expect("unbound class recovery"));
}

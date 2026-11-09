use tsc_binder::bind_source_file;
use tsc_syntax::nodes::{
    DeleteExpressionData, EmptyStatementData, ParenthesizedExpressionData, PropertyAssignmentData,
    SpreadElementData, TypeOfExpressionData,
};
use tsc_syntax::{parse_source_file, NodeData, NodeId, ParseOptions, ScriptKind, SyntaxKind};
use tsc_types::{CheckMode, CompilerOptions, IdentityDomain, NodeFlags};

use crate::state::CheckerState;

#[test]
fn missing_expression_slots_use_operator_specific_recovery_values() {
    let mut source = parse_source_file(
        "expression-recovery.ts".to_owned(),
        "const live = 1;\n".to_owned(),
        ParseOptions {
            script_kind: ScriptKind::Ts,
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
    // The checker reads a published tree; publication moves the ids.
    source
        .relocate_into_identity_domain(&IdentityDomain::new())
        .expect("test source identities");
    let at = |node: NodeId| NodeId::new(node.index() + source.arena.node_base());
    let (parenthesized, spread, property, type_of, delete, detached_this, unbound_class) = (
        at(parenthesized),
        at(spread),
        at(property),
        at(type_of),
        at(delete),
        at(detached_this),
        at(unbound_class),
    );

    let options = CompilerOptions::default();
    let binder = bind_source_file(&mut source);
    let mut state = CheckerState::new(binder.source, &binder, &options);

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

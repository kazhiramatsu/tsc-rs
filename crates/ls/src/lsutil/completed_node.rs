//! tsgo `ls/lsutil/completednode.go`: whether a node is complete (its last
//! token written), which decides whether a position after it belongs to it.

use tsc_binder::node_util;
use tsc_syntax::{NodeData, NodeId, SyntaxKind};

use crate::astnav::{Found, Navigator};
use crate::tree::View;

/// tsgo `PositionBelongsToNode`: the position is in the node, or just after
/// it while the node is incomplete.
pub(crate) fn position_belongs_to_node(
    navigator: &Navigator<'_>,
    candidate: Found,
    position: u32,
) -> bool {
    assert!(
        navigator.pos(candidate) <= position,
        "Expected candidate.pos <= position"
    );
    position < navigator.end(candidate) || !is_completed_node(navigator, Some(candidate))
}

/// tsgo `IsCompletedNode`.
pub(crate) fn is_completed_node(navigator: &Navigator<'_>, node: Option<Found>) -> bool {
    let Some(node) = node else {
        return false;
    };
    let kind = navigator.kind(node);
    if navigator.pos(node) == navigator.end(node) && kind != SyntaxKind::EndOfFileToken {
        return false;
    }
    let Found::Node(view @ View::Node(id)) = node else {
        return true;
    };
    let source = navigator.file();
    let completed = |child: Option<NodeId>| {
        is_completed_node(
            navigator,
            child.map(|child| Found::Node(navigator.tree().view_of(child))),
        )
    };
    let has_child_of_kind = |kind| navigator.child_of_kind(view, kind).is_some();
    let node_is_present = |child: Option<NodeId>| {
        child.is_some_and(|child| !node_util::node_is_missing(source, Some(child)))
    };
    match &source.arena.node(id).data {
        NodeData::ClassDeclaration(_)
        | NodeData::InterfaceDeclaration(_)
        | NodeData::EnumDeclaration(_)
        | NodeData::ObjectLiteralExpression(_)
        | NodeData::ObjectBindingPattern(_)
        | NodeData::TypeLiteral(_)
        | NodeData::Block(_)
        | NodeData::ModuleBlock(_)
        | NodeData::CaseBlock(_)
        | NodeData::NamedImports(_)
        | NodeData::NamedExports(_) => navigator.node_ends_with(view, SyntaxKind::CloseBraceToken),

        NodeData::CatchClause(data) => completed(data.block),

        NodeData::NewExpression(data) if data.arguments.is_none() => true,

        NodeData::NewExpression(_)
        | NodeData::CallExpression(_)
        | NodeData::ParenthesizedExpression(_)
        | NodeData::ParenthesizedType(_) => {
            navigator.node_ends_with(view, SyntaxKind::CloseParenToken)
        }

        NodeData::FunctionType(_) | NodeData::ConstructorType(_) => {
            completed(node_util::type_node_of(source, id))
        }

        NodeData::Constructor(_)
        | NodeData::GetAccessor(_)
        | NodeData::SetAccessor(_)
        | NodeData::FunctionDeclaration(_)
        | NodeData::FunctionExpression(_)
        | NodeData::MethodDeclaration(_)
        | NodeData::MethodSignature(_)
        | NodeData::ConstructSignature(_)
        | NodeData::CallSignature(_)
        | NodeData::ArrowFunction(_) => {
            if let Some(body) = node_util::body_of(source, id) {
                return completed(Some(body));
            }
            if let Some(ty) = node_util::type_node_of(source, id) {
                return completed(Some(ty));
            }
            // Even though type parameters can be unclosed, we can get away with
            // having at least a closing paren.
            has_child_of_kind(SyntaxKind::CloseParenToken)
        }

        NodeData::ModuleDeclaration(data) => data.body.is_some() && completed(data.body),

        NodeData::IfStatement(data) => match data.else_statement {
            Some(else_statement) => completed(Some(else_statement)),
            None => completed(data.then_statement),
        },

        NodeData::ExpressionStatement(data) => {
            completed(data.expression) || has_child_of_kind(SyntaxKind::SemicolonToken)
        }

        NodeData::ArrayLiteralExpression(_)
        | NodeData::ArrayBindingPattern(_)
        | NodeData::ElementAccessExpression(_)
        | NodeData::ComputedPropertyName(_)
        | NodeData::TupleType(_) => navigator.node_ends_with(view, SyntaxKind::CloseBracketToken),

        NodeData::IndexSignature(_) => match node_util::type_node_of(source, id) {
            Some(ty) => completed(Some(ty)),
            None => has_child_of_kind(SyntaxKind::CloseBracketToken),
        },

        // there is no such thing as terminator token for CaseClause/DefaultClause so for simplicity always consider them non-completed
        NodeData::CaseClause(_) | NodeData::DefaultClause(_) => false,

        NodeData::ForStatement(data) => completed(data.statement),
        NodeData::ForInStatement(data) => completed(data.statement),
        NodeData::ForOfStatement(data) => completed(data.statement),
        NodeData::WhileStatement(data) => completed(data.statement),
        NodeData::DoStatement(data) => {
            // rough approximation: if DoStatement has While keyword - then if node is completed is checking the presence of ')';
            if has_child_of_kind(SyntaxKind::WhileKeyword) {
                return navigator.node_ends_with(view, SyntaxKind::CloseParenToken);
            }
            completed(data.statement)
        }

        NodeData::TypeQuery(data) => completed(data.expr_name),

        NodeData::TypeOfExpression(data) => completed(data.expression),
        NodeData::DeleteExpression(data) => completed(data.expression),
        NodeData::VoidExpression(data) => completed(data.expression),
        NodeData::YieldExpression(data) => completed(data.expression),
        NodeData::SpreadElement(data) => completed(data.expression),

        NodeData::TaggedTemplateExpression(data) => completed(data.template),

        NodeData::TemplateExpression(data) => {
            let Some(spans) = data.template_spans else {
                return false;
            };
            completed(source.arena.node_array(spans).nodes.last().copied())
        }

        NodeData::TemplateSpan(data) => node_is_present(data.literal),

        NodeData::ExportDeclaration(data) => node_is_present(data.module_specifier),
        NodeData::ImportDeclaration(data) => node_is_present(data.module_specifier),

        NodeData::PrefixUnaryExpression(data) => completed(data.operand),

        NodeData::BinaryExpression(data) => completed(data.right),

        NodeData::ConditionalExpression(data) => completed(data.when_false),

        _ => true,
    }
}

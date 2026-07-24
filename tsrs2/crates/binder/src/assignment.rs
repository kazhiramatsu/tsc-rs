//! Assignment-declaration classification shared by the binder and checker.
//!
//! This is the non-JSDoc part of tsc's assignment-declaration utility
//! family. Keeping it here avoids the former checker-side TS-only
//! reimplementation drifting from the binder.

use crate::node_util::{
    id_text, is_entity_name_expression, is_left_hand_side_expression,
    is_string_or_numeric_literal_like, kind_of, literal_text_of, node_flags, parent_of,
    skip_parentheses_pub,
};
use crate::symbols::escape_leading_underscores;
use tsrs2_syntax::{NodeData, NodeId, SourceFile, SyntaxKind};
use tsrs2_types::NodeFlags;

/// tsc AssignmentDeclarationKind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum AssignmentDeclarationKind {
    None = 0,
    ExportsProperty = 1,
    ModuleExports = 2,
    PrototypeProperty = 3,
    ThisProperty = 4,
    Property = 5,
    Prototype = 6,
    ObjectDefinePropertyValue = 7,
    ObjectDefinePropertyExports = 8,
    ObjectDefinePrototypeProperty = 9,
}

/// tsc-port: getAssignmentDeclarationKind @6.0.3
/// tsc-hash: 86ed418c050973f93d14271122ba9f948961e1e038e6c338eb6df19543402bd2
/// tsc-span: _tsc.js:15055-15058
pub fn get_assignment_declaration_kind(
    source: &SourceFile,
    expr: NodeId,
) -> AssignmentDeclarationKind {
    let special = get_assignment_declaration_kind_worker(source, expr);
    if special == AssignmentDeclarationKind::Property
        || node_flags(source, source.root).intersects(NodeFlags::JAVA_SCRIPT_FILE)
    {
        special
    } else {
        AssignmentDeclarationKind::None
    }
}

/// The name node used by binder `declareSymbol` for non-JSDoc
/// assignment declarations. Kept separate from the checker-facing
/// general declaration-name utility until 9.8b activates the consumers.
pub fn get_assignment_declaration_name(source: &SourceFile, declaration: NodeId) -> Option<NodeId> {
    if matches!(
        kind_of(source, declaration),
        SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
    ) {
        let parent = parent_of(source, declaration)?;
        if matches!(
            &source.arena.node(parent).data,
            NodeData::BinaryExpression(data) if data.left == Some(declaration)
        ) && get_assignment_declaration_kind(source, parent) != AssignmentDeclarationKind::None
        {
            return get_element_or_property_access_argument_expression_or_name(source, declaration);
        }
        return None;
    }
    match get_assignment_declaration_kind(source, declaration) {
        AssignmentDeclarationKind::ExportsProperty
        | AssignmentDeclarationKind::ThisProperty
        | AssignmentDeclarationKind::Property
        | AssignmentDeclarationKind::PrototypeProperty => {
            let NodeData::BinaryExpression(data) = &source.arena.node(declaration).data else {
                return None;
            };
            get_element_or_property_access_argument_expression_or_name(source, data.left?)
        }
        AssignmentDeclarationKind::ObjectDefinePropertyValue
        | AssignmentDeclarationKind::ObjectDefinePropertyExports
        | AssignmentDeclarationKind::ObjectDefinePrototypeProperty => {
            let NodeData::CallExpression(data) = &source.arena.node(declaration).data else {
                return None;
            };
            let arguments = data.arguments?;
            source.arena.node_array(arguments).nodes.get(1).copied()
        }
        _ => None,
    }
}

/// tsc-port: getAssignmentDeclarationKindWorker @6.0.3
/// tsc-hash: 748a8d0ff34b41c4230a22f31b31e87e5752191e17183fafd67e39f4e2773d51
/// tsc-span: _tsc.js:15095-15120
fn get_assignment_declaration_kind_worker(
    source: &SourceFile,
    expr: NodeId,
) -> AssignmentDeclarationKind {
    if let NodeData::CallExpression(data) = &source.arena.node(expr).data {
        if !is_bindable_object_define_property_call(source, expr) {
            return AssignmentDeclarationKind::None;
        }
        let arguments = data.arguments.expect("checked by predicate");
        let entity_name = source.arena.node_array(arguments).nodes[0];
        if is_exports_identifier(source, entity_name)
            || is_module_exports_access_expression(source, entity_name)
        {
            return AssignmentDeclarationKind::ObjectDefinePropertyExports;
        }
        if is_bindable_static_access_expression(source, entity_name, false)
            && get_element_or_property_access_name(source, entity_name).as_deref()
                == Some("prototype")
        {
            return AssignmentDeclarationKind::ObjectDefinePrototypeProperty;
        }
        return AssignmentDeclarationKind::ObjectDefinePropertyValue;
    }

    let NodeData::BinaryExpression(data) = &source.arena.node(expr).data else {
        return AssignmentDeclarationKind::None;
    };
    let operator = data
        .operator_token
        .map(|token| kind_of(source, token))
        .unwrap_or(SyntaxKind::Unknown);
    let Some(left) = data.left else {
        return AssignmentDeclarationKind::None;
    };
    if operator != SyntaxKind::EqualsToken
        || !matches!(
            kind_of(source, left),
            SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
        )
        || is_void_zero(source, get_right_most_assigned_expression(source, expr))
    {
        return AssignmentDeclarationKind::None;
    }
    let left_expression = access_expression_of(source, left);
    if left_expression
        .is_some_and(|expression| is_bindable_static_name_expression(source, expression, true))
        && get_element_or_property_access_name(source, left).as_deref() == Some("prototype")
        && kind_of(source, get_initializer_of_binary_expression(source, expr))
            == SyntaxKind::ObjectLiteralExpression
    {
        return AssignmentDeclarationKind::Prototype;
    }
    get_assignment_declaration_property_access_kind(source, left)
}

fn get_assignment_declaration_property_access_kind(
    source: &SourceFile,
    lhs: NodeId,
) -> AssignmentDeclarationKind {
    let Some(lhs_expression) = access_expression_of(source, lhs) else {
        return AssignmentDeclarationKind::None;
    };
    if kind_of(source, lhs_expression) == SyntaxKind::ThisKeyword {
        return AssignmentDeclarationKind::ThisProperty;
    }
    if is_module_exports_access_expression(source, lhs) {
        return AssignmentDeclarationKind::ModuleExports;
    }
    if is_bindable_static_name_expression(source, lhs_expression, true) {
        if is_prototype_access(source, lhs_expression) {
            return AssignmentDeclarationKind::PrototypeProperty;
        }
        let mut next_to_last = lhs;
        while let Some(expression) = access_expression_of(source, next_to_last) {
            if kind_of(source, expression) == SyntaxKind::Identifier {
                break;
            }
            next_to_last = expression;
        }
        let id = access_expression_of(source, next_to_last).unwrap_or(next_to_last);
        let id_escaped = match &source.arena.node(id).data {
            NodeData::Identifier(data) => Some(data.escaped_text.as_str()),
            _ => None,
        };
        if (id_escaped == Some("exports")
            || id_escaped == Some("module")
                && get_element_or_property_access_name(source, next_to_last).as_deref()
                    == Some("exports"))
            && is_bindable_static_access_expression(source, lhs, false)
        {
            return AssignmentDeclarationKind::ExportsProperty;
        }
        if is_bindable_static_name_expression(source, lhs, true)
            || kind_of(source, lhs) == SyntaxKind::ElementAccessExpression
                && get_element_or_property_access_name(source, lhs).is_none()
        {
            return AssignmentDeclarationKind::Property;
        }
    }
    AssignmentDeclarationKind::None
}

pub fn is_bindable_object_define_property_call(source: &SourceFile, expr: NodeId) -> bool {
    let NodeData::CallExpression(data) = &source.arena.node(expr).data else {
        return false;
    };
    let Some(arguments) = data.arguments else {
        return false;
    };
    let arguments = &source.arena.node_array(arguments).nodes;
    if arguments.len() != 3 {
        return false;
    }
    let Some(expression) = data.expression else {
        return false;
    };
    let NodeData::PropertyAccessExpression(access) = &source.arena.node(expression).data else {
        return false;
    };
    access
        .expression
        .is_some_and(|object| id_text(source, object) == Some("Object"))
        && access
            .name
            .is_some_and(|name| id_text(source, name) == Some("defineProperty"))
        && is_string_or_numeric_literal_like(source, arguments[1])
        && is_bindable_static_name_expression(source, arguments[0], true)
}

/// The `.expression` of an access expression.
pub fn access_expression_of(source: &SourceFile, node: NodeId) -> Option<NodeId> {
    match &source.arena.node(node).data {
        NodeData::PropertyAccessExpression(data) => data.expression,
        NodeData::ElementAccessExpression(data) => data.expression,
        _ => None,
    }
}

pub fn is_exports_identifier(source: &SourceFile, node: NodeId) -> bool {
    matches!(
        &source.arena.node(node).data,
        NodeData::Identifier(data) if data.escaped_text == "exports"
    )
}

pub fn is_module_exports_access_expression(source: &SourceFile, node: NodeId) -> bool {
    let is_access = matches!(kind_of(source, node), SyntaxKind::PropertyAccessExpression)
        || is_literal_like_element_access(source, node);
    is_access
        && access_expression_of(source, node).is_some_and(|expression| {
            matches!(
                &source.arena.node(expression).data,
                NodeData::Identifier(data) if data.escaped_text == "module"
            )
        })
        && get_element_or_property_access_name(source, node).as_deref() == Some("exports")
}

pub fn is_literal_like_element_access(source: &SourceFile, node: NodeId) -> bool {
    matches!(
        &source.arena.node(node).data,
        NodeData::ElementAccessExpression(data)
            if data.argument_expression.is_some_and(|argument| {
                is_string_or_numeric_literal_like(source, argument)
            })
    )
}

pub fn is_bindable_static_access_expression(
    source: &SourceFile,
    node: NodeId,
    exclude_this_keyword: bool,
) -> bool {
    if let NodeData::PropertyAccessExpression(data) = &source.arena.node(node).data {
        let this_ok = !exclude_this_keyword
            && data
                .expression
                .is_some_and(|expression| kind_of(source, expression) == SyntaxKind::ThisKeyword);
        let static_ok = data
            .name
            .is_some_and(|name| kind_of(source, name) == SyntaxKind::Identifier)
            && data.expression.is_some_and(|expression| {
                is_bindable_static_name_expression(source, expression, true)
            });
        if this_ok || static_ok {
            return true;
        }
    }
    is_bindable_static_element_access_expression(source, node, exclude_this_keyword)
}

pub fn is_bindable_static_element_access_expression(
    source: &SourceFile,
    node: NodeId,
    exclude_this_keyword: bool,
) -> bool {
    is_literal_like_element_access(source, node)
        && access_expression_of(source, node).is_some_and(|expression| {
            !exclude_this_keyword && kind_of(source, expression) == SyntaxKind::ThisKeyword
                || is_entity_name_expression(source, expression)
                || is_bindable_static_access_expression(source, expression, true)
        })
}

pub fn is_bindable_static_name_expression(
    source: &SourceFile,
    node: NodeId,
    exclude_this_keyword: bool,
) -> bool {
    is_entity_name_expression(source, node)
        || is_bindable_static_access_expression(source, node, exclude_this_keyword)
}

pub fn get_element_or_property_access_argument_expression_or_name(
    source: &SourceFile,
    node: NodeId,
) -> Option<NodeId> {
    match &source.arena.node(node).data {
        NodeData::PropertyAccessExpression(data) => data.name,
        NodeData::ElementAccessExpression(data) => {
            let argument = skip_parentheses_pub(source, data.argument_expression?);
            if is_string_or_numeric_literal_like(source, argument) {
                Some(argument)
            } else {
                Some(node)
            }
        }
        _ => None,
    }
}

pub fn get_element_or_property_access_name(source: &SourceFile, node: NodeId) -> Option<String> {
    let name = get_element_or_property_access_argument_expression_or_name(source, node)?;
    match &source.arena.node(name).data {
        NodeData::Identifier(data) => Some(data.escaped_text.clone()),
        _ if is_string_or_numeric_literal_like(source, name) => {
            literal_text_of(source, name).map(escape_leading_underscores)
        }
        _ => None,
    }
}

pub fn get_right_most_assigned_expression(source: &SourceFile, mut node: NodeId) -> NodeId {
    loop {
        let NodeData::BinaryExpression(data) = &source.arena.node(node).data else {
            return node;
        };
        let is_assignment = data
            .operator_token
            .is_some_and(|token| kind_of(source, token) == SyntaxKind::EqualsToken)
            && data
                .left
                .is_some_and(|left| is_left_hand_side_expression(source, left));
        if !is_assignment {
            return node;
        }
        match data.right {
            Some(right) => node = right,
            None => return node,
        }
    }
}

fn is_void_zero(source: &SourceFile, node: NodeId) -> bool {
    match &source.arena.node(node).data {
        NodeData::VoidExpression(data) => data.expression.is_some_and(|expression| {
            matches!(
                &source.arena.node(expression).data,
                NodeData::NumericLiteral(data) if data.text == "0"
            )
        }),
        _ => false,
    }
}

pub fn get_initializer_of_binary_expression(source: &SourceFile, mut expr: NodeId) -> NodeId {
    loop {
        let NodeData::BinaryExpression(data) = &source.arena.node(expr).data else {
            return expr;
        };
        match data.right {
            Some(right)
                if matches!(
                    &source.arena.node(right).data,
                    NodeData::BinaryExpression(_)
                ) =>
            {
                expr = right;
            }
            Some(right) => return right,
            None => return expr,
        }
    }
}

pub fn is_prototype_access(source: &SourceFile, node: NodeId) -> bool {
    is_bindable_static_access_expression(source, node, false)
        && get_element_or_property_access_name(source, node).as_deref() == Some("prototype")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsrs2_syntax::{parse_source_file, ParseOptions};

    fn kinds(text: &str, javascript_file: bool) -> Vec<AssignmentDeclarationKind> {
        let source = parse_source_file(
            if javascript_file { "a.js" } else { "a.ts" },
            text,
            ParseOptions {
                javascript_file,
                ..ParseOptions::default()
            },
            None,
        );
        source
            .arena
            .nodes()
            .iter()
            .enumerate()
            .filter(|(_, node)| {
                matches!(
                    node.kind,
                    SyntaxKind::BinaryExpression | SyntaxKind::CallExpression
                )
            })
            .map(|(index, _)| get_assignment_declaration_kind(&source, NodeId(index as u32)))
            .filter(|kind| *kind != AssignmentDeclarationKind::None)
            .collect()
    }

    #[test]
    fn assignment_kind_matrix_matches_tsc_6_0_3() {
        let text = "\
exports.x = 1;
module.exports = {};
C.prototype.x = function () {};
this.x = 1;
F.x = 1;
C.prototype = {};
Object.defineProperty(F, \"x\", { value: 1 });
Object.defineProperty(exports, \"x\", { value: 1 });
Object.defineProperty(C.prototype, \"x\", { value: 1 });
";
        assert_eq!(
            kinds(text, true),
            [
                AssignmentDeclarationKind::ExportsProperty,
                AssignmentDeclarationKind::ModuleExports,
                AssignmentDeclarationKind::PrototypeProperty,
                AssignmentDeclarationKind::ThisProperty,
                AssignmentDeclarationKind::Property,
                AssignmentDeclarationKind::Prototype,
                AssignmentDeclarationKind::ObjectDefinePropertyValue,
                AssignmentDeclarationKind::ObjectDefinePropertyExports,
                AssignmentDeclarationKind::ObjectDefinePrototypeProperty,
            ]
        );
        assert_eq!(kinds(text, false), [AssignmentDeclarationKind::Property]);
    }

    #[test]
    fn void_zero_and_dynamic_export_edges_match_tsc() {
        assert!(kinds("F.x = void 0;", true).is_empty());
        assert_eq!(
            kinds("F[key] = 1;", true),
            [AssignmentDeclarationKind::Property]
        );
        assert_eq!(
            kinds("exports[key] = 1;", true),
            [AssignmentDeclarationKind::Property]
        );
    }
}

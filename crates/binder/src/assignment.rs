//! Assignment-declaration classification shared by the binder and checker.
//!
//! This is the non-JSDoc part of tsc's assignment-declaration utility
//! family. Keeping it here avoids the former checker-side TS-only
//! reimplementation drifting from the binder.

use crate::node_util::{
    id_text, is_entity_name_expression, is_left_hand_side_expression,
    is_string_or_numeric_literal_like, kind_of, literal_text_of, name_field_of, node_flags,
    parent_of, skip_parentheses_pub,
};
use crate::symbols::escape_leading_underscores;
use tsc_syntax::{NodeData, NodeId, NodeLookup, SyntaxKind};
use tsc_types::NodeFlags;
use tsc_types::{EscapedName, JsStr, JsString};

/// tsgo JSDeclarationKind (TypeScript 7.1): tsc 6.0's PrototypeProperty,
/// Prototype and ObjectDefinePrototypeProperty kinds are gone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum AssignmentDeclarationKind {
    None = 0,
    ExportsProperty = 1,
    ModuleExports = 2,
    ThisProperty = 4,
    Property = 5,
    ObjectDefinePropertyValue = 7,
    ObjectDefinePropertyExports = 8,
}

/// tsgo-port: GetAssignmentDeclarationKind @7.1 (ast/utilities.go:1547-1579).
///
/// A `=` assignment to an access expression: in JavaScript,
/// `module.exports = x` (unless `x` is `exports`), `exports.p` /
/// `module.exports.p`, and `this.p`; in either language, `E.p` or `E[x]`
/// where `E` is an entity name expression (in JavaScript `this` and
/// literal element accesses count). A JavaScript `Object.defineProperty`
/// call with a bindable target. tsc 6.0's `void 0` exclusion is gone.
pub fn get_assignment_declaration_kind(
    tree: &impl NodeLookup,
    expr: NodeId,
) -> AssignmentDeclarationKind {
    let in_js = node_flags(tree, expr).intersects(NodeFlags::JAVA_SCRIPT_FILE);
    match &tree.node(expr).data() {
        NodeData::BinaryExpression(data) => {
            let (Some(left), Some(operator)) = (data.left, data.operator_token) else {
                return AssignmentDeclarationKind::None;
            };
            if kind_of(tree, operator) != SyntaxKind::EqualsToken {
                return AssignmentDeclarationKind::None;
            }
            let Some(left_expression) = access_expression_of(tree, left) else {
                return AssignmentDeclarationKind::None;
            };
            if in_js {
                if is_module_exports_access_expression(tree, left)
                    && !data
                        .right
                        .is_some_and(|right| is_exports_identifier(tree, right))
                {
                    return AssignmentDeclarationKind::ModuleExports;
                }
                if (is_module_exports_access_expression(tree, left_expression)
                    || is_exports_identifier(tree, left_expression))
                    && get_element_or_property_access_name(tree, left).is_some()
                {
                    return AssignmentDeclarationKind::ExportsProperty;
                }
                if kind_of(tree, left_expression) == SyntaxKind::ThisKeyword {
                    return AssignmentDeclarationKind::ThisProperty;
                }
            }
            let is_property = match &tree.node(left).data() {
                NodeData::PropertyAccessExpression(access) => {
                    access
                        .name
                        .is_some_and(|name| kind_of(tree, name) == SyntaxKind::Identifier)
                        && is_entity_name_expression_ex(tree, left_expression, in_js)
                }
                NodeData::ElementAccessExpression(_) => {
                    is_entity_name_expression_ex(tree, left_expression, in_js)
                }
                _ => false,
            };
            if is_property {
                AssignmentDeclarationKind::Property
            } else {
                AssignmentDeclarationKind::None
            }
        }
        NodeData::CallExpression(data) => {
            if !in_js || !is_bindable_object_define_property_call(tree, expr) {
                return AssignmentDeclarationKind::None;
            }
            let arguments = data.arguments.expect("checked by predicate");
            let entity_name = tree.node_array(arguments).nodes[0];
            if is_exports_identifier(tree, entity_name)
                || is_module_exports_access_expression(tree, entity_name)
            {
                AssignmentDeclarationKind::ObjectDefinePropertyExports
            } else {
                AssignmentDeclarationKind::ObjectDefinePropertyValue
            }
        }
        _ => AssignmentDeclarationKind::None,
    }
}

/// tsgo-port: IsEntityNameExpressionEx @7.1 (ast/utilities.go:1624-1636):
/// an identifier or a dotted identifier chain; with `allow_js`, also
/// `this` and string/numeric literal element accesses.
pub fn is_entity_name_expression_ex(tree: &impl NodeLookup, node: NodeId, allow_js: bool) -> bool {
    match &tree.node(node).data() {
        NodeData::Identifier(_) => true,
        NodeData::PropertyAccessExpression(data) => {
            data.name
                .is_some_and(|name| kind_of(tree, name) == SyntaxKind::Identifier)
                && data.expression.is_some_and(|expression| {
                    is_entity_name_expression_ex(tree, expression, allow_js)
                })
        }
        NodeData::ElementAccessExpression(data) => {
            allow_js
                && data
                    .argument_expression
                    .is_some_and(|argument| is_string_or_numeric_literal_like(tree, argument))
                && data.expression.is_some_and(|expression| {
                    is_entity_name_expression_ex(tree, expression, allow_js)
                })
        }
        _ => allow_js && kind_of(tree, node) == SyntaxKind::ThisKeyword,
    }
}

/// The name node used by binder `declareSymbol` for non-JSDoc
/// assignment declarations. Kept separate from the checker-facing
/// general declaration-name utility until 9.8b activates the consumers.
pub fn get_assignment_declaration_name(
    tree: &impl NodeLookup,
    declaration: NodeId,
) -> Option<NodeId> {
    if matches!(
        kind_of(tree, declaration),
        SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
    ) {
        let parent = parent_of(tree, declaration)?;
        if matches!(
            &tree.node(parent).data(),
            NodeData::BinaryExpression(data) if data.left == Some(declaration)
        ) && get_assignment_declaration_kind(tree, parent) != AssignmentDeclarationKind::None
        {
            return get_element_or_property_access_argument_expression_or_name(tree, declaration);
        }
        return None;
    }
    match get_assignment_declaration_kind(tree, declaration) {
        AssignmentDeclarationKind::ExportsProperty
        | AssignmentDeclarationKind::ThisProperty
        | AssignmentDeclarationKind::Property => {
            let NodeData::BinaryExpression(data) = &tree.node(declaration).data() else {
                return None;
            };
            get_element_or_property_access_argument_expression_or_name(tree, data.left?)
        }
        AssignmentDeclarationKind::ObjectDefinePropertyValue
        | AssignmentDeclarationKind::ObjectDefinePropertyExports => {
            let NodeData::CallExpression(data) = &tree.node(declaration).data() else {
                return None;
            };
            let arguments = data.arguments?;
            tree.node_array(arguments).nodes.get(1).copied()
        }
        _ => None,
    }
}

pub fn get_assignment_declaration_property_access_kind(
    tree: &impl NodeLookup,
    lhs: NodeId,
) -> AssignmentDeclarationKind {
    let Some(lhs_expression) = access_expression_of(tree, lhs) else {
        return AssignmentDeclarationKind::None;
    };
    if kind_of(tree, lhs_expression) == SyntaxKind::ThisKeyword {
        return AssignmentDeclarationKind::ThisProperty;
    }
    if is_module_exports_access_expression(tree, lhs) {
        return AssignmentDeclarationKind::ModuleExports;
    }
    if is_bindable_static_name_expression(tree, lhs_expression, true) {
        // tsgo (TypeScript 7.1) has no PrototypeProperty kind: `F.prototype.m
        // = x` is a plain Property assignment on the entity `F.prototype`.
        let mut next_to_last = lhs;
        while let Some(expression) = access_expression_of(tree, next_to_last) {
            if kind_of(tree, expression) == SyntaxKind::Identifier {
                break;
            }
            next_to_last = expression;
        }
        let id = access_expression_of(tree, next_to_last).unwrap_or(next_to_last);
        let id_escaped = match &tree.node(id).data() {
            NodeData::Identifier(data) => Some(data.escaped_text.identifier_text()),
            _ => None,
        };
        if (id_escaped == Some("exports")
            || id_escaped == Some("module")
                && get_element_or_property_access_name(tree, next_to_last)
                    .is_some_and(|name| name == "exports"))
            && is_bindable_static_access_expression(tree, lhs, false)
        {
            return AssignmentDeclarationKind::ExportsProperty;
        }
        if is_bindable_static_name_expression(tree, lhs, true)
            || kind_of(tree, lhs) == SyntaxKind::ElementAccessExpression
                && get_element_or_property_access_name(tree, lhs).is_none()
        {
            return AssignmentDeclarationKind::Property;
        }
    }
    AssignmentDeclarationKind::None
}

pub fn is_bindable_object_define_property_call(tree: &impl NodeLookup, expr: NodeId) -> bool {
    let NodeData::CallExpression(data) = &tree.node(expr).data() else {
        return false;
    };
    let Some(arguments) = data.arguments else {
        return false;
    };
    let arguments = &tree.node_array(arguments).nodes;
    if arguments.len() != 3 {
        return false;
    }
    let Some(expression) = data.expression else {
        return false;
    };
    let NodeData::PropertyAccessExpression(access) = &tree.node(expression).data() else {
        return false;
    };
    access
        .expression
        .is_some_and(|object| id_text(tree, object) == Some("Object"))
        && access
            .name
            .is_some_and(|name| id_text(tree, name) == Some("defineProperty"))
        && is_string_or_numeric_literal_like(tree, arguments[1])
        && is_bindable_static_name_expression(tree, arguments[0], true)
}

/// The `.expression` of an access expression.
pub fn access_expression_of(tree: &impl NodeLookup, node: NodeId) -> Option<NodeId> {
    match &tree.node(node).data() {
        NodeData::PropertyAccessExpression(data) => data.expression,
        NodeData::ElementAccessExpression(data) => data.expression,
        _ => None,
    }
}

pub fn is_exports_identifier(tree: &impl NodeLookup, node: NodeId) -> bool {
    matches!(
        &tree.node(node).data(),
        NodeData::Identifier(data) if data.escaped_text == "exports"
    )
}

pub fn is_module_exports_access_expression(tree: &impl NodeLookup, node: NodeId) -> bool {
    let is_access = matches!(kind_of(tree, node), SyntaxKind::PropertyAccessExpression)
        || is_literal_like_element_access(tree, node);
    is_access
        && access_expression_of(tree, node).is_some_and(|expression| {
            matches!(
                &tree.node(expression).data(),
                NodeData::Identifier(data) if data.escaped_text == "module"
            )
        })
        && get_element_or_property_access_name(tree, node).is_some_and(|name| name == "exports")
}

pub fn is_literal_like_element_access(tree: &impl NodeLookup, node: NodeId) -> bool {
    matches!(
        &tree.node(node).data(),
        NodeData::ElementAccessExpression(data)
            if data.argument_expression.is_some_and(|argument| {
                is_string_or_numeric_literal_like(tree, argument)
            })
    )
}

pub fn is_bindable_static_access_expression(
    tree: &impl NodeLookup,
    node: NodeId,
    exclude_this_keyword: bool,
) -> bool {
    if let NodeData::PropertyAccessExpression(data) = &tree.node(node).data() {
        let this_ok = !exclude_this_keyword
            && data
                .expression
                .is_some_and(|expression| kind_of(tree, expression) == SyntaxKind::ThisKeyword);
        let static_ok = data
            .name
            .is_some_and(|name| kind_of(tree, name) == SyntaxKind::Identifier)
            && data.expression.is_some_and(|expression| {
                is_bindable_static_name_expression(tree, expression, true)
            });
        if this_ok || static_ok {
            return true;
        }
    }
    is_bindable_static_element_access_expression(tree, node, exclude_this_keyword)
}

pub fn is_bindable_static_element_access_expression(
    tree: &impl NodeLookup,
    node: NodeId,
    exclude_this_keyword: bool,
) -> bool {
    is_literal_like_element_access(tree, node)
        && access_expression_of(tree, node).is_some_and(|expression| {
            !exclude_this_keyword && kind_of(tree, expression) == SyntaxKind::ThisKeyword
                || is_entity_name_expression(tree, expression)
                || is_bindable_static_access_expression(tree, expression, true)
        })
}

pub fn is_bindable_static_name_expression(
    tree: &impl NodeLookup,
    node: NodeId,
    exclude_this_keyword: bool,
) -> bool {
    is_entity_name_expression(tree, node)
        || is_bindable_static_access_expression(tree, node, exclude_this_keyword)
}

pub fn get_element_or_property_access_argument_expression_or_name(
    tree: &impl NodeLookup,
    node: NodeId,
) -> Option<NodeId> {
    match &tree.node(node).data() {
        NodeData::PropertyAccessExpression(data) => data.name,
        NodeData::ElementAccessExpression(data) => {
            let argument = skip_parentheses_pub(tree, data.argument_expression?);
            if is_string_or_numeric_literal_like(tree, argument) {
                Some(argument)
            } else {
                Some(node)
            }
        }
        _ => None,
    }
}

pub fn get_element_or_property_access_name(
    tree: &impl NodeLookup,
    node: NodeId,
) -> Option<EscapedName> {
    let name = get_element_or_property_access_argument_expression_or_name(tree, node)?;
    match &tree.node(name).data() {
        NodeData::Identifier(data) => Some(data.escaped_text),
        _ if is_string_or_numeric_literal_like(tree, name) => {
            literal_text_of(tree, name).map(escape_leading_underscores)
        }
        _ => None,
    }
}

/// tsgo-port: ast.GetRightMostAssignedExpression @7.1 (ast/utilities.go:125-130):
/// compound assignments (`??=`, `+=`, …) are followed too.
pub fn get_right_most_assigned_expression(tree: &impl NodeLookup, mut node: NodeId) -> NodeId {
    loop {
        let NodeData::BinaryExpression(data) = &tree.node(node).data() else {
            return node;
        };
        let is_assignment = data
            .operator_token
            .is_some_and(|token| crate::node_util::is_assignment_operator(kind_of(tree, token)))
            && data
                .left
                .is_some_and(|left| is_left_hand_side_expression(tree, left));
        if !is_assignment {
            return node;
        }
        match data.right {
            Some(right) => node = right,
            None => return node,
        }
    }
}

pub fn get_initializer_of_binary_expression(tree: &impl NodeLookup, mut expr: NodeId) -> NodeId {
    loop {
        let NodeData::BinaryExpression(data) = &tree.node(expr).data() else {
            return expr;
        };
        match data.right {
            Some(right) if matches!(&tree.node(right).data(), NodeData::BinaryExpression(_)) => {
                expr = right;
            }
            Some(right) => return right,
            None => return expr,
        }
    }
}

pub fn is_prototype_access(tree: &impl NodeLookup, node: NodeId) -> bool {
    is_bindable_static_access_expression(tree, node, false)
        && get_element_or_property_access_name(tree, node).is_some_and(|name| name == "prototype")
}

/// tsc-port: getEffectiveInitializer/getDeclaredExpandoInitializer @6.0.3
/// tsc-hash: eca11ccf0f981170a15e543924cf9f73ada6af69a51a889bd5845460204a2db9
/// tsc-span: _tsc.js:14967-14976
pub fn get_declared_expando_initializer(tree: &impl NodeLookup, node: NodeId) -> Option<NodeId> {
    let initializer = match &tree.node(node).data() {
        NodeData::VariableDeclaration(data) => data.initializer,
        NodeData::Parameter(data) => data.initializer,
        NodeData::BindingElement(data) => data.initializer,
        NodeData::PropertyDeclaration(data) => data.initializer,
        NodeData::PropertyAssignment(data) => data.initializer,
        NodeData::EnumMember(data) => data.initializer,
        _ => None,
    }?;
    let effective = if node_flags(tree, initializer).intersects(NodeFlags::JAVA_SCRIPT_FILE) {
        match &tree.node(initializer).data() {
            NodeData::BinaryExpression(data)
                if data.operator_token.is_some_and(|token| {
                    matches!(
                        kind_of(tree, token),
                        SyntaxKind::BarBarToken | SyntaxKind::QuestionQuestionToken
                    )
                }) && name_field_of(tree, node).is_some_and(|name| {
                    is_entity_name_expression(tree, name)
                        && data
                            .left
                            .is_some_and(|left| is_same_entity_name(tree, name, left))
                }) =>
            {
                data.right?
            }
            _ => initializer,
        }
    } else {
        initializer
    };
    let name = name_field_of(tree, node)?;
    get_expando_initializer(tree, effective, is_prototype_access(tree, name))
}

/// tsc getAssignedExpandoInitializer / getExpandoInitializer family.
///
/// This is syntax-only and shared with the checker trampoline: for an
/// assignment LHS it returns the expando initializer on the enclosing
/// `=`, and for Object.defineProperty it returns a bindable `value`
/// initializer.
pub fn get_assigned_expando_initializer(tree: &impl NodeLookup, node: NodeId) -> Option<NodeId> {
    if let Some(parent) = parent_of(tree, node) {
        if let NodeData::BinaryExpression(data) = &tree.node(parent).data() {
            let is_equals = data
                .operator_token
                .is_some_and(|token| kind_of(tree, token) == SyntaxKind::EqualsToken);
            if is_equals {
                let name = data.left?;
                let initializer = data.right?;
                let is_prototype_assignment = is_prototype_access(tree, name);
                return get_expando_initializer(tree, initializer, is_prototype_assignment)
                    .or_else(|| {
                        get_defaulted_expando_initializer(
                            tree,
                            name,
                            initializer,
                            is_prototype_assignment,
                        )
                    });
            }
        }
    }

    if !is_bindable_object_define_property_call(tree, node) {
        return None;
    }
    let NodeData::CallExpression(data) = &tree.node(node).data() else {
        return None;
    };
    let arguments = data.arguments?;
    let arguments = &tree.node_array(arguments).nodes;
    let descriptor = *arguments.get(2)?;
    let NodeData::ObjectLiteralExpression(data) = &tree.node(descriptor).data() else {
        return None;
    };
    let properties = data.properties?;
    let initializer = tree
        .node_array(properties)
        .nodes
        .iter()
        .find_map(|&property| match &tree.node(property).data() {
            NodeData::PropertyAssignment(data)
                if data
                    .name
                    .is_some_and(|name| id_text(tree, name) == Some("value")) =>
            {
                data.initializer
            }
            _ => None,
        })?;
    let is_prototype_assignment =
        literal_text_of(tree, *arguments.get(1)?).is_some_and(|text| text == "prototype");
    get_expando_initializer(tree, initializer, is_prototype_assignment)
}

pub fn get_expando_initializer(
    tree: &impl NodeLookup,
    initializer: NodeId,
    is_prototype_assignment: bool,
) -> Option<NodeId> {
    match &tree.node(initializer).data() {
        NodeData::CallExpression(data) => {
            let expression = skip_parentheses_pub(tree, data.expression?);
            matches!(
                kind_of(tree, expression),
                SyntaxKind::FunctionExpression | SyntaxKind::ArrowFunction
            )
            .then_some(initializer)
        }
        NodeData::FunctionExpression(_)
        | NodeData::ClassExpression(_)
        | NodeData::ArrowFunction(_) => Some(initializer),
        NodeData::ObjectLiteralExpression(data)
            if is_prototype_assignment
                || data
                    .properties
                    .is_none_or(|properties| tree.node_array(properties).nodes.is_empty()) =>
        {
            Some(initializer)
        }
        _ => None,
    }
}

fn get_defaulted_expando_initializer(
    tree: &impl NodeLookup,
    name: NodeId,
    initializer: NodeId,
    is_prototype_assignment: bool,
) -> Option<NodeId> {
    let NodeData::BinaryExpression(data) = &tree.node(initializer).data() else {
        return None;
    };
    let is_defaulting = data.operator_token.is_some_and(|token| {
        matches!(
            kind_of(tree, token),
            SyntaxKind::BarBarToken | SyntaxKind::QuestionQuestionToken
        )
    });
    let (Some(left), Some(right)) = (data.left, data.right) else {
        return None;
    };
    if !is_defaulting || !is_same_entity_name(tree, name, left) {
        return None;
    }
    get_expando_initializer(tree, right, is_prototype_assignment)
}

pub fn is_same_entity_name(tree: &impl NodeLookup, name: NodeId, initializer: NodeId) -> bool {
    let name_text = identifier_or_literal_text(tree, name);
    let initializer_text = identifier_or_literal_text(tree, initializer);
    if name_text.is_some() && name_text == initializer_text {
        return true;
    }

    if name_text.is_some() {
        if let Some(receiver) = access_expression_of(tree, initializer) {
            let is_global_receiver = kind_of(tree, receiver) == SyntaxKind::ThisKeyword
                || id_text(tree, receiver)
                    .is_some_and(|text| matches!(text, "window" | "self" | "global"));
            if is_global_receiver {
                return get_element_or_property_access_argument_expression_or_name(
                    tree,
                    initializer,
                )
                .is_some_and(|initializer_name| is_same_entity_name(tree, name, initializer_name));
            }
        }
    }

    let (Some(name_expression), Some(initializer_expression)) = (
        access_expression_of(tree, name),
        access_expression_of(tree, initializer),
    ) else {
        return false;
    };
    let (Some(name_property), Some(initializer_property)) = (
        get_element_or_property_access_name(tree, name),
        get_element_or_property_access_name(tree, initializer),
    ) else {
        return false;
    };
    name_property == initializer_property
        && is_same_entity_name(tree, name_expression, initializer_expression)
}

fn identifier_or_literal_text(tree: &impl NodeLookup, node: NodeId) -> Option<JsString> {
    id_text(tree, node).map(JsString::from).or_else(|| {
        if is_string_or_numeric_literal_like(tree, node) {
            literal_text_of(tree, node).map(JsStr::to_owned)
        } else {
            None
        }
    })
}

#[cfg(test)]
#[path = "../tests/unit/assignment/tests.rs"]
mod tests;

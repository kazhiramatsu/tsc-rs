//! The assignment-pattern conversion of tsc's `destructuring.ts` family
//! (`_tsc.js:93251-93697`): the top-level `using` hoist of `es_next.rs`
//! rewrites a binding pattern into an assignment pattern. The flattener
//! itself went with the ES2015 transform (TypeScript 7 has no ES5 target);
//! object rest/spread lowering is the plan-based lowering in `es2018.rs`.

use tsc_syntax::{NodeData, SyntaxKind};

use crate::{
    InternalEmitFlags, SourceRange, TransformError, TransformFlags, TransformNode,
    TransformSourceId, TransformationContext,
};

/// Binding-pattern leaves normally retain their existing identity. The
/// top-level using hoist emits them as runtime assignment references; clone
/// only those leaves so their parsed BindingElement parent cannot suppress
/// module substitution. Generic assignment flattening keeps its old role.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AssignmentTargetRole {
    Existing,
    ExpressionReference,
}

// ---------------------------------------------------------------------------
// The pattern constructors (the two closure sets)
// ---------------------------------------------------------------------------

/// tsc-port: makeArrayAssignmentPattern @6.0.3
/// tsc-hash: f1bd32dfcb0316067e434c2091cb8d51614bf8f81baa20e7e753a53bb91a297d
/// tsc-span: _tsc.js:93677-93680
fn make_array_assignment_pattern(
    context: &mut TransformationContext,
    source: TransformSourceId,
    elements: Vec<TransformNode>,
    role: AssignmentTargetRole,
) -> Result<TransformNode, TransformError> {
    let mut converted = Vec::with_capacity(elements.len());
    for element in elements {
        converted.push(convert_to_array_assignment_element(
            context, source, element, role,
        )?);
    }
    let elements = context.factory()?.create_node_array(source, converted)?;
    let flags = context.arena().array_transform_flags(elements);
    context.factory()?.create_node(
        source,
        NodeData::ArrayLiteralExpression(tsc_syntax::nodes::ArrayLiteralExpressionData {
            elements: Some(elements.array()),
        }),
        flags,
    )
}

/// tsc-port: makeObjectAssignmentPattern @6.0.3
/// tsc-hash: b4b3a87526d8fe5aa891460fdb6d316503a622c7ddbab26cff50af715c91fe87
/// tsc-span: _tsc.js:93685-93688
fn make_object_assignment_pattern(
    context: &mut TransformationContext,
    source: TransformSourceId,
    elements: Vec<TransformNode>,
    role: AssignmentTargetRole,
) -> Result<TransformNode, TransformError> {
    let mut converted = Vec::with_capacity(elements.len());
    for element in elements {
        converted.push(convert_to_object_assignment_element(
            context, source, element, role,
        )?);
    }
    let elements = context.factory()?.create_node_array(source, converted)?;
    let flags = context.arena().array_transform_flags(elements);
    context.factory()?.create_node(
        source,
        NodeData::ObjectLiteralExpression(tsc_syntax::nodes::ObjectLiteralExpressionData {
            properties: Some(elements.array()),
        }),
        flags,
    )
}

// ---------------------------------------------------------------------------
// The node converters (factory.converters, reached from the two
// make*AssignmentPattern constructors)
// ---------------------------------------------------------------------------

/// tsc-port: convertToArrayAssignmentElement @6.0.3
/// tsc-hash: a53d9e38c4ae559cf9c2cd29ef7b8bb1f189b05f53f47ca88e0ca5228c1c23bb
/// tsc-span: _tsc.js:20716-20732
///
/// tsgo convertBindingElementToArrayAssignmentElement (#64422): a rest
/// element's name is converted like any element's, so a nested binding
/// pattern becomes an assignment pattern (tsc asserted an identifier).
fn convert_to_array_assignment_element(
    context: &mut TransformationContext,
    source: TransformSourceId,
    element: TransformNode,
    role: AssignmentTargetRole,
) -> Result<TransformNode, TransformError> {
    if let NodeData::BindingElement(data) = context.arena().node(element)?.data.clone() {
        let name = data
            .name
            .and_then(|name| context.arena().node_ref(source, name))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::BindingElement,
                field: "name",
            })?;
        let expression = convert_to_assignment_element_target(context, source, name, role)?;
        if data.dot_dot_dot_token.is_some() {
            let name = expression;
            let flags = context.arena().propagate_child_flags(name)?
                | TransformFlags::CONTAINS_ES_2015
                | TransformFlags::CONTAINS_REST_OR_SPREAD;
            let spread = context.factory()?.create_node(
                source,
                NodeData::SpreadElement(tsc_syntax::nodes::SpreadElementData {
                    expression: Some(name.node()),
                }),
                flags,
            )?;
            return with_original_and_range(context, spread, element);
        }
        return match data.initializer {
            Some(initializer) => {
                let initializer = context.arena().node_ref(source, initializer).ok_or(
                    TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::BindingElement,
                        field: "initializer",
                    },
                )?;
                let assignment =
                    create_assignment_in_context(context, source, expression, initializer)?;
                with_original_and_range(context, assignment, element)
            }
            None => Ok(expression),
        };
    }
    require_not_binding_shape(context, element, "array assignment element")?;
    Ok(element)
}

/// tsc-port: convertToObjectAssignmentElement @6.0.3
/// tsc-hash: ceda6ad456fc04784db6d09c68f51944a485356b200901d4289907922e469b09
/// tsc-span: _tsc.js:20733-20747
fn convert_to_object_assignment_element(
    context: &mut TransformationContext,
    source: TransformSourceId,
    element: TransformNode,
    role: AssignmentTargetRole,
) -> Result<TransformNode, TransformError> {
    if let NodeData::BindingElement(data) = context.arena().node(element)?.data.clone() {
        let name = data
            .name
            .and_then(|name| context.arena().node_ref(source, name))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::BindingElement,
                field: "name",
            })?;
        if data.dot_dot_dot_token.is_some() {
            require_identifier(context, name)?;
            let name = assignment_reference_target(context, name, role)?;
            let flags = context.arena().propagate_child_flags(name)?
                | TransformFlags::CONTAINS_ES_2018
                | TransformFlags::CONTAINS_OBJECT_REST_OR_SPREAD;
            let spread = context.factory()?.create_node(
                source,
                NodeData::SpreadAssignment(tsc_syntax::nodes::SpreadAssignmentData {
                    expression: Some(name.node()),
                }),
                flags,
            )?;
            return with_original_and_range(context, spread, element);
        }
        if let Some(property_name) = data.property_name {
            let property_name = context.arena().node_ref(source, property_name).ok_or(
                TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::BindingElement,
                    field: "property name",
                },
            )?;
            let expression = convert_to_assignment_element_target(context, source, name, role)?;
            let initializer = match data.initializer {
                Some(initializer) => {
                    let initializer = context.arena().node_ref(source, initializer).ok_or(
                        TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::BindingElement,
                            field: "initializer",
                        },
                    )?;
                    create_assignment_in_context(context, source, expression, initializer)?
                }
                None => expression,
            };
            let flags = {
                let arena = context.arena();
                arena.propagate_child_flags(property_name)?
                    | arena.propagate_child_flags(initializer)?
            };
            let assignment = context.factory()?.create_node(
                source,
                NodeData::PropertyAssignment(tsc_syntax::nodes::PropertyAssignmentData {
                    name: Some(property_name.node()),
                    initializer: Some(initializer.node()),
                    modifiers: None,
                    question_token: None,
                    exclamation_token: None,
                }),
                flags,
            )?;
            return with_original_and_range(context, assignment, element);
        }
        require_identifier(context, name)?;
        let name = assignment_reference_target(context, name, role)?;
        let initializer = data
            .initializer
            .and_then(|initializer| context.arena().node_ref(source, initializer));
        let flags = {
            let arena = context.arena();
            let mut flags = arena.propagate_child_flags(name)? | TransformFlags::CONTAINS_ES_2015;
            if let Some(initializer) = initializer {
                flags |= arena.propagate_child_flags(initializer)?;
            }
            flags
        };
        let shorthand = context.factory()?.create_node(
            source,
            NodeData::ShorthandPropertyAssignment(
                tsc_syntax::nodes::ShorthandPropertyAssignmentData {
                    name: Some(name.node()),
                    equals_token: None,
                    object_assignment_initializer: initializer.map(TransformNode::node),
                    modifiers: None,
                    question_token: None,
                    exclamation_token: None,
                },
            ),
            flags,
        )?;
        return with_original_and_range(context, shorthand, element);
    }
    require_not_binding_shape(context, element, "object assignment element")?;
    Ok(element)
}

/// tsc-port: convertToAssignmentPattern @6.0.3
/// tsc-hash: 63b77454be5b3cb32c2723a1990da4cb9ac84663c7182dcb8c955302aba5b6a9
/// tsc-span: _tsc.js:20748-20757
pub(super) fn convert_to_assignment_pattern(
    context: &mut TransformationContext,
    source: TransformSourceId,
    node: TransformNode,
    role: AssignmentTargetRole,
) -> Result<TransformNode, TransformError> {
    match context.arena().node(node)?.kind {
        SyntaxKind::ArrayBindingPattern | SyntaxKind::ArrayLiteralExpression => {
            convert_to_array_assignment_pattern(context, source, node, role)
        }
        SyntaxKind::ObjectBindingPattern | SyntaxKind::ObjectLiteralExpression => {
            convert_to_object_assignment_pattern(context, source, node, role)
        }
        kind => Err(TransformError::RequiredChildRemoved {
            parent: kind,
            field: "assignment pattern",
        }),
    }
}

/// tsc-port: convertToObjectAssignmentPattern @6.0.3
/// tsc-hash: e6f6e3aa9e2039d2937eea5b5032475643399a980d61a32b504b0a33fd4b9f0d
/// tsc-span: _tsc.js:20758-20769
fn convert_to_object_assignment_pattern(
    context: &mut TransformationContext,
    source: TransformSourceId,
    node: TransformNode,
    role: AssignmentTargetRole,
) -> Result<TransformNode, TransformError> {
    match context.arena().node(node)?.kind {
        SyntaxKind::ObjectBindingPattern => {
            let NodeData::ObjectBindingPattern(data) = context.arena().node(node)?.data.clone()
            else {
                unreachable!("kind checked above");
            };
            let elements = array_nodes_in_context(context, source, data.elements)?;
            let literal = make_object_assignment_pattern(context, source, elements, role)?;
            with_original_and_range(context, literal, node)
        }
        SyntaxKind::ObjectLiteralExpression => Ok(node),
        kind => Err(TransformError::RequiredChildRemoved {
            parent: kind,
            field: "object assignment pattern",
        }),
    }
}

/// tsc-port: convertToArrayAssignmentPattern @6.0.3
/// tsc-hash: 395755484303c41fbecc2ff8af6fc8509f4b4480fe1bb957772acec107f002ed
/// tsc-span: _tsc.js:20770-20781
fn convert_to_array_assignment_pattern(
    context: &mut TransformationContext,
    source: TransformSourceId,
    node: TransformNode,
    role: AssignmentTargetRole,
) -> Result<TransformNode, TransformError> {
    match context.arena().node(node)?.kind {
        SyntaxKind::ArrayBindingPattern => {
            let NodeData::ArrayBindingPattern(data) = context.arena().node(node)?.data.clone()
            else {
                unreachable!("kind checked above");
            };
            let elements = array_nodes_in_context(context, source, data.elements)?;
            let literal = make_array_assignment_pattern(context, source, elements, role)?;
            with_original_and_range(context, literal, node)
        }
        SyntaxKind::ArrayLiteralExpression => Ok(node),
        kind => Err(TransformError::RequiredChildRemoved {
            parent: kind,
            field: "array assignment pattern",
        }),
    }
}

/// tsc-port: convertToAssignmentElementTarget @6.0.3
/// tsc-hash: d2b8e01bb66232d9ecebbf2b7b8874357bfaf509cc0423b7ed4655c545ab2e0c
/// tsc-span: _tsc.js:20782-20787
fn convert_to_assignment_element_target(
    context: &mut TransformationContext,
    source: TransformSourceId,
    node: TransformNode,
    role: AssignmentTargetRole,
) -> Result<TransformNode, TransformError> {
    if matches!(
        context.arena().node(node)?.kind,
        SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern
    ) {
        return convert_to_assignment_pattern(context, source, node, role);
    }
    require_not_binding_shape(context, node, "assignment element target")?;
    assignment_reference_target(context, node, role)
}

fn assignment_reference_target(
    context: &mut TransformationContext,
    name: TransformNode,
    role: AssignmentTargetRole,
) -> Result<TransformNode, TransformError> {
    if role == AssignmentTargetRole::Existing
        || context.arena().node(name)?.kind != SyntaxKind::Identifier
        || context
            .arena()
            .metadata(name)
            .and_then(|m| m.generated_binding_id())
            .is_some()
    {
        return Ok(name);
    }
    let original = context.arena().get_original_node(name);
    let record = context.arena().node(original)?;
    let positions = context
        .arena()
        .source(original.source())?
        .syntax()
        .positions();
    let range = SourceRange::from_raw(record.pos, record.end, positions).map_err(|error| {
        TransformError::InvalidSourceRange {
            node: original,
            error,
        }
    })?;
    if matches!(range, SourceRange::Synthesized) {
        return Ok(name);
    }
    // factory converters reuse the binding name, including its emit flags.
    // Our explicit reference role belongs to this clone, never the parse node.
    let clone = context.factory()?.clone_node(name)?;
    let clone = with_original_and_range(context, clone, name)?;
    let metadata = context.arena_mut()?.metadata_mut(clone);
    metadata.set_internal_flags(InternalEmitFlags::from_bits(
        metadata.internal_flags().bits() | InternalEmitFlags::DECLARATION_NAME_REFERENCE.bits(),
    ));
    Ok(clone)
}

// ---------------------------------------------------------------------------
// Construction plumbing (the es2018 idioms over the shared factory)
// ---------------------------------------------------------------------------

fn require_identifier(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<(), TransformError> {
    let kind = context.arena().node(node)?.kind;
    if kind != SyntaxKind::Identifier {
        return Err(TransformError::RequiredChildRemoved {
            parent: kind,
            field: "identifier",
        });
    }
    Ok(())
}

fn require_not_binding_shape(
    context: &TransformationContext,
    node: TransformNode,
    field: &'static str,
) -> Result<(), TransformError> {
    let kind = context.arena().node(node)?.kind;
    if matches!(
        kind,
        SyntaxKind::BindingElement
            | SyntaxKind::ObjectBindingPattern
            | SyntaxKind::ArrayBindingPattern
    ) {
        return Err(TransformError::RequiredChildRemoved {
            parent: kind,
            field,
        });
    }
    Ok(())
}

fn with_original_and_range(
    context: &mut TransformationContext,
    node: TransformNode,
    original: TransformNode,
) -> Result<TransformNode, TransformError> {
    context
        .arena_mut()?
        .set_original_node(node, Some(original))?;
    context.factory()?.set_text_range(node, original)?;
    Ok(node)
}

fn array_nodes_in_context(
    context: &TransformationContext,
    source: TransformSourceId,
    array: Option<tsc_syntax::NodeArrayId>,
) -> Result<Vec<TransformNode>, TransformError> {
    let Some(array) = array.and_then(|array| context.arena().node_array_ref(source, array)) else {
        return Ok(Vec::new());
    };
    let nodes = context.arena().node_array(array)?.nodes.to_vec();
    nodes
        .iter()
        .map(|node| {
            context
                .arena()
                .node_ref(source, *node)
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::SyntaxList,
                    field: "array element",
                })
        })
        .collect()
}

fn create_assignment_in_context(
    context: &mut TransformationContext,
    source: TransformSourceId,
    left: TransformNode,
    right: TransformNode,
) -> Result<TransformNode, TransformError> {
    create_binary_in_context(context, source, left, SyntaxKind::EqualsToken, right)
}

fn create_binary_in_context(
    context: &mut TransformationContext,
    source: TransformSourceId,
    left: TransformNode,
    operator: SyntaxKind,
    right: TransformNode,
) -> Result<TransformNode, TransformError> {
    let operator_kind = operator;
    let operator = context
        .factory()?
        .create_token(source, operator, TransformFlags::NONE)?;
    let mut flags =
        [left, operator, right]
            .into_iter()
            .try_fold(TransformFlags::NONE, |flags, node| {
                context
                    .arena()
                    .propagate_child_flags(node)
                    .map(|child| flags | child)
            })?;
    // `createBinaryExpression`'s EqualsToken facet arms
    // (_tsc.js:22794-22801): a literal-pattern left marks the
    // destructuring assignment for the downstream owners.
    if operator_kind == SyntaxKind::EqualsToken {
        let left_kind = context.arena().node(left)?.kind;
        let pattern_flags = if context
            .arena()
            .transform_flags(left)
            .contains(TransformFlags::CONTAINS_OBJECT_REST_OR_SPREAD)
        {
            TransformFlags::CONTAINS_OBJECT_REST_OR_SPREAD
        } else {
            TransformFlags::NONE
        };
        if left_kind == SyntaxKind::ObjectLiteralExpression {
            flags |= TransformFlags::CONTAINS_ES_2015
                | TransformFlags::CONTAINS_ES_2018
                | TransformFlags::CONTAINS_DESTRUCTURING_ASSIGNMENT
                | pattern_flags;
        } else if left_kind == SyntaxKind::ArrayLiteralExpression {
            flags |= TransformFlags::CONTAINS_ES_2015
                | TransformFlags::CONTAINS_DESTRUCTURING_ASSIGNMENT
                | pattern_flags;
        }
    }
    context.factory()?.create_node(
        source,
        NodeData::BinaryExpression(tsc_syntax::nodes::BinaryExpressionData {
            left: Some(left.node()),
            operator_token: Some(operator.node()),
            right: Some(right.node()),
        }),
        flags,
    )
}

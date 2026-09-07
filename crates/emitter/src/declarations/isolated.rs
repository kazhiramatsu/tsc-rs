//! Diagnostic selection for isolated declaration inference callbacks.

use tsc_diagnostics::{gen as d, Diagnostic, DiagnosticMessage, RelatedInfo};
use tsc_syntax::{NodeData, NodeId, SourceFile, SyntaxKind};

use crate::{TransformError, UnsupportedEmitFeature};

use super::diagnostics::{diagnostic_for_source_node, name_of_declaration};
use super::ensure::is_function_like;
use super::tracker::text_of_node;

/// tsc-port: createGetIsolatedDeclarationErrors @6.0.3
/// tsc-hash: b3e7aef2bd5f93d15e0b3987570eae6e9e2e3bd63234699fde0320e8eb7e0326
/// tsc-span: _tsc.js:114054-114246
/// Accessor diagnostics use callback-projected symbol declarations in the
/// tracker, as does the entity-in-type classification from existing checker predicates.
pub(crate) fn inference_error(
    source: &SourceFile,
    node: NodeId,
    parameter_add_undefined: Option<bool>,
) -> Result<Diagnostic, TransformError> {
    let record = source.arena.node(node);
    if is_in_heritage_clause(source, node) {
        return Ok(diagnostic_for_source_node(
            source,
            node,
            &d::Extends_clause_can_t_contain_an_expression_with_isolatedDeclarations,
            &[],
        ));
    }
    match &record.data {
        NodeData::GetAccessor(_) | NodeData::SetAccessor(_) => unsupported(),
        NodeData::Parameter(data) => {
            if let Some(add_undefined) = parameter_add_undefined {
                return parameter_error(source, node, add_undefined);
            }
            if data.r#type.is_some()
                || data.modifiers.is_some()
                || record
                    .parent
                    .is_some_and(|parent| source.arena.node(parent).kind == SyntaxKind::SetAccessor)
            {
                return unsupported();
            }
            if let Some(initializer) = data.initializer {
                return expression_error(source, initializer, None);
            }
            declaration_error(source, node)
        }
        NodeData::VariableDeclaration(_) | NodeData::PropertyDeclaration(_) => {
            declaration_error(source, node)
        }
        NodeData::BindingElement(_) => Ok(diagnostic_for_source_node(
            source,
            node,
            &d::Binding_elements_can_t_be_exported_directly_with_isolatedDeclarations,
            &[],
        )),
        NodeData::PropertyAssignment(data) => expression_error(
            source,
            data.initializer
                .ok_or_else(|| contract("property assignment has no initializer"))?,
            None,
        ),
        NodeData::ClassExpression(_) => expression_error(
            source,
            node,
            Some(&d::Inference_from_class_expressions_is_not_supported_with_isolatedDeclarations),
        ),
        _ if matches!(
            record.kind,
            SyntaxKind::ComputedPropertyName
                | SyntaxKind::ShorthandPropertyAssignment
                | SyntaxKind::SpreadAssignment
                | SyntaxKind::ArrayLiteralExpression
                | SyntaxKind::SpreadElement
        ) =>
        {
            let mut diagnostic =
                diagnostic_for_source_node(source, node, error_message(record.kind)?, &[]);
            add_parent_suggestion(source, node, &mut diagnostic)?;
            Ok(diagnostic)
        }
        _ if matches!(
            record.kind,
            SyntaxKind::MethodDeclaration
                | SyntaxKind::ConstructSignature
                | SyntaxKind::FunctionExpression
                | SyntaxKind::ArrowFunction
                | SyntaxKind::FunctionDeclaration
        ) =>
        {
            let mut diagnostic =
                diagnostic_for_source_node(source, node, error_message(record.kind)?, &[]);
            add_parent_suggestion(source, node, &mut diagnostic)?;
            add_suggestion(source, node, &mut diagnostic)?;
            Ok(diagnostic)
        }
        _ => expression_error(source, node, None),
    }
}

/// tsc-port: createEntityInTypeNodeError @6.0.3
/// tsc-hash: 46c286508f1b29370500f49e1b4c0fa9403c95fcf2448ed7d27c0d9215457d53
/// tsc-span: _tsc.js:114214-114222
pub(crate) fn entity_in_type_node_error(
    source: &SourceFile,
    node: NodeId,
) -> Result<Diagnostic, TransformError> {
    let mut diagnostic = diagnostic_for_source_node(
        source,
        node,
        &d::Type_containing_private_name_0_can_t_be_used_with_isolatedDeclarations,
        &[text_of_node(source, node)],
    );
    add_parent_suggestion(source, node, &mut diagnostic)?;
    Ok(diagnostic)
}

pub(crate) fn is_in_heritage_clause(source: &SourceFile, node: NodeId) -> bool {
    ancestor(source, Some(node), |kind| {
        kind == SyntaxKind::HeritageClause
    })
    .is_some()
}

fn declaration_error(source: &SourceFile, node: NodeId) -> Result<Diagnostic, TransformError> {
    let mut diagnostic = diagnostic_for_source_node(
        source,
        node,
        error_message(source.arena.node(node).kind)?,
        &[],
    );
    add_suggestion(source, node, &mut diagnostic)?;
    Ok(diagnostic)
}

/// createParameterError consumes the callback-safe resolver decision made
/// with the original parameter's actual parent as its enclosing declaration.
fn parameter_error(
    source: &SourceFile,
    node: NodeId,
    add_undefined: bool,
) -> Result<Diagnostic, TransformError> {
    let record = source.arena.node(node);
    let NodeData::Parameter(data) = &record.data else {
        return Err(contract("parameter diagnostic anchor is not a parameter"));
    };
    if record
        .parent
        .is_some_and(|parent| source.arena.node(parent).kind == SyntaxKind::SetAccessor)
    {
        return unsupported();
    }
    if !add_undefined {
        if let Some(initializer) = data.initializer {
            return expression_error(source, initializer, None);
        }
        return declaration_error(source, node);
    }
    let mut diagnostic = diagnostic_for_source_node(
        source, node,
        &d::Declaration_emit_for_this_parameter_requires_implicitly_adding_undefined_to_its_type_This_is_not_supported_with_isolatedDeclarations,
        &[],
    );
    add_suggestion(source, node, &mut diagnostic)?;
    Ok(diagnostic)
}

fn expression_error(
    source: &SourceFile,
    node: NodeId,
    message: Option<&'static DiagnosticMessage>,
) -> Result<Diagnostic, TransformError> {
    let Some(declaration) = nearest_declaration(source, node) else {
        return Ok(diagnostic_for_source_node(
            source,
            node,
            message.unwrap_or(&d::Expression_type_can_t_be_inferred_with_isolatedDeclarations),
            &[],
        ));
    };
    // findAncestor's "quit" result stops at a statement before selecting it.
    let mut parent = source.arena.node(node).parent;
    while let Some(current) = parent {
        let record = source.arena.node(current);
        if record.kind == SyntaxKind::ExportAssignment {
            break;
        }
        if is_statement(record.kind) {
            parent = None;
            break;
        }
        if !matches!(
            record.kind,
            SyntaxKind::ParenthesizedExpression
                | SyntaxKind::TypeAssertionExpression
                | SyntaxKind::AsExpression
        ) {
            break;
        }
        parent = record.parent;
    }
    let direct = parent == Some(declaration);
    let message = match message {
        Some(message) => message,
        None if direct => error_message(source.arena.node(declaration).kind)?,
        None => &d::Expression_type_can_t_be_inferred_with_isolatedDeclarations,
    };
    let mut diagnostic = diagnostic_for_source_node(source, node, message, &[]);
    add_suggestion(source, declaration, &mut diagnostic)?;
    if !direct {
        add_related(&mut diagnostic, diagnostic_for_source_node(source, node,
            &d::Add_satisfies_and_a_type_assertion_to_this_expression_satisfies_T_as_T_to_make_the_type_explicit, &[]));
    }
    Ok(diagnostic)
}

fn add_parent_suggestion(
    source: &SourceFile,
    node: NodeId,
    diagnostic: &mut Diagnostic,
) -> Result<(), TransformError> {
    if let Some(parent) = nearest_declaration(source, node) {
        add_suggestion(source, parent, diagnostic)?;
    }
    Ok(())
}

fn add_suggestion(
    source: &SourceFile,
    node: NodeId,
    diagnostic: &mut Diagnostic,
) -> Result<(), TransformError> {
    let name = name_of_declaration(source, node)
        .map(|name| text_of_node(source, name))
        .unwrap_or_default();
    add_related(
        diagnostic,
        diagnostic_for_source_node(
            source,
            node,
            suggestion(source.arena.node(node).kind)?,
            &[name],
        ),
    );
    Ok(())
}

fn add_related(diagnostic: &mut Diagnostic, related: Diagnostic) {
    diagnostic.related_information_present = true;
    diagnostic.related.push(RelatedInfo {
        file_name: related.file_name,
        start: related.start,
        length: related.length,
        message: related.message,
    });
}

fn nearest_declaration(source: &SourceFile, node: NodeId) -> Option<NodeId> {
    let nearest = ancestor(source, Some(node), |kind| {
        is_statement(kind)
            || matches!(
                kind,
                SyntaxKind::ExportAssignment
                    | SyntaxKind::VariableDeclaration
                    | SyntaxKind::PropertyDeclaration
                    | SyntaxKind::Parameter
            )
    })?;
    match source.arena.node(nearest).kind {
        SyntaxKind::ExportAssignment => Some(nearest),
        SyntaxKind::ReturnStatement => ancestor(source, Some(nearest), |kind| {
            is_function_like(kind) && kind != SyntaxKind::Constructor
        }),
        kind if is_statement(kind) => None,
        _ => Some(nearest),
    }
}

fn ancestor(
    source: &SourceFile,
    mut node: Option<NodeId>,
    predicate: impl Fn(SyntaxKind) -> bool,
) -> Option<NodeId> {
    while let Some(current) = node {
        let record = source.arena.node(current);
        if predicate(record.kind) {
            return Some(current);
        }
        node = record.parent;
    }
    None
}

fn is_statement(kind: SyntaxKind) -> bool {
    (SyntaxKind::FirstStatement..=SyntaxKind::LastStatement).contains(&kind)
        || matches!(
            kind,
            SyntaxKind::FunctionDeclaration
                | SyntaxKind::MissingDeclaration
                | SyntaxKind::ClassDeclaration
                | SyntaxKind::InterfaceDeclaration
                | SyntaxKind::TypeAliasDeclaration
                | SyntaxKind::EnumDeclaration
                | SyntaxKind::ModuleDeclaration
                | SyntaxKind::ImportDeclaration
                | SyntaxKind::ImportEqualsDeclaration
                | SyntaxKind::ExportDeclaration
                | SyntaxKind::ExportAssignment
                | SyntaxKind::NamespaceExportDeclaration
                | SyntaxKind::Block
        )
}

fn error_message(kind: SyntaxKind) -> Result<&'static DiagnosticMessage, TransformError> {
    Ok(match kind {
        SyntaxKind::FunctionExpression | SyntaxKind::FunctionDeclaration | SyntaxKind::ArrowFunction => &d::Function_must_have_an_explicit_return_type_annotation_with_isolatedDeclarations,
        SyntaxKind::MethodDeclaration | SyntaxKind::ConstructSignature => &d::Method_must_have_an_explicit_return_type_annotation_with_isolatedDeclarations,
        SyntaxKind::GetAccessor | SyntaxKind::SetAccessor => &d::At_least_one_accessor_must_have_an_explicit_type_annotation_with_isolatedDeclarations,
        SyntaxKind::Parameter => &d::Parameter_must_have_an_explicit_type_annotation_with_isolatedDeclarations,
        SyntaxKind::VariableDeclaration => &d::Variable_must_have_an_explicit_type_annotation_with_isolatedDeclarations,
        SyntaxKind::PropertyDeclaration | SyntaxKind::PropertySignature => &d::Property_must_have_an_explicit_type_annotation_with_isolatedDeclarations,
        SyntaxKind::ComputedPropertyName => &d::Computed_property_names_on_class_or_object_literals_cannot_be_inferred_with_isolatedDeclarations,
        SyntaxKind::SpreadAssignment => &d::Objects_that_contain_spread_assignments_can_t_be_inferred_with_isolatedDeclarations,
        SyntaxKind::ShorthandPropertyAssignment => &d::Objects_that_contain_shorthand_properties_can_t_be_inferred_with_isolatedDeclarations,
        SyntaxKind::ArrayLiteralExpression => &d::Only_const_arrays_can_be_inferred_with_isolatedDeclarations,
        SyntaxKind::ExportAssignment => &d::Default_exports_can_t_be_inferred_with_isolatedDeclarations,
        SyntaxKind::SpreadElement => &d::Arrays_with_spread_elements_can_t_inferred_with_isolatedDeclarations,
        _ => return unsupported(),
    })
}

fn suggestion(kind: SyntaxKind) -> Result<&'static DiagnosticMessage, TransformError> {
    Ok(match kind {
        SyntaxKind::ArrowFunction | SyntaxKind::FunctionExpression => {
            &d::Add_a_return_type_to_the_function_expression
        }
        SyntaxKind::MethodDeclaration => &d::Add_a_return_type_to_the_method,
        SyntaxKind::FunctionDeclaration | SyntaxKind::ConstructSignature => {
            &d::Add_a_return_type_to_the_function_declaration
        }
        SyntaxKind::GetAccessor => &d::Add_a_return_type_to_the_get_accessor_declaration,
        SyntaxKind::SetAccessor => &d::Add_a_type_to_parameter_of_the_set_accessor_declaration,
        SyntaxKind::Parameter => &d::Add_a_type_annotation_to_the_parameter_0,
        SyntaxKind::VariableDeclaration => &d::Add_a_type_annotation_to_the_variable_0,
        SyntaxKind::PropertyDeclaration | SyntaxKind::PropertySignature => {
            &d::Add_a_type_annotation_to_the_property_0
        }
        SyntaxKind::ExportAssignment => {
            &d::Move_the_expression_in_default_export_to_a_variable_and_add_a_type_annotation_to_it
        }
        _ => return unsupported(),
    })
}

fn unsupported<T>() -> Result<T, TransformError> {
    Err(TransformError::Unsupported(
        UnsupportedEmitFeature::IsolatedDeclarations,
    ))
}

fn contract(detail: &'static str) -> TransformError {
    TransformError::UnsupportedCompilerOption {
        option: "isolated declaration diagnostic",
        detail,
    }
}

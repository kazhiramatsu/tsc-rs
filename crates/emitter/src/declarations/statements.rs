use tsc_syntax::nodes::*;
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{ModifierFlags, NodeFlags};

use crate::{
    EmitInternalNodeBuilderFlags, EmitNodeBuilderFlags, EmitResolverError, NodeFactory,
    TransformError, TransformNode, TransformNodeArray, TransformSourceId, TransformationContext,
};

use super::{DeclarationTransformer, VisitResult};

/// The declaration transformer's scope-fix sentinel (`export {}`).
///
/// tsrs-native: the empty-export factory is owned by the declaration root
/// scope-fix branch.
pub(crate) fn create_empty_exports(
    factory: &mut NodeFactory<'_>,
    source: TransformSourceId,
) -> Result<TransformNode, TransformError> {
    let elements = factory.create_node_array(source, Vec::new())?;
    let exports = factory.create_named_exports(source, elements)?;
    factory.create_export_declaration(source, None, false, Some(exports), None, None)
}

/// tsc-port: visitDeclarationStatements @6.0.3
/// tsc-hash: 100f1afe5d377fafea89e51ca881c30b77f4539360c83050587b3864c38b9958
/// tsc-span: _tsc.js:115260-115314
pub(crate) fn visit_declaration_statement(
    transformer: &mut DeclarationTransformer<'_>,
    context: &mut TransformationContext,
    input: TransformNode,
) -> Result<VisitResult, TransformError> {
    if !is_preserved_declaration_statement(context, input)?
        || should_strip_internal(transformer, context, input)?
    {
        return Ok(None);
    }

    match context.arena().node(input)?.kind {
        SyntaxKind::ExportDeclaration => {
            let data = export_declaration_data(context, input)?;
            transformer.state.result_has_scope_marker = true;
            if is_source_file_parent(context, input)? {
                transformer.state.result_has_external_module_indicator = true;
            }
            let module_specifier = data
                .module_specifier
                .and_then(|node| context.arena().node_ref(input.source(), node));
            let module_specifier =
                rewrite_module_specifier(transformer, context, input, module_specifier)?;
            let attributes = try_get_resolution_mode_override(
                context,
                data.attributes
                    .and_then(|node| context.arena().node_ref(input.source(), node)),
            );
            let modifiers = data
                .modifiers
                .and_then(|array| context.arena().node_array_ref(input.source(), array));
            let export_clause = data
                .export_clause
                .and_then(|node| context.arena().node_ref(input.source(), node));
            let mut factory = context.factory()?;
            let updated = factory.update_export_declaration(
                input,
                modifiers,
                data.is_type_only,
                export_clause,
                module_specifier,
                attributes,
            )?;
            Ok(Some(vec![updated]))
        }
        SyntaxKind::ExportAssignment => {
            let data = export_assignment_data(context, input)?;
            transformer.state.result_has_scope_marker = true;
            if is_source_file_parent(context, input)? {
                transformer.state.result_has_external_module_indicator = true;
            }
            let expression = required_node(
                context,
                input.source(),
                data.expression,
                SyntaxKind::ExportAssignment,
                "expression",
            )?;
            if context.arena().node(expression)?.kind == SyntaxKind::Identifier {
                return Ok(Some(vec![input]));
            }

            // The default-export split is intentionally structural here.  The
            // P1 resolver/diagnostic channel supplies the inferred type when
            // it is available; the Any fallback keeps the dormant foundation
            // total for a syntax-only harness.
            let original_modifiers = data
                .modifiers
                .and_then(|array| context.arena().node_array_ref(input.source(), array));
            let mut factory = context.factory()?;
            let new_id = factory.create_unique_name(
                input.source(),
                "_default",
                crate::GeneratedIdentifierFlags::OPTIMISTIC,
            )?;
            let type_node =
                factory.create_keyword_type_node(input.source(), SyntaxKind::AnyKeyword)?;
            let declaration = factory.create_variable_declaration(
                input.source(),
                new_id,
                None,
                Some(type_node),
                None,
            )?;
            let declarations = factory.create_node_array(input.source(), vec![declaration])?;
            let declaration_list = factory.create_variable_declaration_list(
                input.source(),
                declarations,
                NodeFlags::CONST,
            )?;
            let modifiers = if transformer.state.needs_declare {
                factory
                    .create_modifiers_from_modifier_flags(input.source(), ModifierFlags::AMBIENT)?
            } else {
                None
            };
            let statement =
                factory.create_variable_statement(input.source(), modifiers, declaration_list)?;
            let assignment = factory.update_export_assignment(input, original_modifiers, new_id)?;
            Ok(Some(vec![statement, assignment]))
        }
        _ => {
            let result = transform_top_level_declaration(transformer, context, input)?;
            let key = context.arena().get_original_node(input).node();
            transformer
                .state
                .late_statement_replacement
                .insert(key, result);
            Ok(Some(vec![input]))
        }
    }
}

/// tsc-port: transformTopLevelDeclaration @6.0.3
/// tsc-hash: 589ad1298da2d91bdb3004c11ab6160aa9d51254fae612f1a80e8a5e981ac9c6
/// tsc-span: _tsc.js:115337-115704
pub(crate) fn transform_top_level_declaration(
    transformer: &mut DeclarationTransformer<'_>,
    context: &mut TransformationContext,
    input: TransformNode,
) -> Result<VisitResult, TransformError> {
    if let Some(late) = transformer.state.late_marked_statements.as_mut() {
        // tsc's `orderedRemoveItem` removes every occurrence before the
        // top-level switch consumes the statement.
        late.retain(|candidate| *candidate != input);
    }
    if should_strip_internal(transformer, context, input)? {
        return Ok(None);
    }

    match context.arena().node(input)?.kind {
        SyntaxKind::ImportEqualsDeclaration => {
            return transform_import_equals_declaration(transformer, context, input);
        }
        SyntaxKind::ImportDeclaration => {
            return transform_import_declaration(transformer, context, input);
        }
        _ => {}
    }

    if is_declaration_and_not_visible(transformer, context, input)?
        || context.arena().node(input)?.kind == SyntaxKind::JSDocImportTag
    {
        return Ok(None);
    }

    if is_function_like_kind(context.arena().node(input)?.kind) {
        let resolver_node = transformer.resolver_node(context, input)?;
        if resolver_bool(
            transformer
                .resolver
                .is_implementation_of_overload(resolver_node),
        )? {
            return Ok(None);
        }
    }

    let previous_enclosing = transformer.state.enclosing_declaration;
    if is_enclosing_declaration(context, input)? {
        transformer.state.enclosing_declaration = Some(input);
    }
    let previous_needs_declare = transformer.state.needs_declare;
    let result = match context.arena().node(input)?.kind {
        SyntaxKind::TypeAliasDeclaration => {
            transformer.state.needs_declare = false;
            let data = type_alias_data(context, input)?;
            let name = required_node(
                context,
                input.source(),
                data.name,
                input_kind(context, input)?,
                "name",
            )?;
            let type_node = data
                .r#type
                .and_then(|node| context.arena().node_ref(input.source(), node))
                .unwrap_or_else(|| TransformNode::new(input.source(), name.node()));
            let type_parameters = array_handle(context, input.source(), data.type_parameters);
            let modifiers = ensure_modifiers(transformer, context, input)?;
            let mut factory = context.factory()?;
            let updated = factory.update_type_alias_declaration(
                input,
                modifiers,
                name,
                type_parameters,
                type_node,
            )?;
            Some(vec![updated])
        }
        SyntaxKind::InterfaceDeclaration => {
            let data = interface_data(context, input)?;
            let name = required_node(
                context,
                input.source(),
                data.name,
                input_kind(context, input)?,
                "name",
            )?;
            let members = array_or_empty(context, input.source(), data.members)?;
            let type_parameters = array_handle(context, input.source(), data.type_parameters);
            let heritage = transform_heritage_clauses(
                transformer,
                context,
                array_handle(context, input.source(), data.heritage_clauses),
            )?;
            let modifiers = ensure_modifiers(transformer, context, input)?;
            let mut factory = context.factory()?;
            let updated = factory.update_interface_declaration(
                input,
                modifiers,
                name,
                type_parameters,
                heritage,
                members,
            )?;
            Some(vec![updated])
        }
        SyntaxKind::FunctionDeclaration => {
            let data = function_data(context, input)?;
            let original_parameters = array_or_empty(context, input.source(), data.parameters)?;
            let parameters = update_params_list(transformer, context, input, original_parameters)?;
            let type_node = ensure_type(transformer, context, input, data.r#type)?;
            let name = data
                .name
                .and_then(|node| context.arena().node_ref(input.source(), node));
            let type_parameters = array_handle(context, input.source(), data.type_parameters);
            let modifiers = ensure_modifiers(transformer, context, input)?;
            let mut factory = context.factory()?;
            let updated = factory.update_function_declaration(
                input,
                modifiers,
                None,
                name,
                type_parameters,
                parameters,
                type_node,
                None,
            )?;
            let mut outputs = vec![updated];
            if should_emit_function_properties(transformer, context, input)? {
                outputs.extend(expando_declaration_arm(
                    transformer,
                    context,
                    input,
                    updated,
                )?);
            }
            Some(outputs)
        }
        SyntaxKind::ModuleDeclaration => {
            let data = module_data(context, input)?;
            transformer.state.needs_declare = false;
            let body = if let Some(body) = data
                .body
                .and_then(|node| context.arena().node_ref(input.source(), node))
            {
                if context.arena().node(body)?.kind == SyntaxKind::ModuleBlock {
                    let block_data = module_block_data(context, body)?;
                    let mut statements = Vec::new();
                    for statement in source_array(context, body.source(), block_data.statements)? {
                        if let Some(result) =
                            visit_declaration_statement(transformer, context, statement)?
                        {
                            statements.extend(result);
                        }
                    }
                    let statements = transform_and_replace_late_painted_statements(
                        transformer,
                        context,
                        statements,
                    )?;
                    let mut factory = context.factory()?;
                    let array = factory.create_node_array(body.source(), statements)?;
                    Some(factory.update_module_block(body, array)?)
                } else {
                    Some(body)
                }
            } else {
                None
            };
            let modifiers = ensure_modifiers(transformer, context, input)?;
            let name = required_node(
                context,
                input.source(),
                data.name,
                input_kind(context, input)?,
                "name",
            )?;
            Some(vec![update_module_declaration_and_keyword(
                transformer,
                context,
                input,
                modifiers,
                name,
                body,
            )?])
        }
        SyntaxKind::ClassDeclaration => {
            let data = class_data(context, input)?;
            let name = data
                .name
                .and_then(|node| context.arena().node_ref(input.source(), node));
            let original_members = array_or_empty(context, input.source(), data.members)?;
            let type_parameters = array_handle(context, input.source(), data.type_parameters);
            let heritage = transform_heritage_clauses(
                transformer,
                context,
                array_handle(context, input.source(), data.heritage_clauses),
            )?;
            let modifiers = ensure_modifiers(transformer, context, input)?;
            let constructor_properties = parameter_properties(
                transformer,
                context,
                first_constructor_with_body(context, input)?,
            )?;
            let mut has_private_identifier = false;
            for member in source_array(
                context,
                original_members.source(),
                Some(original_members.array()),
            )? {
                if let Some(name) = member_name(context, member)? {
                    if context.arena().node(name)?.kind == SyntaxKind::PrivateIdentifier {
                        has_private_identifier = true;
                        break;
                    }
                }
            }
            let private_identifier = if has_private_identifier {
                let mut factory = context.factory()?;
                let name = factory.create_private_identifier(input.source(), "#private")?;
                Some(factory.create_property_declaration(
                    input.source(),
                    None,
                    name,
                    None,
                    None,
                    None,
                )?)
            } else {
                None
            };
            let mut member_nodes = Vec::new();
            if let Some(private_identifier) = private_identifier {
                member_nodes.push(private_identifier);
            }
            let class_resolver_node = transformer.resolver_node(context, input)?;
            let enclosing_resolver = transformer
                .state
                .enclosing_declaration
                .and_then(|node| transformer.resolver_node(context, node).ok())
                .unwrap_or(class_resolver_node);
            let late_indexes = match transformer.resolver.create_late_bound_index_signatures(
                context.arena_mut()?,
                input.source(),
                class_resolver_node,
                enclosing_resolver,
                EmitNodeBuilderFlags::DECLARATION_EMIT,
                EmitInternalNodeBuilderFlags::DECLARATION_EMIT,
                &mut transformer.tracker,
            ) {
                Ok(Some(nodes)) => nodes,
                Ok(None) | Err(EmitResolverError::Unavailable { .. }) => Vec::new(),
                Err(error) => return Err(TransformError::Resolver(error)),
            };
            member_nodes.extend(late_indexes);
            member_nodes.extend(constructor_properties);
            for member in source_array(
                context,
                original_members.source(),
                Some(original_members.array()),
            )? {
                if let Some(nodes) = visit_declaration_subtree(transformer, context, member)? {
                    member_nodes.extend(nodes);
                }
            }
            let members = {
                let mut factory = context.factory()?;
                factory.update_node_array(original_members, member_nodes)?
            };
            let mut factory = context.factory()?;
            let updated = factory.update_class_declaration(
                input,
                modifiers,
                name,
                type_parameters,
                heritage,
                members,
            )?;
            Some(vec![updated])
        }
        SyntaxKind::VariableStatement => Some(vec![transform_variable_statement(
            transformer,
            context,
            input,
        )?]),
        SyntaxKind::EnumDeclaration => {
            let data = enum_data(context, input)?;
            let name = required_node(
                context,
                input.source(),
                data.name,
                input_kind(context, input)?,
                "name",
            )?;
            let members = array_or_empty(context, input.source(), data.members)?;
            let modifiers = ensure_modifiers(transformer, context, input)?;
            let mut factory = context.factory()?;
            let updated = factory.update_enum_declaration(input, modifiers, name, members)?;
            Some(vec![updated])
        }
        _ => Some(vec![input]),
    };

    finish_top_level(
        transformer,
        context,
        input,
        previous_enclosing,
        previous_needs_declare,
        result,
    )
}

/// tsc-port: cleanup @6.0.3
/// tsc-hash: 9a72f6e59941aaf218e0e68825f0865509a3667441e6303ae6e2b8328ce3d5c3
/// tsc-span: _tsc.js:115687-115703
pub(crate) fn finish_top_level(
    transformer: &mut DeclarationTransformer<'_>,
    context: &mut TransformationContext,
    input: TransformNode,
    previous_enclosing: Option<TransformNode>,
    previous_needs_declare: bool,
    result: VisitResult,
) -> Result<VisitResult, TransformError> {
    transformer.state.enclosing_declaration = previous_enclosing;
    transformer.state.needs_declare = previous_needs_declare;
    transformer.tracker.error_name_node = None;
    transformer.tracker.error_fallback_node = None;
    let Some(nodes) = result else {
        return Ok(None);
    };
    let mut adopted = Vec::with_capacity(nodes.len());
    for node in nodes {
        if node != input {
            context.arena_mut()?.set_original_node(node, Some(input))?;
        }
        adopted.push(node);
    }
    Ok(Some(adopted))
}

/// tsc-port: transformAndReplaceLatePaintedStatements @6.0.3
/// tsc-hash: 183c11f048a7ea806e96531bf699a300a29bfa6b564933ceb960118cd8e61e4d
/// tsc-span: _tsc.js:114919-114951
pub(crate) fn transform_and_replace_late_painted_statements(
    transformer: &mut DeclarationTransformer<'_>,
    context: &mut TransformationContext,
    mut statements: Vec<TransformNode>,
) -> Result<Vec<TransformNode>, TransformError> {
    let late = transformer
        .state
        .late_marked_statements
        .take()
        .unwrap_or_default();
    for input in late {
        if !is_late_visibility_painted_statement(context, input)? {
            return Err(TransformError::MissingTransformHandoff {
                producer: "late visibility paint",
                consumer: "declaration transformer",
                node: input,
                handoff: "late-marked declaration statement",
            });
        }
        let previous_needs_declare = transformer.state.needs_declare;
        transformer.state.needs_declare = is_source_file_parent(context, input)?;
        let result = transform_top_level_declaration(transformer, context, input)?;
        transformer.state.needs_declare = previous_needs_declare;
        transformer
            .state
            .late_statement_replacement
            .insert(context.arena().get_original_node(input).node(), result);
    }

    let mut replaced = Vec::with_capacity(statements.len());
    for statement in statements.drain(..) {
        if let Some(result) =
            visit_late_visibility_marked_statement(transformer, context, statement)?
        {
            replaced.extend(result);
        }
    }
    Ok(replaced)
}

/// tsc-port: visitLateVisibilityMarkedStatements @6.0.3
/// tsc-hash: 5038431826b1d33840175854e6806a4f7a28a27789617a600ae94f2ca71bfa81
/// tsc-span: _tsc.js:114932-114950
pub(crate) fn visit_late_visibility_marked_statement(
    transformer: &mut DeclarationTransformer<'_>,
    context: &mut TransformationContext,
    statement: TransformNode,
) -> Result<VisitResult, TransformError> {
    if !is_late_visibility_painted_statement(context, statement)? {
        return Ok(Some(vec![statement]));
    }
    let key = context.arena().get_original_node(statement).node();
    let Some(result) = transformer.state.late_statement_replacement.remove(&key) else {
        return Ok(Some(vec![statement]));
    };
    if let Some(result) = &result {
        if result.iter().any(|node| is_scope_marker(context, *node)) {
            transformer.state.needs_scope_fix_marker = true;
        }
        if is_source_file_parent(context, statement)?
            && result
                .iter()
                .any(|node| is_external_module_indicator(context, *node))
        {
            transformer.state.result_has_external_module_indicator = true;
        }
    }
    Ok(result)
}

/// tsc-port: transformVariableStatement @6.0.3
/// tsc-hash: f9aeb31b513a6c1db3a8650375fc1d909bec498d17eb9aefa7613687fa8ba773
/// tsc-span: _tsc.js:115705-115720
pub(crate) fn transform_variable_statement(
    transformer: &mut DeclarationTransformer<'_>,
    context: &mut TransformationContext,
    input: TransformNode,
) -> Result<TransformNode, TransformError> {
    let data = variable_statement_data(context, input)?;
    let list = required_node(
        context,
        input.source(),
        data.declaration_list,
        SyntaxKind::VariableStatement,
        "declarationList",
    )?;
    let list_data = variable_declaration_list_data(context, list)?;
    let mut declarations = Vec::new();
    for declaration in source_array(context, input.source(), list_data.declarations)? {
        if let Some(result) = transform_variable_declaration(transformer, context, declaration)? {
            declarations.extend(result);
        }
    }
    let modifiers = ensure_modifiers(transformer, context, input)?;
    let mut factory = context.factory()?;
    let declarations = factory.create_node_array(input.source(), declarations)?;
    let declaration_list = factory.update_variable_declaration_list(list, declarations)?;
    factory.update_variable_statement(input, modifiers, declaration_list)
}

fn first_constructor_with_body(
    context: &TransformationContext,
    class: TransformNode,
) -> Result<Option<TransformNode>, TransformError> {
    for member in source_array(context, class.source(), class_data(context, class)?.members)? {
        if let NodeData::Constructor(data) = &context.arena().node(member)?.data {
            if data.body.is_some() {
                return Ok(Some(member));
            }
        }
    }
    Ok(None)
}

fn parameter_properties(
    transformer: &mut DeclarationTransformer<'_>,
    context: &mut TransformationContext,
    constructor: Option<TransformNode>,
) -> Result<Vec<TransformNode>, TransformError> {
    let Some(constructor) = constructor else {
        return Ok(Vec::new());
    };
    let parameters = match &context.arena().node(constructor)?.data {
        NodeData::Constructor(data) => data.parameters,
        _ => None,
    };
    let mut result = Vec::new();
    for parameter in source_array(context, constructor.source(), parameters)? {
        if !modifier_flags(context, parameter)?
            .intersects(ModifierFlags::PARAMETER_PROPERTY_MODIFIER)
            || should_strip_internal(transformer, context, parameter)?
        {
            continue;
        }
        let data = match &context.arena().node(parameter)?.data {
            NodeData::Parameter(data) => data.clone(),
            _ => continue,
        };
        let Some(name) = data
            .name
            .and_then(|node| context.arena().node_ref(parameter.source(), node))
        else {
            continue;
        };
        if matches!(
            context.arena().node(name)?.kind,
            SyntaxKind::ArrayBindingPattern | SyntaxKind::ObjectBindingPattern
        ) {
            result.extend(walk_binding_pattern(transformer, context, name, parameter)?);
            continue;
        }
        if !binding_name_visible(transformer, context, parameter)? {
            continue;
        }
        let modifiers = ensure_modifiers(transformer, context, parameter)?;
        let question_token = data
            .question_token
            .and_then(|node| context.arena().node_ref(parameter.source(), node));
        let type_node = ensure_type(transformer, context, parameter, data.r#type)?;
        let mut factory = context.factory()?;
        result.push(factory.create_property_declaration(
            parameter.source(),
            modifiers,
            name,
            question_token,
            type_node,
            None,
        )?);
    }
    Ok(result)
}

fn member_name(
    context: &TransformationContext,
    member: TransformNode,
) -> Result<Option<TransformNode>, TransformError> {
    let name = match &context.arena().node(member)?.data {
        NodeData::Constructor(data) => data.name,
        NodeData::MethodDeclaration(data) => data.name,
        NodeData::GetAccessor(data) => data.name,
        NodeData::SetAccessor(data) => data.name,
        NodeData::PropertyDeclaration(data) => data.name,
        NodeData::MethodSignature(data) => data.name,
        NodeData::PropertySignature(data) => data.name,
        NodeData::EnumMember(data) => data.name,
        _ => None,
    };
    Ok(name.and_then(|node| context.arena().node_ref(member.source(), node)))
}

/// tsc-port: walkBindingPattern @6.0.3
/// tsc-hash: 91469ba97a357ad2469b2d4631eab8de90d66c6379f1e16e4f71c74d899ee484
/// tsc-span: _tsc.js:115572-115592
pub(crate) fn walk_binding_pattern(
    transformer: &mut DeclarationTransformer<'_>,
    context: &mut TransformationContext,
    pattern: TransformNode,
    parameter: TransformNode,
) -> Result<Vec<TransformNode>, TransformError> {
    let mut result = Vec::new();
    for element in source_array(
        context,
        pattern.source(),
        binding_pattern_elements(context, pattern)?,
    )? {
        if context.arena().node(element)?.kind == SyntaxKind::OmittedExpression
            || should_strip_internal(transformer, context, element)?
        {
            continue;
        }
        let data = binding_element_data(context, element)?;
        let Some(name) = data
            .name
            .and_then(|node| context.arena().node_ref(element.source(), node))
        else {
            continue;
        };
        if !binding_name_visible(transformer, context, element)? {
            continue;
        }
        if matches!(
            context.arena().node(name)?.kind,
            SyntaxKind::ArrayBindingPattern | SyntaxKind::ObjectBindingPattern
        ) {
            result.extend(walk_binding_pattern(transformer, context, name, parameter)?);
            continue;
        }
        let modifiers = ensure_modifiers(transformer, context, parameter)?;
        let type_node = ensure_type(transformer, context, element, None)?;
        let mut factory = context.factory()?;
        result.push(factory.create_property_declaration(
            element.source(),
            modifiers,
            name,
            None,
            type_node,
            None,
        )?);
    }
    Ok(result)
}

/// tsc-port: recreateBindingPattern @6.0.3
/// tsc-hash: db7dae8edbdeb2e3101a1570828b0d0baf0db0aa52a7406f7618c01d6def10a7
/// tsc-span: _tsc.js:115721-115723
pub(crate) fn recreate_binding_pattern(
    transformer: &mut DeclarationTransformer<'_>,
    context: &mut TransformationContext,
    pattern: TransformNode,
) -> Result<Vec<TransformNode>, TransformError> {
    let elements = match &context.arena().node(pattern)?.data {
        NodeData::ArrayBindingPattern(data) => data.elements,
        NodeData::ObjectBindingPattern(data) => data.elements,
        _ => None,
    };
    let mut result = Vec::new();
    for element in source_array(context, pattern.source(), elements)? {
        if let Some(nodes) = recreate_binding_element(transformer, context, element)? {
            result.extend(nodes);
        }
    }
    Ok(result)
}

/// tsc-port: recreateBindingElement @6.0.3
/// tsc-hash: 7a3e6247886a781f2af22a23464aa541f3b1f0e88b001cb550a71797675398d7
/// tsc-span: _tsc.js:115724-115743
pub(crate) fn recreate_binding_element(
    transformer: &mut DeclarationTransformer<'_>,
    context: &mut TransformationContext,
    element: TransformNode,
) -> Result<VisitResult, TransformError> {
    if context.arena().node(element)?.kind == SyntaxKind::OmittedExpression {
        return Ok(None);
    }
    let data = binding_element_data(context, element)?;
    let Some(name) = data
        .name
        .and_then(|node| context.arena().node_ref(element.source(), node))
    else {
        return Ok(None);
    };
    if matches!(
        context.arena().node(name)?.kind,
        SyntaxKind::ArrayBindingPattern | SyntaxKind::ObjectBindingPattern
    ) {
        return Ok(Some(recreate_binding_pattern(transformer, context, name)?));
    }
    if !binding_name_visible(transformer, context, element)? {
        return Ok(None);
    }
    let type_node = ensure_type(transformer, context, element, data.name);
    let mut factory = context.factory()?;
    let declaration =
        factory.create_variable_declaration(element.source(), name, None, type_node?, None)?;
    Ok(Some(vec![declaration]))
}

/// tsc-port: stripExportModifiers @6.0.3
/// tsc-hash: cab52b51edb6418ae744dfaf78406756b5ad29ea61ac3aa2a472afe6596f8c07
/// tsc-span: _tsc.js:115315-115321
#[allow(dead_code)]
pub(crate) fn strip_export_modifiers(
    context: &mut TransformationContext,
    statement: TransformNode,
) -> Result<TransformNode, TransformError> {
    if matches!(
        context.arena().node(statement)?.kind,
        SyntaxKind::ImportEqualsDeclaration
    ) {
        return Ok(statement);
    }
    let flags = modifier_flags(context, statement)?;
    if flags.contains(ModifierFlags::DEFAULT) {
        return Ok(statement);
    }
    let flags = ModifierFlags::from_bits(flags.bits() & !ModifierFlags::EXPORT.bits());
    let mut factory = context.factory()?;
    let modifiers = factory.create_modifiers_from_modifier_flags(statement.source(), flags)?;
    factory.replace_modifiers(statement, modifiers)
}

/// tsc-port: updateModuleDeclarationAndKeyword @6.0.3
/// tsc-hash: 57b072abb084b1007ccc20d1299739ddcc776e34215a5a37809d4f3fa6dd5f98
/// tsc-span: _tsc.js:115322-115336
pub(crate) fn update_module_declaration_and_keyword(
    _transformer: &mut DeclarationTransformer<'_>,
    context: &mut TransformationContext,
    node: TransformNode,
    modifiers: Option<TransformNodeArray>,
    name: TransformNode,
    body: Option<TransformNode>,
) -> Result<TransformNode, TransformError> {
    let updated = {
        let mut factory = context.factory()?;
        factory.update_module_declaration(node, modifiers, name, body)?
    };
    let record = context.arena().node(updated)?.clone();
    let flags = NodeFlags::from_bits(record.flags);
    if flags.contains(NodeFlags::AMBIENT) || flags.contains(NodeFlags::NAMESPACE) {
        return Ok(updated);
    }
    let fixed = {
        let mut factory = context.factory()?;
        let fixed = factory.create_module_declaration(
            updated.source(),
            modifiers,
            name,
            body,
            flags | NodeFlags::NAMESPACE,
        )?;
        factory.set_text_range(fixed, updated)?;
        fixed
    };
    context
        .arena_mut()?
        .set_original_node(fixed, Some(updated))?;
    Ok(fixed)
}

/// tsc-port: isScopeMarker2 @6.0.3
/// tsc-hash: cd989074af00eafd68cc7ef4920ee35c59d8afe9e2d4cd1ac873c5958327db69
/// tsc-span: _tsc.js:115763-115765
pub(crate) fn is_scope_marker(context: &TransformationContext, node: TransformNode) -> bool {
    matches!(
        context.arena().node(node).ok().map(|node| node.kind),
        Some(SyntaxKind::ExportAssignment | SyntaxKind::ExportDeclaration)
    )
}

/// tsc-port: hasScopeMarker2 @6.0.3
/// tsc-hash: 093d9b2459b8e2507d9420e4ea56d3f22e0cb49ce59126472aada63e19bd2a90
/// tsc-span: _tsc.js:115766-115768
#[allow(dead_code)]
pub(crate) fn has_scope_marker(
    context: &TransformationContext,
    statements: &[TransformNode],
) -> bool {
    statements
        .iter()
        .copied()
        .any(|node| is_scope_marker(context, node))
}

/// tsc-port: transformHeritageClauses @6.0.3
/// tsc-hash: ca275bd51650c71c34b5150f451a1484430b629dccab150d3ca6b770f9752674
/// tsc-span: _tsc.js:115787-115801
pub(crate) fn transform_heritage_clauses(
    _transformer: &mut DeclarationTransformer<'_>,
    context: &mut TransformationContext,
    clauses: Option<TransformNodeArray>,
) -> Result<Option<TransformNodeArray>, TransformError> {
    let Some(clauses) = clauses else {
        return Ok(None);
    };
    let clause_nodes = context.arena().node_array(clauses)?.nodes.clone();
    let mut result = Vec::new();
    for clause in clause_nodes {
        let clause = TransformNode::new(clauses.source(), clause);
        let data = heritage_clause_data(context, clause)?;
        let mut types = Vec::new();
        for type_node in source_array(context, clause.source(), data.types)? {
            let expression = match &context.arena().node(type_node)?.data {
                NodeData::ExpressionWithTypeArguments(data) => data
                    .expression
                    .and_then(|node| context.arena().node_ref(type_node.source(), node)),
                _ => None,
            };
            let keep = if let Some(expression) = expression {
                is_entity_name_expression(context, expression)?
                    || data.token == SyntaxKind::ExtendsKeyword
                        && context
                            .arena()
                            .node(expression)
                            .is_ok_and(|node| node.kind == SyntaxKind::NullKeyword)
            } else {
                false
            };
            if !keep {
                continue;
            }
            if let Some(visited) = visit_declaration_subtree(_transformer, context, type_node)? {
                types.extend(visited);
            }
        }
        if types.is_empty() {
            continue;
        }
        let original_types = array_handle(context, clause.source(), data.types).ok_or(
            TransformError::RequiredChildRemoved {
                parent: SyntaxKind::HeritageClause,
                field: "types",
            },
        )?;
        let mut factory = context.factory()?;
        let types = factory.update_node_array(original_types, types)?;
        let updated = factory.update_heritage_clause(clause, types)?;
        result.push(updated);
    }
    if result.is_empty() {
        return Ok(None);
    }
    let mut factory = context.factory()?;
    Ok(Some(factory.update_node_array(clauses, result)?))
}

fn is_entity_name_expression(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<bool, TransformError> {
    let record = context.arena().node(node)?;
    match &record.data {
        NodeData::Identifier(_) => Ok(true),
        NodeData::QualifiedName(data) => {
            let Some(left) = data
                .left
                .and_then(|id| context.arena().node_ref(node.source(), id))
            else {
                return Ok(false);
            };
            let Some(right) = data
                .right
                .and_then(|id| context.arena().node_ref(node.source(), id))
            else {
                return Ok(false);
            };
            Ok(is_entity_name_expression(context, left)?
                && context.arena().node(right)?.kind == SyntaxKind::Identifier)
        }
        NodeData::PropertyAccessExpression(data) => {
            let Some(expression) = data
                .expression
                .and_then(|id| context.arena().node_ref(node.source(), id))
            else {
                return Ok(false);
            };
            let Some(name) = data
                .name
                .and_then(|id| context.arena().node_ref(node.source(), id))
            else {
                return Ok(false);
            };
            Ok(is_entity_name_expression(context, expression)?
                && context.arena().node(name)?.kind == SyntaxKind::Identifier)
        }
        _ => Ok(false),
    }
}

/// tsc-port: transformImportEqualsDeclaration @6.0.3
/// tsc-hash: 93ddbcf0f5b98071069f57713946215db5ba4c1d88224c1b0be4942c1d7e25fa
/// tsc-span: _tsc.js:114822-114840
pub(crate) fn transform_import_equals_declaration(
    transformer: &mut DeclarationTransformer<'_>,
    context: &mut TransformationContext,
    declaration: TransformNode,
) -> Result<VisitResult, TransformError> {
    let resolver_node = transformer.resolver_node(context, declaration)?;
    if !resolver_visibility(transformer.resolver.is_declaration_visible(resolver_node))? {
        return Ok(None);
    }
    let data = import_equals_data(context, declaration)?;
    let name = required_node(
        context,
        declaration.source(),
        data.name,
        SyntaxKind::ImportEqualsDeclaration,
        "name",
    )?;
    let module_reference = required_node(
        context,
        declaration.source(),
        data.module_reference,
        SyntaxKind::ImportEqualsDeclaration,
        "moduleReference",
    )?;
    if context.arena().node(module_reference)?.kind != SyntaxKind::ExternalModuleReference {
        return Ok(Some(vec![declaration]));
    }
    let external = external_module_reference_data(context, module_reference)?;
    let expression = required_node(
        context,
        declaration.source(),
        external.expression,
        SyntaxKind::ExternalModuleReference,
        "expression",
    )?;
    let expression = rewrite_module_specifier(transformer, context, declaration, Some(expression))?
        .ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::ExternalModuleReference,
            field: "expression",
        })?;
    let modifiers = data
        .modifiers
        .and_then(|array| context.arena().node_array_ref(declaration.source(), array));
    let mut factory = context.factory()?;
    let external = factory.update_external_module_reference(module_reference, expression)?;
    let updated = factory.update_import_equals_declaration(
        declaration,
        modifiers,
        data.is_type_only,
        name,
        external,
    )?;
    Ok(Some(vec![updated]))
}

/// tsc-port: transformImportDeclaration @6.0.3
/// tsc-hash: 7378b26b9aa92b509ccdcf1b5e3872188bb6604c102714fe9e90013ad9ac2bf5
/// tsc-span: _tsc.js:114841-114914
pub(crate) fn transform_import_declaration(
    transformer: &mut DeclarationTransformer<'_>,
    context: &mut TransformationContext,
    declaration: TransformNode,
) -> Result<VisitResult, TransformError> {
    let data = import_declaration_data(context, declaration)?;
    let module_specifier = data
        .module_specifier
        .and_then(|node| context.arena().node_ref(declaration.source(), node));
    let module_specifier =
        rewrite_module_specifier(transformer, context, declaration, module_specifier)?;
    let attributes = try_get_resolution_mode_override(
        context,
        data.attributes
            .and_then(|node| context.arena().node_ref(declaration.source(), node)),
    );
    let Some(import_clause) = data
        .import_clause
        .and_then(|node| context.arena().node_ref(declaration.source(), node))
    else {
        let modifiers = data
            .modifiers
            .and_then(|array| context.arena().node_array_ref(declaration.source(), array));
        let mut factory = context.factory()?;
        let module_specifier = module_specifier.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::ImportDeclaration,
            field: "moduleSpecifier",
        })?;
        return Ok(Some(vec![factory.update_import_declaration(
            declaration,
            modifiers,
            None,
            module_specifier,
            attributes,
        )?]));
    };

    let clause_data = import_clause_data(context, import_clause)?;
    let phase_modifier = (clause_data.phase_modifier != Some(SyntaxKind::DeferKeyword))
        .then_some(clause_data.phase_modifier)
        .flatten();
    let visible_default = clause_data
        .name
        .and_then(|node| context.arena().node_ref(declaration.source(), node))
        .filter(|node| resolver_bool_for_node(transformer, context, *node).unwrap_or(true));
    let named_bindings = clause_data
        .named_bindings
        .and_then(|node| context.arena().node_ref(declaration.source(), node));
    let visible_named: Option<(TransformNode, Vec<TransformNode>)> =
        if let Some(named) = named_bindings {
            match context.arena().node(named)?.kind {
                SyntaxKind::NamespaceImport => resolver_bool_for_node(transformer, context, named)
                    .ok()
                    .filter(|visible| *visible)
                    .map(|_| (named, Vec::new())),
                SyntaxKind::NamedImports => {
                    let data = named_imports_data(context, named)?;
                    let elements = source_array(context, named.source(), data.elements)?
                        .into_iter()
                        .filter(|node| {
                            resolver_bool_for_node(transformer, context, *node).unwrap_or(true)
                        })
                        .collect::<Vec<_>>();
                    if elements.is_empty() {
                        None
                    } else {
                        Some((named, elements))
                    }
                }
                _ => None,
            }
        } else {
            None
        };

    if visible_default.is_none() && visible_named.is_none() {
        let resolver_node = transformer.resolver_node(context, declaration)?;
        if !resolver_bool(
            transformer
                .resolver
                .is_import_required_by_augmentation(resolver_node),
        )? {
            return Ok(None);
        }
    }

    let modifiers = data
        .modifiers
        .and_then(|array| context.arena().node_array_ref(declaration.source(), array));
    let named_import_kind = visible_named
        .as_ref()
        .map(|(named, _)| context.arena().node(*named).map(|node| node.kind))
        .transpose()?;
    let mut factory = context.factory()?;
    let bindings = match visible_named {
        Some((named, elements)) if named_import_kind == Some(SyntaxKind::NamedImports) => {
            let elements = factory.create_node_array(named.source(), elements)?;
            Some(factory.update_named_imports(named, elements)?)
        }
        Some((named, _)) => Some(named),
        None => None,
    };
    let clause =
        factory.update_import_clause(import_clause, phase_modifier, visible_default, bindings)?;
    let module_specifier = module_specifier.ok_or(TransformError::RequiredChildRemoved {
        parent: SyntaxKind::ImportDeclaration,
        field: "moduleSpecifier",
    })?;
    Ok(Some(vec![factory.update_import_declaration(
        declaration,
        modifiers,
        Some(clause),
        module_specifier,
        attributes,
    )?]))
}

/// tsc-port: tryGetResolutionModeOverride @6.0.3
/// tsc-hash: b053ae32b7f35937942850bffecc079c94d67af2ebe2aeb87f33add1a5b4444d
/// tsc-span: _tsc.js:114915-114918
pub(crate) fn try_get_resolution_mode_override(
    context: &TransformationContext,
    node: Option<TransformNode>,
) -> Option<TransformNode> {
    let node = node?;
    let NodeData::ImportAttributes(data) = &context.arena().node(node).ok()?.data else {
        return None;
    };
    let elements = context
        .arena()
        .node_array_ref(node.source(), data.elements?)?;
    let elements = context.arena().node_array(elements).ok()?;
    if elements.nodes.len() != 1 {
        return None;
    }
    let attribute = TransformNode::new(node.source(), elements.nodes[0]);
    let NodeData::ImportAttribute(data) = &context.arena().node(attribute).ok()?.data else {
        return None;
    };
    let name = data
        .name
        .and_then(|id| context.arena().node_ref(node.source(), id))?;
    let name = match &context.arena().node(name).ok()?.data {
        NodeData::StringLiteral(data) => data.text.as_str(),
        _ => return None,
    };
    if name != "resolution-mode" {
        return None;
    }
    let value = data
        .value
        .and_then(|id| context.arena().node_ref(node.source(), id))?;
    let value = match &context.arena().node(value).ok()?.data {
        NodeData::StringLiteral(data) => data.text.as_str(),
        NodeData::NoSubstitutionTemplateLiteral(data) => data.text.as_str(),
        _ => return None,
    };
    matches!(value, "import" | "require").then_some(node)
}

/// tsc-port: rewriteModuleSpecifier2 @6.0.3
/// tsc-hash: 14cd74dd8be907b92a346f04166a44dcb69e498586b9c573584ebceccd55a05d
/// tsc-span: _tsc.js:114809-114821
pub(crate) fn rewrite_module_specifier(
    transformer: &mut DeclarationTransformer<'_>,
    context: &TransformationContext,
    parent: TransformNode,
    input: Option<TransformNode>,
) -> Result<Option<TransformNode>, TransformError> {
    let Some(input) = input else {
        return Ok(None);
    };
    let parent_kind = context.arena().node(parent)?.kind;
    if !matches!(
        parent_kind,
        SyntaxKind::ModuleDeclaration | SyntaxKind::ImportType
    ) {
        transformer.state.result_has_external_module_indicator = true;
    }
    if transformer.is_bundled_emit || transformer.state.is_bundled_emit {
        return Err(TransformError::Unsupported(
            crate::UnsupportedEmitFeature::BundleRoot,
        ));
    }
    Ok(Some(input))
}

/// tsc-port: shouldEmitFunctionProperties @6.0.3
/// tsc-hash: 1019be7df9648f1710946cbbe99f1b872a3b8c516f16028a4da3dfcd0880e2e9
/// tsc-span: _tsc.js:114736-114743
pub(crate) fn should_emit_function_properties(
    transformer: &mut DeclarationTransformer<'_>,
    context: &TransformationContext,
    input: TransformNode,
) -> Result<bool, TransformError> {
    let data = function_data(context, input)?;
    if data.body.is_some() {
        return Ok(true);
    }
    let resolver_node = transformer.resolver_node(context, input)?;
    resolver_bool(
        transformer
            .resolver
            .is_last_bodiless_overload_of_symbol(resolver_node),
    )
}

/// tsc-port: isPreservedDeclarationStatement @6.0.3
/// tsc-hash: 7dec1787aaa63bc98f6c7853fa5c80b187326ddf34a13f68da25ea16cada13be
/// tsc-span: _tsc.js:115833-115849
pub(crate) fn is_preserved_declaration_statement(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<bool, TransformError> {
    Ok(matches!(
        context.arena().node(node)?.kind,
        SyntaxKind::FunctionDeclaration
            | SyntaxKind::ModuleDeclaration
            | SyntaxKind::ImportEqualsDeclaration
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::ClassDeclaration
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::EnumDeclaration
            | SyntaxKind::VariableStatement
            | SyntaxKind::ImportDeclaration
            | SyntaxKind::ExportDeclaration
            | SyntaxKind::ExportAssignment
    ))
}

/// P1 compatibility boundary for the declaration subtree visitor.
/// tsc-port: visitDeclarationSubtree @6.0.3
/// tsc-hash: 49f1c56e7d287ca5c9d8ac236fe1d91a482f181858627f0e21a6770dad67b16b
/// tsc-span: _tsc.js:114952-115256
#[allow(dead_code)]
pub(crate) fn visit_declaration_subtree(
    _transformer: &mut DeclarationTransformer<'_>,
    context: &TransformationContext,
    input: TransformNode,
) -> Result<VisitResult, TransformError> {
    Ok(Some(vec![input]).filter(|_| context.arena().node(input).is_ok()))
}

fn transform_variable_declaration(
    transformer: &mut DeclarationTransformer<'_>,
    context: &mut TransformationContext,
    input: TransformNode,
) -> Result<VisitResult, TransformError> {
    let data = variable_declaration_data(context, input)?;
    let name = required_node(
        context,
        input.source(),
        data.name,
        SyntaxKind::VariableDeclaration,
        "name",
    )?;
    if matches!(
        context.arena().node(name)?.kind,
        SyntaxKind::ArrayBindingPattern | SyntaxKind::ObjectBindingPattern
    ) {
        return Ok(Some(recreate_binding_pattern(transformer, context, name)?));
    }
    if !binding_name_visible(transformer, context, input)? {
        return Ok(None);
    }
    let type_node = data
        .r#type
        .and_then(|node| context.arena().node_ref(input.source(), node));
    let mut factory = context.factory()?;
    let updated = factory.update_variable_declaration(input, name, None, type_node, None)?;
    Ok(Some(vec![updated]))
}

fn expando_declaration_arm(
    transformer: &mut DeclarationTransformer<'_>,
    context: &mut TransformationContext,
    input: TransformNode,
    function: TransformNode,
) -> Result<Vec<TransformNode>, TransformError> {
    let resolver_node = transformer.resolver_node(context, input)?;
    if !resolver_bool(
        transformer
            .resolver
            .is_expando_function_declaration(resolver_node),
    )? {
        return Ok(Vec::new());
    }
    let properties = match transformer
        .resolver
        .get_properties_of_container_function(resolver_node)
    {
        Ok(properties) => properties,
        Err(EmitResolverError::Unavailable { .. }) => return Ok(Vec::new()),
        Err(error) => return Err(TransformError::Resolver(error)),
    };
    let enclosing_resolver = transformer
        .state
        .enclosing_declaration
        .and_then(|node| transformer.resolver_node(context, node).ok())
        .unwrap_or(resolver_node);
    let mut property_types = Vec::new();
    for property in properties {
        let Some(value_declaration) = property.value_declaration else {
            continue;
        };
        let Some(value_declaration_transform) = context
            .arena()
            .parse_tree_transform_node(value_declaration)?
        else {
            continue;
        };
        if !matches!(
            context.arena().node(value_declaration_transform)?.kind,
            SyntaxKind::PropertyAccessExpression
                | SyntaxKind::ElementAccessExpression
                | SyntaxKind::BinaryExpression
        ) || !tsc_syntax::is_identifier_text(&property.name)
        {
            continue;
        }
        let type_node = transformer
            .resolver
            .create_type_of_declaration_in_expando_scope(
                context.arena_mut()?,
                function.source(),
                value_declaration,
                resolver_node,
                enclosing_resolver,
                EmitNodeBuilderFlags::DECLARATION_EMIT,
                EmitInternalNodeBuilderFlags::DECLARATION_EMIT
                    .union(EmitInternalNodeBuilderFlags::NO_SYNTACTIC_PRINTER),
                &mut transformer.tracker,
            )
            .map_err(DeclarationTransformer::resolver_error)?;
        if let Some(type_node) = type_node {
            let is_keyword = is_non_contextual_keyword(&property.name);
            property_types.push((
                property.name,
                value_declaration_transform,
                type_node,
                is_keyword,
            ));
        }
    }
    if property_types.is_empty() {
        return Ok(Vec::new());
    }
    let mut declarations = Vec::new();
    let mut export_mappings = Vec::new();
    let function_data = function_data(context, function)?;
    let function_name = function_data
        .name
        .and_then(|node| context.arena().node_ref(function.source(), node));
    let function_modifiers = function_data
        .modifiers
        .and_then(|array| context.arena().node_array_ref(function.source(), array));
    let is_default = modifier_flags(context, function)?.contains(ModifierFlags::DEFAULT);
    let clean_flags = ModifierFlags::from_bits(
        (modifier_flags(context, function)?.bits()
            & !(ModifierFlags::DEFAULT.bits() | ModifierFlags::EXPORT.bits()))
            | ModifierFlags::AMBIENT.bits(),
    );
    let type_parameters = function_data
        .type_parameters
        .and_then(|array| context.arena().node_array_ref(function.source(), array));
    let parameters = function_data
        .parameters
        .and_then(|array| context.arena().node_array_ref(function.source(), array))
        .ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::FunctionDeclaration,
            field: "parameters",
        })?;
    let return_type = function_data
        .r#type
        .and_then(|node| context.arena().node_ref(function.source(), node));
    let mut factory = context.factory()?;
    let namespace_name = match function_name {
        Some(name) => name,
        None => factory.create_identifier(input.source(), "_default")?,
    };
    for (property_name, value_declaration, type_node, is_keyword) in property_types {
        let name = if is_keyword {
            factory.get_generated_name_for_node(
                value_declaration,
                crate::GeneratedIdentifierFlags::OPTIMISTIC,
                None,
                None,
            )?
        } else {
            factory.create_identifier(input.source(), &property_name)?
        };
        if is_keyword {
            export_mappings.push((name, property_name.clone()));
        }
        let declaration = factory.create_variable_declaration(
            input.source(),
            name,
            None,
            Some(type_node),
            None,
        )?;
        let declaration_array = factory.create_node_array(input.source(), vec![declaration])?;
        let list = factory.create_variable_declaration_list(
            input.source(),
            declaration_array,
            NodeFlags::CONST,
        )?;
        let modifiers = if is_keyword {
            None
        } else {
            let export = factory.create_modifier(input.source(), SyntaxKind::ExportKeyword)?;
            Some(factory.create_node_array(input.source(), vec![export])?)
        };
        let statement = factory.create_variable_statement(input.source(), modifiers, list)?;
        declarations.push(statement);
    }
    if export_mappings.is_empty() {
        declarations = declarations
            .into_iter()
            .map(|declaration| factory.replace_modifiers(declaration, None))
            .collect::<Result<Vec<_>, _>>()?;
    } else {
        let mut specifiers = Vec::new();
        for (generated, property_name) in export_mappings {
            let property_name = factory.create_identifier(input.source(), property_name)?;
            specifiers.push(factory.create_export_specifier(
                input.source(),
                false,
                Some(generated),
                property_name,
            )?);
        }
        let specifiers = factory.create_node_array(input.source(), specifiers)?;
        let named_exports = factory.create_named_exports(input.source(), specifiers)?;
        declarations.push(factory.create_export_declaration(
            input.source(),
            None,
            false,
            Some(named_exports),
            None,
            None,
        )?);
    }
    let body = {
        let statements = factory.create_node_array(input.source(), declarations)?;
        factory.create_module_block(input.source(), statements)?
    };
    if !is_default {
        let namespace = factory.create_module_declaration(
            input.source(),
            function_modifiers,
            namespace_name,
            Some(body),
            NodeFlags::NAMESPACE,
        )?;
        return Ok(vec![namespace]);
    }

    let clean_modifiers =
        factory.create_modifiers_from_modifier_flags(input.source(), clean_flags)?;
    let clean_function = factory.update_function_declaration(
        function,
        clean_modifiers,
        None,
        function_name,
        type_parameters,
        parameters,
        return_type,
        None,
    )?;
    let namespace = factory.create_module_declaration(
        input.source(),
        clean_modifiers,
        namespace_name,
        Some(body),
        NodeFlags::NAMESPACE,
    )?;
    let export_default =
        factory.create_export_assignment(input.source(), None, false, namespace_name)?;
    transformer.state.result_has_external_module_indicator = true;
    transformer.state.result_has_scope_marker = true;
    Ok(vec![clean_function, namespace, export_default])
}

fn ensure_type(
    _transformer: &mut DeclarationTransformer<'_>,
    context: &mut TransformationContext,
    input: TransformNode,
    type_id: Option<NodeId>,
) -> Result<Option<TransformNode>, TransformError> {
    if let Some(type_id) = type_id {
        return Ok(context.arena().node_ref(input.source(), type_id));
    }
    let mut factory = context.factory()?;
    Ok(Some(factory.create_keyword_type_node(
        input.source(),
        SyntaxKind::AnyKeyword,
    )?))
}

fn update_params_list(
    _transformer: &mut DeclarationTransformer<'_>,
    context: &mut TransformationContext,
    input: TransformNode,
    parameters: TransformNodeArray,
) -> Result<TransformNodeArray, TransformError> {
    if modifier_flags(context, input)?.contains(ModifierFlags::PRIVATE) {
        return context
            .factory()
            .and_then(|mut factory| factory.create_node_array(input.source(), Vec::new()));
    }
    let mut result = Vec::new();
    for parameter in context.arena().node_array(parameters)?.nodes.clone() {
        let parameter = TransformNode::new(parameters.source(), parameter);
        let data = parameter_data(context, parameter)?;
        let name = required_node(
            context,
            parameter.source(),
            data.name,
            SyntaxKind::Parameter,
            "name",
        )?;
        let modifiers = data
            .modifiers
            .and_then(|array| context.arena().node_array_ref(parameter.source(), array));
        let dot_dot_dot_token = data
            .dot_dot_dot_token
            .and_then(|node| context.arena().node_ref(parameter.source(), node));
        let question_token = data
            .question_token
            .and_then(|node| context.arena().node_ref(parameter.source(), node));
        let type_node = data
            .r#type
            .and_then(|node| context.arena().node_ref(parameter.source(), node));
        let updated = {
            let mut factory = context.factory()?;
            factory.update_parameter_declaration(
                parameter,
                modifiers,
                dot_dot_dot_token,
                name,
                question_token,
                type_node,
                None,
            )?
        };
        result.push(updated);
    }
    let mut factory = context.factory()?;
    factory.create_node_array(input.source(), result)
}

fn ensure_modifiers(
    transformer: &DeclarationTransformer<'_>,
    context: &mut TransformationContext,
    node: TransformNode,
) -> Result<Option<TransformNodeArray>, TransformError> {
    let current = match context.arena().node(node)?.data.clone() {
        NodeData::FunctionDeclaration(data) => data.modifiers,
        NodeData::ClassDeclaration(data) => data.modifiers,
        NodeData::InterfaceDeclaration(data) => data.modifiers,
        NodeData::TypeAliasDeclaration(data) => data.modifiers,
        NodeData::EnumDeclaration(data) => data.modifiers,
        NodeData::ModuleDeclaration(data) => data.modifiers,
        NodeData::VariableStatement(data) => data.modifiers,
        NodeData::Parameter(data) => data.modifiers,
        _ => None,
    };
    let current = current.and_then(|array| context.arena().node_array_ref(node.source(), array));
    let old_flags = modifier_flags_from_array(context, current)?;
    let new_flags = ensure_modifier_flags(transformer, context, node, old_flags)?;
    if old_flags == new_flags {
        return Ok(current);
    }
    context
        .factory()?
        .create_modifiers_from_modifier_flags(node.source(), new_flags)
}

fn ensure_modifier_flags(
    transformer: &DeclarationTransformer<'_>,
    context: &TransformationContext,
    node: TransformNode,
    current: ModifierFlags,
) -> Result<ModifierFlags, TransformError> {
    let mut bits = current.bits()
        & (ModifierFlags::ALL.bits()
            & !(ModifierFlags::PUBLIC.bits()
                | ModifierFlags::ASYNC.bits()
                | ModifierFlags::OVERRIDE.bits()));
    if transformer.state.needs_declare
        && !matches!(
            context.arena().node(node)?.kind,
            SyntaxKind::InterfaceDeclaration
        )
        && is_source_file_parent(context, node)?
    {
        bits |= ModifierFlags::AMBIENT.bits();
    }
    if bits & ModifierFlags::DEFAULT.bits() != 0 && bits & ModifierFlags::EXPORT.bits() == 0 {
        bits |= ModifierFlags::EXPORT.bits();
    }
    if bits & ModifierFlags::DEFAULT.bits() != 0 {
        bits &= !ModifierFlags::AMBIENT.bits();
    }
    Ok(ModifierFlags::from_bits(bits))
}

fn binding_name_visible(
    transformer: &mut DeclarationTransformer<'_>,
    context: &TransformationContext,
    node: TransformNode,
) -> Result<bool, TransformError> {
    if context.arena().node(node)?.kind == SyntaxKind::OmittedExpression {
        return Ok(false);
    }
    let data = match &context.arena().node(node)?.data {
        NodeData::VariableDeclaration(data) => data.name,
        NodeData::BindingElement(data) => data.name,
        _ => None,
    };
    if let Some(name) = data.and_then(|name| context.arena().node_ref(node.source(), name)) {
        if matches!(
            context.arena().node(name)?.kind,
            SyntaxKind::ArrayBindingPattern | SyntaxKind::ObjectBindingPattern
        ) {
            return Ok(source_array(
                context,
                name.source(),
                binding_pattern_elements(context, name)?,
            )?
            .into_iter()
            .any(|element| binding_name_visible(transformer, context, element).unwrap_or(true)));
        }
    }
    resolver_bool_for_node(transformer, context, node)
}

fn should_strip_internal(
    _transformer: &DeclarationTransformer<'_>,
    _context: &TransformationContext,
    _node: TransformNode,
) -> Result<bool, TransformError> {
    // P1 owns the source/JSDoc internal-declaration marker.  The bootstrap
    // option is refused by the production boundary, so the dormant P2 lane
    // remains fail-closed without guessing at comments.
    Ok(false)
}

fn is_declaration_and_not_visible(
    transformer: &mut DeclarationTransformer<'_>,
    context: &TransformationContext,
    node: TransformNode,
) -> Result<bool, TransformError> {
    match context.arena().node(node)?.kind {
        SyntaxKind::FunctionDeclaration
        | SyntaxKind::ModuleDeclaration
        | SyntaxKind::InterfaceDeclaration
        | SyntaxKind::ClassDeclaration
        | SyntaxKind::TypeAliasDeclaration
        | SyntaxKind::EnumDeclaration => Ok(!resolver_bool_for_node(transformer, context, node)?),
        SyntaxKind::ClassStaticBlockDeclaration => Ok(true),
        _ => Ok(false),
    }
}

fn resolver_bool_for_node(
    transformer: &mut DeclarationTransformer<'_>,
    context: &TransformationContext,
    node: TransformNode,
) -> Result<bool, TransformError> {
    let resolver_node = transformer.resolver_node(context, node)?;
    resolver_visibility(transformer.resolver.is_declaration_visible(resolver_node))
}

fn resolver_visibility(result: Result<bool, EmitResolverError>) -> Result<bool, TransformError> {
    match result {
        Ok(value) => Ok(value),
        // A syntax-only harness has no checker visibility table.  Declaration
        // syntax is retained until P1 supplies the authoritative answer.
        Err(EmitResolverError::Unavailable { .. }) => Ok(true),
        Err(error) => Err(TransformError::Resolver(error)),
    }
}

fn resolver_bool(result: Result<bool, EmitResolverError>) -> Result<bool, TransformError> {
    match result {
        Ok(value) => Ok(value),
        // P1's checker-backed resolver is absent in this checkout.  The
        // syntax-only fallback observes the upstream false branch while still
        // propagating real checker failures.
        Err(EmitResolverError::Unavailable { .. }) => Ok(false),
        Err(error) => Err(TransformError::Resolver(error)),
    }
}

fn is_enclosing_declaration(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<bool, TransformError> {
    Ok(matches!(
        context.arena().node(node)?.kind,
        SyntaxKind::SourceFile
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::ModuleDeclaration
            | SyntaxKind::ClassDeclaration
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::IndexSignature
            | SyntaxKind::MappedType
    ))
}

fn is_function_like_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::ArrowFunction
            | SyntaxKind::CallSignature
            | SyntaxKind::Constructor
            | SyntaxKind::ConstructSignature
            | SyntaxKind::ConstructorType
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::FunctionExpression
            | SyntaxKind::FunctionType
            | SyntaxKind::GetAccessor
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::MethodSignature
            | SyntaxKind::SetAccessor
    )
}

fn is_non_contextual_keyword(name: &str) -> bool {
    matches!(
        name,
        "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "debugger"
            | "default"
            | "delete"
            | "do"
            | "else"
            | "enum"
            | "export"
            | "extends"
            | "false"
            | "finally"
            | "for"
            | "function"
            | "if"
            | "import"
            | "in"
            | "instanceof"
            | "new"
            | "null"
            | "return"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "true"
            | "try"
            | "typeof"
            | "var"
            | "void"
            | "while"
            | "with"
    )
}

fn is_late_visibility_painted_statement(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<bool, TransformError> {
    // P1's late-visibility marker is an emitter metadata bit.  No marker is
    // synthesized by the compatibility scaffold, so ordinary statements take
    // the fast path while marked nodes remain recognized when P1 populates it.
    Ok(context
        .arena()
        .metadata(node)
        .is_some_and(|metadata| metadata.flags().contains(crate::EmitFlags::LOCAL_NAME)))
}

fn is_external_module_indicator(context: &TransformationContext, node: TransformNode) -> bool {
    matches!(
        context.arena().node(node).ok().map(|node| node.kind),
        Some(SyntaxKind::ImportDeclaration)
            | Some(SyntaxKind::ImportEqualsDeclaration)
            | Some(SyntaxKind::ExportDeclaration)
            | Some(SyntaxKind::ExportAssignment)
    )
}

fn is_source_file_parent(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<bool, TransformError> {
    let Some(parent) = context.arena().node(node)?.parent else {
        return Ok(false);
    };
    Ok(context
        .arena()
        .node_ref(node.source(), parent)
        .is_some_and(|parent| {
            context
                .arena()
                .node(parent)
                .is_ok_and(|parent| parent.kind == SyntaxKind::SourceFile)
        }))
}

fn modifier_flags(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<ModifierFlags, TransformError> {
    let array = match &context.arena().node(node)?.data {
        NodeData::FunctionDeclaration(data) => data.modifiers,
        NodeData::ClassDeclaration(data) => data.modifiers,
        NodeData::InterfaceDeclaration(data) => data.modifiers,
        NodeData::TypeAliasDeclaration(data) => data.modifiers,
        NodeData::EnumDeclaration(data) => data.modifiers,
        NodeData::ModuleDeclaration(data) => data.modifiers,
        NodeData::VariableStatement(data) => data.modifiers,
        NodeData::ImportEqualsDeclaration(data) => data.modifiers,
        NodeData::ImportDeclaration(data) => data.modifiers,
        NodeData::ExportAssignment(data) => data.modifiers,
        NodeData::ExportDeclaration(data) => data.modifiers,
        _ => None,
    }
    .and_then(|array| context.arena().node_array_ref(node.source(), array));
    modifier_flags_from_array(context, array)
}

fn modifier_flags_from_array(
    context: &TransformationContext,
    array: Option<TransformNodeArray>,
) -> Result<ModifierFlags, TransformError> {
    let Some(array) = array else {
        return Ok(ModifierFlags::NONE);
    };
    let mut flags = ModifierFlags::NONE;
    for node in context.arena().node_array(array)?.nodes.iter().copied() {
        flags |= match context
            .arena()
            .node(TransformNode::new(array.source(), node))?
            .kind
        {
            SyntaxKind::ExportKeyword => ModifierFlags::EXPORT,
            SyntaxKind::DeclareKeyword => ModifierFlags::AMBIENT,
            SyntaxKind::DefaultKeyword => ModifierFlags::DEFAULT,
            SyntaxKind::ConstKeyword => ModifierFlags::CONST,
            SyntaxKind::PublicKeyword => ModifierFlags::PUBLIC,
            SyntaxKind::PrivateKeyword => ModifierFlags::PRIVATE,
            SyntaxKind::ProtectedKeyword => ModifierFlags::PROTECTED,
            SyntaxKind::AbstractKeyword => ModifierFlags::ABSTRACT,
            SyntaxKind::StaticKeyword => ModifierFlags::STATIC,
            SyntaxKind::OverrideKeyword => ModifierFlags::OVERRIDE,
            SyntaxKind::ReadonlyKeyword => ModifierFlags::READONLY,
            SyntaxKind::AccessorKeyword => ModifierFlags::ACCESSOR,
            SyntaxKind::AsyncKeyword => ModifierFlags::ASYNC,
            SyntaxKind::InKeyword => ModifierFlags::IN,
            SyntaxKind::OutKeyword => ModifierFlags::OUT,
            _ => ModifierFlags::NONE,
        };
    }
    Ok(flags)
}

fn array_handle(
    context: &TransformationContext,
    source: TransformSourceId,
    array: Option<tsc_syntax::NodeArrayId>,
) -> Option<TransformNodeArray> {
    array.and_then(|array| context.arena().node_array_ref(source, array))
}

fn array_or_empty(
    context: &mut TransformationContext,
    source: TransformSourceId,
    array: Option<tsc_syntax::NodeArrayId>,
) -> Result<TransformNodeArray, TransformError> {
    if let Some(array) = array_handle(context, source, array) {
        return Ok(array);
    }
    context.factory()?.create_node_array(source, Vec::new())
}

fn source_array(
    context: &TransformationContext,
    source: TransformSourceId,
    array: Option<tsc_syntax::NodeArrayId>,
) -> Result<Vec<TransformNode>, TransformError> {
    let Some(array) = array_handle(context, source, array) else {
        return Ok(Vec::new());
    };
    Ok(context
        .arena()
        .node_array(array)?
        .nodes
        .iter()
        .copied()
        .map(|node| TransformNode::new(source, node))
        .collect())
}

fn required_node(
    context: &TransformationContext,
    source: TransformSourceId,
    node: Option<NodeId>,
    parent: SyntaxKind,
    field: &'static str,
) -> Result<TransformNode, TransformError> {
    node.and_then(|node| context.arena().node_ref(source, node))
        .ok_or(TransformError::RequiredChildRemoved { parent, field })
}

fn input_kind(
    context: &TransformationContext,
    input: TransformNode,
) -> Result<SyntaxKind, TransformError> {
    Ok(context.arena().node(input)?.kind)
}

fn binding_pattern_elements(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<Option<tsc_syntax::NodeArrayId>, TransformError> {
    Ok(match &context.arena().node(node)?.data {
        NodeData::ArrayBindingPattern(data) => data.elements,
        NodeData::ObjectBindingPattern(data) => data.elements,
        _ => None,
    })
}

fn export_declaration_data(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<ExportDeclarationData, TransformError> {
    match &context.arena().node(node)?.data {
        NodeData::ExportDeclaration(data) => Ok(data.clone()),
        _ => Err(TransformError::FactoryKindMismatch {
            expected: SyntaxKind::ExportDeclaration,
            actual: context.arena().node(node)?.kind,
        }),
    }
}

fn export_assignment_data(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<ExportAssignmentData, TransformError> {
    match &context.arena().node(node)?.data {
        NodeData::ExportAssignment(data) => Ok(data.clone()),
        _ => Err(TransformError::FactoryKindMismatch {
            expected: SyntaxKind::ExportAssignment,
            actual: context.arena().node(node)?.kind,
        }),
    }
}

fn type_alias_data(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<TypeAliasDeclarationData, TransformError> {
    match &context.arena().node(node)?.data {
        NodeData::TypeAliasDeclaration(data) => Ok(data.clone()),
        _ => Err(TransformError::FactoryKindMismatch {
            expected: SyntaxKind::TypeAliasDeclaration,
            actual: context.arena().node(node)?.kind,
        }),
    }
}

fn interface_data(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<InterfaceDeclarationData, TransformError> {
    match &context.arena().node(node)?.data {
        NodeData::InterfaceDeclaration(data) => Ok(data.clone()),
        _ => Err(TransformError::FactoryKindMismatch {
            expected: SyntaxKind::InterfaceDeclaration,
            actual: context.arena().node(node)?.kind,
        }),
    }
}

fn function_data(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<FunctionDeclarationData, TransformError> {
    match &context.arena().node(node)?.data {
        NodeData::FunctionDeclaration(data) => Ok(data.clone()),
        _ => Err(TransformError::FactoryKindMismatch {
            expected: SyntaxKind::FunctionDeclaration,
            actual: context.arena().node(node)?.kind,
        }),
    }
}

fn module_data(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<ModuleDeclarationData, TransformError> {
    match &context.arena().node(node)?.data {
        NodeData::ModuleDeclaration(data) => Ok(data.clone()),
        _ => Err(TransformError::FactoryKindMismatch {
            expected: SyntaxKind::ModuleDeclaration,
            actual: context.arena().node(node)?.kind,
        }),
    }
}

fn module_block_data(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<ModuleBlockData, TransformError> {
    match &context.arena().node(node)?.data {
        NodeData::ModuleBlock(data) => Ok(data.clone()),
        _ => Err(TransformError::FactoryKindMismatch {
            expected: SyntaxKind::ModuleBlock,
            actual: context.arena().node(node)?.kind,
        }),
    }
}

fn class_data(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<ClassDeclarationData, TransformError> {
    match &context.arena().node(node)?.data {
        NodeData::ClassDeclaration(data) => Ok(data.clone()),
        _ => Err(TransformError::FactoryKindMismatch {
            expected: SyntaxKind::ClassDeclaration,
            actual: context.arena().node(node)?.kind,
        }),
    }
}

fn enum_data(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<EnumDeclarationData, TransformError> {
    match &context.arena().node(node)?.data {
        NodeData::EnumDeclaration(data) => Ok(data.clone()),
        _ => Err(TransformError::FactoryKindMismatch {
            expected: SyntaxKind::EnumDeclaration,
            actual: context.arena().node(node)?.kind,
        }),
    }
}

fn variable_statement_data(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<VariableStatementData, TransformError> {
    match &context.arena().node(node)?.data {
        NodeData::VariableStatement(data) => Ok(data.clone()),
        _ => Err(TransformError::FactoryKindMismatch {
            expected: SyntaxKind::VariableStatement,
            actual: context.arena().node(node)?.kind,
        }),
    }
}

fn variable_declaration_list_data(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<VariableDeclarationListData, TransformError> {
    match &context.arena().node(node)?.data {
        NodeData::VariableDeclarationList(data) => Ok(data.clone()),
        _ => Err(TransformError::FactoryKindMismatch {
            expected: SyntaxKind::VariableDeclarationList,
            actual: context.arena().node(node)?.kind,
        }),
    }
}

fn variable_declaration_data(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<VariableDeclarationData, TransformError> {
    match &context.arena().node(node)?.data {
        NodeData::VariableDeclaration(data) => Ok(data.clone()),
        _ => Err(TransformError::FactoryKindMismatch {
            expected: SyntaxKind::VariableDeclaration,
            actual: context.arena().node(node)?.kind,
        }),
    }
}

fn binding_element_data(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<BindingElementData, TransformError> {
    match &context.arena().node(node)?.data {
        NodeData::BindingElement(data) => Ok(data.clone()),
        _ => Err(TransformError::FactoryKindMismatch {
            expected: SyntaxKind::BindingElement,
            actual: context.arena().node(node)?.kind,
        }),
    }
}

fn import_equals_data(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<ImportEqualsDeclarationData, TransformError> {
    match &context.arena().node(node)?.data {
        NodeData::ImportEqualsDeclaration(data) => Ok(data.clone()),
        _ => Err(TransformError::FactoryKindMismatch {
            expected: SyntaxKind::ImportEqualsDeclaration,
            actual: context.arena().node(node)?.kind,
        }),
    }
}

fn external_module_reference_data(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<ExternalModuleReferenceData, TransformError> {
    match &context.arena().node(node)?.data {
        NodeData::ExternalModuleReference(data) => Ok(data.clone()),
        _ => Err(TransformError::FactoryKindMismatch {
            expected: SyntaxKind::ExternalModuleReference,
            actual: context.arena().node(node)?.kind,
        }),
    }
}

fn import_declaration_data(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<ImportDeclarationData, TransformError> {
    match &context.arena().node(node)?.data {
        NodeData::ImportDeclaration(data) => Ok(data.clone()),
        _ => Err(TransformError::FactoryKindMismatch {
            expected: SyntaxKind::ImportDeclaration,
            actual: context.arena().node(node)?.kind,
        }),
    }
}

fn import_clause_data(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<ImportClauseData, TransformError> {
    match &context.arena().node(node)?.data {
        NodeData::ImportClause(data) => Ok(data.clone()),
        _ => Err(TransformError::FactoryKindMismatch {
            expected: SyntaxKind::ImportClause,
            actual: context.arena().node(node)?.kind,
        }),
    }
}

fn named_imports_data(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<NamedImportsData, TransformError> {
    match &context.arena().node(node)?.data {
        NodeData::NamedImports(data) => Ok(data.clone()),
        _ => Err(TransformError::FactoryKindMismatch {
            expected: SyntaxKind::NamedImports,
            actual: context.arena().node(node)?.kind,
        }),
    }
}

fn parameter_data(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<ParameterData, TransformError> {
    match &context.arena().node(node)?.data {
        NodeData::Parameter(data) => Ok(data.clone()),
        _ => Err(TransformError::FactoryKindMismatch {
            expected: SyntaxKind::Parameter,
            actual: context.arena().node(node)?.kind,
        }),
    }
}

fn heritage_clause_data(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<HeritageClauseData, TransformError> {
    match &context.arena().node(node)?.data {
        NodeData::HeritageClause(data) => Ok(data.clone()),
        _ => Err(TransformError::FactoryKindMismatch {
            expected: SyntaxKind::HeritageClause,
            actual: context.arena().node(node)?.kind,
        }),
    }
}

use tsc_syntax::nodes::*;
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{ModifierFlags, NodeFlags};

use crate::{
    EmitConstantValue, EmitInternalNodeBuilderFlags, EmitNodeBuilderFlags,
    GeneratedIdentifierFlags, NodeFactory, TransformError, TransformFlags, TransformNode,
    TransformNodeArray, TransformSourceId, TransformationContext,
};

use super::state::adopt_result;
use super::tracker::materialize_effects;
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
        || transformer.should_strip_internal(context, Some(input))?
    {
        return Ok(VisitResult::None);
    }

    match context.arena().node(input)?.kind {
        SyntaxKind::ExportDeclaration => {
            let data = export_declaration_data(context, input)?;
            transformer.state_mut()?.result_has_scope_marker = true;
            if is_source_file_parent(context, input)? {
                transformer
                    .state_mut()?
                    .result_has_external_module_indicator = true;
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
            Ok(VisitResult::Node(updated))
        }
        SyntaxKind::ExportAssignment => {
            let data = export_assignment_data(context, input)?;
            transformer.state_mut()?.result_has_scope_marker = true;
            if is_source_file_parent(context, input)? {
                transformer
                    .state_mut()?
                    .result_has_external_module_indicator = true;
            }
            let expression = required_node(
                context,
                input.source(),
                data.expression,
                SyntaxKind::ExportAssignment,
                "expression",
            )?;
            if context.arena().node(expression)?.kind == SyntaxKind::Identifier {
                return Ok(VisitResult::Node(input));
            }

            let original_modifiers = data
                .modifiers
                .and_then(|array| context.arena().node_array_ref(input.source(), array));
            let new_id = context.factory()?.create_unique_name(
                input.source(),
                "_default",
                crate::GeneratedIdentifierFlags::OPTIMISTIC,
            )?;
            let saved_diagnostic = transformer.tracker.replace_diagnostic_context(
                context.arena(),
                super::diagnostics::DiagnosticContext::DefaultExport(input),
            )?;
            let saved_error_fallback = transformer
                .tracker
                .error_fallback_node
                .replace(super::tracker::TrackerAnchor::Transform(input));
            let type_node = transformer.ensure_type(context, input, false);
            transformer.tracker.error_fallback_node = saved_error_fallback;
            transformer
                .tracker
                .restore_diagnostic_context(saved_diagnostic);
            let type_node = type_node?;
            let statement = {
                let mut factory = context.factory()?;
                let declaration = factory.create_variable_declaration(
                    input.source(),
                    new_id,
                    None,
                    type_node,
                    None,
                )?;
                let declarations = factory.create_node_array(input.source(), vec![declaration])?;
                let declaration_list = factory.create_variable_declaration_list(
                    input.source(),
                    declarations,
                    NodeFlags::CONST,
                )?;
                let modifiers = if transformer.state()?.needs_declare {
                    factory.create_modifiers_from_modifier_flags(
                        input.source(),
                        ModifierFlags::AMBIENT,
                    )?
                } else {
                    None
                };
                factory.create_variable_statement(input.source(), modifiers, declaration_list)?
            };
            let statement = super::subtree::preserve_js_doc(context, statement, input)?;
            context.arena_mut()?.remove_all_comments(input);
            let assignment =
                context
                    .factory()?
                    .update_export_assignment(input, original_modifiers, new_id)?;
            Ok(VisitResult::Nodes(vec![statement, assignment]))
        }
        _ => {
            let result = transform_top_level_declaration(transformer, context, input)?;
            let key = context.arena().get_original_node(input).node();
            transformer
                .state_mut()?
                .late_statement_replacement
                .insert(key, result);
            Ok(VisitResult::Node(input))
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
    let result = transform_top_level_declaration_worker(transformer, context, input);
    if let Ok(result) = &result {
        transformer.observe_boundary(context, true, input, result);
    }
    result
}

fn transform_top_level_declaration_worker(
    transformer: &mut DeclarationTransformer<'_>,
    context: &mut TransformationContext,
    input: TransformNode,
) -> Result<VisitResult, TransformError> {
    if let Some(late) = transformer.tracker.late_marked_statements.as_mut() {
        // tsc's `orderedRemoveItem` removes every occurrence before the
        // top-level switch consumes the statement.
        late.retain(|candidate| *candidate != input);
    }
    if transformer.should_strip_internal(context, Some(input))? {
        return Ok(VisitResult::None);
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

    if transformer.is_declaration_and_not_visible(context, input)?
        || context.arena().node(input)?.kind == SyntaxKind::JSDocImportTag
    {
        return Ok(VisitResult::None);
    }

    if super::ensure::is_function_like(context.arena().node(input)?.kind) {
        let resolver_node = transformer.required_resolver_node(context, input)?;
        if transformer
            .resolver
            .is_implementation_of_overload(resolver_node)?
        {
            return Ok(VisitResult::None);
        }
    }

    let previous_enclosing = transformer.state()?.enclosing_declaration;
    if transformer.is_enclosing_declaration(context, input)? {
        transformer.state_mut()?.enclosing_declaration = Some(input);
    }
    let previous_diagnostic =
        if super::diagnostics::can_produce_diagnostics(context.arena().node(input)?.kind) {
            Some(transformer.tracker.replace_diagnostic_context(
                context.arena(),
                super::diagnostics::DiagnosticContext::ForNode(input),
            )?)
        } else {
            None
        };
    let previous_needs_declare = transformer.state()?.needs_declare;
    let result = (|| -> Result<VisitResult, TransformError> {
        Ok(match context.arena().node(input)?.kind {
            SyntaxKind::TypeAliasDeclaration => {
                transformer.state_mut()?.needs_declare = false;
                let data = type_alias_data(context, input)?;
                let name = required_node(
                    context,
                    input.source(),
                    data.name,
                    input_kind(context, input)?,
                    "name",
                )?;
                let type_parameters = transformer.visit_type_node_array(
                    context,
                    input.source(),
                    data.type_parameters,
                    SyntaxKind::TypeParameter,
                )?;
                let type_node =
                    visit_required_subtree(transformer, context, input, data.r#type, "type")?;
                let modifiers = transformer.ensure_modifiers(context, input)?;
                let mut factory = context.factory()?;
                let updated = factory.update_type_alias_declaration(
                    input,
                    modifiers,
                    name,
                    type_parameters,
                    type_node,
                )?;
                VisitResult::Node(updated)
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
                let modifiers = transformer.ensure_modifiers(context, input)?;
                let type_parameters =
                    transformer.ensure_type_params(context, input, data.type_parameters)?;
                let heritage = transform_heritage_clauses(
                    transformer,
                    context,
                    array_handle(context, input.source(), data.heritage_clauses),
                )?;
                let members =
                    visit_subtree_array(transformer, context, input.source(), data.members)?;
                let mut factory = context.factory()?;
                let updated = factory.update_interface_declaration(
                    input,
                    modifiers,
                    name,
                    type_parameters,
                    heritage,
                    members,
                )?;
                VisitResult::Node(updated)
            }
            SyntaxKind::FunctionDeclaration => {
                let data = function_data(context, input)?;
                let modifiers = transformer.ensure_modifiers(context, input)?;
                let type_parameters =
                    transformer.ensure_type_params(context, input, data.type_parameters)?;
                let parameters = transformer.update_params_list(
                    context,
                    input,
                    data.parameters,
                    ModifierFlags::from_bits(
                        ModifierFlags::ALL.bits() ^ ModifierFlags::PUBLIC.bits(),
                    ),
                )?;
                let type_node = transformer.ensure_type(context, input, false)?;
                let name = data
                    .name
                    .and_then(|node| context.arena().node_ref(input.source(), node));
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
                let is_expando = transformer.resolver.is_expando_function_declaration(
                    transformer.required_resolver_node(context, input)?,
                )?;
                if is_expando && should_emit_function_properties(transformer, context, input)? {
                    outputs.extend(expando_declaration_arm(
                        transformer,
                        context,
                        input,
                        updated,
                    )?);
                }
                VisitResult::Nodes(outputs)
            }
            SyntaxKind::ModuleDeclaration => {
                let data = module_data(context, input)?;
                transformer.state_mut()?.needs_declare = false;
                let inner = data
                    .body
                    .and_then(|node| context.arena().node_ref(input.source(), node));
                let (body, modifiers) = if let Some(body) = inner {
                    if context.arena().node(body)?.kind == SyntaxKind::ModuleBlock {
                        let previous_needs_scope = transformer.state()?.needs_scope_fix_marker;
                        let previous_has_scope = transformer.state()?.result_has_scope_marker;
                        transformer.state_mut()?.needs_scope_fix_marker = false;
                        transformer.state_mut()?.result_has_scope_marker = false;
                        let block_result = (|| -> Result<TransformNode, TransformError> {
                            let block_data = module_block_data(context, body)?;
                            let mut statements = Vec::new();
                            for statement in
                                source_array(context, body.source(), block_data.statements)?
                            {
                                match visit_declaration_statement(transformer, context, statement)?
                                {
                                    VisitResult::None => {}
                                    VisitResult::Node(statement) => statements.push(statement),
                                    VisitResult::Nodes(result) => statements.extend(result),
                                }
                            }
                            let mut statements = transform_and_replace_late_painted_statements(
                                transformer,
                                context,
                                statements,
                            )?;
                            if NodeFlags::from_bits(context.arena().node(input)?.flags)
                                .contains(NodeFlags::AMBIENT)
                            {
                                transformer.state_mut()?.needs_scope_fix_marker = false;
                            }
                            let is_global_augmentation =
                                NodeFlags::from_bits(context.arena().node(input)?.flags)
                                    .contains(NodeFlags::GLOBAL_AUGMENTATION);
                            if !is_global_augmentation
                                && !has_scope_marker(context, &statements)
                                && !transformer.state()?.result_has_scope_marker
                            {
                                if transformer.state()?.needs_scope_fix_marker {
                                    let empty = create_empty_exports(
                                        &mut context.factory()?,
                                        body.source(),
                                    )?;
                                    statements.push(empty);
                                } else {
                                    let mut stripped = Vec::with_capacity(statements.len());
                                    for statement in statements {
                                        stripped.push(strip_export_modifiers(context, statement)?);
                                    }
                                    statements = stripped;
                                }
                            }
                            let array = match block_data.statements.and_then(|array| {
                                context.arena().node_array_ref(body.source(), array)
                            }) {
                                Some(original) => {
                                    context.factory()?.update_node_array(original, statements)?
                                }
                                None => context
                                    .factory()?
                                    .create_node_array(body.source(), statements)?,
                            };
                            context.factory()?.update_module_block(body, array)
                        })();
                        transformer.state_mut()?.needs_declare = previous_needs_declare;
                        transformer.state_mut()?.needs_scope_fix_marker = previous_needs_scope;
                        transformer.state_mut()?.result_has_scope_marker = previous_has_scope;
                        let body = block_result?;
                        let modifiers = transformer.ensure_modifiers(context, input)?;
                        (Some(body), modifiers)
                    } else {
                        transformer.state_mut()?.needs_declare = previous_needs_declare;
                        let modifiers = transformer.ensure_modifiers(context, input)?;
                        transformer.state_mut()?.needs_declare = false;
                        let result = visit_declaration_statement(transformer, context, body)?;
                        let key = context.arena().get_original_node(body).node();
                        let replacement = transformer
                            .state_mut()?
                            .late_statement_replacement
                            .remove(&key)
                            .unwrap_or(result);
                        let body = match replacement {
                            VisitResult::Node(body) => Some(body),
                            VisitResult::None => None,
                            VisitResult::Nodes(_) => {
                                return Err(DeclarationTransformer::contract(
                                    "nested module body expanded to multiple statements",
                                ));
                            }
                        };
                        (body, modifiers)
                    }
                } else {
                    transformer.state_mut()?.needs_declare = previous_needs_declare;
                    let modifiers = transformer.ensure_modifiers(context, input)?;
                    transformer.state_mut()?.needs_declare = false;
                    (None, modifiers)
                };
                let name = required_node(
                    context,
                    input.source(),
                    data.name,
                    input_kind(context, input)?,
                    "name",
                )?;
                VisitResult::Node(update_module_declaration_and_keyword(
                    transformer,
                    context,
                    input,
                    modifiers,
                    name,
                    body,
                )?)
            }
            SyntaxKind::ClassDeclaration => {
                transformer.tracker.error_name_node = class_data(context, input)?
                    .name
                    .and_then(|node| context.arena().node_ref(input.source(), node));
                transformer.tracker.error_fallback_node =
                    Some(super::tracker::TrackerAnchor::Transform(input));
                let data = class_data(context, input)?;
                let name = data
                    .name
                    .and_then(|node| context.arena().node_ref(input.source(), node));
                let original_members = array_or_empty(context, input.source(), data.members)?;
                let modifiers = transformer.ensure_modifiers(context, input)?;
                let type_parameters =
                    transformer.ensure_type_params(context, input, data.type_parameters)?;
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
                let class_resolver_node = transformer.required_resolver_node(context, input)?;
                let enclosing_resolver = transformer
                    .state()?
                    .enclosing_declaration
                    .and_then(|node| transformer.required_resolver_node(context, node).ok())
                    .unwrap_or(class_resolver_node);
                let late_indexes_result = transformer.resolver.create_late_bound_index_signatures(
                    context.arena_mut()?,
                    input.source(),
                    class_resolver_node,
                    enclosing_resolver,
                    EmitNodeBuilderFlags::DECLARATION_EMIT,
                    EmitInternalNodeBuilderFlags::DECLARATION_EMIT,
                    &mut transformer.tracker,
                );
                let effects = transformer.tracker.take_pending_effects();
                materialize_effects(context, transformer.host, effects)?;
                let late_indexes = late_indexes_result?.unwrap_or_default();
                member_nodes.extend(late_indexes);
                member_nodes.extend(constructor_properties);
                for member in source_array(
                    context,
                    original_members.source(),
                    Some(original_members.array()),
                )? {
                    match transformer.visit_declaration_subtree(context, member)? {
                        VisitResult::None => {}
                        VisitResult::Node(member) => member_nodes.push(member),
                        VisitResult::Nodes(nodes) => member_nodes.extend(nodes),
                    }
                }
                let members = {
                    let mut factory = context.factory()?;
                    factory.update_node_array(original_members, member_nodes)?
                };
                let heritage = array_handle(context, input.source(), data.heritage_clauses);
                if let Some((base_type, base_expression)) =
                    effective_base_type_expression(context, heritage)?
                {
                    if !transformer.is_entity_name_expression(context, base_expression)?
                        && context.arena().node(base_expression)?.kind != SyntaxKind::NullKeyword
                    {
                        let base_name = match name
                            .and_then(|name| context.arena().node(name).ok())
                            .map(|record| &record.data)
                        {
                            Some(NodeData::Identifier(data)) => data.text.clone(),
                            _ => "default".to_owned(),
                        };
                        let generated = context.factory()?.create_unique_name(
                            input.source(),
                            format!("{base_name}_base"),
                            GeneratedIdentifierFlags::OPTIMISTIC,
                        )?;
                        let saved_diagnostic = transformer.tracker.replace_diagnostic_context(
                            context.arena(),
                            super::diagnostics::DiagnosticContext::ForNode(base_type),
                        )?;
                        let base_type_node = (|| {
                            let expression_resolver =
                                transformer.required_resolver_node(context, base_expression)?;
                            let class_resolver =
                                transformer.required_resolver_node(context, input)?;
                            let result = transformer.resolver.create_type_of_expression(
                                context.arena_mut()?,
                                input.source(),
                                expression_resolver,
                                class_resolver,
                                EmitNodeBuilderFlags::DECLARATION_EMIT,
                                EmitInternalNodeBuilderFlags::DECLARATION_EMIT,
                                &mut transformer.tracker,
                            );
                            let effects = transformer.tracker.take_pending_effects();
                            materialize_effects(context, transformer.host, effects)?;
                            result.map_err(TransformError::from)
                        })();
                        transformer
                            .tracker
                            .restore_diagnostic_context(saved_diagnostic);
                        let base_type_node = base_type_node?;
                        let statement = {
                            let mut factory = context.factory()?;
                            let declaration = factory.create_variable_declaration(
                                input.source(),
                                generated,
                                None,
                                base_type_node,
                                None,
                            )?;
                            let declarations =
                                factory.create_node_array(input.source(), vec![declaration])?;
                            let declaration_list = factory.create_variable_declaration_list(
                                input.source(),
                                declarations,
                                NodeFlags::CONST,
                            )?;
                            let statement_modifiers = if transformer.state()?.needs_declare {
                                factory.create_modifiers_from_modifier_flags(
                                    input.source(),
                                    ModifierFlags::AMBIENT,
                                )?
                            } else {
                                Some(factory.create_node_array(input.source(), Vec::new())?)
                            };
                            factory.create_variable_statement(
                                input.source(),
                                statement_modifiers,
                                declaration_list,
                            )?
                        };
                        let heritage = transform_heritage_clauses_with_base(
                            transformer,
                            context,
                            heritage,
                            generated,
                        )?;
                        let updated = context.factory()?.update_class_declaration(
                            input,
                            modifiers,
                            name,
                            type_parameters,
                            heritage,
                            members,
                        )?;
                        VisitResult::Nodes(vec![statement, updated])
                    } else {
                        let heritage = transform_heritage_clauses(transformer, context, heritage)?;
                        let updated = context.factory()?.update_class_declaration(
                            input,
                            modifiers,
                            name,
                            type_parameters,
                            heritage,
                            members,
                        )?;
                        VisitResult::Node(updated)
                    }
                } else {
                    let heritage = transform_heritage_clauses(transformer, context, heritage)?;
                    let updated = context.factory()?.update_class_declaration(
                        input,
                        modifiers,
                        name,
                        type_parameters,
                        heritage,
                        members,
                    )?;
                    VisitResult::Node(updated)
                }
            }
            SyntaxKind::VariableStatement => {
                transform_variable_statement(transformer, context, input)?
            }
            SyntaxKind::EnumDeclaration => {
                let data = enum_data(context, input)?;
                let name = required_node(
                    context,
                    input.source(),
                    data.name,
                    input_kind(context, input)?,
                    "name",
                )?;
                let mut members = Vec::new();
                for member in source_array(context, input.source(), data.members)? {
                    if transformer.should_strip_internal(context, Some(member))? {
                        continue;
                    }
                    let member_data = match &context.arena().node(member)?.data {
                        NodeData::EnumMember(data) => data.clone(),
                        _ => {
                            return Err(DeclarationTransformer::contract(
                                "enum declaration contains a non-enum member",
                            ));
                        }
                    };
                    let name = required_node(
                        context,
                        member.source(),
                        member_data.name,
                        SyntaxKind::EnumMember,
                        "name",
                    )?;
                    let value = transformer.resolver.get_enum_member_value(
                        transformer.required_resolver_node(context, member)?,
                    )?;
                    let initializer = value
                        .as_ref()
                        .and_then(crate::EmitEnumMemberValue::value)
                        .map(|value| enum_constant_initializer(context, member.source(), value))
                        .transpose()?;
                    let updated =
                        context
                            .factory()?
                            .update_enum_member(member, name, initializer)?;
                    members.push(super::subtree::preserve_js_doc(context, updated, member)?);
                }
                let modifiers = transformer.ensure_modifiers(context, input)?;
                let mut factory = context.factory()?;
                let members = factory.create_node_array(input.source(), members)?;
                let updated = factory.update_enum_declaration(input, modifiers, name, members)?;
                VisitResult::Node(updated)
            }
            _ => VisitResult::Node(input),
        })
    })();

    finish_top_level(
        transformer,
        context,
        input,
        previous_enclosing,
        previous_diagnostic,
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
    previous_diagnostic: Option<(
        super::diagnostics::DiagnosticContext,
        super::diagnostics::DiagnosticContextPlan,
    )>,
    previous_needs_declare: bool,
    result: Result<VisitResult, TransformError>,
) -> Result<VisitResult, TransformError> {
    transformer.state_mut()?.enclosing_declaration = previous_enclosing;
    if let Some(previous_diagnostic) = previous_diagnostic {
        transformer
            .tracker
            .restore_diagnostic_context(previous_diagnostic);
    }
    transformer.state_mut()?.needs_declare = previous_needs_declare;
    transformer.tracker.error_name_node = None;
    transformer.tracker.error_fallback_node = None;
    let result = result?;
    let result = match result {
        VisitResult::Node(node) => adopt_result(context, input, VisitResult::Node(node))?,
        VisitResult::Nodes(nodes) => {
            let mut adopted = Vec::with_capacity(nodes.len());
            for node in nodes {
                if context.arena().node(node)?.kind == context.arena().node(input)?.kind {
                    match adopt_result(context, input, VisitResult::Node(node))? {
                        VisitResult::Node(node) => adopted.push(node),
                        VisitResult::None | VisitResult::Nodes(_) => {
                            return Err(DeclarationTransformer::contract(
                                "top-level adoption changed a single-node result shape",
                            ));
                        }
                    }
                } else {
                    adopted.push(node);
                }
            }
            VisitResult::Nodes(adopted)
        }
        VisitResult::None => VisitResult::None,
    };
    Ok(result)
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
        .tracker
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
        let previous_needs_declare = transformer.state()?.needs_declare;
        transformer.state_mut()?.needs_declare = is_source_file_parent(context, input)?;
        let result = transform_top_level_declaration(transformer, context, input)?;
        transformer.state_mut()?.needs_declare = previous_needs_declare;
        transformer
            .state_mut()?
            .late_statement_replacement
            .insert(context.arena().get_original_node(input).node(), result);
    }

    let mut replaced = Vec::with_capacity(statements.len());
    for statement in statements.drain(..) {
        match visit_late_visibility_marked_statement(transformer, context, statement)? {
            VisitResult::None => {}
            VisitResult::Node(statement) => replaced.push(statement),
            VisitResult::Nodes(result) => replaced.extend(result),
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
        return Ok(VisitResult::Node(statement));
    }
    let key = context.arena().get_original_node(statement).node();
    let Some(result) = transformer
        .state_mut()?
        .late_statement_replacement
        .remove(&key)
    else {
        return Ok(VisitResult::Node(statement));
    };
    let result_nodes: &[TransformNode] = match &result {
        VisitResult::None => &[],
        VisitResult::Node(node) => std::slice::from_ref(node),
        VisitResult::Nodes(nodes) => nodes,
    };
    if !result_nodes.is_empty() {
        if result_nodes
            .iter()
            .copied()
            .map(|node| needs_scope_marker(context, node))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .any(|needed| needed)
        {
            transformer.state_mut()?.needs_scope_fix_marker = true;
        }
        if is_source_file_parent(context, statement)?
            && result_nodes
                .iter()
                .copied()
                .map(|node| is_external_module_indicator(context, node))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .any(|indicator| indicator)
        {
            transformer
                .state_mut()?
                .result_has_external_module_indicator = true;
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
) -> Result<VisitResult, TransformError> {
    let data = variable_statement_data(context, input)?;
    let list = required_node(
        context,
        input.source(),
        data.declaration_list,
        SyntaxKind::VariableStatement,
        "declarationList",
    )?;
    let list_data = variable_declaration_list_data(context, list)?;
    let source_declarations = source_array(context, input.source(), list_data.declarations)?;
    let mut any_visible = false;
    for declaration in &source_declarations {
        if transformer.binding_name_visible(context, *declaration)? {
            any_visible = true;
            break;
        }
    }
    if !any_visible {
        return Ok(VisitResult::None);
    }
    let mut declarations = Vec::new();
    for declaration in source_declarations {
        match transformer.visit_declaration_subtree(context, declaration)? {
            VisitResult::None => {}
            VisitResult::Node(declaration) => declarations.push(declaration),
            VisitResult::Nodes(result) => declarations.extend(result),
        }
    }
    let modifiers = transformer.ensure_modifiers(context, input)?;
    let original_declarations = list_data
        .declarations
        .and_then(|array| context.arena().node_array_ref(input.source(), array));
    let mut factory = context.factory()?;
    let declarations = match original_declarations {
        Some(original) => factory.update_node_array(original, declarations)?,
        None => factory.create_node_array(input.source(), declarations)?,
    };
    let declaration_list = factory.update_variable_declaration_list(list, declarations)?;
    Ok(VisitResult::Node(factory.update_variable_statement(
        input,
        modifiers,
        declaration_list,
    )?))
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
            || transformer.should_strip_internal(context, Some(parameter))?
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
            let saved_diagnostic = transformer.tracker.replace_diagnostic_context(
                context.arena(),
                super::diagnostics::DiagnosticContext::ForNode(parameter),
            )?;
            let properties = walk_binding_pattern(transformer, context, name, parameter);
            transformer
                .tracker
                .restore_diagnostic_context(saved_diagnostic);
            result.extend(properties?);
            continue;
        }
        let saved_diagnostic = transformer.tracker.replace_diagnostic_context(
            context.arena(),
            super::diagnostics::DiagnosticContext::ForNode(parameter),
        )?;
        let property = (|| {
            let modifiers = transformer.ensure_modifiers(context, parameter)?;
            let question_token = data
                .question_token
                .and_then(|node| context.arena().node_ref(parameter.source(), node));
            let type_node = transformer.ensure_type(context, parameter, false)?;
            context.factory()?.create_property_declaration(
                parameter.source(),
                modifiers,
                name,
                question_token,
                type_node,
                None,
            )
        })();
        transformer
            .tracker
            .restore_diagnostic_context(saved_diagnostic);
        result.push(property?);
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
            || transformer.should_strip_internal(context, Some(element))?
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
        if !transformer.binding_name_visible(context, element)? {
            continue;
        }
        if matches!(
            context.arena().node(name)?.kind,
            SyntaxKind::ArrayBindingPattern | SyntaxKind::ObjectBindingPattern
        ) {
            result.extend(walk_binding_pattern(transformer, context, name, parameter)?);
            continue;
        }
        let modifiers = transformer.ensure_modifiers(context, parameter)?;
        let type_node = transformer.ensure_type(context, element, false)?;
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
) -> Result<VisitResult, TransformError> {
    let elements = match &context.arena().node(pattern)?.data {
        NodeData::ArrayBindingPattern(data) => data.elements,
        NodeData::ObjectBindingPattern(data) => data.elements,
        _ => None,
    };
    let mut result = Vec::new();
    for element in source_array(context, pattern.source(), elements)? {
        match recreate_binding_element(transformer, context, element)? {
            VisitResult::None => {}
            VisitResult::Node(node) => result.push(node),
            VisitResult::Nodes(nodes) => result.extend(nodes),
        }
    }
    Ok(VisitResult::Nodes(result))
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
        return Ok(VisitResult::None);
    }
    let data = binding_element_data(context, element)?;
    let Some(name) = data
        .name
        .and_then(|node| context.arena().node_ref(element.source(), node))
    else {
        return Ok(VisitResult::None);
    };
    if matches!(
        context.arena().node(name)?.kind,
        SyntaxKind::ArrayBindingPattern | SyntaxKind::ObjectBindingPattern
    ) {
        return recreate_binding_pattern(transformer, context, name);
    }
    if !transformer.binding_name_visible(context, element)? {
        return Ok(VisitResult::None);
    }
    let type_node = transformer.ensure_type(context, element, false);
    let mut factory = context.factory()?;
    let declaration =
        factory.create_variable_declaration(element.source(), name, None, type_node?, None)?;
    Ok(VisitResult::Node(declaration))
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
    let is_ambient_module = context.arena().node(name)?.kind == SyntaxKind::StringLiteral
        || flags.contains(NodeFlags::GLOBAL_AUGMENTATION);
    if is_ambient_module || flags.contains(NodeFlags::NAMESPACE) {
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
    transformer: &mut DeclarationTransformer<'_>,
    context: &mut TransformationContext,
    clauses: Option<TransformNodeArray>,
) -> Result<Option<TransformNodeArray>, TransformError> {
    let Some(clauses) = clauses else {
        return Ok(Some(context.factory()?.create_node_array(
            transformer.state()?.current_source_file,
            Vec::new(),
        )?));
    };
    let clause_nodes = context.arena().node_array(clauses)?.nodes.clone();
    let mut result = Vec::new();
    for clause in clause_nodes {
        let clause = TransformNode::new(clauses.source(), clause);
        let data = heritage_clause_data(context, clause)?;
        let mut filtered_types = Vec::new();
        for type_node in source_array(context, clause.source(), data.types)? {
            let expression = match &context.arena().node(type_node)?.data {
                NodeData::ExpressionWithTypeArguments(data) => data
                    .expression
                    .and_then(|node| context.arena().node_ref(type_node.source(), node)),
                _ => None,
            };
            let keep = if let Some(expression) = expression {
                transformer.is_entity_name_expression(context, expression)?
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
            filtered_types.push(type_node);
        }
        let filtered_types = context
            .factory()?
            .create_node_array(clause.source(), filtered_types)?;
        let mut types = Vec::new();
        for &type_node in &context.arena().node_array(filtered_types)?.nodes.clone() {
            let type_node = TransformNode::new(clause.source(), type_node);
            match transformer.visit_declaration_subtree(context, type_node)? {
                VisitResult::None => {}
                VisitResult::Node(node) => types.push(node),
                VisitResult::Nodes(_) => {
                    return Err(DeclarationTransformer::contract(
                        "heritage type visitor returned an array",
                    ));
                }
            }
        }
        let mut factory = context.factory()?;
        let types = factory.update_node_array(filtered_types, types)?;
        let updated = factory.update_heritage_clause(clause, types)?;
        if !factory.arena().node_array(types)?.nodes.is_empty() {
            result.push(updated);
        }
    }
    let mut factory = context.factory()?;
    Ok(Some(factory.create_node_array(clauses.source(), result)?))
}

fn effective_base_type_expression(
    context: &TransformationContext,
    clauses: Option<TransformNodeArray>,
) -> Result<Option<(TransformNode, TransformNode)>, TransformError> {
    let Some(clauses) = clauses else {
        return Ok(None);
    };
    for clause in source_array(context, clauses.source(), Some(clauses.array()))? {
        let data = heritage_clause_data(context, clause)?;
        if data.token != SyntaxKind::ExtendsKeyword {
            continue;
        }
        let Some(base_type) = source_array(context, clause.source(), data.types)?
            .into_iter()
            .next()
        else {
            return Ok(None);
        };
        let expression = match &context.arena().node(base_type)?.data {
            NodeData::ExpressionWithTypeArguments(data) => data
                .expression
                .and_then(|node| context.arena().node_ref(base_type.source(), node)),
            _ => None,
        }
        .ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::ExpressionWithTypeArguments,
            field: "expression",
        })?;
        return Ok(Some((base_type, expression)));
    }
    Ok(None)
}

fn transform_heritage_clauses_with_base(
    transformer: &mut DeclarationTransformer<'_>,
    context: &mut TransformationContext,
    clauses: Option<TransformNodeArray>,
    generated: TransformNode,
) -> Result<Option<TransformNodeArray>, TransformError> {
    let Some(clauses) = clauses else {
        return Ok(None);
    };
    let mut updated_clauses = Vec::new();
    for clause in source_array(context, clauses.source(), Some(clauses.array()))? {
        let data = heritage_clause_data(context, clause)?;
        let original_types = data
            .types
            .and_then(|array| context.arena().node_array_ref(clause.source(), array))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::HeritageClause,
                field: "types",
            })?;
        let mut updated_types = Vec::new();
        for type_node in source_array(context, clause.source(), data.types)? {
            if data.token == SyntaxKind::ExtendsKeyword {
                let NodeData::ExpressionWithTypeArguments(type_data) =
                    context.arena().node(type_node)?.data.clone()
                else {
                    return Err(DeclarationTransformer::contract(
                        "heritage clause contains a non-expression type",
                    ));
                };
                let type_arguments = transformer.visit_type_node_array(
                    context,
                    type_node.source(),
                    type_data.type_arguments,
                    SyntaxKind::Unknown,
                )?;
                updated_types.push(context.factory()?.update_expression_with_type_arguments(
                    type_node,
                    generated,
                    type_arguments,
                )?);
            } else {
                let expression = match &context.arena().node(type_node)?.data {
                    NodeData::ExpressionWithTypeArguments(type_data) => type_data
                        .expression
                        .and_then(|node| context.arena().node_ref(type_node.source(), node)),
                    _ => None,
                };
                let keep = if let Some(expression) = expression {
                    transformer.is_entity_name_expression(context, expression)?
                        || context.arena().node(expression)?.kind == SyntaxKind::NullKeyword
                } else {
                    false
                };
                if !keep {
                    continue;
                }
                match transformer.visit_declaration_subtree(context, type_node)? {
                    VisitResult::None => {}
                    VisitResult::Node(node) => updated_types.push(node),
                    VisitResult::Nodes(_) => {
                        return Err(DeclarationTransformer::contract(
                            "heritage type visitor returned an array",
                        ));
                    }
                }
            }
        }
        let types = context
            .factory()?
            .update_node_array(original_types, updated_types)?;
        updated_clauses.push(context.factory()?.update_heritage_clause(clause, types)?);
    }
    Ok(Some(
        context
            .factory()?
            .update_node_array(clauses, updated_clauses)?,
    ))
}

/// tsc-port: transformImportEqualsDeclaration @6.0.3
/// tsc-hash: 93ddbcf0f5b98071069f57713946215db5ba4c1d88224c1b0be4942c1d7e25fa
/// tsc-span: _tsc.js:114822-114840
pub(crate) fn transform_import_equals_declaration(
    transformer: &mut DeclarationTransformer<'_>,
    context: &mut TransformationContext,
    declaration: TransformNode,
) -> Result<VisitResult, TransformError> {
    let resolver_node = transformer.required_resolver_node(context, declaration)?;
    if !transformer.resolver.is_declaration_visible(resolver_node)? {
        return Ok(VisitResult::None);
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
        let enclosing = transformer.state()?.enclosing_declaration.ok_or_else(|| {
            DeclarationTransformer::contract(
                "import-equals declaration has no enclosing declaration",
            )
        })?;
        let saved_diagnostic = transformer.tracker.replace_diagnostic_context(
            context.arena(),
            super::diagnostics::DiagnosticContext::ForNode(declaration),
        )?;
        let visibility =
            transformer.check_entity_name_visibility(context, module_reference, enclosing);
        transformer
            .tracker
            .restore_diagnostic_context(saved_diagnostic);
        visibility?;
        return Ok(VisitResult::Node(declaration));
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
    Ok(VisitResult::Node(updated))
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
    let original_module_specifier = data
        .module_specifier
        .and_then(|node| context.arena().node_ref(declaration.source(), node));
    let original_attributes = data
        .attributes
        .and_then(|node| context.arena().node_ref(declaration.source(), node));
    let modifiers = data
        .modifiers
        .and_then(|array| context.arena().node_array_ref(declaration.source(), array));
    let Some(import_clause) = data
        .import_clause
        .and_then(|node| context.arena().node_ref(declaration.source(), node))
    else {
        let module_specifier =
            rewrite_module_specifier(transformer, context, declaration, original_module_specifier)?
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ImportDeclaration,
                    field: "moduleSpecifier",
                })?;
        let attributes = try_get_resolution_mode_override(context, original_attributes);
        let mut factory = context.factory()?;
        return Ok(VisitResult::Node(factory.update_import_declaration(
            declaration,
            modifiers,
            None,
            module_specifier,
            attributes,
        )?));
    };

    let clause_data = import_clause_data(context, import_clause)?;
    let phase_modifier = (clause_data.phase_modifier != Some(SyntaxKind::DeferKeyword))
        .then_some(clause_data.phase_modifier)
        .flatten();
    let default_binding = clause_data
        .name
        .and_then(|node| context.arena().node_ref(declaration.source(), node));
    let visible_default = if default_binding.is_some()
        && transformer
            .resolver
            .is_declaration_visible(transformer.required_resolver_node(context, import_clause)?)?
    {
        default_binding
    } else {
        None
    };
    let named_bindings = clause_data
        .named_bindings
        .and_then(|node| context.arena().node_ref(declaration.source(), node));
    match named_bindings {
        None => {
            let Some(visible_default) = visible_default else {
                return Ok(VisitResult::None);
            };
            let module_specifier = rewrite_module_specifier(
                transformer,
                context,
                declaration,
                original_module_specifier,
            )?
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ImportDeclaration,
                field: "moduleSpecifier",
            })?;
            let attributes = try_get_resolution_mode_override(context, original_attributes);
            let mut factory = context.factory()?;
            let clause = factory.update_import_clause(
                import_clause,
                phase_modifier,
                Some(visible_default),
                None,
            )?;
            Ok(VisitResult::Node(factory.update_import_declaration(
                declaration,
                modifiers,
                Some(clause),
                module_specifier,
                attributes,
            )?))
        }
        Some(named) if context.arena().node(named)?.kind == SyntaxKind::NamespaceImport => {
            let visible_named = transformer
                .resolver
                .is_declaration_visible(transformer.required_resolver_node(context, named)?)?
                .then_some(named);
            if visible_default.is_none() && visible_named.is_none() {
                return Ok(VisitResult::None);
            }
            let module_specifier = rewrite_module_specifier(
                transformer,
                context,
                declaration,
                original_module_specifier,
            )?
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ImportDeclaration,
                field: "moduleSpecifier",
            })?;
            let attributes = try_get_resolution_mode_override(context, original_attributes);
            let mut factory = context.factory()?;
            let clause = factory.update_import_clause(
                import_clause,
                phase_modifier,
                visible_default,
                visible_named,
            )?;
            Ok(VisitResult::Node(factory.update_import_declaration(
                declaration,
                modifiers,
                Some(clause),
                module_specifier,
                attributes,
            )?))
        }
        Some(named) => {
            let named_data = named_imports_data(context, named)?;
            let mut visible_elements = Vec::new();
            for element in source_array(context, named.source(), named_data.elements)? {
                if transformer
                    .resolver
                    .is_declaration_visible(transformer.required_resolver_node(context, element)?)?
                {
                    visible_elements.push(element);
                }
            }
            if visible_elements.is_empty() && visible_default.is_none() {
                if !transformer.resolver.is_import_required_by_augmentation(
                    transformer.required_resolver_node(context, declaration)?,
                )? {
                    return Ok(VisitResult::None);
                }
                let module_specifier = rewrite_module_specifier(
                    transformer,
                    context,
                    declaration,
                    original_module_specifier,
                )?
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ImportDeclaration,
                    field: "moduleSpecifier",
                })?;
                let attributes = try_get_resolution_mode_override(context, original_attributes);
                return Ok(VisitResult::Node(
                    context.factory()?.update_import_declaration(
                        declaration,
                        modifiers,
                        None,
                        module_specifier,
                        attributes,
                    )?,
                ));
            }
            let module_specifier = rewrite_module_specifier(
                transformer,
                context,
                declaration,
                original_module_specifier,
            )?
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ImportDeclaration,
                field: "moduleSpecifier",
            })?;
            let attributes = try_get_resolution_mode_override(context, original_attributes);
            let mut factory = context.factory()?;
            let bindings = if visible_elements.is_empty() {
                None
            } else {
                let elements = factory.create_node_array(named.source(), visible_elements)?;
                Some(factory.update_named_imports(named, elements)?)
            };
            let clause = factory.update_import_clause(
                import_clause,
                phase_modifier,
                visible_default,
                bindings,
            )?;
            Ok(VisitResult::Node(factory.update_import_declaration(
                declaration,
                modifiers,
                Some(clause),
                module_specifier,
                attributes,
            )?))
        }
    }
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
        transformer
            .state_mut()?
            .result_has_external_module_indicator = true;
    }
    if transformer.state()?.is_bundled_emit {
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
    let resolver_node = transformer.required_resolver_node(context, input)?;
    Ok(transformer
        .resolver
        .is_last_bodiless_overload_of_symbol(resolver_node)?)
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

fn expando_declaration_arm(
    transformer: &mut DeclarationTransformer<'_>,
    context: &mut TransformationContext,
    input: TransformNode,
    function: TransformNode,
) -> Result<Vec<TransformNode>, TransformError> {
    let resolver_node = transformer.required_resolver_node(context, input)?;
    let properties = transformer
        .resolver
        .get_properties_of_container_function(resolver_node)?;
    let enclosing_resolver = transformer
        .state()?
        .enclosing_declaration
        .and_then(|node| transformer.required_resolver_node(context, node).ok())
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
        let saved_diagnostic = transformer.tracker.replace_diagnostic_context(
            context.arena(),
            super::diagnostics::DiagnosticContext::ForNode(value_declaration_transform),
        )?;
        let type_node_result = (|| {
            let result = transformer
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
                .map_err(TransformError::from);
            let effects = transformer.tracker.take_pending_effects();
            materialize_effects(context, transformer.host, effects)?;
            result
        })();
        transformer
            .tracker
            .restore_diagnostic_context(saved_diagnostic);
        let type_node = type_node_result?;
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
            factory.get_generated_name_for_non_member_node(value_declaration)?
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
            NodeFlags::NONE,
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
    transformer
        .state_mut()?
        .result_has_external_module_indicator = true;
    transformer.state_mut()?.result_has_scope_marker = true;
    Ok(vec![clean_function, namespace, export_default])
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
    Ok(matches!(
        context.arena().node(node)?.kind,
        SyntaxKind::ImportDeclaration
            | SyntaxKind::ImportEqualsDeclaration
            | SyntaxKind::VariableStatement
            | SyntaxKind::ClassDeclaration
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::ModuleDeclaration
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::EnumDeclaration
    ))
}

fn needs_scope_marker(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<bool, TransformError> {
    let record = context.arena().node(node)?;
    let is_import_or_re_export = matches!(
        record.kind,
        SyntaxKind::ImportDeclaration
            | SyntaxKind::ImportEqualsDeclaration
            | SyntaxKind::ExportDeclaration
    );
    let is_ambient_module = match &record.data {
        NodeData::ModuleDeclaration(data) => {
            data.name
                .and_then(|name| context.arena().node_ref(node.source(), name))
                .is_some_and(|name| {
                    context
                        .arena()
                        .node(name)
                        .is_ok_and(|name| name.kind == SyntaxKind::StringLiteral)
                })
                || NodeFlags::from_bits(record.flags).contains(NodeFlags::GLOBAL_AUGMENTATION)
        }
        _ => false,
    };
    Ok(!is_import_or_re_export
        && record.kind != SyntaxKind::ExportAssignment
        && !modifier_flags(context, node)?.contains(ModifierFlags::EXPORT)
        && !is_ambient_module)
}

fn is_external_module_indicator(
    context: &TransformationContext,
    node: TransformNode,
) -> Result<bool, TransformError> {
    Ok(matches!(
        context.arena().node(node)?.kind,
        SyntaxKind::ImportDeclaration
            | SyntaxKind::ImportEqualsDeclaration
            | SyntaxKind::ExportDeclaration
            | SyntaxKind::ExportAssignment
    ) || modifier_flags(context, node)?.contains(ModifierFlags::EXPORT))
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
        NodeData::Parameter(data) => data.modifiers,
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

fn visit_required_subtree(
    transformer: &mut DeclarationTransformer<'_>,
    context: &mut TransformationContext,
    parent: TransformNode,
    child: Option<NodeId>,
    field: &'static str,
) -> Result<TransformNode, TransformError> {
    let child = required_node(
        context,
        parent.source(),
        child,
        context.arena().node(parent)?.kind,
        field,
    )?;
    match transformer.visit_declaration_subtree(context, child)? {
        VisitResult::Node(child) => Ok(child),
        VisitResult::None => Err(TransformError::RequiredChildRemoved {
            parent: context.arena().node(parent)?.kind,
            field,
        }),
        VisitResult::Nodes(_) => Err(DeclarationTransformer::contract(
            "required declaration child visitor returned a statement array",
        )),
    }
}

fn visit_subtree_array(
    transformer: &mut DeclarationTransformer<'_>,
    context: &mut TransformationContext,
    source: TransformSourceId,
    nodes: Option<tsc_syntax::NodeArrayId>,
) -> Result<TransformNodeArray, TransformError> {
    if let Some(nodes) =
        transformer.visit_type_node_array(context, source, nodes, SyntaxKind::Unknown)?
    {
        Ok(nodes)
    } else {
        context.factory()?.create_node_array(source, Vec::new())
    }
}

fn enum_constant_initializer(
    context: &mut TransformationContext,
    source: TransformSourceId,
    value: &EmitConstantValue,
) -> Result<TransformNode, TransformError> {
    match value {
        EmitConstantValue::String(value) => context
            .factory()?
            .create_string_literal_from_code_units(source, value.code_units(), false),
        EmitConstantValue::Number(value) => {
            let value = value.as_f64();
            if value < 0.0 {
                let operand = context
                    .factory()?
                    .create_numeric_literal(source, tsc_types::js_number_to_string(-value))?;
                context.factory()?.create_prefix_unary_expression(
                    source,
                    SyntaxKind::MinusToken,
                    operand,
                )
            } else {
                context
                    .factory()?
                    .create_numeric_literal(source, tsc_types::js_number_to_string(value))
            }
        }
        EmitConstantValue::Boolean(value) => context.factory()?.create_token(
            source,
            if *value {
                SyntaxKind::TrueKeyword
            } else {
                SyntaxKind::FalseKeyword
            },
            TransformFlags::NONE,
        ),
    }
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

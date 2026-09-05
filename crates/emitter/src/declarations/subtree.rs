use tsc_syntax::{
    try_visit_each_child, NodeArrayId, NodeData, NodeDataChildVisitor, NodeId, SyntaxKind,
};
use tsc_types::ModifierFlags;

use crate::{
    EmitFlags, TransformError, TransformFlags, TransformNode, TransformNodeArray,
    TransformationContext,
};

use super::diagnostics::{
    can_produce_diagnostics, comment_range, DiagnosticContext, DiagnosticContextPlan,
};
use super::ensure::is_function_like;
use super::state::{adopt_result, VisitResult};
use super::tracker::materialize_effects;
use super::{statements, DeclarationTransformer};

struct SubtreeFrame {
    previous_enclosing: Option<Option<TransformNode>>,
    previous_diagnostic: Option<(DiagnosticContext, DiagnosticContextPlan)>,
    previous_suppression: bool,
    restore_suppression: bool,
    can_produce_diagnostic: bool,
}

impl DeclarationTransformer<'_> {
    /// tsc-port: visitDeclarationSubtree @6.0.3
    /// tsc-hash: 49f1c56e7d287ca5c9d8ac236fe1d91a482f181858627f0e21a6770dad67b16b
    /// tsc-span: _tsc.js:114952-115256
    pub(crate) fn visit_declaration_subtree(
        &mut self,
        cx: &mut TransformationContext,
        input: TransformNode,
    ) -> Result<VisitResult, TransformError> {
        let result = self.visit_declaration_subtree_worker(cx, input);
        if let Ok(result) = &result {
            self.observe_boundary(cx, false, input, result);
        }
        result
    }

    #[allow(clippy::mem_replace_option_with_some)] // packet §5.2 owned-frame form
    fn visit_declaration_subtree_worker(
        &mut self,
        cx: &mut TransformationContext,
        input: TransformNode,
    ) -> Result<VisitResult, TransformError> {
        if self.should_strip_internal(cx, Some(input))? {
            return Ok(VisitResult::None);
        }
        if is_declaration(cx, input)? && self.is_declaration_and_not_visible(cx, input)? {
            return Ok(VisitResult::None);
        }
        if is_declaration(cx, input)? {
            if let Some(expression) = dynamic_name_expression(cx, input)? {
                if self.options.isolated_declarations == Some(true) {
                    return Err(TransformError::Unsupported(
                        crate::UnsupportedEmitFeature::IsolatedDeclarations,
                    ));
                }
                let late_bound = self
                    .resolver
                    .is_late_bound(self.required_resolver_node(cx, input)?)?;
                if !late_bound || !self.is_entity_name_expression(cx, expression)? {
                    return Ok(VisitResult::None);
                }
            }
        }
        if is_function_like(self.kind(cx, input)?)
            && self
                .resolver
                .is_implementation_of_overload(self.required_resolver_node(cx, input)?)?
        {
            return Ok(VisitResult::None);
        }
        if self.kind(cx, input)? == SyntaxKind::SemicolonClassElement {
            return Ok(VisitResult::None);
        }

        let enclosing = self.is_enclosing_declaration(cx, input)?;
        let previous_enclosing = if enclosing {
            Some(std::mem::replace(
                &mut self.state_mut()?.enclosing_declaration,
                Some(input),
            ))
        } else {
            None
        };
        let can_produce = can_produce_diagnostics(self.kind(cx, input)?);
        let old_suppression = self.tracker.suppress_new_diagnostic_contexts;
        let mut enter_suppression = matches!(
            self.kind(cx, input)?,
            SyntaxKind::TypeLiteral | SyntaxKind::MappedType
        ) && self.parent(cx, input)?.is_none_or(|parent| {
            cx.arena()
                .node(parent)
                .is_ok_and(|node| node.kind != SyntaxKind::TypeAliasDeclaration)
        });

        // Upstream's first direct return: the private method/signature that is
        // not the symbol's first declaration leaks the enclosing-declaration
        // write and skips every cleanup/adoption operation.
        if matches!(
            self.kind(cx, input)?,
            SyntaxKind::MethodDeclaration | SyntaxKind::MethodSignature
        ) && self.has_effective_modifier(cx, input, ModifierFlags::PRIVATE)?
        {
            let first_declaration = self
                .resolver
                .is_first_declaration_of_symbol(self.required_resolver_node(cx, input)?);
            let first_declaration = match first_declaration {
                Ok(first_declaration) => first_declaration,
                Err(error) => {
                    if let Some(previous) = previous_enclosing {
                        self.state_mut()?.enclosing_declaration = previous;
                    }
                    return Err(error.into());
                }
            };
            if !first_declaration {
                return Ok(VisitResult::None);
            }
            let frame = SubtreeFrame {
                previous_enclosing,
                previous_diagnostic: None,
                previous_suppression: old_suppression,
                restore_suppression: enter_suppression,
                can_produce_diagnostic: can_produce,
            };
            let result = (|| {
                let name = declaration_name(cx, input)?
                    .ok_or_else(|| Self::contract("private method has no declaration name"))?;
                let modifiers = self.ensure_modifiers(cx, input)?;
                let property = cx.factory()?.create_property_declaration(
                    input.source(),
                    modifiers,
                    name,
                    None,
                    None,
                    None,
                )?;
                Ok(VisitResult::Node(property))
            })();
            return self.finish_subtree(cx, input, frame, result);
        }

        let previous_diagnostic = if can_produce && !old_suppression {
            match self
                .tracker
                .replace_diagnostic_context(cx.arena(), DiagnosticContext::ForNode(input))
            {
                Ok(saved) => Some(saved),
                Err(error) => {
                    if let Some(previous) = previous_enclosing {
                        self.state_mut()?.enclosing_declaration = previous;
                    }
                    return Err(error);
                }
            }
        } else {
            None
        };

        if self.kind(cx, input)? == SyntaxKind::TypeQuery {
            if let NodeData::TypeQuery(data) = &cx.arena().node(input)?.data {
                if let Some(name) = data
                    .expr_name
                    .and_then(|name| cx.arena().node_ref(input.source(), name))
                {
                    let enclosing = self
                        .state()?
                        .enclosing_declaration
                        .ok_or_else(|| Self::contract("type query has no enclosing declaration"))?;
                    if let Err(error) = self.check_entity_name_visibility(cx, name, enclosing) {
                        let frame = SubtreeFrame {
                            previous_enclosing,
                            previous_diagnostic,
                            previous_suppression: old_suppression,
                            restore_suppression: enter_suppression,
                            can_produce_diagnostic: can_produce,
                        };
                        return self.finish_subtree(cx, input, frame, Err(error));
                    }
                }
            }
        }
        if enter_suppression {
            self.tracker.suppress_new_diagnostic_contexts = true;
        }

        if self.kind(cx, input)? == SyntaxKind::VariableDeclaration {
            let name = declaration_name(cx, input)?
                .ok_or_else(|| Self::contract("variable declaration has no name"))?;
            if matches!(
                self.kind(cx, name)?,
                SyntaxKind::ArrayBindingPattern | SyntaxKind::ObjectBindingPattern
            ) {
                // Upstream's second direct return: this happens after the
                // diagnostic context was replaced and skips cleanup,
                // adoption, and every frame restoration.
                return statements::recreate_binding_pattern(self, cx, name);
            }
        }

        let frame = SubtreeFrame {
            previous_enclosing,
            previous_diagnostic,
            previous_suppression: old_suppression,
            restore_suppression: enter_suppression,
            can_produce_diagnostic: can_produce,
        };

        if is_processed_component(self.kind(cx, input)?) {
            let result = (|| -> Result<VisitResult, TransformError> {
                match self.kind(cx, input)? {
                    SyntaxKind::ExpressionWithTypeArguments => {
                        let expression = match &cx.arena().node(input)?.data {
                            NodeData::ExpressionWithTypeArguments(data) => data.expression,
                            _ => None,
                        }
                        .and_then(|node| cx.arena().node_ref(input.source(), node));
                        if let Some(expression) = expression {
                            if self.is_entity_name_expression(cx, expression)? {
                                let enclosing =
                                    self.state()?.enclosing_declaration.ok_or_else(|| {
                                        Self::contract("heritage type has no enclosing declaration")
                                    })?;
                                self.check_entity_name_visibility(cx, expression, enclosing)?;
                            }
                        }
                        self.visit_each_child(cx, input).map(VisitResult::Node)
                    }
                    SyntaxKind::TypeReference => {
                        let name = match &cx.arena().node(input)?.data {
                            NodeData::TypeReference(data) => data.type_name,
                            _ => None,
                        }
                        .and_then(|node| cx.arena().node_ref(input.source(), node))
                        .ok_or_else(|| Self::contract("type reference has no type name"))?;
                        let enclosing = self.state()?.enclosing_declaration.ok_or_else(|| {
                            Self::contract("type reference has no enclosing declaration")
                        })?;
                        self.check_entity_name_visibility(cx, name, enclosing)?;
                        self.visit_each_child(cx, input).map(VisitResult::Node)
                    }
                    SyntaxKind::ConstructSignature => {
                        let NodeData::ConstructSignature(data) =
                            cx.arena().node(input)?.data.clone()
                        else {
                            return Err(Self::contract("construct-signature kind/data mismatch"));
                        };
                        let type_parameters =
                            self.ensure_type_params(cx, input, data.type_parameters)?;
                        let parameters = self.update_params_list(
                            cx,
                            input,
                            data.parameters,
                            default_modifier_mask(),
                        )?;
                        let r#type = self.ensure_type(cx, input, false)?;
                        let updated =
                            update_signature(cx, input, type_parameters, parameters, r#type, None)?;
                        Ok(VisitResult::Node(updated))
                    }
                    SyntaxKind::Constructor => {
                        let parameters = match &cx.arena().node(input)?.data {
                            NodeData::Constructor(data) => data.parameters,
                            _ => None,
                        };
                        let modifiers = self.ensure_modifiers(cx, input)?;
                        let parameters =
                            self.update_params_list(cx, input, parameters, ModifierFlags::NONE)?;
                        let created = cx.factory()?.create_constructor_declaration(
                            input.source(),
                            modifiers,
                            parameters,
                            None,
                        )?;
                        Ok(VisitResult::Node(update_from_created(cx, input, created)?))
                    }
                    SyntaxKind::MethodDeclaration => {
                        let NodeData::MethodDeclaration(data) =
                            cx.arena().node(input)?.data.clone()
                        else {
                            return Err(Self::contract("method kind/data mismatch"));
                        };
                        if declaration_name(cx, input)?.is_some_and(|name| {
                            cx.arena()
                                .node(name)
                                .is_ok_and(|node| node.kind == SyntaxKind::PrivateIdentifier)
                        }) {
                            Ok(VisitResult::None)
                        } else {
                            let name = declaration_name(cx, input)?
                                .ok_or_else(|| Self::contract("method has no name"))?;
                            let modifiers = self.ensure_modifiers(cx, input)?;
                            let type_parameters =
                                self.ensure_type_params(cx, input, data.type_parameters)?;
                            let parameters = self.update_params_list(
                                cx,
                                input,
                                data.parameters,
                                default_modifier_mask(),
                            )?;
                            let r#type = self.ensure_type(cx, input, false)?;
                            let question_token = data
                                .question_token
                                .and_then(|node| cx.arena().node_ref(input.source(), node));
                            let created = cx.factory()?.create_method_declaration(
                                input.source(),
                                modifiers,
                                None,
                                name,
                                question_token,
                                type_parameters,
                                parameters,
                                r#type,
                                None,
                            )?;
                            Ok(VisitResult::Node(update_from_created(cx, input, created)?))
                        }
                    }
                    SyntaxKind::GetAccessor => {
                        if has_private_identifier_name(cx, input)? {
                            Ok(VisitResult::None)
                        } else {
                            let name = declaration_name(cx, input)?
                                .ok_or_else(|| Self::contract("getter has no name"))?;
                            let modifiers = self.ensure_modifiers(cx, input)?;
                            let parameters = self.update_accessor_params_list(
                                cx,
                                input,
                                self.has_effective_modifier(cx, input, ModifierFlags::PRIVATE)?,
                            )?;
                            let r#type = self.ensure_type(cx, input, false)?;
                            Ok(VisitResult::Node(
                                cx.factory()?.update_get_accessor_declaration(
                                    input,
                                    modifiers.map(TransformNodeArray::array),
                                    Some(name.node()),
                                    Some(parameters.array()),
                                    r#type.map(TransformNode::node),
                                    None,
                                    TransformFlags::CONTAINS_TYPE_SCRIPT,
                                )?,
                            ))
                        }
                    }
                    SyntaxKind::SetAccessor => {
                        if has_private_identifier_name(cx, input)? {
                            Ok(VisitResult::None)
                        } else {
                            let name = declaration_name(cx, input)?
                                .ok_or_else(|| Self::contract("setter has no name"))?;
                            let modifiers = self.ensure_modifiers(cx, input)?;
                            let parameters = self.update_accessor_params_list(
                                cx,
                                input,
                                self.has_effective_modifier(cx, input, ModifierFlags::PRIVATE)?,
                            )?;
                            Ok(VisitResult::Node(
                                cx.factory()?.update_set_accessor_declaration(
                                    input,
                                    modifiers.map(TransformNodeArray::array),
                                    Some(name.node()),
                                    Some(parameters.array()),
                                    None,
                                    TransformFlags::CONTAINS_TYPE_SCRIPT,
                                )?,
                            ))
                        }
                    }
                    SyntaxKind::PropertyDeclaration | SyntaxKind::PropertySignature => {
                        if has_private_identifier_name(cx, input)? {
                            Ok(VisitResult::None)
                        } else {
                            let r#type = self.ensure_type(cx, input, false)?;
                            let initializer =
                                if self.kind(cx, input)? == SyntaxKind::PropertyDeclaration {
                                    self.ensure_no_initializer(cx, input)?
                                } else {
                                    None
                                };
                            let modifiers = self.ensure_modifiers(cx, input)?;
                            let updated =
                                update_property(cx, input, modifiers, r#type, initializer)?;
                            Ok(VisitResult::Node(updated))
                        }
                    }
                    SyntaxKind::MethodSignature => {
                        if has_private_identifier_name(cx, input)? {
                            Ok(VisitResult::None)
                        } else {
                            let NodeData::MethodSignature(data) =
                                cx.arena().node(input)?.data.clone()
                            else {
                                return Err(Self::contract("method-signature kind/data mismatch"));
                            };
                            let type_parameters =
                                self.ensure_type_params(cx, input, data.type_parameters)?;
                            let parameters = self.update_params_list(
                                cx,
                                input,
                                data.parameters,
                                default_modifier_mask(),
                            )?;
                            let r#type = self.ensure_type(cx, input, false)?;
                            let modifiers = self.ensure_modifiers(cx, input)?;
                            Ok(VisitResult::Node(update_signature(
                                cx,
                                input,
                                type_parameters,
                                parameters,
                                r#type,
                                modifiers,
                            )?))
                        }
                    }
                    SyntaxKind::CallSignature => {
                        let NodeData::CallSignature(data) = cx.arena().node(input)?.data.clone()
                        else {
                            return Err(Self::contract("call-signature kind/data mismatch"));
                        };
                        let type_parameters =
                            self.ensure_type_params(cx, input, data.type_parameters)?;
                        let parameters = self.update_params_list(
                            cx,
                            input,
                            data.parameters,
                            default_modifier_mask(),
                        )?;
                        let r#type = self.ensure_type(cx, input, false)?;
                        Ok(VisitResult::Node(update_signature(
                            cx,
                            input,
                            type_parameters,
                            parameters,
                            r#type,
                            None,
                        )?))
                    }
                    SyntaxKind::IndexSignature => {
                        let NodeData::IndexSignature(data) = cx.arena().node(input)?.data.clone()
                        else {
                            return Err(Self::contract("index-signature kind/data mismatch"));
                        };
                        let parameters = self.update_params_list(
                            cx,
                            input,
                            data.parameters,
                            default_modifier_mask(),
                        )?;
                        let r#type = match data
                            .r#type
                            .and_then(|node| cx.arena().node_ref(input.source(), node))
                        {
                            Some(node) => match self.visit_declaration_subtree(cx, node)? {
                                VisitResult::Node(node) => node,
                                VisitResult::None => cx.factory()?.create_keyword_type_node(
                                    input.source(),
                                    SyntaxKind::AnyKeyword,
                                )?,
                                VisitResult::Nodes(_) => {
                                    return Err(Self::contract(
                                        "index type visitor returned an array",
                                    ));
                                }
                            },
                            None => cx
                                .factory()?
                                .create_keyword_type_node(input.source(), SyntaxKind::AnyKeyword)?,
                        };
                        let modifiers = self.ensure_modifiers(cx, input)?;
                        Ok(VisitResult::Node(update_signature(
                            cx,
                            input,
                            None,
                            parameters,
                            Some(r#type),
                            modifiers,
                        )?))
                    }
                    SyntaxKind::VariableDeclaration => {
                        enter_suppression = true;
                        self.tracker.suppress_new_diagnostic_contexts = true;
                        let r#type = self.ensure_type(cx, input, false)?;
                        let initializer = self.ensure_no_initializer(cx, input)?;
                        Ok(VisitResult::Node(update_variable(
                            cx,
                            input,
                            r#type,
                            initializer,
                        )?))
                    }
                    SyntaxKind::TypeParameter => {
                        let NodeData::TypeParameter(data) = cx.arena().node(input)?.data.clone()
                        else {
                            return Err(Self::contract("type-parameter kind/data mismatch"));
                        };
                        if self.is_private_method_type_parameter(cx, input)?
                            && (data.r#default.is_some() || data.constraint.is_some())
                        {
                            let name = declaration_name(cx, input)?
                                .ok_or_else(|| Self::contract("type parameter has no name"))?;
                            let modifiers = data
                                .modifiers
                                .and_then(|array| cx.arena().node_array_ref(input.source(), array));
                            Ok(VisitResult::Node(
                                cx.factory()?.update_type_parameter_declaration(
                                    input, modifiers, name, None, None,
                                )?,
                            ))
                        } else {
                            self.visit_each_child(cx, input).map(VisitResult::Node)
                        }
                    }
                    SyntaxKind::ConditionalType => {
                        let NodeData::ConditionalType(data) = cx.arena().node(input)?.data.clone()
                        else {
                            return Err(Self::contract("conditional-type kind/data mismatch"));
                        };
                        let check =
                            self.visit_required_type(cx, input, data.check_type, "checkType")?;
                        let extends =
                            self.visit_required_type(cx, input, data.extends_type, "extendsType")?;
                        let true_input = data
                            .true_type
                            .and_then(|node| cx.arena().node_ref(input.source(), node))
                            .ok_or_else(|| Self::contract("conditional type has no trueType"))?;
                        let old_enclosing = std::mem::replace(
                            &mut self.state_mut()?.enclosing_declaration,
                            Some(true_input),
                        );
                        let true_result = self.visit_declaration_subtree(cx, true_input);
                        self.state_mut()?.enclosing_declaration = old_enclosing;
                        let true_type = required_single_type(true_result?, "trueType")?;
                        let false_type =
                            self.visit_required_type(cx, input, data.false_type, "falseType")?;
                        Ok(VisitResult::Node(
                            cx.factory()?.update_conditional_type_node(
                                input, check, extends, true_type, false_type,
                            )?,
                        ))
                    }
                    SyntaxKind::FunctionType | SyntaxKind::ConstructorType => {
                        let (type_parameters, parameters, r#type, modifiers) =
                            signature_parts(cx, input)?;
                        let type_parameters =
                            self.visit_type_parameters(cx, input, type_parameters)?;
                        let parameters = self.update_params_list(
                            cx,
                            input,
                            parameters,
                            default_modifier_mask(),
                        )?;
                        let r#type = self.visit_required_type(cx, input, r#type, "type")?;
                        let modifiers = if self.kind(cx, input)? == SyntaxKind::ConstructorType {
                            self.ensure_modifiers(cx, input)?
                        } else {
                            modifiers
                                .and_then(|array| cx.arena().node_array_ref(input.source(), array))
                        };
                        Ok(VisitResult::Node(update_signature(
                            cx,
                            input,
                            type_parameters,
                            parameters,
                            Some(r#type),
                            modifiers,
                        )?))
                    }
                    SyntaxKind::ImportType => {
                        let NodeData::ImportType(data) = cx.arena().node(input)?.data.clone()
                        else {
                            return Err(Self::contract("import-type kind/data mismatch"));
                        };
                        let argument = data
                            .argument
                            .and_then(|node| cx.arena().node_ref(input.source(), node))
                            .ok_or_else(|| Self::contract("import type has no argument"))?;
                        let literal = match &cx.arena().node(argument)?.data {
                            NodeData::LiteralType(data) => data
                                .literal
                                .and_then(|node| cx.arena().node_ref(input.source(), node)),
                            _ => None,
                        };
                        let Some(literal) = literal.filter(|literal| {
                            cx.arena()
                                .node(*literal)
                                .is_ok_and(|node| node.kind == SyntaxKind::StringLiteral)
                        }) else {
                            return Ok(VisitResult::Node(input));
                        };
                        let rewritten =
                            statements::rewrite_module_specifier(self, cx, input, Some(literal))?
                                .ok_or_else(|| {
                                Self::contract("literal import type lost its specifier")
                            })?;
                        let argument = update_literal_type(cx, argument, rewritten)?;
                        let attributes = data
                            .attributes
                            .and_then(|node| cx.arena().node_ref(input.source(), node));
                        let qualifier = data
                            .qualifier
                            .and_then(|node| cx.arena().node_ref(input.source(), node));
                        let type_arguments = self.visit_type_node_array(
                            cx,
                            input.source(),
                            data.type_arguments,
                            SyntaxKind::Unknown,
                        )?;
                        Ok(VisitResult::Node(cx.factory()?.update_import_type_node(
                            input,
                            argument,
                            attributes,
                            qualifier,
                            type_arguments,
                            data.is_type_of,
                        )?))
                    }
                    _ => Err(Self::contract(
                        "processed declaration component is unhandled",
                    )),
                }
            })();
            let mut frame = frame;
            frame.restore_suppression = enter_suppression;
            return self.finish_subtree(cx, input, frame, result);
        }

        if self.kind(cx, input)? == SyntaxKind::TupleType {
            let record = cx.arena().node(input)?;
            let positions = cx.arena().source(input.source())?.syntax().positions();
            if positions
                .line_and_character_byte(record.pos)
                .zip(positions.line_and_character_byte(record.end))
                .is_some_and(|(start, end)| start.line == end.line)
            {
                cx.arena_mut()?
                    .metadata_mut(input)
                    .add_flags(EmitFlags::SINGLE_LINE);
            }
        }
        let result = self.visit_each_child(cx, input).map(VisitResult::Node);
        self.finish_subtree(cx, input, frame, result)
    }

    /// tsc-port: cleanup @6.0.3 (visitDeclarationSubtree)
    /// tsc-hash: 8b5311179bff89546fbee6fe9f8e1fe9f91bcbc0c994289ec03616576925a7a0
    /// tsc-span: _tsc.js:115238-115255
    fn finish_subtree(
        &mut self,
        cx: &mut TransformationContext,
        input: TransformNode,
        frame: SubtreeFrame,
        result: Result<VisitResult, TransformError>,
    ) -> Result<VisitResult, TransformError> {
        let check_result = match &result {
            Ok(VisitResult::Node(_) | VisitResult::Nodes(_)) if frame.can_produce_diagnostic => {
                match dynamic_name_expression(cx, input) {
                    Ok(Some(_)) => self.check_name(cx, input),
                    Ok(None) => Ok(()),
                    Err(error) => Err(error),
                }
            }
            _ => Ok(()),
        };
        if let Some(previous) = frame.previous_enclosing {
            self.state_mut()?.enclosing_declaration = previous;
        }
        if let Some(previous) = frame.previous_diagnostic {
            self.tracker.restore_diagnostic_context(previous);
        }
        if frame.restore_suppression {
            self.tracker.suppress_new_diagnostic_contexts = frame.previous_suppression;
        }
        check_result?;
        adopt_result(cx, input, result?)
    }

    /// tsc-port: isDeclarationAndNotVisible @6.0.3
    /// tsc-hash: 5fb323cf8b0d98939e4fdac90db83ae7c37223e9e31254b02cde2b5aae371cc6
    /// tsc-span: _tsc.js:114713-114735
    pub(crate) fn is_declaration_and_not_visible(
        &mut self,
        cx: &mut TransformationContext,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        let parse = cx
            .arena()
            .parse_tree_node(node)?
            .ok_or(TransformError::ResolverNodeNotInParseTree(node))?;
        match self.kind(cx, parse)? {
            SyntaxKind::FunctionDeclaration
            | SyntaxKind::ModuleDeclaration
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::ClassDeclaration
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::EnumDeclaration => Ok(!self
                .resolver
                .is_declaration_visible(self.required_resolver_node(cx, parse)?)?),
            SyntaxKind::VariableDeclaration => Ok(!self.binding_name_visible(cx, parse)?),
            SyntaxKind::ImportEqualsDeclaration
            | SyntaxKind::ImportDeclaration
            | SyntaxKind::ExportDeclaration
            | SyntaxKind::ExportAssignment => Ok(false),
            SyntaxKind::ClassStaticBlockDeclaration => Ok(true),
            _ => Ok(false),
        }
    }

    /// tsc-port: getBindingNameVisible @6.0.3
    /// tsc-hash: e8566d584544ff2b4d74a066a4785c361513ba4fbeb3569bbc374c8af73b1f06
    /// tsc-span: _tsc.js:114744-114753
    pub(crate) fn binding_name_visible(
        &mut self,
        cx: &mut TransformationContext,
        element: TransformNode,
    ) -> Result<bool, TransformError> {
        if self.kind(cx, element)? == SyntaxKind::OmittedExpression {
            return Ok(false);
        }
        let Some(name) = declaration_name(cx, element)? else {
            return Ok(false);
        };
        if matches!(
            self.kind(cx, name)?,
            SyntaxKind::ArrayBindingPattern | SyntaxKind::ObjectBindingPattern
        ) {
            let elements = match &cx.arena().node(name)?.data {
                NodeData::ArrayBindingPattern(data) => data.elements,
                NodeData::ObjectBindingPattern(data) => data.elements,
                _ => None,
            };
            if let Some(elements) =
                elements.and_then(|array| cx.arena().node_array_ref(name.source(), array))
            {
                for &element in &cx.arena().node_array(elements)?.nodes.clone() {
                    if self.binding_name_visible(cx, TransformNode::new(name.source(), element))? {
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        } else {
            Ok(self
                .resolver
                .is_declaration_visible(self.required_resolver_node(cx, element)?)?)
        }
    }

    /// tsc-port: isEnclosingDeclaration @6.0.3
    /// tsc-hash: 5ff3baaf19f958a9456c268b5e617dde68c124c9336ad07d1ea7cc5cb2bd36f3
    /// tsc-span: _tsc.js:114796-114798
    pub(crate) fn is_enclosing_declaration(
        &self,
        cx: &TransformationContext,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        let kind = self.kind(cx, node)?;
        Ok(matches!(
            kind,
            SyntaxKind::SourceFile
                | SyntaxKind::TypeAliasDeclaration
                | SyntaxKind::ModuleDeclaration
                | SyntaxKind::ClassDeclaration
                | SyntaxKind::InterfaceDeclaration
                | SyntaxKind::IndexSignature
                | SyntaxKind::MappedType
        ) || is_function_like(kind))
    }

    /// tsc-port: checkEntityNameVisibility @6.0.3
    /// tsc-hash: 1d2fc9f947ea666aa659dad23eb6f2e6cd4ced7c0a3cfff67f5f8e4d222fc408
    /// tsc-span: _tsc.js:114799-114802
    pub(crate) fn check_entity_name_visibility(
        &mut self,
        cx: &mut TransformationContext,
        entity_name: TransformNode,
        enclosing: TransformNode,
    ) -> Result<(), TransformError> {
        let result = self.resolver.is_entity_name_visible(
            self.required_resolver_node(cx, entity_name)?,
            self.required_resolver_node(cx, enclosing)?,
        )?;
        self.tracker.handle_symbol_accessibility_error(result);
        let effects = self.tracker.take_pending_effects();
        materialize_effects(cx, self.host, effects)
    }

    /// tsc-port: checkName @6.0.3
    /// tsc-hash: dab628e1026bd9f8753f67c20a9112870d331d852a79db45f68e54dbc8e4d861
    /// tsc-span: _tsc.js:115744-115759
    pub(crate) fn check_name(
        &mut self,
        cx: &mut TransformationContext,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        let saved_diag = if self.tracker.suppress_new_diagnostic_contexts {
            None
        } else {
            Some(
                self.tracker
                    .replace_diagnostic_context(cx.arena(), DiagnosticContext::ForNodeName(node))?,
            )
        };
        let name = declaration_name(cx, node)?
            .ok_or_else(|| Self::contract("dynamic declaration has no name"))?;
        self.tracker.error_name_node = Some(name);
        let result = (|| {
            let NodeData::ComputedPropertyName(data) = &cx.arena().node(name)?.data else {
                return Err(Self::contract(
                    "checkName requires a computed property name",
                ));
            };
            let expression = data
                .expression
                .and_then(|node| cx.arena().node_ref(name.source(), node))
                .ok_or_else(|| Self::contract("computed property name has no expression"))?;
            let enclosing = self
                .state()?
                .enclosing_declaration
                .ok_or_else(|| Self::contract("computed name has no enclosing declaration"))?;
            self.check_entity_name_visibility(cx, expression, enclosing)
        })();
        if let Some(saved) = saved_diag {
            self.tracker.restore_diagnostic_context(saved);
        }
        self.tracker.error_name_node = None;
        result
    }

    /// tsc-port: isPrivateMethodTypeParameter @6.0.3
    /// tsc-hash: 61d566438bb84a89700ee249ec413c690f423f31fc14709d8e919e8e687225ca
    /// tsc-span: _tsc.js:115257-115259
    pub(crate) fn is_private_method_type_parameter(
        &self,
        cx: &TransformationContext,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        let Some(parent) = self.parent(cx, node)? else {
            return Ok(false);
        };
        Ok(self.kind(cx, parent)? == SyntaxKind::MethodDeclaration
            && self.has_effective_modifier(cx, parent, ModifierFlags::PRIVATE)?)
    }

    fn visit_each_child(
        &mut self,
        cx: &mut TransformationContext,
        input: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let mut data = cx.arena().node(input)?.data.clone();
        {
            let mut visitor = DeclarationChildVisitor {
                transformer: self,
                cx,
                source: input.source(),
            };
            try_visit_each_child(&mut data, &mut visitor)?;
        }
        if cx.arena().node(input)?.data == data {
            return Ok(input);
        }
        let kind = cx.arena().node(input)?.kind;
        let mut flags = cx.arena().transform_flags(input);
        if kind == SyntaxKind::TypeParameter
            || kind as u16 >= SyntaxKind::FirstTypeNode as u16
                && kind as u16 <= SyntaxKind::LastTypeNode as u16
        {
            flags |= TransformFlags::CONTAINS_TYPE_SCRIPT;
        }
        cx.factory()?.update_node(input, data, flags)
    }

    fn visit_required_type(
        &mut self,
        cx: &mut TransformationContext,
        owner: TransformNode,
        node: Option<NodeId>,
        field: &'static str,
    ) -> Result<TransformNode, TransformError> {
        let node = node
            .and_then(|node| cx.arena().node_ref(owner.source(), node))
            .ok_or_else(|| Self::contract("required declaration type child is absent"))?;
        required_single_type(self.visit_declaration_subtree(cx, node)?, field)
    }

    fn visit_type_parameters(
        &mut self,
        cx: &mut TransformationContext,
        owner: TransformNode,
        nodes: Option<NodeArrayId>,
    ) -> Result<Option<TransformNodeArray>, TransformError> {
        let Some(array) = nodes.and_then(|array| cx.arena().node_array_ref(owner.source(), array))
        else {
            return Ok(None);
        };
        let mut output = Vec::new();
        for &node in &cx.arena().node_array(array)?.nodes.clone() {
            match self.visit_declaration_subtree(cx, TransformNode::new(owner.source(), node))? {
                VisitResult::None => {}
                VisitResult::Node(node) => output.push(node),
                VisitResult::Nodes(_) => {
                    return Err(Self::contract("type-parameter visitor returned an array"))
                }
            }
        }
        Ok(Some(cx.factory()?.update_node_array(array, output)?))
    }
}

/// tsc-port: preserveJsDoc @6.0.3
/// tsc-hash: c7b4e75ac7ce986490523f63b53f7ebb54de1cd40398dd0f7319de5a3cedc226
/// tsc-span: _tsc.js:114803-114808
pub(crate) fn preserve_js_doc(
    cx: &mut TransformationContext,
    updated: TransformNode,
    original: TransformNode,
) -> Result<TransformNode, TransformError> {
    let original_js_doc = cx.arena().node(original)?.js_doc;
    let updated_js_doc = cx.arena().node(updated)?.js_doc;
    if original_js_doc.is_some() && updated_js_doc != original_js_doc {
        cx.factory()?.set_js_doc_from_original(updated, original)?;
    }
    if let Some(range) = comment_range(cx.arena(), original)? {
        cx.arena_mut()?
            .metadata_mut(updated)
            .set_comment_range(range);
    }
    Ok(updated)
}

/// tsc-port: isProcessedComponent @6.0.3
/// tsc-hash: ef7c43559523e6dd5d65c77d1226901dd0d9101bdacaaee1809dc6142c692922
/// tsc-span: _tsc.js:115850-115873
pub(crate) const fn is_processed_component(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::ConstructSignature
            | SyntaxKind::Constructor
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::PropertySignature
            | SyntaxKind::MethodSignature
            | SyntaxKind::CallSignature
            | SyntaxKind::IndexSignature
            | SyntaxKind::VariableDeclaration
            | SyntaxKind::TypeParameter
            | SyntaxKind::ExpressionWithTypeArguments
            | SyntaxKind::TypeReference
            | SyntaxKind::ConditionalType
            | SyntaxKind::FunctionType
            | SyntaxKind::ConstructorType
            | SyntaxKind::ImportType
    )
}

struct DeclarationChildVisitor<'a, 't> {
    transformer: &'a mut DeclarationTransformer<'t>,
    cx: &'a mut TransformationContext,
    source: crate::TransformSourceId,
}

impl NodeDataChildVisitor for DeclarationChildVisitor<'_, '_> {
    type Error = TransformError;

    fn node_kind(&self, id: NodeId) -> SyntaxKind {
        self.cx
            .arena()
            .node(TransformNode::new(self.source, id))
            .map_or(SyntaxKind::Unknown, |node| node.kind)
    }

    fn visit_node(&mut self, id: NodeId) -> Result<Option<NodeId>, Self::Error> {
        match self
            .transformer
            .visit_declaration_subtree(self.cx, TransformNode::new(self.source, id))?
        {
            VisitResult::None => Ok(None),
            VisitResult::Node(node) => Ok(Some(node.node())),
            VisitResult::Nodes(_) => Err(DeclarationTransformer::contract(
                "declaration subtree array reached a single-node child",
            )),
        }
    }

    fn visit_nodes(&mut self, id: NodeArrayId) -> Result<Option<NodeArrayId>, Self::Error> {
        let original = TransformNodeArray::new(self.source, id);
        let mut output = Vec::new();
        for &node in &self.cx.arena().node_array(original)?.nodes.clone() {
            match self
                .transformer
                .visit_declaration_subtree(self.cx, TransformNode::new(self.source, node))?
            {
                VisitResult::None => {}
                VisitResult::Node(node) => output.push(node),
                VisitResult::Nodes(nodes) => output.extend(nodes),
            }
        }
        Ok(Some(
            self.cx
                .factory()?
                .update_node_array(original, output)?
                .array(),
        ))
    }

    fn required_child_removed(&mut self, parent: SyntaxKind, field: &'static str) -> Self::Error {
        TransformError::RequiredChildRemoved { parent, field }
    }
}

fn default_modifier_mask() -> ModifierFlags {
    ModifierFlags::from_bits(ModifierFlags::ALL.bits() ^ ModifierFlags::PUBLIC.bits())
}

fn required_single_type(
    result: VisitResult,
    _field: &'static str,
) -> Result<TransformNode, TransformError> {
    match result {
        VisitResult::Node(node) => Ok(node),
        VisitResult::None | VisitResult::Nodes(_) => Err(DeclarationTransformer::contract(
            "required declaration type child was removed or expanded",
        )),
    }
}

fn is_declaration(cx: &TransformationContext, node: TransformNode) -> Result<bool, TransformError> {
    Ok(matches!(
        cx.arena().node(node)?.kind,
        SyntaxKind::ArrowFunction
            | SyntaxKind::BindingElement
            | SyntaxKind::ClassDeclaration
            | SyntaxKind::ClassExpression
            | SyntaxKind::ClassStaticBlockDeclaration
            | SyntaxKind::Constructor
            | SyntaxKind::EnumDeclaration
            | SyntaxKind::EnumMember
            | SyntaxKind::ExportSpecifier
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::FunctionExpression
            | SyntaxKind::GetAccessor
            | SyntaxKind::ImportClause
            | SyntaxKind::ImportEqualsDeclaration
            | SyntaxKind::ImportSpecifier
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::MethodSignature
            | SyntaxKind::ModuleDeclaration
            | SyntaxKind::NamespaceExportDeclaration
            | SyntaxKind::NamespaceImport
            | SyntaxKind::NamespaceExport
            | SyntaxKind::Parameter
            | SyntaxKind::PropertyAssignment
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::PropertySignature
            | SyntaxKind::SetAccessor
            | SyntaxKind::ShorthandPropertyAssignment
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::TypeParameter
            | SyntaxKind::VariableDeclaration
            | SyntaxKind::JSDocTypedefTag
            | SyntaxKind::JSDocCallbackTag
            | SyntaxKind::JSDocPropertyTag
            | SyntaxKind::NamedTupleMember
    ))
}

fn declaration_name(
    cx: &TransformationContext,
    node: TransformNode,
) -> Result<Option<TransformNode>, TransformError> {
    let name = match &cx.arena().node(node)?.data {
        NodeData::BindingElement(data) => data.name,
        NodeData::ClassDeclaration(data) => data.name,
        NodeData::FunctionDeclaration(data) => data.name,
        NodeData::GetAccessor(data) => data.name,
        NodeData::ImportClause(data) => data.name,
        NodeData::ImportEqualsDeclaration(data) => data.name,
        NodeData::InterfaceDeclaration(data) => data.name,
        NodeData::MethodDeclaration(data) => data.name,
        NodeData::MethodSignature(data) => data.name,
        NodeData::ModuleDeclaration(data) => data.name,
        NodeData::NamespaceImport(data) => data.name,
        NodeData::Parameter(data) => data.name,
        NodeData::PropertyDeclaration(data) => data.name,
        NodeData::PropertySignature(data) => data.name,
        NodeData::SetAccessor(data) => data.name,
        NodeData::TypeAliasDeclaration(data) => data.name,
        NodeData::TypeParameter(data) => data.name,
        NodeData::VariableDeclaration(data) => data.name,
        _ => None,
    };
    Ok(name.and_then(|name| cx.arena().node_ref(node.source(), name)))
}

fn dynamic_name_expression(
    cx: &TransformationContext,
    node: TransformNode,
) -> Result<Option<TransformNode>, TransformError> {
    let Some(name) = declaration_name(cx, node)? else {
        return Ok(None);
    };
    let NodeData::ComputedPropertyName(data) = &cx.arena().node(name)?.data else {
        return Ok(None);
    };
    Ok(data
        .expression
        .and_then(|expression| cx.arena().node_ref(node.source(), expression)))
}

fn has_private_identifier_name(
    cx: &TransformationContext,
    node: TransformNode,
) -> Result<bool, TransformError> {
    Ok(declaration_name(cx, node)?.is_some_and(|name| {
        cx.arena()
            .node(name)
            .is_ok_and(|node| node.kind == SyntaxKind::PrivateIdentifier)
    }))
}

type SignatureParts = (
    Option<NodeArrayId>,
    Option<NodeArrayId>,
    Option<NodeId>,
    Option<NodeArrayId>,
);

fn signature_parts(
    cx: &TransformationContext,
    node: TransformNode,
) -> Result<SignatureParts, TransformError> {
    match &cx.arena().node(node)?.data {
        NodeData::FunctionType(data) => Ok((
            data.type_parameters,
            data.parameters,
            data.r#type,
            data.modifiers,
        )),
        NodeData::ConstructorType(data) => Ok((
            data.type_parameters,
            data.parameters,
            data.r#type,
            data.modifiers,
        )),
        _ => Err(DeclarationTransformer::contract(
            "signature update kind mismatch",
        )),
    }
}

fn update_signature(
    cx: &mut TransformationContext,
    original: TransformNode,
    type_parameters: Option<TransformNodeArray>,
    parameters: TransformNodeArray,
    r#type: Option<TransformNode>,
    modifiers: Option<TransformNodeArray>,
) -> Result<TransformNode, TransformError> {
    match cx.arena().node(original)?.data.clone() {
        NodeData::ConstructSignature(_) => {
            cx.factory()?
                .update_construct_signature(original, type_parameters, parameters, r#type)
        }
        NodeData::MethodSignature(data) => {
            let name = data
                .name
                .and_then(|node| cx.arena().node_ref(original.source(), node))
                .ok_or_else(|| DeclarationTransformer::contract("method has no name"))?;
            let question = data
                .question_token
                .and_then(|node| cx.arena().node_ref(original.source(), node));
            cx.factory()?.update_method_signature(
                original,
                modifiers,
                name,
                question,
                type_parameters,
                parameters,
                r#type,
            )
        }
        NodeData::CallSignature(_) => {
            cx.factory()?
                .update_call_signature(original, type_parameters, parameters, r#type)
        }
        NodeData::IndexSignature(_) => {
            let r#type = r#type
                .ok_or_else(|| DeclarationTransformer::contract("index signature has no type"))?;
            cx.factory()?
                .update_index_signature(original, modifiers, parameters, r#type)
        }
        NodeData::FunctionType(_) => {
            let r#type = r#type
                .ok_or_else(|| DeclarationTransformer::contract("function type has no type"))?;
            cx.factory()?
                .update_function_type_node(original, type_parameters, parameters, r#type)
        }
        NodeData::ConstructorType(_) => {
            let r#type = r#type
                .ok_or_else(|| DeclarationTransformer::contract("constructor type has no type"))?;
            cx.factory()?.update_constructor_type_node(
                original,
                modifiers,
                type_parameters,
                parameters,
                r#type,
            )
        }
        _ => Err(DeclarationTransformer::contract(
            "signature update kind mismatch",
        )),
    }
}

/// tsc-port: visitDeclarationSubtree @6.0.3
/// tsc-hash: 4d9576b0e1b4933d0e331071f1b1cdce2c9b432b25d922a5440c8026f1aeb75a
/// tsc-span: _tsc.js:115096-115125
fn update_property(
    cx: &mut TransformationContext,
    original: TransformNode,
    modifiers: Option<TransformNodeArray>,
    r#type: Option<TransformNode>,
    initializer: Option<TransformNode>,
) -> Result<TransformNode, TransformError> {
    let data = cx.arena().node(original)?.data.clone();
    match data {
        NodeData::PropertyDeclaration(data) => {
            let name = data
                .name
                .and_then(|node| cx.arena().node_ref(original.source(), node))
                .ok_or_else(|| DeclarationTransformer::contract("property has no name"))?;
            let question = data
                .question_token
                .and_then(|node| cx.arena().node_ref(original.source(), node));
            let updated = cx.factory()?.update_property_declaration(
                original,
                modifiers,
                name,
                question,
                r#type,
                initializer,
            )?;
            if updated != original && contains_typescript_modifier(cx, modifiers)? {
                let flags =
                    cx.arena().transform_flags(updated) | TransformFlags::CONTAINS_TYPE_SCRIPT;
                cx.arena_mut()?.set_transform_flags(updated, flags);
            }
            Ok(updated)
        }
        NodeData::PropertySignature(data) => {
            let name = data
                .name
                .and_then(|node| cx.arena().node_ref(original.source(), node))
                .ok_or_else(|| DeclarationTransformer::contract("property has no name"))?;
            let question = data
                .question_token
                .and_then(|node| cx.arena().node_ref(original.source(), node));
            cx.factory()?
                .update_property_signature(original, modifiers, name, question, r#type)
        }
        _ => Err(DeclarationTransformer::contract(
            "property update kind mismatch",
        )),
    }
}

fn contains_typescript_modifier(
    cx: &TransformationContext,
    modifiers: Option<TransformNodeArray>,
) -> Result<bool, TransformError> {
    let Some(modifiers) = modifiers else {
        return Ok(false);
    };
    Ok(cx.arena().node_array(modifiers)?.nodes.iter().any(|&node| {
        cx.arena()
            .node(TransformNode::new(modifiers.source(), node))
            .is_ok_and(|node| {
                matches!(
                    node.kind,
                    SyntaxKind::PublicKeyword
                        | SyntaxKind::PrivateKeyword
                        | SyntaxKind::ProtectedKeyword
                        | SyntaxKind::ReadonlyKeyword
                        | SyntaxKind::AbstractKeyword
                        | SyntaxKind::DeclareKeyword
                        | SyntaxKind::OverrideKeyword
                        | SyntaxKind::AccessorKeyword
                )
            })
    }))
}

fn update_variable(
    cx: &mut TransformationContext,
    original: TransformNode,
    r#type: Option<TransformNode>,
    initializer: Option<TransformNode>,
) -> Result<TransformNode, TransformError> {
    let NodeData::VariableDeclaration(data) = cx.arena().node(original)?.data.clone() else {
        return Err(DeclarationTransformer::contract(
            "variable update kind mismatch",
        ));
    };
    let name = data
        .name
        .and_then(|node| cx.arena().node_ref(original.source(), node))
        .ok_or_else(|| DeclarationTransformer::contract("variable has no name"))?;
    cx.factory()?
        .update_variable_declaration(original, name, None, r#type, initializer)
}

fn update_literal_type(
    cx: &mut TransformationContext,
    original: TransformNode,
    literal: TransformNode,
) -> Result<TransformNode, TransformError> {
    // Dispatch through the typed literal-type update face.
    let NodeData::LiteralType(mut data) = cx.arena().node(original)?.data.clone() else {
        return Err(DeclarationTransformer::contract(
            "literal-type update kind mismatch",
        ));
    };
    data.literal = Some(literal.node());
    let flags = cx.arena().transform_flags(original);
    cx.factory()?
        .update_node(original, NodeData::LiteralType(data), flags)
}

fn update_from_created(
    cx: &mut TransformationContext,
    original: TransformNode,
    created: TransformNode,
) -> Result<TransformNode, TransformError> {
    // Same-kind create/update bridge; updating the original retains its JSDoc array.
    let created_record = cx.arena().node(created)?.clone();
    if cx.arena().node(original)?.kind != created_record.kind {
        return Err(DeclarationTransformer::contract(
            "same-kind declaration update received a cross-kind node",
        ));
    }
    let flags = cx.arena().transform_flags(created);
    cx.factory()?
        .update_node(original, created_record.data, flags)
}

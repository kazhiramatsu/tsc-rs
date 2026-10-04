//! `export default …` and `export = …` as tsgo's declaration transform
//! writes them (transformExportAssignment, transform.go:1227-1297).
//!
//! An identifier is exported as it is. A class or function expression
//! becomes a class or function declaration under the expression's own name
//! (or `_default`), exported after it; anything else is a `const` typed by
//! the expression, or initialized by it when it is a primitive literal.

use tsc_syntax::{NodeData, SyntaxKind};
use tsc_types::{ModifierFlags, NodeFlags};

use crate::{
    GeneratedIdentifierFlags, TransformError, TransformNode, TransformNodeArray,
    TransformationContext,
};

use super::diagnostics::DiagnosticContext;
use super::state::VisitResult;
use super::subtree::preserve_js_doc;
use super::tracker::{materialize_effects, TrackerAnchor};
use super::DeclarationTransformer;

/// The expression an export assignment writes, past what
/// SkipOuterExpressions(OEKExpressionTypePassthrough) skips.
enum Unwrapped {
    Node(TransformNode),
    /// A JSDoc cast, which tsgo's reparser puts inside the parentheses as an
    /// `as` or `satisfies` expression that is not skipped.
    Cast,
}

/// tsgo ast.SkipOuterExpressions(node, OEKExpressionTypePassthrough)
/// (ast/utilities.go:787-825): parentheses, `=` and `,`.
fn skip_passthrough_expressions(
    cx: &TransformationContext,
    mut node: TransformNode,
) -> Result<Unwrapped, TransformError> {
    loop {
        let next = match &cx.arena().node(node)?.data {
            NodeData::ParenthesizedExpression(data) => data.expression,
            NodeData::BinaryExpression(data)
                if data.operator_token.is_some_and(|operator| {
                    cx.arena()
                        .node(TransformNode::new(node.source(), operator))
                        .is_ok_and(|operator| {
                            matches!(
                                operator.kind,
                                SyntaxKind::EqualsToken | SyntaxKind::CommaToken
                            )
                        })
                }) =>
            {
                data.right
            }
            _ => None,
        };
        let Some(next) = next else {
            return Ok(Unwrapped::Node(node));
        };
        let next = TransformNode::new(node.source(), next);
        if is_parenthesized(cx, node)? && has_hosted_cast(cx, next)? {
            return Ok(Unwrapped::Cast);
        }
        node = next;
    }
}

/// tsgo unwrapParenthesizedExpression (declarations/util.go:142-147), which
/// stops at a reparsed JSDoc cast.
fn unwrap_parenthesized_expression(
    cx: &TransformationContext,
    mut node: TransformNode,
) -> Result<Option<TransformNode>, TransformError> {
    while let NodeData::ParenthesizedExpression(data) = &cx.arena().node(node)?.data {
        let Some(expression) = data.expression else {
            break;
        };
        let expression = TransformNode::new(node.source(), expression);
        if has_hosted_cast(cx, expression)? {
            return Ok(None);
        }
        node = expression;
    }
    Ok(Some(node))
}

/// tsgo ast.SkipParentheses.
pub(crate) fn unwrap_parentheses(
    cx: &TransformationContext,
    mut node: TransformNode,
) -> Result<TransformNode, TransformError> {
    while let NodeData::ParenthesizedExpression(data) = &cx.arena().node(node)?.data {
        let Some(expression) = data.expression else {
            break;
        };
        node = TransformNode::new(node.source(), expression);
    }
    Ok(node)
}

fn is_parenthesized(
    cx: &TransformationContext,
    node: TransformNode,
) -> Result<bool, TransformError> {
    Ok(cx.arena().node(node)?.kind == SyntaxKind::ParenthesizedExpression)
}

fn has_hosted_cast(
    cx: &TransformationContext,
    node: TransformNode,
) -> Result<bool, TransformError> {
    let source = cx.arena().source(node.source())?.syntax();
    Ok(super::javascript::is_javascript_file(source)
        && tsc_binder::jsdoc_hosted(source)
            .cast_of(node.node())
            .is_some())
}

impl DeclarationTransformer<'_> {
    /// tsgo transformExportAssignment (transform.go:1227-1297). `input` is
    /// the statement, `assignment` the declaration whose type is written.
    pub(crate) fn transform_export_assignment(
        &mut self,
        cx: &mut TransformationContext,
        input: TransformNode,
        assignment: TransformNode,
        expression: TransformNode,
        is_export_equals: bool,
    ) -> Result<VisitResult, TransformError> {
        let source = input.source();
        let parent_kind = self
            .parent(cx, input)?
            .map(|parent| cx.arena().node(parent).map(|parent| parent.kind))
            .transpose()?;
        if parent_kind == Some(SyntaxKind::SourceFile) {
            self.state_mut()?.result_has_external_module_indicator = true;
        }
        self.state_mut()?.result_has_scope_marker = true;
        if cx.arena().node(expression)?.kind == SyntaxKind::Identifier
            && matches!(
                parent_kind,
                Some(SyntaxKind::SourceFile | SyntaxKind::ModuleBlock)
            )
        {
            let export = cx.factory()?.create_export_assignment(
                source,
                None,
                is_export_equals,
                expression,
            )?;
            return preserve_js_doc(cx, export, input).map(VisitResult::Node);
        }

        let unwrapped = skip_passthrough_expressions(cx, expression)?;
        let name = self.name_of_exported_assigned_expression(cx, &unwrapped, is_export_equals)?;
        let unwrapped_kind = match unwrapped {
            Unwrapped::Node(node) => Some((node, cx.arena().node(node)?.kind)),
            Unwrapped::Cast => None,
        };
        match unwrapped_kind {
            Some((class, SyntaxKind::ClassExpression)) => {
                let modifiers = self.declare_modifiers(cx, source)?;
                let declaration =
                    self.transform_class_expression_to_declaration(cx, class, name, modifiers)?;
                let declaration = preserve_js_doc(cx, declaration, input)?;
                let export = self.export_assignment_without_comments(cx, name, is_export_equals)?;
                Ok(VisitResult::Nodes(vec![export, declaration]))
            }
            Some((function, kind)) if super::ensure::is_function_like(kind) => {
                let modifiers = self.declare_modifiers(cx, source)?;
                let full_signature_type = super::javascript::hosted_type(cx, assignment)?;
                let declaration = self.transform_function_like_to_declaration(
                    cx,
                    function,
                    name,
                    modifiers,
                    full_signature_type,
                )?;
                let declaration = preserve_js_doc(cx, declaration, input)?;
                let export = self.export_assignment_without_comments(cx, name, is_export_equals)?;
                Ok(VisitResult::Nodes(vec![export, declaration]))
            }
            _ => {
                let statement =
                    self.typed_export_assignment_variable(cx, input, assignment, expression, name)?;
                let statement = preserve_js_doc(cx, statement, input)?;
                let export =
                    cx.factory()?
                        .create_export_assignment(source, None, is_export_equals, name)?;
                Ok(VisitResult::Nodes(vec![statement, export]))
            }
        }
    }

    /// `const name: T;` or `const name = literal;` for an exported
    /// expression (transform.go:1268-1289).
    fn typed_export_assignment_variable(
        &mut self,
        cx: &mut TransformationContext,
        input: TransformNode,
        assignment: TransformNode,
        expression: TransformNode,
        name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = input.source();
        let saved_diagnostic = self
            .tracker
            .replace_diagnostic_context(cx.arena(), DiagnosticContext::DefaultExport(input))?;
        let saved_fallback = self
            .tracker
            .error_fallback_node
            .replace(TrackerAnchor::Transform(assignment));
        let result = (|| {
            let initializer = match unwrap_parenthesized_expression(cx, expression)? {
                Some(literal) if super::ensure::is_primitive_literal_value(cx, literal)? => {
                    let resolver_node = self.required_resolver_node(cx, assignment)?;
                    let target = self.state()?.current_source_file;
                    let result = self.resolver.create_literal_const_value(
                        cx.arena_mut()?,
                        target,
                        resolver_node,
                        &mut self.tracker,
                    );
                    let effects = self.tracker.take_pending_effects();
                    materialize_effects(cx, self.host, effects)?;
                    result.map_err(TransformError::from)?
                }
                _ => None,
            };
            let r#type = match initializer {
                Some(_) => None,
                None => self.ensure_type(cx, assignment, false)?,
            };
            Ok::<_, TransformError>((r#type, initializer))
        })();
        self.tracker.error_fallback_node = saved_fallback;
        self.tracker.restore_diagnostic_context(saved_diagnostic);
        let (r#type, initializer) = result?;
        let modifiers = self.declare_modifiers(cx, source)?;
        let mut factory = cx.factory()?;
        let declaration =
            factory.create_variable_declaration(source, name, None, r#type, initializer)?;
        let declarations = factory.create_node_array(source, vec![declaration])?;
        let list =
            factory.create_variable_declaration_list(source, declarations, NodeFlags::CONST)?;
        factory.create_variable_statement(source, Some(modifiers), list)
    }

    /// `declare` when the statement needs it, else no modifiers.
    fn declare_modifiers(
        &self,
        cx: &mut TransformationContext,
        source: crate::TransformSourceId,
    ) -> Result<TransformNodeArray, TransformError> {
        let needs_declare = self.state()?.needs_declare;
        let mut factory = cx.factory()?;
        let modifiers = if needs_declare {
            factory.create_modifiers_from_modifier_flags(source, ModifierFlags::AMBIENT)?
        } else {
            None
        };
        match modifiers {
            Some(modifiers) => Ok(modifiers),
            None => factory.create_node_array(source, Vec::new()),
        }
    }

    fn export_assignment_without_comments(
        &self,
        cx: &mut TransformationContext,
        name: TransformNode,
        is_export_equals: bool,
    ) -> Result<TransformNode, TransformError> {
        let export =
            cx.factory()?
                .create_export_assignment(name.source(), None, is_export_equals, name)?;
        Ok(cx.arena_mut()?.remove_all_comments(export))
    }

    /// tsgo getNameOfExportedAssignedExpression and
    /// tryGetNameOfAssignedExpression (transform.go:1192-1225): the name of a
    /// named class or function expression or of an identifier, unique if it
    /// resolves at the enclosing declaration; else `_exports` for a
    /// JavaScript `export =`, or `_default`.
    fn name_of_exported_assigned_expression(
        &mut self,
        cx: &mut TransformationContext,
        unwrapped: &Unwrapped,
        is_export_equals: bool,
    ) -> Result<TransformNode, TransformError> {
        let source = self.state()?.current_source_file;
        let name_node = match unwrapped {
            Unwrapped::Node(node) => match &cx.arena().node(*node)?.data {
                NodeData::ClassExpression(data) => data.name,
                NodeData::FunctionExpression(data) => data.name,
                NodeData::Identifier(_) => Some(node.node()),
                _ => None,
            }
            .and_then(|name| cx.arena().node_ref(node.source(), name)),
            Unwrapped::Cast => None,
        };
        let text = match name_node {
            Some(name) => match &cx.arena().node(name)?.data {
                NodeData::Identifier(data) => data.text().to_owned(),
                _ => String::new(),
            },
            None => String::new(),
        };
        if !text.is_empty() && text != "default" {
            let enclosing = self.state()?.enclosing_declaration.ok_or_else(|| {
                Self::contract("an export assignment needs an enclosing declaration")
            })?;
            let resolvable = self
                .resolver
                .is_name_resolvable(self.required_resolver_node(cx, enclosing)?, &text)?;
            let mut factory = cx.factory()?;
            let name = if resolvable {
                factory.create_unique_name(source, text, GeneratedIdentifierFlags::OPTIMISTIC)?
            } else {
                factory.create_identifier(source, text)?
            };
            self.state_mut()?.common_js.export_assignment_name = Some(name);
            return Ok(name);
        }
        let javascript = super::javascript::is_javascript_file(cx.arena().source(source)?.syntax());
        let base = if is_export_equals && javascript {
            "_exports"
        } else {
            "_default"
        };
        let name =
            cx.factory()?
                .create_unique_name(source, base, GeneratedIdentifierFlags::OPTIMISTIC)?;
        self.state_mut()?.common_js.export_assignment_name = Some(name);
        Ok(name)
    }

    /// tsgo transformFunctionLikeToDeclaration (transform.go:1298-1322): a
    /// function declaration, or a `const` of the function's full signature.
    fn transform_function_like_to_declaration(
        &mut self,
        cx: &mut TransformationContext,
        function: TransformNode,
        name: TransformNode,
        modifiers: TransformNodeArray,
        full_signature_type: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let source = name.source();
        let signature = match super::javascript::hosted_full_signature(cx, function)? {
            Some(signature) => Some(signature),
            None => full_signature_type,
        };
        if let Some(signature) = signature {
            let r#type = match self.visit_declaration_subtree(cx, signature)? {
                VisitResult::Node(node) => Some(node),
                _ => None,
            };
            let mut factory = cx.factory()?;
            let declaration =
                factory.create_variable_declaration(source, name, None, r#type, None)?;
            let declarations = factory.create_node_array(source, vec![declaration])?;
            let list =
                factory.create_variable_declaration_list(source, declarations, NodeFlags::CONST)?;
            return factory.create_variable_statement(source, Some(modifiers), list);
        }
        let (type_parameters, parameters) = match &cx.arena().node(function)?.data {
            NodeData::FunctionExpression(data) => (data.type_parameters, data.parameters),
            NodeData::ArrowFunction(data) => (data.type_parameters, data.parameters),
            _ => {
                return Err(Self::contract(
                    "an exported function is not a function expression",
                ))
            }
        };
        let type_parameters = self.ensure_type_params(cx, function, type_parameters)?;
        let parameters = self.update_params_list(
            cx,
            function,
            parameters,
            ModifierFlags::from_bits(ModifierFlags::ALL.bits() ^ ModifierFlags::PUBLIC.bits()),
        )?;
        let return_type = self.ensure_type(cx, function, false)?;
        cx.factory()?.create_function_declaration(
            source,
            Some(modifiers),
            None,
            Some(name),
            type_parameters,
            parameters,
            return_type,
            None,
        )
    }

    /// tsgo transformClassExpressionToDeclaration (transform.go:1571-1599): a
    /// class declaration of the expression's members, serialized with the
    /// class expression as the enclosing declaration.
    pub(crate) fn transform_class_expression_to_declaration(
        &mut self,
        cx: &mut TransformationContext,
        class: TransformNode,
        name: TransformNode,
        modifiers: TransformNodeArray,
    ) -> Result<TransformNode, TransformError> {
        let previous_enclosing = self.state_mut()?.enclosing_declaration.replace(class);
        let previous_in_class_expression =
            std::mem::replace(&mut self.state_mut()?.in_class_expression_declaration, true);
        let result = (|| {
            let extra_members = if super::javascript::is_javascript_file(
                cx.arena().source(class.source())?.syntax(),
            ) {
                self.collect_this_property_assignments(cx, class)?
            } else {
                Vec::new()
            };
            let members = super::statements::build_class_members(self, cx, class, extra_members)?;
            let data = super::statements::class_data(cx, class)?;
            let type_parameters = self.ensure_type_params(cx, class, data.type_parameters)?;
            let hosted_implements = super::javascript::hosted_implements(cx, class)?;
            let heritage = data
                .heritage_clauses
                .and_then(|clauses| cx.arena().node_array_ref(class.source(), clauses));
            let heritage = super::statements::transform_heritage_clauses(
                self,
                cx,
                heritage,
                &hosted_implements,
            )?;
            cx.factory()?.create_class_declaration(
                name.source(),
                Some(modifiers),
                Some(name),
                type_parameters,
                heritage,
                members,
            )
        })();
        let state = self.state_mut()?;
        state.enclosing_declaration = previous_enclosing;
        state.in_class_expression_declaration = previous_in_class_expression;
        result
    }
}
